//! The PDOStatement class (pdo_stmt.c): parameter binding, execution,
//! the fetch modes, the statement iterator.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::zpp::{Arg, Func, parse};

use crate::core::*;
use crate::objects::{ITER_HANDLERS, StmtIter, iter_of, lazy_object, stmt_of};

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

fn new_arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// PHP_STMT_GET_OBJ
fn this_stmt(vm: &mut Vm, f: &Func) -> Option<(Obj, StmtRef)> {
    let o = f.this.clone()?;
    let s = stmt_of(&o)?;
    if s.borrow().dbh.is_none() {
        let m = format!(
            "{} object is uninitialized",
            goro_core::value::display_class_name(&o.borrow().class_name)
        );
        throw(vm, b"Error", &m);
        return None;
    }
    Some((o, s))
}

fn dbh_of_stmt(stmt: &StmtRef) -> DbhRef {
    stmt.borrow().dbh.clone().expect("statement handle")
}

fn methods(stmt: &StmtRef) -> &'static StmtMethods {
    stmt.borrow().methods.expect("statement methods")
}

/// rewrite_name_to_position: 1 ok, 0 / -1 error
fn rewrite_name_to_position(vm: &mut Vm, stmt: &StmtRef, param: &mut BoundParam) -> i32 {
    let (map, template) = {
        let s = stmt.borrow();
        (s.bound_param_map.clone(), s.named_rewrite_template)
    };
    let map = match map {
        Some(m) => m,
        None => return 1,
    };
    if template.is_some() {
        return 1;
    }
    let dbh = dbh_of_stmt(stmt);
    match &param.name {
        None => {
            if let Some(name) = map.get(&HKey::Int(param.paramno)) {
                param.name = Some(name.clone());
                return 1;
            }
            raise_impl_error(
                vm,
                &dbh,
                Some(stmt),
                "HY093",
                Some("parameter was not defined"),
            );
            0
        }
        Some(pname) => {
            let mut position = 0i64;
            for (_, name) in &map.entries {
                if name != pname {
                    position += 1;
                    continue;
                }
                if param.paramno >= 0 {
                    raise_impl_error(
                        vm,
                        &dbh,
                        Some(stmt),
                        "IM001",
                        Some(
                            "PDO refuses to handle repeating the same :named parameter for multiple positions with this driver, as it might be unsafe to do so.  Consider using a separate name for each parameter instead",
                        ),
                    );
                    return -1;
                }
                param.paramno = position;
                return 1;
            }
            raise_impl_error(
                vm,
                &dbh,
                Some(stmt),
                "HY093",
                Some("parameter was not defined"),
            );
            0
        }
    }
}

/// dispatch_param_event
pub fn dispatch_param_event(vm: &mut Vm, stmt: &StmtRef, event: u8) -> bool {
    let dbh = dbh_of_stmt(stmt);
    if dbh.borrow().skip_param_evt & (1 << event) != 0 {
        return true;
    }
    let hook = match methods(stmt).param_hook {
        Some(h) => h,
        None => return true,
    };
    for is_param in [true, false] {
        let keys: Vec<HKey> = {
            let s = stmt.borrow();
            let t = if is_param {
                s.bound_params.as_ref()
            } else {
                s.bound_columns.as_ref()
            };
            match t {
                Some(t) => t.entries.iter().map(|(k, _)| k.clone()).collect(),
                None => continue,
            }
        };
        for k in keys {
            let p = {
                let s = stmt.borrow();
                let t = if is_param {
                    s.bound_params.as_ref()
                } else {
                    s.bound_columns.as_ref()
                };
                t.and_then(|t| t.get(&k).cloned())
            };
            let mut p = match p {
                Some(p) => p,
                None => continue,
            };
            let ok = hook(vm, stmt, &mut p, event);
            {
                let mut s = stmt.borrow_mut();
                let t = if is_param {
                    s.bound_params.as_mut()
                } else {
                    s.bound_columns.as_mut()
                };
                if let Some(slot) = t.and_then(|t| t.get_mut(&k)) {
                    *slot = p;
                }
            }
            if !ok {
                return false;
            }
        }
    }
    true
}

/// pdo_stmt_describe_columns
pub fn describe_columns(vm: &mut Vm, stmt: &StmtRef) -> bool {
    let n = stmt.borrow().column_count.max(0) as usize;
    stmt.borrow_mut().columns = Some(vec![Column::default(); n]);
    let describer = methods(stmt).describer;
    let dbh = dbh_of_stmt(stmt);
    for col in 0..n {
        if !describer(vm, stmt, col) {
            return false;
        }
        let (native, desired) = {
            let d = dbh.borrow();
            (d.native_case, d.desired_case)
        };
        let mut s = stmt.borrow_mut();
        if native != desired && desired != PDO_CASE_NATURAL {
            if let Some(c) = s.columns.as_mut().and_then(|c| c.get_mut(col)) {
                if let Some(name) = c.name.as_mut() {
                    match desired {
                        PDO_CASE_LOWER => name.make_ascii_lowercase(),
                        PDO_CASE_UPPER => name.make_ascii_uppercase(),
                        _ => {}
                    }
                }
            }
        }
        let name = s
            .columns
            .as_ref()
            .and_then(|c| c.get(col))
            .and_then(|c| c.name.clone());
        if let (Some(name), Some(bc)) = (name, s.bound_columns.as_mut()) {
            if let Some(p) = bc.get_mut(&HKey::Str(name)) {
                p.paramno = col as i64;
            }
        }
    }
    true
}

/// pdo_stmt_reset_columns
pub fn reset_columns(stmt: &StmtRef) {
    let mut s = stmt.borrow_mut();
    s.columns = None;
    s.column_count = 0;
}

/// php_pdo_stmt_set_column_count
pub fn set_column_count(s: &mut Stmt, new_count: i32) {
    if s.columns.is_none() {
        s.column_count = new_count;
        return;
    }
    if new_count == s.column_count {
        return;
    }
    s.columns = None;
    s.column_count = new_count;
}

