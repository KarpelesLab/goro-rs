//! pdo_parse_params (pdo_sql_parser.re): placeholder scanning and query
//! rewriting for emulated prepares and placeholder style conversion.

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::core::*;
use crate::scanner;

pub const T_TEXT: i32 = 1;
pub const T_BIND: i32 = 2;
pub const T_BIND_POS: i32 = 3;
pub const T_ESCAPED_QUESTION: i32 = 4;
pub const T_CUSTOM_QUOTE: i32 = 5;
pub const EOI: i32 = 6;

const BINDNO_ESCAPED_CHAR: i64 = -1;

struct Placeholder {
    pos: usize,
    len: usize,
    quoted: Option<Vec<u8>>,
    bindno: i64,
}

/// Returns 0 (no changes), 1 (rewritten into `out`) or -1 (error raised).
pub fn parse_params(vm: &mut Vm, stmt: &StmtRef, inquery: &[u8], out: &mut Vec<u8>) -> i32 {
    let dbh = match stmt.borrow().dbh.clone() {
        Some(d) => d,
        None => return -1,
    };
    let methods = dbh.borrow().methods;
    let scan = methods
        .and_then(|m| m.scanner)
        .unwrap_or(scanner::default_scanner);
    let supports = stmt.borrow().supports_placeholders;
    let mut buf = inquery.to_vec();
    buf.push(0);

    let mut placeholders: Vec<Placeholder> = Vec::new();
    let mut bindno: i64 = 0;
    let mut escapes = 0;
    let mut query_type = PDO_PLACEHOLDER_NONE;
    let mut custom_quote: Option<(usize, usize)> = None;
    let mut cur = 0usize;

    // phase 1: look for args
    loop {
        let tok = cur;
        let (t, next) = scan(&buf, tok);
        cur = next;
        if t == EOI {
            break;
        }
        let mut is_placeholder = false;
        if let Some((qpos, qlen)) = custom_quote {
            if t == T_CUSTOM_QUOTE && qlen == cur - tok && buf[tok..cur] == buf[qpos..qpos + qlen] {
                custom_quote = None;
            } else if t == T_ESCAPED_QUESTION {
                docref_deprecated(
                    vm,
                    "Escaping question marks inside dollar quoted strings is not required anymore and is deprecated",
                );
                is_placeholder = true;
            }
            if !is_placeholder {
                continue;
            }
        } else if t == T_CUSTOM_QUOTE {
            custom_quote = Some((tok, cur - tok));
            continue;
        }
        if is_placeholder || t == T_BIND || t == T_BIND_POS || t == T_ESCAPED_QUESTION {
            if !is_placeholder {
                if t == T_ESCAPED_QUESTION && supports == PDO_PLACEHOLDER_POSITIONAL {
                    continue;
                }
                if t == T_BIND {
                    if tok > 0 && buf[tok - 1].is_ascii_alphanumeric() {
                        continue;
                    }
                    query_type |= PDO_PLACEHOLDER_NAMED;
                } else if t == T_BIND_POS {
                    query_type |= PDO_PLACEHOLDER_POSITIONAL;
                }
            }
            let mut plc = Placeholder {
                pos: tok,
                len: cur - tok,
                quoted: None,
                bindno: 0,
            };
            if t == T_ESCAPED_QUESTION {
                plc.bindno = BINDNO_ESCAPED_CHAR;
                plc.quoted = Some(b"?".to_vec());
                escapes += 1;
            } else {
                plc.bindno = bindno;
                bindno += 1;
            }
            placeholders.push(plc);
        }
    }

    // did the query make sense to me?
    if query_type == (PDO_PLACEHOLDER_NAMED | PDO_PLACEHOLDER_POSITIONAL) {
        raise_impl_error(
            vm,
            &dbh,
            Some(stmt),
            "HY093",
            Some("mixed named and positional parameters"),
        );
        return -1;
    }

    let nparams = stmt.borrow().bound_params.as_ref().map(|p| p.len() as i64);
    let mut goto_safe = false;
    if supports == PDO_PLACEHOLDER_NONE {
        if let Some(np) = nparams {
            if bindno != np {
                let mut ok_safe = false;
                if query_type != PDO_PLACEHOLDER_POSITIONAL && bindno > np {
                    let st = stmt.borrow();
                    let params = st.bound_params.as_ref().unwrap();
                    ok_safe = placeholders.iter().all(|plc| {
                        params
                            .get(&HKey::Str(inquery[plc.pos..plc.pos + plc.len].to_vec()))
                            .is_some()
                    });
                }
                if ok_safe {
                    goto_safe = true;
                } else {
                    raise_impl_error(
                        vm,
                        &dbh,
                        Some(stmt),
                        "HY093",
                        Some("number of bound variables does not match number of tokens"),
                    );
                    return -1;
                }
            }
        }
    }

    if placeholders.is_empty() {
        return 0;
    }

    let named_rewrite_template = stmt.borrow().named_rewrite_template;
    let mut goto_rewrite = false;
    if !goto_safe {
        if supports == query_type && named_rewrite_template.is_none() {
            // query matches native syntax
            if escapes > 0 {
                goto_rewrite = true;
            } else {
                return 0;
            }
        }
        if !goto_rewrite && query_type == PDO_PLACEHOLDER_NAMED && named_rewrite_template.is_some()
        {
            query_type = PDO_PLACEHOLDER_POSITIONAL;
        }
    }

    if !goto_rewrite {
        if supports == PDO_PLACEHOLDER_NONE {
            // query generation: let's quote all the values
            let has_params = stmt.borrow().bound_params.is_some();
            if has_params {
                for plc in placeholders.iter_mut() {
                    if plc.bindno == BINDNO_ESCAPED_CHAR {
                        continue;
                    }
                    if query_type == PDO_PLACEHOLDER_NONE {
                        continue;
                    }
                    let key = if query_type == PDO_PLACEHOLDER_POSITIONAL {
                        HKey::Int(plc.bindno)
                    } else {
                        let p = &*plc;
                        HKey::Str(inquery[p.pos..p.pos + p.len].to_vec())
                    };
                    let param = stmt
                        .borrow()
                        .bound_params
                        .as_ref()
                        .and_then(|t| t.get(&key).cloned());
                    let param = match param {
                        Some(p) => p,
                        None => {
                            raise_impl_error(
                                vm,
                                &dbh,
                                Some(stmt),
                                "HY093",
                                Some("parameter was not defined"),
                            );
                            return -1;
                        }
                    };
                    let quoter = methods.and_then(|m| m.quoter);
                    let parameter = param.value();
                    if let Some(quoter) = quoter {
                        if param.param_type == PDO_PARAM_LOB
                            && matches!(parameter, Value::Resource(_))
                        {
                            let data = match crate::lob::read_all(vm, &parameter) {
                                Some(d) => d,
                                None => {
                                    raise_impl_error(
                                        vm,
                                        &dbh,
                                        Some(stmt),
                                        "HY105",
                                        Some("Expected a stream resource"),
                                    );
                                    return -1;
                                }
                            };
                            match quoter(vm, &dbh, &data, param.param_type) {
                                Some(q) => plc.quoted = Some(q),
                                None => {
                                    let ec = dbh.borrow().error_code.clone();
                                    stmt.borrow_mut().error_code = sqlstate_buf(&ec);
                                    return -1;
                                }
                            }
                        } else {
                            let mut ptype = param.param_type;
                            if matches!(parameter, Value::Null) {
                                ptype = PDO_PARAM_NULL;
                            }
                            match ptype {
                                PDO_PARAM_BOOL => {
                                    plc.quoted = Some(if parameter.is_truthy() {
                                        b"1".to_vec()
                                    } else {
                                        b"0".to_vec()
                                    })
                                }
                                PDO_PARAM_INT => {
                                    plc.quoted =
                                        Some(zval_get_long(&parameter).to_string().into_bytes())
                                }
                                PDO_PARAM_NULL => plc.quoted = Some(b"NULL".to_vec()),
                                _ => {
                                    let s = match vm.value_to_string_checked(&parameter) {
                                        Ok(s) => s.as_bytes().to_vec(),
                                        Err(_) => {
                                            let ec = dbh.borrow().error_code.clone();
                                            stmt.borrow_mut().error_code = sqlstate_buf(&ec);
                                            return -1;
                                        }
                                    };
                                    if vm.current_exception.is_some() {
                                        let ec = dbh.borrow().error_code.clone();
                                        stmt.borrow_mut().error_code = sqlstate_buf(&ec);
                                        return -1;
                                    }
                                    match quoter(vm, &dbh, &s, ptype) {
                                        Some(q) => plc.quoted = Some(q),
                                        None => {
                                            let ec = dbh.borrow().error_code.clone();
                                            stmt.borrow_mut().error_code = sqlstate_buf(&ec);
                                            return -1;
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        plc.quoted = Some(parameter.to_php_string().as_bytes().to_vec());
                    }
                }
            }
        } else if query_type == PDO_PLACEHOLDER_POSITIONAL {
            // rewrite ? to :pdoX
            let tmpl = named_rewrite_template.unwrap_or(":pdo%d");
            let mut bind_no = 1;
            let mut st = stmt.borrow_mut();
            if st.bound_param_map.is_none() {
                st.bound_param_map = Some(Table::new());
            }
            for plc in placeholders.iter_mut() {
                if plc.bindno == BINDNO_ESCAPED_CHAR {
                    continue;
                }
                let name = inquery[plc.pos..plc.pos + plc.len].to_vec();
                let map = st.bound_param_map.as_mut().unwrap();
                let existing = if name.as_slice() == b"?" {
                    None
                } else {
                    map.get(&HKey::Str(name.clone())).cloned()
                };
                let (idxbuf, skip_map) = match existing {
                    Some(p) => (p, true),
                    None => {
                        let s = tmpl.replace("%d", &bind_no.to_string()).into_bytes();
                        bind_no += 1;
                        (s, false)
                    }
                };
                plc.quoted = Some(idxbuf.clone());
                if !skip_map && named_rewrite_template.is_some() {
                    map.update(HKey::Str(name), idxbuf.clone());
                }
                map.update(HKey::Int(plc.bindno), idxbuf);
            }
        } else {
            // rewrite :name to ?
            let mut st = stmt.borrow_mut();
            if st.bound_param_map.is_none() {
                st.bound_param_map = Some(Table::new());
            }
            let map = st.bound_param_map.as_mut().unwrap();
            for plc in placeholders.iter_mut() {
                let name = inquery[plc.pos..plc.pos + plc.len].to_vec();
                map.update(HKey::Int(plc.bindno), name);
                plc.quoted = Some(b"?".to_vec());
            }
        }
    }

    // rewrite: build the query
    out.clear();
    let mut ptr = 0usize;
    for plc in &placeholders {
        out.extend_from_slice(&inquery[ptr..plc.pos]);
        match &plc.quoted {
            Some(q) => out.extend_from_slice(q),
            None => out.extend_from_slice(&inquery[plc.pos..plc.pos + plc.len]),
        }
        ptr = plc.pos + plc.len;
    }
    out.extend_from_slice(&inquery[ptr..]);
    1
}