/// really_register_bound_param
fn really_register_bound_param(
    vm: &mut Vm,
    mut param: BoundParam,
    stmt: &StmtRef,
    is_param: bool,
) -> bool {
    {
        let mut s = stmt.borrow_mut();
        let t = if is_param {
            &mut s.bound_params
        } else {
            &mut s.bound_columns
        };
        if t.is_none() {
            *t = Some(Table::new());
        }
    }
    let ptype = param_type(param.param_type);
    let value = param.value();
    if ptype == PDO_PARAM_STR && param.max_value_len <= 0 && !matches!(value, Value::Null) {
        if !matches!(value, Value::String(_)) {
            match vm.value_to_string_checked(&value) {
                Ok(s) => param.set_value(Value::String(s)),
                Err(_) => return false,
            }
            if vm.current_exception.is_some() {
                return false;
            }
        }
    } else if ptype == PDO_PARAM_INT && matches!(value, Value::True | Value::False) {
        param.set_value(Value::Long(value.to_long()));
    } else if ptype == PDO_PARAM_BOOL && matches!(value, Value::Long(_)) {
        param.set_value(if value.is_truthy() {
            Value::True
        } else {
            Value::False
        });
    }
    param.is_param = is_param;

    let dbh = dbh_of_stmt(stmt);
    if !is_param && param.name.is_some() && stmt.borrow().columns.is_some() {
        let pname = param.name.clone().unwrap();
        {
            let s = stmt.borrow();
            let cols = s.columns.as_ref().unwrap();
            for i in 0..s.column_count.max(0) as usize {
                if cols.get(i).map(|c| c.name()) == Some(pname.as_slice()) {
                    param.paramno = i as i64;
                    break;
                }
            }
        }
        if param.paramno == -1 {
            let m = format!(
                "Did not find column name '{}' in the defined columns; it will not be bound",
                String::from_utf8_lossy(&pname)
            );
            raise_impl_error(vm, &dbh, Some(stmt), "HY000", Some(&m));
        }
    }

    if let Some(name) = &param.name {
        if is_param && name.first() != Some(&b':') {
            let mut n = vec![b':'];
            n.extend_from_slice(name);
            param.name = Some(n);
        }
    }

    if is_param && rewrite_name_to_position(vm, stmt, &mut param) == 0 {
        return false;
    }

    let hook = methods(stmt).param_hook;
    if let Some(hook) = hook {
        if !hook(vm, stmt, &mut param, PDO_PARAM_EVT_NORMALIZE) {
            handle_stmt_err(vm, stmt);
            return false;
        }
    }

    let key = match &param.name {
        Some(n) => HKey::Str(n.clone()),
        None => HKey::Int(param.paramno),
    };
    {
        let mut s = stmt.borrow_mut();
        let t = if is_param {
            s.bound_params.as_mut()
        } else {
            s.bound_columns.as_mut()
        }
        .unwrap();
        if param.paramno >= 0 {
            t.remove(&HKey::Int(param.paramno));
        }
        t.update(key.clone(), param.clone());
    }

    if let Some(hook) = hook {
        let mut p = param;
        let ok = hook(vm, stmt, &mut p, PDO_PARAM_EVT_ALLOC);
        if !ok {
            handle_stmt_err(vm, stmt);
            let mut s = stmt.borrow_mut();
            let t = if is_param {
                s.bound_params.as_mut()
            } else {
                s.bound_columns.as_mut()
            };
            if let Some(t) = t {
                t.remove(&key);
            }
            return false;
        }
        let mut s = stmt.borrow_mut();
        let t = if is_param {
            s.bound_params.as_mut()
        } else {
            s.bound_columns.as_mut()
        };
        if let Some(slot) = t.and_then(|t| t.get_mut(&key)) {
            *slot = p;
        }
    }
    true
}

/// PDOStatement::execute
pub fn stmt_execute(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "|a!", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    stmt.borrow_mut().clear_err();
    if let Arg::Arr(input) = &a[0] {
        let old = stmt.borrow_mut().bound_params.take();
        drop(old);
        let entries: Vec<(ArrayKey, Value)> = input
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (k, v) in entries {
            let mut param = BoundParam::new();
            match k {
                ArrayKey::String(s) => {
                    param.name = Some(s.as_bytes().to_vec());
                    param.paramno = -1;
                }
                ArrayKey::Int(n) => param.paramno = n,
            }
            param.param_type = PDO_PARAM_STR;
            param.parameter = v;
            if !really_register_bound_param(vm, param, &stmt, true) {
                return Value::False;
            }
        }
    }
    let mut ret = true;
    let supports = stmt.borrow().supports_placeholders;
    if supports == PDO_PLACEHOLDER_NONE {
        {
            let mut s = stmt.borrow_mut();
            s.active_query_string = None;
            s.active_is_query = false;
        }
        let q = stmt.borrow().query().to_vec();
        let mut out = Vec::new();
        let r = crate::parser::parse_params(vm, &stmt, &q, &mut out);
        match r {
            0 => {
                let mut s = stmt.borrow_mut();
                s.active_query_string = Some(q);
                s.active_is_query = true;
            }
            -1 => return Value::False,
            _ => stmt.borrow_mut().active_query_string = Some(out),
        }
    } else if !dispatch_param_event(vm, &stmt, PDO_PARAM_EVT_EXEC_PRE) {
        handle_stmt_err(vm, &stmt);
        return Value::False;
    }
    let exec = methods(&stmt).executer;
    let ok = exec(vm, &stmt);
    flush_mysqlnd(vm);
    if ok {
        if !stmt.borrow().executed {
            let dbh = dbh_of_stmt(&stmt);
            if dbh.borrow().alloc_own_columns && stmt.borrow().columns.is_none() {
                ret = describe_columns(vm, &stmt);
            }
            stmt.borrow_mut().executed = true;
        }
        if ret && !dispatch_param_event(vm, &stmt, PDO_PARAM_EVT_EXEC_POST) {
            handle_stmt_err(vm, &stmt);
            return Value::False;
        }
        return vbool(ret);
    }
    handle_stmt_err(vm, &stmt);
    Value::False
}

/// convert_to_string with PHP's conversions
fn convert_to_string(vm: &mut Vm, v: &Value) -> Value {
    match v {
        Value::String(_) => v.clone(),
        other => Value::String(vm.value_to_string(other)),
    }
}

/// fetch_value
pub fn fetch_value(vm: &mut Vm, stmt: &StmtRef, colno: i64, type_override: Option<i64>) -> Value {
    if colno < 0 {
        throw(
            vm,
            b"ValueError",
            "Column index must be greater than or equal to 0",
        );
        return Value::Null;
    }
    if colno >= stmt.borrow().column_count as i64 {
        throw(vm, b"ValueError", "Invalid column index");
        return Value::Null;
    }
    let m = methods(stmt);
    let mut dest = match m.get_col_typed {
        Some(g) => g(vm, stmt, colno as usize, type_override),
        None => (m.get_col)(vm, stmt, colno as usize),
    };
    let dbh = dbh_of_stmt(stmt);
    let (oracle_nulls, stringify) = {
        let d = dbh.borrow();
        (d.oracle_nulls, d.stringify)
    };
    if let Value::String(s) = &dest {
        if s.is_empty() && oracle_nulls == PDO_NULL_EMPTY_STRING {
            dest = Value::Null;
        }
    }
    let type_override = if stringify {
        Some(PDO_PARAM_STR)
    } else {
        type_override
    };
    if let Some(t) = type_override {
        if !matches!(dest, Value::Null) {
            match t {
                PDO_PARAM_INT => dest = Value::Long(dest.to_long()),
                PDO_PARAM_BOOL => dest = vbool(dest.is_truthy()),
                PDO_PARAM_STR => {
                    dest = match &dest {
                        Value::False => vstr("0"),
                        Value::Resource(_) => match crate::lob::read_all(vm, &dest) {
                            Some(d) => vstr(d),
                            None => vstr(""),
                        },
                        other => {
                            let o = other.clone();
                            convert_to_string(vm, &o)
                        }
                    }
                }
                PDO_PARAM_NULL => dest = Value::Null,
                PDO_PARAM_LOB => {
                    if let Value::String(s) = &dest {
                        let data = s.as_bytes().to_vec();
                        dest = crate::lob::memory_stream(vm, &data);
                    }
                }
                _ => {}
            }
        }
    }
    if matches!(dest, Value::Null) && oracle_nulls == PDO_NULL_TO_STRING {
        dest = vstr("");
    }
    dest
}

/// do_fetch_common
fn do_fetch_common(vm: &mut Vm, stmt: &StmtRef, ori: i64, offset: i64) -> bool {
    if !stmt.borrow().executed {
        return false;
    }
    if !dispatch_param_event(vm, stmt, PDO_PARAM_EVT_FETCH_PRE) {
        return false;
    }
    let fetcher = methods(stmt).fetcher;
    let ok = fetcher(vm, stmt, ori, offset);
    flush_mysqlnd(vm);
    if !ok {
        return false;
    }
    if stmt.borrow().columns.is_none() && !describe_columns(vm, stmt) {
        return false;
    }
    if !dispatch_param_event(vm, stmt, PDO_PARAM_EVT_FETCH_POST) {
        return false;
    }
    let bound: Vec<BoundParam> = stmt
        .borrow()
        .bound_columns
        .as_ref()
        .map(|t| t.entries.iter().map(|(_, p)| p.clone()).collect())
        .unwrap_or_default();
    for param in bound {
        if param.paramno >= 0 {
            let r = match &param.parameter {
                Value::Reference(r) => r.clone(),
                _ => continue,
            };
            *r.borrow_mut() = Value::Null;
            let v = fetch_value(vm, stmt, param.paramno, Some(param.param_type));
            *r.borrow_mut() = v;
        }
    }
    true
}

/// pdo_do_key_pair_fetch
fn do_key_pair_fetch(
    vm: &mut Vm,
    stmt: &StmtRef,
    ori: i64,
    offset: i64,
    container: &mut PhpArray,
) -> bool {
    if !do_fetch_common(vm, stmt, ori, offset) {
        return false;
    }
    if stmt.borrow().column_count != 2 {
        let dbh = dbh_of_stmt(stmt);
        raise_impl_error(
            vm,
            &dbh,
            Some(stmt),
            "HY000",
            Some(
                "PDO::FETCH_KEY_PAIR fetch mode requires the result set to contain exactly 2 columns.",
            ),
        );
        return false;
    }
    let key = fetch_value(vm, stmt, 0, None);
    let val = fetch_value(vm, stmt, 1, None);
    match key {
        Value::Long(n) => container.set(ArrayKey::Int(n), val),
        other => {
            let k = convert_to_string(vm, &other);
            container.set(Vm::value_to_array_key(k), val);
        }
    }
    true
}

/// Whether a class implements Serializable (has ce->unserialize)
fn has_unserialize(vm: &Vm, class_lower: &[u8]) -> bool {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let c = match vm.classes.get(&cur) {
            Some(c) => c,
            None => return false,
        };
        if c.interfaces
            .iter()
            .any(|i| i.eq_ignore_ascii_case(b"serializable"))
        {
            return true;
        }
        match &c.parent {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

/// do_fetch: the fetched value, or Err (false).
pub fn do_fetch(
    vm: &mut Vm,
    stmt: &StmtRef,
    how: i64,
    ori: i64,
    offset: i64,
    mut group_key: Option<&mut Value>,
) -> Result<Value, ()> {
    let mut how = how;
    if how == PDO_FETCH_USE_DEFAULT {
        how = stmt.borrow().default_fetch_type;
    }
    let flags = how & PDO_FETCH_FLAGS;
    how &= !PDO_FETCH_FLAGS;

    if !do_fetch_common(vm, stmt, ori, offset) {
        return Err(());
    }
    if how == PDO_FETCH_BOUND {
        return Ok(Value::True);
    }
    if how == PDO_FETCH_LAZY {
        return Ok(lazy_object(vm, stmt));
    }
    let column_count = stmt.borrow().column_count as i64;
    if how == PDO_FETCH_COLUMN {
        let fcol = stmt.borrow().fetch.column;
        let mut colno = fcol;
        if flags & (PDO_FETCH_GROUP | PDO_FETCH_UNIQUE) != 0 && fcol == -1 {
            colno = 1;
        }
        if colno < 0 {
            throw(
                vm,
                b"ValueError",
                "Column index must be greater than or equal to 0",
            );
            return Err(());
        }
        if colno >= column_count {
            throw(vm, b"ValueError", "Invalid column index");
            return Err(());
        }
        let rv = if flags == PDO_FETCH_GROUP && fcol == -1 {
            fetch_value(vm, stmt, 1, None)
        } else if flags == PDO_FETCH_GROUP && colno != 0 {
            fetch_value(vm, stmt, 0, None)
        } else {
            fetch_value(vm, stmt, colno, None)
        };
        if let Some(gk) = group_key.as_deref_mut() {
            let k = if flags == PDO_FETCH_GROUP && fcol > 0 {
                fetch_value(vm, stmt, colno, None)
            } else {
                fetch_value(vm, stmt, 0, None)
            };
            *gk = convert_to_string(vm, &k);
        }
        return Ok(rv);
    }

    let dbh = dbh_of_stmt(stmt);
    stmt.borrow_mut().in_fetch = true;
    let mut column_index: i64 = 0;
    let mut arr: Option<PhpArray> = None;
    let mut obj: Option<Obj> = None;
    // the property scope (ce), None for FETCH_INTO (global scope)
    let mut ce: Option<Vec<u8>> = None;
    let mut ctor_arguments: Option<Arr> = None;
    let mut func_params: Vec<Value> = Vec::new();

    macro_rules! fail {
        () => {{
            stmt.borrow_mut().in_fetch = false;
            return Err(());
        }};
    }

    match how {
        PDO_FETCH_USE_DEFAULT
        | PDO_FETCH_ASSOC
        | PDO_FETCH_BOTH
        | PDO_FETCH_NUM
        | PDO_FETCH_NAMED => {
            arr = Some(PhpArray::new());
        }
        PDO_FETCH_OBJ => {
            ce = Some(b"stdclass".to_vec());
            obj = object_init_ex(vm, b"stdclass");
            if obj.is_none() {
                fail!();
            }
        }
        PDO_FETCH_CLASS => {
            let mut c = stmt
                .borrow()
                .fetch
                .cls_ce
                .clone()
                .map(|n| n.to_ascii_lowercase());
            if flags & PDO_FETCH_CLASSTYPE != 0 {
                let name = fetch_value(vm, stmt, column_index, None);
                column_index += 1;
                if let Value::String(s) = &name {
                    c = lookup_class(vm, s.as_bytes());
                    if vm.current_exception.is_some() {
                        fail!();
                    }
                }
                if c.is_none() {
                    c = Some(b"stdclass".to_vec());
                }
            } else if c.is_none() {
                raise_impl_error(
                    vm,
                    &dbh,
                    Some(stmt),
                    "HY000",
                    Some("No fetch class specified"),
                );
                fail!();
            }
            let cl = c.unwrap();
            ctor_arguments = stmt.borrow().fetch.cls_ctor_args.clone();
            if flags & PDO_FETCH_SERIALIZE != 0 {
                if !has_unserialize(vm, &cl) {
                    raise_impl_error(
                        vm,
                        &dbh,
                        Some(stmt),
                        "HY000",
                        Some("cannot unserialize class"),
                    );
                    fail!();
                }
            } else {
                obj = object_init_ex(vm, &cl);
                let o = match &obj {
                    Some(o) => o.clone(),
                    None => fail!(),
                };
                if flags & PDO_FETCH_PROPS_LATE != 0
                    && constructor_of(vm, &cl).is_some()
                    && !call_constructor(vm, &o, ctor_arguments.as_ref())
                {
                    fail!();
                }
            }
            ce = Some(cl);
        }
        PDO_FETCH_INTO => {
            let into = stmt.borrow().fetch.into.clone();
            match into {
                Some(o) => obj = Some(o),
                None => {
                    raise_impl_error(
                        vm,
                        &dbh,
                        Some(stmt),
                        "HY000",
                        Some("No fetch-into object specified."),
                    );
                    fail!();
                }
            }
            ce = None;
        }
        PDO_FETCH_FUNC => {
            if stmt.borrow().fetch.func.is_none() {
                raise_impl_error(
                    vm,
                    &dbh,
                    Some(stmt),
                    "HY000",
                    Some("No fetch function specified"),
                );
                fail!();
            }
        }
        _ => fail!(),
    }

    if let Some(gk) = group_key {
        let k = fetch_value(vm, stmt, column_index, None);
        *gk = convert_to_string(vm, &k);
        column_index += 1;
    }

    if how == PDO_FETCH_CLASS && flags & PDO_FETCH_SERIALIZE != 0 {
        let data = fetch_value(vm, stmt, column_index, None);
        let s = match &data {
            Value::String(s) => s.clone(),
            _ => PhpString::from_bytes(b""),
        };
        let cl = ce.clone().unwrap();
        // zend_user_unserialize
        let o = match object_init_ex(vm, &cl) {
            Some(o) => o,
            None => fail!(),
        };
        let r = run_in_scope(
            vm,
            "$__o->unserialize($__d);",
            &[("__o", Value::Object(o.clone())), ("__d", Value::String(s))],
            b"",
        );
        if r.is_none() {
            raise_impl_error(
                vm,
                &dbh,
                Some(stmt),
                "HY000",
                Some("cannot unserialize class"),
            );
            fail!();
        }
        obj = Some(o);
        column_index += 1;
    }

    while column_index < column_count {
        let val = fetch_value(vm, stmt, column_index, None);
        let column_name = stmt
            .borrow()
            .columns
            .as_ref()
            .and_then(|c| c.get(column_index as usize))
            .map(|c| c.name().to_vec())
            .unwrap_or_default();
        match how {
            PDO_FETCH_ASSOC => {
                let a = arr.as_mut().unwrap();
                a.set(Vm::value_to_array_key(vstr(&column_name)), val);
            }
            PDO_FETCH_USE_DEFAULT | PDO_FETCH_BOTH => {
                let a = arr.as_mut().unwrap();
                a.set(Vm::value_to_array_key(vstr(&column_name)), val.clone());
                if a.get_int(column_index).is_none() {
                    a.set(ArrayKey::Int(column_index), val);
                }
            }
            PDO_FETCH_NAMED => {
                let a = arr.as_mut().unwrap();
                let key = ArrayKey::String(PhpString::from_bytes(&column_name));
                match a.get(&key).cloned() {
                    Some(cur) => {
                        let group = match cur {
                            Value::Array(g) => g,
                            other => {
                                let mut g = PhpArray::new();
                                g.push(other);
                                Rc::new(RefCell::new(g))
                            }
                        };
                        group.borrow_mut().push(val);
                        a.set(key, Value::Array(group));
                    }
                    None => a.set(key, val),
                }
            }
            PDO_FETCH_NUM => arr.as_mut().unwrap().push(val),
            PDO_FETCH_CLASS | PDO_FETCH_OBJ | PDO_FETCH_INTO => {
                let o = obj.clone().unwrap();
                if ce.as_deref() == Some(b"stdclass") {
                    o.borrow_mut().set_property(column_name, val);
                } else {
                    let scope = ce.clone().unwrap_or_default();
                    if !update_property(vm, &scope, &o, &column_name, val)
                        && vm.current_exception.is_some()
                    {
                        fail!();
                    }
                }
            }
            PDO_FETCH_FUNC => func_params.push(val),
            _ => {}
        }
        column_index += 1;
    }

    let mut result = match (arr, &obj) {
        (Some(a), _) => new_arr(a),
        (None, Some(o)) => Value::Object(o.clone()),
        _ => Value::Null,
    };
    if how == PDO_FETCH_CLASS && flags & (PDO_FETCH_PROPS_LATE | PDO_FETCH_SERIALIZE) == 0 {
        let cl = ce.clone().unwrap();
        if constructor_of(vm, &cl).is_some() {
            let o = obj.clone().unwrap();
            if !call_constructor(vm, &o, ctor_arguments.as_ref()) {
                fail!();
            }
        }
    } else if how == PDO_FETCH_FUNC {
        let func = stmt.borrow().fetch.func.clone().unwrap();
        // zend_call_known_fcc, through call_user_func's callable handling
        let cuf = vm.functions.get(b"call_user_func".as_slice()).copied();
        let mut cargs = vec![func.clone()];
        cargs.extend(func_params);
        // The callable was checked (and its deprecations raised) when the
        // fetch mode was set: the calls of the resolved function don't
        // raise them again.
        let relative = match &func {
            Value::Array(a) => a.borrow().get_int(1).is_some_and(|m| {
                let m = m.to_php_string().as_bytes().to_ascii_lowercase();
                m.starts_with(b"parent::") || m.starts_with(b"self::") || m.starts_with(b"static::")
            }),
            _ => false,
        };
        let saved_er = vm.error_reporting;
        if relative {
            vm.error_reporting &= !8192;
        }
        // (a by-ref function's reference is the row, zend_call_known_fcc())
        let r = vm.call_keeping_ref(|vm| match cuf {
            Some(cuf) => cuf(vm, &cargs),
            None => vm.call_callback(&func, &cargs[1..]),
        });
        if relative && vm.error_reporting == saved_er & !8192 {
            vm.error_reporting = saved_er;
        }
        // A callback that threw ends the fetch (do_fetch() returns false)
        if vm.current_exception.is_some() {
            fail!();
        }
        result = r.unwrap_or(Value::Null);
    }
    stmt.borrow_mut().in_fetch = false;
    Ok(result)
}

/// pdo_verify_fetch_mode
fn verify_fetch_mode(
    vm: &mut Vm,
    default_mode_and_flags: i64,
    mode_and_flags: i64,
    mode_arg_num: u32,
    fetch_all: bool,
) -> bool {
    if !(0..PDO_FIRST_INVALID_FLAG).contains(&mode_and_flags) {
        arg_error(
            vm,
            b"ValueError",
            mode_arg_num,
            "must be a bitmask of PDO::FETCH_* constants",
        );
        return false;
    }
    let mut flags = mode_and_flags & PDO_FETCH_FLAGS;
    let mut mode = mode_and_flags & !PDO_FETCH_FLAGS;
    if mode == PDO_FETCH_USE_DEFAULT {
        flags = default_mode_and_flags & PDO_FETCH_FLAGS;
        mode = default_mode_and_flags & !PDO_FETCH_FLAGS;
    }
    if flags != 0 {
        let has_class_flags =
            flags & (PDO_FETCH_CLASSTYPE | PDO_FETCH_PROPS_LATE | PDO_FETCH_SERIALIZE) != 0;
        if has_class_flags && mode != PDO_FETCH_CLASS {
            arg_error(
                vm,
                b"ValueError",
                mode_arg_num,
                "cannot use PDO::FETCH_CLASSTYPE, PDO::FETCH_PROPS_LATE, or PDO::FETCH_SERIALIZE fetch flags with a fetch mode other than PDO::FETCH_CLASS",
            );
            return false;
        }
        if flags & PDO_FETCH_SERIALIZE != 0 {
            docref_deprecated(vm, "The PDO::FETCH_SERIALIZE mode is deprecated");
            if vm.current_exception.is_some() {
                return false;
            }
        }
    }
    match mode {
        PDO_FETCH_FUNC => {
            if !fetch_all {
                arg_error(
                    vm,
                    b"ValueError",
                    mode_arg_num,
                    "PDO::FETCH_FUNC can only be used with PDOStatement::fetchAll()",
                );
                return false;
            }
            true
        }
        PDO_FETCH_LAZY => {
            if fetch_all {
                arg_error(
                    vm,
                    b"ValueError",
                    mode_arg_num,
                    "PDO::FETCH_LAZY cannot be used with PDOStatement::fetchAll()",
                );
                return false;
            }
            true
        }
        PDO_FETCH_INTO => {
            if fetch_all {
                arg_error(
                    vm,
                    b"ValueError",
                    mode_arg_num,
                    "PDO::FETCH_INTO cannot be used with PDOStatement::fetchAll()",
                );
                return false;
            }
            true
        }
        PDO_FETCH_ASSOC | PDO_FETCH_NUM | PDO_FETCH_BOTH | PDO_FETCH_OBJ | PDO_FETCH_BOUND
        | PDO_FETCH_COLUMN | PDO_FETCH_CLASS | PDO_FETCH_NAMED | PDO_FETCH_KEY_PAIR => true,
        _ => {
            arg_error(
                vm,
                b"ValueError",
                mode_arg_num,
                "must be a bitmask of PDO::FETCH_* constants",
            );
            false
        }
    }
}

/// PDOStatement::fetch
pub fn stmt_fetch(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "|lll", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let how = a[0].long_or(PDO_FETCH_USE_DEFAULT);
    let ori = a[1].long_or(PDO_FETCH_ORI_NEXT);
    let off = a[2].long_or(0);
    stmt.borrow_mut().clear_err();
    let def = stmt.borrow().default_fetch_type;
    if !verify_fetch_mode(vm, def, how, 1, false) {
        return Value::Null;
    }
    if how & !PDO_FETCH_FLAGS == PDO_FETCH_KEY_PAIR {
        let mut arr = PhpArray::new();
        if !do_key_pair_fetch(vm, &stmt, ori, off, &mut arr) {
            handle_stmt_err(vm, &stmt);
            return Value::False;
        }
        return new_arr(arr);
    }
    match do_fetch(vm, &stmt, how, ori, off, None) {
        Ok(v) => v,
        Err(()) => {
            handle_stmt_err(vm, &stmt);
            Value::False
        }
    }
}

/// zend_parse_arg_class (Z_PARAM_CLASS): the lowercase class, Err after
/// throwing.
fn parse_class_arg(vm: &mut Vm, f: &Func, argn: usize, name: &[u8]) -> Result<Vec<u8>, ()> {
    match lookup_class(vm, name) {
        Some(c) => Ok(c),
        None => {
            if vm.current_exception.is_none() {
                let m = format!(
                    "{} must be a valid class name, {} given",
                    f.arg_prefix(argn),
                    String::from_utf8_lossy(name)
                );
                throw(vm, b"TypeError", &m);
            }
            Err(())
        }
    }
}

/// PDOStatement::fetchObject
pub fn stmt_fetch_object(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "|C!a", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let ce = match &a[0] {
        Arg::Class(c) => match parse_class_arg(vm, f, 1, c) {
            Ok(c) => Some(c),
            Err(()) => return Value::Null,
        },
        _ => None,
    };
    let ctor_args = match &a[1] {
        Arg::Arr(x) => Some(x.clone()),
        _ => None,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    stmt.borrow_mut().clear_err();
    let saved = stmt.borrow().fetch.clone();
    let ce = ce.unwrap_or_else(|| b"stdclass".to_vec());
    if let Some(ca) = &ctor_args {
        if !ca.borrow().is_empty() && constructor_of(vm, &ce).is_none() {
            arg_error(
                vm,
                b"ValueError",
                2,
                "must be empty when class provided in argument #1 ($class) does not have a constructor",
            );
            return Value::Null;
        }
    }
    {
        let disp = class_display(vm, &ce);
        let mut s = stmt.borrow_mut();
        s.fetch.cls_ce = Some(disp);
        s.fetch.cls_ctor_args = ctor_args;
    }
    let r = match do_fetch(vm, &stmt, PDO_FETCH_CLASS, PDO_FETCH_ORI_NEXT, 0, None) {
        Ok(v) => v,
        Err(()) => {
            handle_stmt_err(vm, &stmt);
            Value::False
        }
    };
    {
        let mut s = stmt.borrow_mut();
        s.fetch.cls_ce = saved.cls_ce;
        s.fetch.cls_ctor_args = saved.cls_ctor_args;
    }
    r
}

/// PDOStatement::fetchColumn
pub fn stmt_fetch_column(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "|l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    stmt.borrow_mut().clear_err();
    if !do_fetch_common(vm, &stmt, PDO_FETCH_ORI_NEXT, 0) {
        handle_stmt_err(vm, &stmt);
        return Value::False;
    }
    fetch_value(vm, &stmt, a[0].long_or(0), None)
}

/// PDOStatement::fetchAll
pub fn stmt_fetch_all(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "|lz!a!", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let num_args = args
        .iter()
        .rposition(|v| !matches!(v, Value::Undef))
        .map(|p| p + 1)
        .unwrap_or(0);
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let mut how = a[0].long_or(PDO_FETCH_USE_DEFAULT);
    let arg2 = match &a[1] {
        Arg::Any(v) => Some(v.deref()),
        _ => None,
    };
    let ctor_args = match &a[2] {
        Arg::Arr(x) => Some(x.clone()),
        _ => None,
    };
    let def = stmt.borrow().default_fetch_type;
    if !verify_fetch_mode(vm, def, how, 1, true) {
        return Value::Null;
    }
    let mut fetch_mode = how & !PDO_FETCH_FLAGS;
    let mut flags = how & PDO_FETCH_FLAGS;
    let saved = stmt.borrow().fetch.clone();
    let fname = f.name.clone();

    match fetch_mode {
        PDO_FETCH_CLASS => {
            let fetch_class = match &arg2 {
                Some(v) => {
                    let name = match v {
                        Value::String(s) => s.as_bytes().to_vec(),
                        other => {
                            let m = format!(
                                "must be of type string, {} given",
                                Vm::value_type_name(other)
                            );
                            arg_error(vm, b"TypeError", 2, &m);
                            return Value::Null;
                        }
                    };
                    match lookup_class(vm, &name) {
                        Some(c) => c,
                        None => {
                            if vm.current_exception.is_none() {
                                arg_error(vm, b"TypeError", 2, "must be a valid class");
                            }
                            return Value::Null;
                        }
                    }
                }
                None => b"stdclass".to_vec(),
            };
            if let Some(ca) = &ctor_args {
                if !ca.borrow().is_empty() {
                    if constructor_of(vm, &fetch_class).is_none() {
                        arg_error(
                            vm,
                            b"ValueError",
                            3,
                            "must be empty when class provided in argument #2 ($class) does not have a constructor",
                        );
                        return Value::Null;
                    }
                    stmt.borrow_mut().fetch.cls_ctor_args = Some(ca.clone());
                }
            }
            let disp = class_display(vm, &fetch_class);
            stmt.borrow_mut().fetch.cls_ce = Some(disp);
        }
        PDO_FETCH_FUNC => {
            if num_args != 2 {
                let m = format!(
                    "{}() expects exactly 2 argument for PDO::FETCH_FUNC, {} given",
                    fname, num_args
                );
                count_error(vm, &m);
                return Value::Null;
            }
            let callable = match &arg2 {
                Some(c) => c.clone(),
                None => {
                    arg_error(vm, b"TypeError", 2, "must be a callable, null given");
                    return Value::Null;
                }
            };
            if let Some(err) = vm.callable_error(&callable) {
                if vm.current_exception.is_some() {
                    return Value::Null;
                }
                if err.is_empty() {
                    throw(
                        vm,
                        b"TypeError",
                        "User-supplied function must be a valid callback",
                    );
                } else {
                    throw(vm, b"TypeError", &err);
                }
                return Value::Null;
            }
            stmt.borrow_mut().fetch.func = Some(callable);
        }
        PDO_FETCH_COLUMN => {
            if num_args > 2 {
                let m = format!(
                    "{}() expects at most 2 argument for the fetch mode provided, {} given",
                    fname, num_args
                );
                count_error(vm, &m);
                return Value::Null;
            }
            match &arg2 {
                Some(v) => {
                    let n = match v {
                        Value::Long(n) => *n,
                        other => {
                            let m = format!(
                                "must be of type int, {} given",
                                Vm::value_type_name(other)
                            );
                            arg_error(vm, b"TypeError", 2, &m);
                            return Value::Null;
                        }
                    };
                    if n < 0 {
                        arg_error(vm, b"ValueError", 2, "must be greater than or equal to 0");
                        return Value::Null;
                    }
                    stmt.borrow_mut().fetch.column = n;
                }
                None => {
                    stmt.borrow_mut().fetch.column =
                        if flags & (PDO_FETCH_GROUP | PDO_FETCH_UNIQUE) != 0 {
                            -1
                        } else {
                            0
                        };
                }
            }
        }
        _ => {
            if num_args > 1 {
                let m = format!(
                    "{}() expects exactly 1 argument for the fetch mode provided, {} given",
                    fname, num_args
                );
                count_error(vm, &m);
                return Value::Null;
            }
        }
    }

    if fetch_mode == PDO_FETCH_USE_DEFAULT {
        flags |= def & PDO_FETCH_FLAGS;
        fetch_mode = def & !PDO_FETCH_FLAGS;
        how = fetch_mode | flags;
    }
    stmt.borrow_mut().clear_err();
    let mut ret = PhpArray::new();
    if fetch_mode == PDO_FETCH_KEY_PAIR {
        while do_key_pair_fetch(vm, &stmt, PDO_FETCH_ORI_NEXT, 0, &mut ret) {}
        handle_stmt_err(vm, &stmt);
        return new_arr(ret);
    }
    if flags & PDO_FETCH_GROUP != 0 || flags & PDO_FETCH_UNIQUE != 0 {
        loop {
            let mut gk = Value::Null;
            let data = match do_fetch(vm, &stmt, how | flags, PDO_FETCH_ORI_NEXT, 0, Some(&mut gk))
            {
                Ok(d) => d,
                Err(()) => break,
            };
            let key = Vm::value_to_array_key(gk);
            if flags & PDO_FETCH_UNIQUE == PDO_FETCH_UNIQUE {
                ret.set(key, data);
            } else {
                let group = match ret.get(&key) {
                    Some(Value::Array(g)) => g.clone(),
                    _ => {
                        let g = Rc::new(RefCell::new(PhpArray::new()));
                        ret.set(key.clone(), Value::Array(g.clone()));
                        g
                    }
                };
                group.borrow_mut().push(data);
            }
        }
    } else {
        while let Ok(d) = do_fetch(vm, &stmt, how, PDO_FETCH_ORI_NEXT, 0, None) {
            ret.push(d);
        }
    }
    stmt.borrow_mut().fetch = saved;
    if stmt.borrow().has_error() {
        handle_stmt_err(vm, &stmt);
        return new_arr(PhpArray::new());
    }
    new_arr(ret)
}

/// Z_PARAM_STR_OR_LONG for the parameter name / number: (name, paramno)
fn parse_str_or_long(vm: &mut Vm, f: &Func, v: &Value) -> Result<(Option<Vec<u8>>, i64), ()> {
    let v = v.deref();
    match &v {
        Value::String(s) => Ok((Some(s.as_bytes().to_vec()), 0)),
        Value::Long(n) => Ok((None, *n)),
        _ => {
            let strict = vm.caller_strict_types;
            let line = vm.current_line;
            if !strict {
                if matches!(v, Value::Null) {
                    let m = format!(
                        "{}(): Passing null to parameter #1 (${}) of type string|int is deprecated",
                        f.name,
                        f.params.first().copied().unwrap_or("param")
                    );
                    vm.emit_deprecated(&m);
                    if vm.current_exception.is_some() {
                        return Err(());
                    }
                    return Ok((None, 0));
                }
                if let Ok(Some(Value::Long(n))) =
                    vm.zpp_coerce_scalar(goro_core::arginfo::T_INT, &v, false, line)
                {
                    return Ok((None, n));
                }
                if vm.current_exception.is_some() {
                    return Err(());
                }
                if let Ok(Some(Value::String(s))) =
                    vm.zpp_coerce_scalar(goro_core::arginfo::T_STRING, &v, false, line)
                {
                    return Ok((Some(s.as_bytes().to_vec()), 0));
                }
                if vm.current_exception.is_some() {
                    return Err(());
                }
            }
            f.type_error(vm, 1, "string|int", &v);
            Err(())
        }
    }
}

/// register_bound_param (bindParam / bindColumn) and bindValue
fn register_bound_param(
    vm: &mut Vm,
    f: &Func,
    args: &[Value],
    is_param: bool,
    by_value: bool,
) -> Value {
    let spec = if by_value { "zz|l" } else { "zz|llz!" };
    let a = match parse(vm, f, spec, &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let first = match &a[0] {
        Arg::Any(v) => v.clone(),
        _ => Value::Null,
    };
    let (name, mut paramno) = match parse_str_or_long(vm, f, &first) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let parameter = match &a[1] {
        Arg::Any(v) => v.clone(),
        _ => Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let mut param = BoundParam::new();
    param.param_type = a[2].long_or(PDO_PARAM_STR) as i32 as i64;
    if !by_value {
        param.max_value_len = a[3].long_or(0);
        if let Arg::Any(d) = &a[4] {
            param.driver_params = d.clone();
        }
    }
    match name {
        Some(n) => {
            if n.is_empty() {
                f.must_not_be_empty(vm, 1);
                return Value::Null;
            }
            param.name = Some(n);
            paramno = -1;
        }
        None => {
            if paramno > 0 {
                paramno -= 1;
            } else {
                f.value_error(vm, 1, "must be greater than or equal to 1");
                return Value::Null;
            }
        }
    }
    param.paramno = paramno;
    param.parameter = if by_value {
        parameter.deref()
    } else {
        parameter
    };
    if !really_register_bound_param(vm, param, &stmt, is_param) {
        return Value::False;
    }
    Value::True
}

pub fn stmt_bind_value(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    register_bound_param(vm, f, args, true, true)
}

pub fn stmt_bind_param(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    register_bound_param(vm, f, args, true, false)
}

pub fn stmt_bind_column(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    register_bound_param(vm, f, args, false, false)
}

/// PDOStatement::rowCount
pub fn stmt_row_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    Value::Long(stmt.borrow().row_count)
}

/// PDOStatement::errorCode
pub fn stmt_error_code(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let s = stmt.borrow();
    if s.error_code.is_empty() {
        return Value::Null;
    }
    vstr(&s.error_code)
}

/// PDOStatement::errorInfo
pub fn stmt_error_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let mut arr = PhpArray::new();
    let code = stmt.borrow().error_code.clone();
    arr.push(vstr(&code));
    if code.as_slice() != PDO_ERR_NONE {
        let dbh = dbh_of_stmt(&stmt);
        let fe = dbh.borrow().methods.and_then(|m| m.fetch_err);
        if let Some(fe) = fe {
            fe(vm, &dbh, Some(&stmt), &mut arr);
        }
    }
    while arr.len() < 3 {
        arr.push(Value::Null);
    }
    new_arr(arr)
}

/// PDOStatement::setAttribute
pub fn stmt_set_attribute(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "lz!", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let dbh = dbh_of_stmt(&stmt);
    let sa = methods(&stmt).set_attribute;
    let sa = match sa {
        Some(s) => s,
        None => {
            raise_impl_error(
                vm,
                &dbh,
                Some(&stmt),
                "IM001",
                Some("This driver doesn't support setting attributes"),
            );
            return Value::False;
        }
    };
    stmt.borrow_mut().clear_err();
    let v = match &a[1] {
        Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };
    if sa(vm, &stmt, a[0].long_or(0), &v) {
        return Value::True;
    }
    handle_stmt_err(vm, &stmt);
    Value::False
}

/// generic_stmt_attr_get
fn generic_stmt_attr_get(stmt: &StmtRef, attr: i64) -> Option<Value> {
    match attr {
        PDO_ATTR_EMULATE_PREPARES => Some(vbool(
            stmt.borrow().supports_placeholders == PDO_PLACEHOLDER_NONE,
        )),
        _ => None,
    }
}

/// PDOStatement::getAttribute
pub fn stmt_get_attribute(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let attr = a[0].long_or(0);
    let dbh = dbh_of_stmt(&stmt);
    let ga = methods(&stmt).get_attribute;
    let ga = match ga {
        Some(g) => g,
        None => {
            return match generic_stmt_attr_get(&stmt, attr) {
                Some(v) => v,
                None => {
                    raise_impl_error(
                        vm,
                        &dbh,
                        Some(&stmt),
                        "IM001",
                        Some("This driver doesn't support getting attributes"),
                    );
                    Value::False
                }
            };
        }
    };
    stmt.borrow_mut().clear_err();
    match ga(vm, &stmt, attr) {
        AttrResult::Error => {
            handle_stmt_err(vm, &stmt);
            Value::False
        }
        AttrResult::Unsupported => match generic_stmt_attr_get(&stmt, attr) {
            Some(v) => v,
            None => {
                raise_impl_error(
                    vm,
                    &dbh,
                    Some(&stmt),
                    "IM001",
                    Some("driver doesn't support getting that attribute"),
                );
                Value::False
            }
        },
        AttrResult::Value(v) => v,
    }
}

/// PDOStatement::columnCount
pub fn stmt_column_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    Value::Long(stmt.borrow().column_count as i64)
}

/// PDOStatement::getColumnMeta
pub fn stmt_get_column_meta(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let colno = a[0].long_or(0);
    if colno < 0 {
        f.value_error(vm, 1, "must be greater than or equal to 0");
        return Value::Null;
    }
    let dbh = dbh_of_stmt(&stmt);
    let gm = match methods(&stmt).get_column_meta {
        Some(g) => g,
        None => {
            raise_impl_error(
                vm,
                &dbh,
                Some(&stmt),
                "IM001",
                Some("driver doesn't support meta data"),
            );
            return Value::False;
        }
    };
    stmt.borrow_mut().clear_err();
    let mut arr = match gm(vm, &stmt, colno) {
        Some(a) => a,
        None => {
            handle_stmt_err(vm, &stmt);
            return Value::False;
        }
    };
    let col = stmt
        .borrow()
        .columns
        .as_ref()
        .and_then(|c| c.get(colno as usize).cloned())
        .unwrap_or_default();
    let s = |k: &str| ArrayKey::String(PhpString::from_bytes(k.as_bytes()));
    arr.set(s("name"), vstr(col.name()));
    arr.set(s("len"), Value::Long(col.maxlen as i64));
    arr.set(s("precision"), Value::Long(col.precision as i64));
    new_arr(arr)
}

/// pdo_stmt_free_default_fetch_mode
fn free_default_fetch_mode(stmt: &StmtRef) {
    let old = std::mem::take(&mut stmt.borrow_mut().fetch);
    drop(old);
}

/// pdo_stmt_setup_fetch_mode
pub fn setup_fetch_mode(
    vm: &mut Vm,
    stmt: &StmtRef,
    mode: i64,
    mode_arg_num: u32,
    args: &[Value],
) -> bool {
    let arg1_arg_num = mode_arg_num + 1;
    let constructor_arg_num = mode_arg_num + 2;
    let variadic_num_args = args.len() as u32;
    let total_num_args = mode_arg_num + variadic_num_args;
    free_default_fetch_mode(stmt);
    stmt.borrow_mut().default_fetch_type = PDO_FETCH_BOTH;
    let flags = mode & PDO_FETCH_FLAGS;
    if !verify_fetch_mode(vm, PDO_FETCH_BOTH, mode, mode_arg_num, false) {
        return false;
    }
    let fname = active_function_name(vm);
    match mode & !PDO_FETCH_FLAGS {
        PDO_FETCH_USE_DEFAULT
        | PDO_FETCH_LAZY
        | PDO_FETCH_ASSOC
        | PDO_FETCH_NUM
        | PDO_FETCH_BOTH
        | PDO_FETCH_OBJ
        | PDO_FETCH_BOUND
        | PDO_FETCH_NAMED
        | PDO_FETCH_KEY_PAIR => {
            if variadic_num_args != 0 {
                let m = format!(
                    "{}() expects exactly {} arguments for the fetch mode provided, {} given",
                    fname, mode_arg_num, total_num_args
                );
                count_error(vm, &m);
                return false;
            }
        }
        PDO_FETCH_COLUMN => {
            if variadic_num_args != 1 {
                let m = format!(
                    "{}() expects exactly {} arguments for the fetch mode provided, {} given",
                    fname, arg1_arg_num, total_num_args
                );
                count_error(vm, &m);
                return false;
            }
            let n = match &args[0] {
                Value::Long(n) => *n,
                other => {
                    let m = format!("must be of type int, {} given", Vm::value_type_name(other));
                    arg_error(vm, b"TypeError", arg1_arg_num, &m);
                    return false;
                }
            };
            if n < 0 {
                arg_error(
                    vm,
                    b"ValueError",
                    arg1_arg_num,
                    "must be greater than or equal to 0",
                );
                return false;
            }
            stmt.borrow_mut().fetch.column = n;
        }
        PDO_FETCH_CLASS => {
            if flags & PDO_FETCH_CLASSTYPE == PDO_FETCH_CLASSTYPE {
                if variadic_num_args != 0 {
                    let m = format!(
                        "{}() expects exactly {} arguments for the fetch mode provided, {} given",
                        fname, mode_arg_num, total_num_args
                    );
                    count_error(vm, &m);
                    return false;
                }
            } else {
                if variadic_num_args == 0 {
                    let m = format!(
                        "{}() expects at least {} arguments for the fetch mode provided, {} given",
                        fname, arg1_arg_num, total_num_args
                    );
                    count_error(vm, &m);
                    return false;
                }
                if variadic_num_args > 2 {
                    let m = format!(
                        "{}() expects at most {} arguments for the fetch mode provided, {} given",
                        fname, constructor_arg_num, total_num_args
                    );
                    count_error(vm, &m);
                    return false;
                }
                let name = match &args[0] {
                    Value::String(s) => s.as_bytes().to_vec(),
                    other => {
                        let m = format!(
                            "must be of type string, {} given",
                            Vm::value_type_name(other)
                        );
                        arg_error(vm, b"TypeError", arg1_arg_num, &m);
                        return false;
                    }
                };
                let cep = match lookup_class(vm, &name) {
                    Some(c) => c,
                    None => {
                        if vm.current_exception.is_none() {
                            arg_error(vm, b"TypeError", arg1_arg_num, "must be a valid class");
                        }
                        return false;
                    }
                };
                if variadic_num_args == 2 {
                    match &args[1] {
                        Value::Null => {}
                        Value::Array(a) => {
                            if !a.borrow().is_empty() {
                                if constructor_of(vm, &cep).is_none() {
                                    arg_error(
                                        vm,
                                        b"ValueError",
                                        3,
                                        "must be empty when class provided in argument #2 ($class) does not have a constructor",
                                    );
                                    return false;
                                }
                                stmt.borrow_mut().fetch.cls_ctor_args = Some(a.clone());
                            }
                        }
                        other => {
                            let m = format!(
                                "must be of type ?array, {} given",
                                Vm::value_type_name(other)
                            );
                            arg_error(vm, b"TypeError", constructor_arg_num, &m);
                            return false;
                        }
                    }
                }
                let disp = class_display(vm, &cep);
                stmt.borrow_mut().fetch.cls_ce = Some(disp);
            }
        }
        PDO_FETCH_INTO => {
            if total_num_args != arg1_arg_num {
                let m = format!(
                    "{}() expects exactly {} arguments for the fetch mode provided, {} given",
                    fname, arg1_arg_num, total_num_args
                );
                count_error(vm, &m);
                return false;
            }
            match &args[0] {
                Value::Object(o) => stmt.borrow_mut().fetch.into = Some(o.clone()),
                other => {
                    let m = format!(
                        "must be of type object, {} given",
                        Vm::value_type_name(other)
                    );
                    arg_error(vm, b"TypeError", arg1_arg_num, &m);
                    return false;
                }
            }
        }
        _ => {
            arg_error(
                vm,
                b"ValueError",
                mode_arg_num,
                "must be one of the PDO::FETCH_* constants",
            );
            return false;
        }
    }
    stmt.borrow_mut().default_fetch_type = mode;
    true
}

/// PDOStatement::setFetchMode
pub fn stmt_set_fetch_mode(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "l*", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    if stmt.borrow().in_fetch {
        throw(
            vm,
            b"Error",
            "Cannot change default fetch mode while fetching",
        );
        return Value::Null;
    }
    let rest: Vec<Value> = match &a[1] {
        Arg::Rest(r) => r.iter().map(|v| v.deref()).collect(),
        _ => Vec::new(),
    };
    if !setup_fetch_mode(vm, &stmt, a[0].long_or(0), 1, &rest) {
        return Value::Null;
    }
    Value::True
}

/// pdo_stmt_do_next_rowset
fn do_next_rowset(vm: &mut Vm, stmt: &StmtRef) -> bool {
    reset_columns(stmt);
    let nr = methods(stmt).next_rowset.unwrap();
    let ok = nr(vm, stmt);
    flush_mysqlnd(vm);
    if !ok {
        stmt.borrow_mut().executed = false;
        return false;
    }
    describe_columns(vm, stmt);
    true
}

/// PDOStatement::nextRowset
pub fn stmt_next_rowset(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    if methods(&stmt).next_rowset.is_none() {
        let dbh = dbh_of_stmt(&stmt);
        raise_impl_error(
            vm,
            &dbh,
            Some(&stmt),
            "IM001",
            Some("driver does not support multiple rowsets"),
        );
        return Value::False;
    }
    stmt.borrow_mut().clear_err();
    if !do_next_rowset(vm, &stmt) {
        handle_stmt_err(vm, &stmt);
        return Value::False;
    }
    Value::True
}

/// PDOStatement::closeCursor
pub fn stmt_close_cursor(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let m = methods(&stmt);
    let closer = match m.cursor_closer {
        Some(c) => c,
        None => {
            loop {
                while (m.fetcher)(vm, &stmt, PDO_FETCH_ORI_NEXT, 0) {}
                if m.next_rowset.is_none() {
                    break;
                }
                if !do_next_rowset(vm, &stmt) {
                    break;
                }
            }
            stmt.borrow_mut().executed = false;
            return Value::True;
        }
    };
    stmt.borrow_mut().clear_err();
    let ok = closer(vm, &stmt);
    flush_mysqlnd(vm);
    if !ok {
        handle_stmt_err(vm, &stmt);
        return Value::False;
    }
    stmt.borrow_mut().executed = false;
    Value::True
}

/// PDOStatement::debugDumpParams
pub fn stmt_debug_dump_params(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (_, stmt) = match this_stmt(vm, f) {
        Some(x) => x,
        None => return Value::Null,
    };
    let mut out: Vec<u8> = Vec::new();
    let s = stmt.borrow();
    let q = s.query();
    out.extend_from_slice(format!("SQL: [{}] ", q.len()).as_bytes());
    out.extend_from_slice(q);
    out.push(b'\n');
    if let Some(a) = &s.active_query_string {
        if !s.active_is_query {
            out.extend_from_slice(format!("Sent SQL: [{}] ", a.len()).as_bytes());
            out.extend_from_slice(a);
            out.push(b'\n');
        }
    }
    out.extend_from_slice(
        format!(
            "Params:  {}\n",
            s.bound_params.as_ref().map(|p| p.len()).unwrap_or(0)
        )
        .as_bytes(),
    );
    if let Some(bp) = &s.bound_params {
        for (k, p) in &bp.entries {
            match k {
                HKey::Str(key) => {
                    out.extend_from_slice(format!("Key: Name: [{}] ", key.len()).as_bytes());
                    out.extend_from_slice(key);
                    out.push(b'\n');
                }
                HKey::Int(n) => {
                    out.extend_from_slice(format!("Key: Position #{}:\n", *n as u64).as_bytes())
                }
            }
            let name = p.name.clone().unwrap_or_default();
            out.extend_from_slice(
                format!("paramno={}\nname=[{}] \"", p.paramno, name.len()).as_bytes(),
            );
            out.extend_from_slice(&name);
            out.extend_from_slice(
                format!(
                    "\"\nis_param={}\nparam_type={}\n",
                    p.is_param as i32, p.param_type as i32
                )
                .as_bytes(),
            );
        }
    }
    drop(s);
    vm.write_output(&out);
    Value::Null
}

// ── iteration ─────────────────────────────────────────────────────────

/// PDOStatement::getIterator (pdo_stmt_iter_get)
pub fn stmt_get_iterator(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let o = match &f.this {
        Some(o) => o.clone(),
        None => return Value::Null,
    };
    new_iterator(vm, &o).unwrap_or(Value::Null)
}

/// zend_create_internal_iterator_zval over pdo_stmt_iter_get
pub fn new_iterator(vm: &mut Vm, o: &Obj) -> Option<Value> {
    let stmt = stmt_of(o)?;
    if stmt.borrow().dbh.is_none() {
        let m = format!(
            "{} object is uninitialized",
            goro_core::value::display_class_name(&o.borrow().class_name)
        );
        throw(vm, b"Error", &m);
        return None;
    }
    // the iterator takes its object handle before the first fetch; under
    // foreach (ce->get_iterator) it is the zend_object_iterator itself, so
    // the VM must not take a second handle for it
    if vm.foreach_native_iter == 1 {
        vm.foreach_native_iter = 2;
    }
    let id = vm.alloc_object_id();
    let it = StmtIter {
        stmt_obj: o.clone(),
        stmt: stmt.clone(),
        key: Cell::new(Some(0)),
        fetch_ahead: RefCell::new(Value::Undef),
    };
    match do_fetch(
        vm,
        &stmt,
        PDO_FETCH_USE_DEFAULT,
        PDO_FETCH_ORI_NEXT,
        0,
        None,
    ) {
        Ok(v) => *it.fetch_ahead.borrow_mut() = v,
        Err(()) => {
            handle_stmt_err(vm, &stmt);
            it.key.set(None);
        }
    }
    let mut obj = goro_core::object::PhpObject::new(b"InternalIterator".to_vec(), id);
    obj.native = Some(goro_core::object::NativeData {
        data: Rc::new(it) as Rc<dyn std::any::Any>,
        handlers: &ITER_HANDLERS,
    });
    Some(Value::Object(Rc::new(RefCell::new(obj))))
}

/// pdo_stmt_iter_move_forwards
fn iter_move_forwards(vm: &mut Vm, it: &StmtIter) {
    *it.fetch_ahead.borrow_mut() = Value::Undef;
    match do_fetch(
        vm,
        &it.stmt,
        PDO_FETCH_USE_DEFAULT,
        PDO_FETCH_ORI_NEXT,
        0,
        None,
    ) {
        Ok(v) => {
            *it.fetch_ahead.borrow_mut() = v;
            it.key
                .set(it.key.get().map(|k| k.wrapping_add(1)).or(Some(0)));
        }
        Err(()) => {
            handle_stmt_err(vm, &it.stmt);
            it.key.set(None);
        }
    }
}

/// The InternalIterator methods over a statement iterator
pub fn iter_call(vm: &mut Vm, o: &Obj, method: &[u8]) -> Option<Value> {
    let it = iter_of(o)?;
    Some(match method {
        b"rewind" => Value::Null,
        b"valid" => vbool(!matches!(*it.fetch_ahead.borrow(), Value::Undef)),
        b"current" => match &*it.fetch_ahead.borrow() {
            Value::Undef => Value::Null,
            v => v.clone(),
        },
        b"key" => match it.key.get() {
            Some(k) => Value::Long(k as i64),
            None => Value::Null,
        },
        b"next" => {
            iter_move_forwards(vm, &it);
            Value::Null
        }
        _ => return None,
    })
}
