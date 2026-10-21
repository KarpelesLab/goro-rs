//! Dom\Document, Dom\XMLDocument, Dom\HTMLDocument (creation only),
//! Dom\Implementation (document.c, xml_document.c, domimplementation.c,
//! html_document.c property handlers).

use super::*;
use crate::dom::node::str_of;
use crate::parser::*;

fn doc_node(vm: &mut Vm, o: &Obj) -> R<NodeId> {
    need_node(vm, o)
}

// ---------------------------------------------------------------------------
// Properties

fn r_implementation(vm: &mut Vm, o: &Obj, _n: NodeId) -> R<Value> {
    let c = cached_obj(o, "implementation", false, || {
        let i = new_object(vm, b"Dom\\Implementation");
        attach_with(&i, Kind::Implementation, &UNCLONEABLE_HANDLERS);
        i
    });
    Ok(Value::Object(c))
}

fn r_url(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        match t.doc_info(n).and_then(|d| d.url.clone()) {
            Some(u) => s(&u),
            None => s("about:blank"),
        }
    }))
}

fn w_url(_vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        if let Some(di) = t.doc_info_mut(n) {
            di.url = Some(st);
        }
    });
    Ok(())
}

fn r_encoding(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        opt_s(t.doc_info(n).and_then(|d| d.encoding.as_deref()))
    }))
}

/// WHATWG encoding labels (lxb_encoding_data_by_name) -> canonical names.
pub fn whatwg_encoding(label: &str) -> Option<&'static str> {
    let l = label
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_ascii_lowercase();
    Some(match l.as_str() {
        "unicode-1-1-utf-8" | "unicode11utf8" | "unicode20utf8" | "utf-8" | "utf8"
        | "x-unicode20utf8" => "UTF-8",
        "866" | "cp866" | "csibm866" | "ibm866" => "IBM866",
        "csisolatin2" | "iso-8859-2" | "iso-ir-101" | "iso8859-2" | "iso88592" | "iso_8859-2"
        | "iso_8859-2:1987" | "l2" | "latin2" => "ISO-8859-2",
        "csisolatin3" | "iso-8859-3" | "iso-ir-109" | "iso8859-3" | "iso88593" | "iso_8859-3"
        | "iso_8859-3:1988" | "l3" | "latin3" => "ISO-8859-3",
        "csisolatin4" | "iso-8859-4" | "iso-ir-110" | "iso8859-4" | "iso88594" | "iso_8859-4"
        | "iso_8859-4:1988" | "l4" | "latin4" => "ISO-8859-4",
        "csisolatincyrillic" | "cyrillic" | "iso-8859-5" | "iso-ir-144" | "iso8859-5"
        | "iso88595" | "iso_8859-5" | "iso_8859-5:1988" => "ISO-8859-5",
        "arabic" | "asmo-708" | "csiso88596e" | "csiso88596i" | "csisolatinarabic" | "ecma-114"
        | "iso-8859-6" | "iso-8859-6-e" | "iso-8859-6-i" | "iso-ir-127" | "iso8859-6"
        | "iso88596" | "iso_8859-6" | "iso_8859-6:1987" => "ISO-8859-6",
        "csisolatingreek" | "ecma-118" | "elot_928" | "greek" | "greek8" | "iso-8859-7"
        | "iso-ir-126" | "iso8859-7" | "iso88597" | "iso_8859-7" | "iso_8859-7:1987"
        | "sun_eu_greek" => "ISO-8859-7",
        "csiso88598e" | "csisolatinhebrew" | "hebrew" | "iso-8859-8" | "iso-8859-8-e"
        | "iso-ir-138" | "iso8859-8" | "iso88598" | "iso_8859-8" | "iso_8859-8:1988" | "visual" => {
            "ISO-8859-8"
        }
        "csiso88598i" | "iso-8859-8-i" | "logical" => "ISO-8859-8-I",
        "csisolatin6" | "iso-8859-10" | "iso-ir-157" | "iso8859-10" | "iso885910" | "l6"
        | "latin6" => "ISO-8859-10",
        "iso-8859-13" | "iso8859-13" | "iso885913" => "ISO-8859-13",
        "iso-8859-14" | "iso8859-14" | "iso885914" => "ISO-8859-14",
        "csisolatin9" | "iso-8859-15" | "iso8859-15" | "iso885915" | "iso_8859-15" | "l9" => {
            "ISO-8859-15"
        }
        "iso-8859-16" => "ISO-8859-16",
        "cskoi8r" | "koi" | "koi8" | "koi8-r" | "koi8_r" => "KOI8-R",
        "koi8-ru" | "koi8-u" => "KOI8-U",
        "csmacintosh" | "mac" | "macintosh" | "x-mac-roman" => "macintosh",
        "dos-874" | "iso-8859-11" | "iso8859-11" | "iso885911" | "tis-620" | "windows-874" => {
            "windows-874"
        }
        "cp1250" | "windows-1250" | "x-cp1250" => "windows-1250",
        "cp1251" | "windows-1251" | "x-cp1251" => "windows-1251",
        "ansi_x3.4-1968" | "ascii" | "cp1252" | "cp819" | "csisolatin1" | "ibm819"
        | "iso-8859-1" | "iso-ir-100" | "iso8859-1" | "iso88591" | "iso_8859-1"
        | "iso_8859-1:1987" | "l1" | "latin1" | "us-ascii" | "windows-1252" | "x-cp1252" => {
            "windows-1252"
        }
        "cp1253" | "windows-1253" | "x-cp1253" => "windows-1253",
        "cp1254" | "csisolatin5" | "iso-8859-9" | "iso-ir-148" | "iso8859-9" | "iso88599"
        | "iso_8859-9" | "iso_8859-9:1989" | "l5" | "latin5" | "windows-1254" | "x-cp1254" => {
            "windows-1254"
        }
        "cp1255" | "windows-1255" | "x-cp1255" => "windows-1255",
        "cp1256" | "windows-1256" | "x-cp1256" => "windows-1256",
        "cp1257" | "windows-1257" | "x-cp1257" => "windows-1257",
        "cp1258" | "windows-1258" | "x-cp1258" => "windows-1258",
        "x-mac-cyrillic" | "x-mac-ukrainian" => "x-mac-cyrillic",
        "chinese" | "csgb2312" | "csiso58gb231280" | "gb2312" | "gb_2312" | "gb_2312-80"
        | "gbk" | "iso-ir-58" | "x-gbk" => "GBK",
        "gb18030" => "gb18030",
        "big5" | "big5-hkscs" | "cn-big5" | "csbig5" | "x-x-big5" => "Big5",
        "cseucpkdfmtjapanese" | "euc-jp" | "x-euc-jp" => "EUC-JP",
        "csiso2022jp" | "iso-2022-jp" => "ISO-2022-JP",
        "csshiftjis" | "ms932" | "ms_kanji" | "shift-jis" | "shift_jis" | "sjis"
        | "windows-31j" | "x-sjis" => "Shift_JIS",
        "cseuckr" | "csksc56011987" | "euc-kr" | "iso-ir-149" | "korean" | "ks_c_5601-1987"
        | "ks_c_5601-1989" | "ksc5601" | "ksc_5601" | "windows-949" => "EUC-KR",
        "unicodefffe" | "utf-16be" => "UTF-16BE",
        "csunicode" | "iso-10646-ucs-2" | "ucs-2" | "unicode" | "unicodefeff" | "utf-16"
        | "utf-16le" => "UTF-16LE",
        "x-user-defined" => "x-user-defined",
        _ => return None,
    })
}

fn w_encoding(vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    match whatwg_encoding(&st) {
        Some(e) => {
            tree::with(|t| {
                if let Some(di) = t.doc_info_mut(n) {
                    di.encoding = Some(e.to_string());
                }
            });
            Ok(())
        }
        None => {
            throw_error(vm, b"ValueError", "Invalid document encoding");
            Err(())
        }
    }
}

/// dom_html_document_element_read_raw
fn html_child_of_root(t: &Tree, d: NodeId, accept: &dyn Fn(&str) -> bool) -> Option<NodeId> {
    let root = t.root_element(d)?;
    if !(ns_is(t, t.nodes[root].ns, XHTML_NS) && t.nodes[root].name == "html") {
        return None;
    }
    let mut c = t.nodes[root].children;
    while let Some(x) = c {
        if t.nodes[x].t == NType::Element
            && ns_is(t, t.nodes[x].ns, XHTML_NS)
            && accept(&t.nodes[x].name)
        {
            return Some(x);
        }
        c = t.nodes[x].next;
    }
    None
}

fn accept_body(n: &str) -> bool {
    n == "body" || n == "frameset"
}

fn r_body(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let b = tree::with(|t| html_child_of_root(t, n, &accept_body));
    Ok(node_obj_opt(vm, b))
}

fn r_head(vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    let b = tree::with(|t| html_child_of_root(t, n, &|x| x == "head"));
    Ok(node_obj_opt(vm, b))
}

fn w_body(vm: &mut Vm, _o: &Obj, d: NodeId, v: &Value) -> R<()> {
    if let Value::Object(no) = v {
        if let Some(nn) = node_of(no) {
            let is_body = tree::with(|t| accept_body(&t.nodes[nn].name));
            if is_body {
                let cur = tree::with(|t| html_child_of_root(t, d, &accept_body));
                if cur == Some(nn) {
                    return Ok(());
                }
                if let Some(c) = cur {
                    tree::with(|t| {
                        adopt_into(t, nn, d);
                        t.unlink(nn);
                        t.insert_before_raw(c, nn);
                        t.unlink(c);
                    });
                    return Ok(());
                }
                let root = tree::with(|t| t.root_element(d));
                match root {
                    None => {
                        dom_err_msg(
                            vm,
                            HIERARCHY_REQUEST_ERR,
                            "A body can only be set if there is a document element",
                        );
                        return Err(());
                    }
                    Some(r) => {
                        tree::with(|t| {
                            adopt_into(t, nn, d);
                            t.unlink(nn);
                            t.link_child_raw(r, nn);
                        });
                        return Ok(());
                    }
                }
            }
        }
    }
    dom_err_msg(
        vm,
        HIERARCHY_REQUEST_ERR,
        "The new body must either be a body or a frameset tag",
    );
    Err(())
}

fn child_text_content(t: &Tree, n: NodeId) -> String {
    let mut out = String::new();
    let mut c = t.nodes[n].children;
    while let Some(x) = c {
        if matches!(t.nodes[x].t, NType::Text | NType::CData) {
            out.push_str(&t.nodes[x].content);
        }
        c = t.nodes[x].next;
    }
    out
}

fn title_element(t: &Tree, d: NodeId) -> Option<NodeId> {
    let mut cur = t.nodes[d].children;
    while let Some(x) = cur {
        if t.nodes[x].t == NType::Element
            && ns_is(t, t.nodes[x].ns, XHTML_NS)
            && t.nodes[x].name == "title"
        {
            return Some(x);
        }
        cur = next_in_tree_order(t, x, None);
    }
    None
}

fn svg_title_element(t: &Tree, svg: NodeId) -> Option<NodeId> {
    let mut c = t.nodes[svg].children;
    while let Some(x) = c {
        if t.nodes[x].t == NType::Element
            && ns_is(t, t.nodes[x].ns, SVG_NS)
            && t.nodes[x].name == "title"
        {
            return Some(x);
        }
        c = t.nodes[x].next;
    }
    None
}

/// dom_strip_and_collapse_ascii_whitespace
pub fn strip_and_collapse(s_: &str) -> String {
    s_.split(['\t', '\n', '\x0C', '\r', ' '])
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn r_title(_vm: &mut Vm, _o: &Obj, d: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        let root = match t.root_element(d) {
            Some(r) => r,
            None => return s(""),
        };
        let value = if ns_is(t, t.nodes[root].ns, SVG_NS) && t.nodes[root].name == "svg" {
            svg_title_element(t, root)
                .map(|x| child_text_content(t, x))
                .unwrap_or_default()
        } else {
            title_element(t, d)
                .map(|x| child_text_content(t, x))
                .unwrap_or_default()
        };
        s(&strip_and_collapse(&value))
    }))
}

fn string_replace_all(t: &mut Tree, d: NodeId, el: NodeId, v: &str) {
    crate::dom::node::remove_all_children(t, el);
    let tx = t.new_text(Some(d), v);
    t.add_child(el, tx);
}

fn w_title(_vm: &mut Vm, _o: &Obj, d: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    tree::with(|t| {
        let root = match t.root_element(d) {
            Some(r) => r,
            None => return,
        };
        if ns_is(t, t.nodes[root].ns, SVG_NS) && t.nodes[root].name == "svg" {
            let el = match svg_title_element(t, root) {
                Some(e) => e,
                None => {
                    let mut ns = t.nodes[root].ns;
                    if ns.map(|x| t.nss[x].prefix.is_some()).unwrap_or(false) {
                        ns = mapper_get_ns(t, d, None, Some(SVG_NS));
                    }
                    let e = t.new_element(Some(d), "title");
                    t.nodes[e].ns = ns;
                    match t.nodes[root].children {
                        Some(f) => t.insert_before_raw(f, e),
                        None => t.link_child_raw(root, e),
                    }
                    t.nodes[e].parent = Some(root);
                    e
                }
            };
            string_replace_all(t, d, el, &st);
        } else if ns_is(t, t.nodes[root].ns, XHTML_NS) {
            let title = title_element(t, d);
            let head = html_child_of_root(t, d, &|x| x == "head");
            if title.is_none() && head.is_none() {
                return;
            }
            let el = match title {
                Some(e) => e,
                None => {
                    let ns = mapper_get_ns(t, d, None, Some(XHTML_NS));
                    let e = t.new_element(Some(d), "title");
                    t.nodes[e].ns = ns;
                    t.add_child(head.unwrap(), e);
                    e
                }
            };
            string_replace_all(t, d, el, &st);
        }
    });
    Ok(())
}

pub fn document_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::Document, n);
        p.owner = "";
        p
    };
    let mut v = vec![
        p("implementation", r_implementation, None, "mixed"),
        p("URL", r_url, Some(w_url), "string"),
        p("documentURI", r_url, Some(w_url), "string"),
        p("characterSet", r_encoding, Some(w_encoding), "string"),
        p("charset", r_encoding, Some(w_encoding), "string"),
        p("inputEncoding", r_encoding, Some(w_encoding), "string"),
        l("doctype"),
        l("documentElement"),
    ];
    v.extend(node::parent_props(t));
    v.push(p("body", r_body, Some(w_body), "mixed"));
    v.push(p("head", r_head, None, "mixed"));
    v.push(p("title", r_title, Some(w_title), "string"));
    v
}

fn w_version(vm: &mut Vm, _o: &Obj, n: NodeId, v: &Value) -> R<()> {
    let st = str_of(v);
    if st != "1.0" && st != "1.1" {
        throw_error(vm, b"ValueError", "Invalid XML version");
        return Err(());
    }
    tree::with(|t| {
        if let Some(di) = t.doc_info_mut(n) {
            di.version = Some(st);
        }
    });
    Ok(())
}

fn r_version(_vm: &mut Vm, _o: &Obj, n: NodeId) -> R<Value> {
    Ok(tree::with(|t| {
        opt_s(t.doc_info(n).and_then(|d| d.version.as_deref()))
    }))
}

pub fn xml_document_props(t: &HashMap<Base, Rc<Vec<Prop>>>) -> Vec<Prop> {
    let l = |n: &str| {
        let mut p = lp(t, Base::Document, n);
        p.owner = "";
        p
    };
    vec![
        p("xmlEncoding", r_encoding, None, "string"),
        l("xmlStandalone"),
        p("xmlVersion", r_version, Some(w_version), "string"),
        l("formatOutput"),
    ]
}

// ---------------------------------------------------------------------------
// Creation

const VALID_OPTIONS: i64 = XML_PARSE_RECOVER
    | XML_PARSE_NOENT
    | XML_PARSE_NO_XXE
    | XML_PARSE_DTDLOAD
    | XML_PARSE_DTDATTR
    | XML_PARSE_DTDVALID
    | XML_PARSE_NOERROR
    | XML_PARSE_NOWARNING
    | XML_PARSE_NOBLANKS
    | XML_PARSE_XINCLUDE
    | XML_PARSE_NSCLEAN
    | XML_PARSE_NOCDATA
    | XML_PARSE_NONET
    | XML_PARSE_PEDANTIC
    | XML_PARSE_COMPACT
    | XML_PARSE_HUGE
    | XML_PARSE_BIG_LINES;

/// A new modern document object for `d`.
pub fn modern_doc_obj(vm: &mut Vm, d: NodeId) -> Value {
    set_class_type(d, 2);
    node_obj(vm, d)
}

pub fn xml_create_empty(vm: &mut Vm, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 2)?;
    let version = if a.raw(0).is_some() {
        a.str(vm, 0, "version")?
    } else {
        "1.0".to_string()
    };
    let encoding = if a.raw(1).is_some() {
        a.path_str(vm, 1, "encoding")?
    } else {
        "UTF-8".to_string()
    };
    if !encoding_supported(&encoding) {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($encoding) is not a valid document encoding",
                a.f
            ),
        );
        return Err(());
    }
    let d = tree::with(|t| {
        let d = t.new_doc(Some(crate::dom::node::cstr(&version)));
        if let Some(di) = t.doc_info_mut(d) {
            di.encoding = Some(encoding.clone());
        }
        d
    });
    mapper_reset(d);
    Ok(modern_doc_obj(vm, d))
}

pub fn html_create_empty(vm: &mut Vm, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let encoding = if a.raw(0).is_some() {
        a.path_str(vm, 0, "encoding")?
    } else {
        "UTF-8".to_string()
    };
    if whatwg_encoding(&encoding).is_none() {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($encoding) must be a valid document encoding",
                a.f
            ),
        );
        return Err(());
    }
    let d = tree::with(|t| {
        let d = t.new_html_doc();
        if let Some(di) = t.doc_info_mut(d) {
            // htmlNewDocNoDtD marks the document standalone
            di.standalone = 1;
            di.encoding = Some(encoding.clone());
        }
        d
    });
    mapper_reset(d);
    Ok(modern_doc_obj(vm, d))
}

/// Dom\XMLDocument::createFromString / createFromFile (load_from_helper)
pub fn xml_load(vm: &mut Vm, a: &[Value], file: bool) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 3)?;
    let argname = if file { "path" } else { "source" };
    let source = a.str_bytes(vm, 0, argname)?;
    let mut options = a.long(vm, 1, "options", 0)?;
    let override_enc = a.opt_path_str(vm, 2, "overrideEncoding")?;
    if source.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 (${}) must not be empty", a.f, argname),
        );
        return Err(());
    }
    if file && source.windows(3).any(|w| w == b"%00") {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 (${}) must not contain percent-encoded NUL bytes",
                a.f, argname
            ),
        );
        return Err(());
    }
    if options & !VALID_OPTIONS != 0 {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #2 ($options) contains invalid flags (allowed flags: LIBXML_RECOVER, LIBXML_NOENT, LIBXML_NO_XXE, LIBXML_DTDLOAD, LIBXML_DTDATTR, LIBXML_DTDVALID, LIBXML_NOERROR, LIBXML_NOWARNING, LIBXML_NOBLANKS, LIBXML_XINCLUDE, LIBXML_NSCLEAN, LIBXML_NOCDATA, LIBXML_NONET, LIBXML_PEDANTIC, LIBXML_COMPACT, LIBXML_PARSEHUGE, LIBXML_BIGLINES)",
                a.f
            ),
        );
        return Err(());
    }
    if let Some(e) = &override_enc {
        if !encoding_supported(e) {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #3 ($overrideEncoding) must be a valid document encoding",
                    a.f
                ),
            );
            return Err(());
        }
        options |= XML_PARSE_IGNORE_ENC;
    }
    let fname = a.f.clone();
    let recover = options & XML_PARSE_RECOVER != 0;
    let src_str = String::from_utf8_lossy(&source).into_owned();
    let (mut data, url) = if file {
        if source.contains(&0) {
            throw_error(
                vm,
                b"ValueError",
                &format!(
                    "{}(): Argument #1 ($path) must not contain any null bytes",
                    a.f
                ),
            );
            return Err(());
        }
        let path = crate::dom::document::valid_file_path(&src_str);
        let loaded = path
            .as_ref()
            .and_then(|p| crate::io::read_resource(vm, p).map(|d| (d, p.clone())));
        match loaded {
            Some((d, p)) => (d, Some(p)),
            None => {
                let p = path.unwrap_or(src_str.clone());
                let msg = format!(
                    "I/O warning : failed to load \"{}\": No such file or directory",
                    p
                );
                let mut e = XmlError::simple(
                    FROM_IO,
                    1549,
                    ERR_WARNING,
                    format!("failed to load \"{}\": No such file or directory\n", p),
                );
                e.formatted = vec![msg];
                e.file = None;
                crate::libxml::report(vm, &fname, &[e], crate::libxml::Style::Generic);
                if vm.current_exception.is_none() {
                    let ex = vm.create_exception(
                        b"Exception",
                        &format!("Cannot open file '{}'", src_str),
                        vm.current_line,
                    );
                    vm.current_exception = Some(ex);
                }
                return Err(());
            }
        }
    } else {
        (source, None)
    };
    if let Some(e) = &override_enc {
        let up = e.to_ascii_uppercase();
        if up != "UTF-8" && up != "UTF8" {
            if let Some(conv) = decode_to_utf8(&data, e) {
                data = conv;
            }
        }
    }
    let saved_er = vm.error_reporting;
    if recover {
        vm.error_reporting |= 2;
    }
    let out = crate::sax2::parse_doc(&data, url.as_deref(), options);
    crate::libxml::report(vm, &fname, &out.errors, crate::libxml::Style::Ctx);
    if recover {
        vm.error_reporting = saved_er;
    }
    if vm.current_exception.is_some() {
        return Err(());
    }
    let doc = if out.well_formed || recover {
        out.doc
    } else {
        None
    };
    let d = match doc {
        Some(d) => d,
        None => {
            dom_err_msg(vm, SYNTAX_ERR, "XML fragment is not well-formed");
            return Err(());
        }
    };
    tree::with(|t| {
        if let Some(di) = t.doc_info_mut(d) {
            if di.encoding.is_none() || override_enc.is_some() {
                di.encoding = Some(override_enc.clone().unwrap_or_else(|| "UTF-8".to_string()));
            }
            match &di.url {
                None => {
                    if !file {
                        di.url = crate::dom::document::cwd_url();
                    }
                }
                Some(u) => {
                    if file && !u.contains("://") && !u.starts_with("file:/") {
                        di.url = Some(format!("file://{}", u));
                    }
                }
            }
        }
    });
    if options & XML_PARSE_DTDVALID != 0 {
        crate::dom::document::validate_doc(vm, d, &fname);
    }
    if options & XML_PARSE_XINCLUDE != 0 {
        crate::xinclude::process(
            vm,
            &fname,
            d,
            options | crate::xinclude::XML_PARSE_NOXINCNODE,
        );
    }
    convert_to_modern(d);
    Ok(node_obj(vm, d))
}

// ---------------------------------------------------------------------------
// Methods

fn m_create_element(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.str(vm, 0, "localName")?;
    let d = doc_node(vm, o)?;
    if !is_valid_name(&name) {
        dom_err(vm, INVALID_CHARACTER_ERR);
        return Err(());
    }
    let n = tree::with(|t| {
        if t.nodes[d].t == NType::HtmlDocument {
            let ns = mapper_get_ns(t, d, None, Some(XHTML_NS));
            let e = t.new_element(Some(d), &name.to_ascii_lowercase());
            t.nodes[e].ns = ns;
            e
        } else {
            t.new_element(Some(d), &name)
        }
    });
    Ok(node_obj(vm, n))
}

fn m_create_element_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let d = doc_node(vm, o)?;
    match crate::dom::document::validate_and_extract(uri.as_deref(), &name) {
        Ok((prefix, local)) => {
            let n = tree::with(|t| {
                let ns = mapper_get_ns(t, d, prefix.as_deref(), uri.as_deref());
                let e = t.new_element(Some(d), &local);
                t.nodes[e].ns = ns;
                e
            });
            Ok(node_obj(vm, n))
        }
        Err(code) => {
            dom_err(vm, code);
            Err(())
        }
    }
}

fn m_create_cdata_section(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let v = a.str(vm, 0, "data")?;
    let d = doc_node(vm, o)?;
    if tree::with(|t| t.nodes[d].t == NType::HtmlDocument) {
        dom_err_msg(
            vm,
            NOT_SUPPORTED_ERR,
            "This operation is not supported for HTML documents",
        );
        return Err(());
    }
    if v.contains("]]>") {
        dom_err_msg(
            vm,
            INVALID_CHARACTER_ERR,
            "Invalid character sequence \"]]>\" in CDATA section",
        );
        return Err(());
    }
    let n = tree::with(|t| t.new_cdata(Some(d), &v));
    Ok(node_obj(vm, n))
}

fn m_create_pi(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let target = a.str(vm, 0, "target")?;
    let data = a.str(vm, 1, "data")?;
    let d = doc_node(vm, o)?;
    if !is_valid_name(crate::dom::node::cstr(&target)) {
        dom_err(vm, INVALID_CHARACTER_ERR);
        return Err(());
    }
    if data.contains("?>") {
        dom_err_msg(
            vm,
            INVALID_CHARACTER_ERR,
            "Invalid character sequence \"?>\" in processing instruction",
        );
        return Err(());
    }
    let n = tree::with(|t| {
        t.new_pi(
            Some(d),
            crate::dom::node::cstr(&target),
            crate::dom::node::cstr(&data),
        )
    });
    Ok(node_obj(vm, n))
}

fn m_create_attribute(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.path_str(vm, 0, "localName")?;
    let d = doc_node(vm, o)?;
    if !is_valid_name(&name) {
        dom_err(vm, INVALID_CHARACTER_ERR);
        return Err(());
    }
    let n = tree::with(|t| {
        let nm = if t.nodes[d].t == NType::HtmlDocument {
            name.to_ascii_lowercase()
        } else {
            name.clone()
        };
        t.new_attr_node(Some(d), None, &nm, None)
    });
    Ok(node_obj(vm, n))
}

fn m_create_attribute_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let uri = a.opt_str(vm, 0, "namespace")?;
    let name = a.str(vm, 1, "qualifiedName")?;
    let d = doc_node(vm, o)?;
    match crate::dom::document::validate_and_extract(uri.as_deref(), &name) {
        Ok((prefix, local)) => {
            let n = tree::with(|t| {
                let at = t.new_attr_node(Some(d), None, &local, None);
                if let Some(u) = uri.as_deref().filter(|u| !u.is_empty()) {
                    t.nodes[at].ns = mapper_get_ns(t, d, prefix.as_deref(), Some(u));
                }
                at
            });
            Ok(node_obj(vm, n))
        }
        Err(code) => {
            dom_err(vm, code);
            Err(())
        }
    }
}

fn import_common(vm: &mut Vm, o: &Obj, a: &[Value], class: &str) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let no = a.obj_of(vm, 0, "node", class, false)?.unwrap();
    let deep = a.bool(vm, 1, "deep", false)?;
    let d = doc_node(vm, o)?;
    let n = need_node(vm, &no)?;
    if tree::with(|t| t.is_doc(n)) {
        dom_err(vm, NOT_SUPPORTED_ERR);
        return Err(());
    }
    if tree::with(|t| t.nodes[n].doc) == Some(d) {
        return Ok(Value::Object(no));
    }
    let c = tree::with(|t| node::clone_node(t, n, Some(d), deep, Some(d)));
    Ok(node_obj(vm, c))
}

fn m_import_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    import_common(vm, o, a, "Dom\\Node")
}

fn m_import_legacy_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    import_common(vm, o, a, "DOMNode")
}

/// php_dom_adopt_node (modern): unlink, move to the document and move the
/// namespaces into its mapper.
pub fn adopt_into(t: &mut Tree, n: NodeId, d: NodeId) {
    let old = t.nodes[n].doc;
    if old != Some(d) {
        t.unlink(n);
        t.set_tree_doc(n, Some(d));
        reconcile_modern(t, d, n);
    } else {
        t.unlink(n);
    }
}

fn m_adopt_node(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let no = a.obj_of(vm, 0, "node", "Dom\\Node", false)?.unwrap();
    let n = need_node(vm, &no)?;
    let bad = tree::with(|t| {
        matches!(
            t.nodes[n].t,
            NType::Document
                | NType::HtmlDocument
                | NType::DocumentType
                | NType::Dtd
                | NType::Entity
                | NType::Notation
        )
    });
    if bad {
        dom_err(vm, NOT_SUPPORTED_ERR);
        return Err(());
    }
    let d = doc_node(vm, o)?;
    tree::with(|t| adopt_into(t, n, d));
    Ok(Value::Object(no))
}

fn m_register_node_class(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let base = a.str(vm, 0, "baseClass")?;
    let ext = a.opt_str(vm, 1, "extendedClass")?;
    let bl = base.trim_start_matches('\\').to_ascii_lowercase();
    if !vm.classes.contains_key(bl.as_bytes()) || !class_is(vm, bl.as_bytes(), "Dom\\Node") {
        throw_error(
            vm,
            b"TypeError",
            &format!(
                "{}(): Argument #1 ($baseClass) must be a class name derived from Dom\\Node, {} given",
                a.f, base
            ),
        );
        return Err(());
    }
    if vm
        .classes
        .get(bl.as_bytes())
        .map(|c| c.is_abstract)
        .unwrap_or(false)
    {
        throw_error(
            vm,
            b"ValueError",
            &format!(
                "{}(): Argument #1 ($baseClass) must not be an abstract class",
                a.f
            ),
        );
        return Err(());
    }
    let base_canon = vm
        .classes
        .get(bl.as_bytes())
        .map(|c| String::from_utf8_lossy(&c.name).into_owned())
        .unwrap_or(base.clone());
    let d = doc_node(vm, o)?;
    match ext {
        None => {
            with_doc_props(d, |p| {
                p.classmap.remove(&bl);
            });
            Ok(Value::Null)
        }
        Some(e) => {
            let el = e.trim_start_matches('\\').to_ascii_lowercase();
            if !vm.classes.contains_key(el.as_bytes()) {
                throw_error(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #2 ($extendedClass) must be a class name derived from Dom\\Node or null, {} given",
                        a.f, e
                    ),
                );
                return Err(());
            }
            if !class_is(vm, el.as_bytes(), &bl) {
                let ecanon = vm
                    .classes
                    .get(el.as_bytes())
                    .map(|c| String::from_utf8_lossy(&c.name).into_owned())
                    .unwrap_or(e.clone());
                throw_error(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #2 must be a class name derived from {} or null, {} given",
                        a.f, base_canon, ecanon
                    ),
                );
                return Err(());
            }
            if vm
                .classes
                .get(el.as_bytes())
                .map(|c| c.is_abstract)
                .unwrap_or(false)
            {
                throw_error(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #2 ($extendedClass) must not be an abstract class",
                        a.f
                    ),
                );
                return Err(());
            }
            let ecanon = vm
                .classes
                .get(el.as_bytes())
                .map(|c| c.name.clone())
                .unwrap_or_else(|| e.clone().into_bytes());
            with_doc_props(d, |p| {
                p.classmap.insert(bl.clone(), ecanon);
            });
            Ok(Value::Null)
        }
    }
}

fn m_get_elements_by_tag_name(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let name = a.path_str(vm, 0, "qualifiedName")?;
    need_node(vm, o)?;
    Ok(mlist_obj(
        vm,
        b"Dom\\HTMLCollection",
        ListKind::ByQName {
            base: o.clone(),
            qname: name,
        },
    ))
}

fn m_get_elements_by_tag_name_ns(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 2)?;
    let ns = a.opt_path_str(vm, 0, "namespace")?;
    let name = a.path_str(vm, 1, "localName")?;
    need_node(vm, o)?;
    Ok(mlist_obj(
        vm,
        b"Dom\\HTMLCollection",
        ListKind::ByTag {
            base: o.clone(),
            ns: Some(ns.unwrap_or_default()),
            local: name,
        },
    ))
}

pub fn get_elements_by_class_name(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let names = a.path_str(vm, 0, "classNames")?;
    let n = need_node(vm, o)?;
    let quirks = false;
    let _ = n;
    let classes = tokenlist::ordered_set_parse(&names, quirks);
    Ok(mlist_obj(
        vm,
        b"Dom\\HTMLCollection",
        ListKind::ByClass {
            base: o.clone(),
            classes,
            quirks,
        },
    ))
}

fn save_options(vm: &mut Vm, a: &Args, i: usize) -> R<i64> {
    a.long(vm, i, "options", 0)
}

fn m_save_xml(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 2)?;
    let node = a.obj_of(vm, 0, "node", "Dom\\Node", true)?;
    let options = save_options(vm, &a, 1)?;
    let d = doc_node(vm, o)?;
    let format = doc_props(Some(d)).format_output;
    let no_empty = options & 4 != 0;
    let target = match node {
        Some(no) => {
            let n = need_node(vm, &no)?;
            if tree::with(|t| t.nodes[n].doc) != Some(d) {
                dom_err(vm, WRONG_DOCUMENT_ERR);
                return Err(());
            }
            (n, false)
        }
        None => (d, options & 2 != 0),
    };
    let enc = tree::with(|t| t.doc_info(d).and_then(|i| i.encoding.clone()));
    let r = tree::with(|t| {
        serializer::serialize(
            t,
            target.0,
            format,
            false,
            no_empty,
            target.1,
            enc.as_deref(),
        )
    });
    match r {
        Ok(s_) => Ok(Value::String(PhpString::from_vec(
            crate::serialize::encode_output(&s_, enc.as_deref()),
        ))),
        Err(()) => {
            warn(vm, "Could not save document");
            Ok(Value::False)
        }
    }
}

fn m_save_xml_file(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 2)?;
    let file = a.path_str(vm, 0, "filename")?;
    let options = save_options(vm, &a, 1)?;
    if file.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($filename) must not be empty", a.f),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    let format = doc_props(Some(d)).format_output;
    let enc = tree::with(|t| t.doc_info(d).and_then(|i| i.encoding.clone()));
    let r = tree::with(|t| {
        serializer::serialize(t, d, format, false, options & 4 != 0, false, enc.as_deref())
    });
    let bytes = match r {
        Ok(s_) => crate::serialize::encode_output(&s_, enc.as_deref()),
        Err(()) => return Ok(Value::False),
    };
    let n = crate::io::write_resource_len(vm, &file, &bytes);
    if n < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(n))
}

fn m_xinclude(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let flags = a.long(vm, 0, "options", 0)?;
    if flags > i32::MAX as i64 || flags < i32::MIN as i64 {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($options) is too large", a.f),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    let n = crate::xinclude::process(vm, &a.f, d, flags | crate::xinclude::XML_PARSE_NOXINCNODE);
    if n < 0 {
        dom_err(vm, INVALID_MODIFICATION_ERR);
        return Err(());
    }
    tree::with(|t| mark_namespaces_as_attributes(t, d));
    Ok(Value::Long(n))
}

pub fn document_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        (
            "getElementsByTagName",
            m_get_elements_by_tag_name as MethodFn,
        ),
        ("getElementsByTagNameNS", m_get_elements_by_tag_name_ns),
        ("getElementsByClassName", get_elements_by_class_name),
        ("createElement", m_create_element),
        ("createElementNS", m_create_element_ns),
        (
            "createDocumentFragment",
            legacy_method(Base::Document, "createDocumentFragment"),
        ),
        (
            "createTextNode",
            legacy_method(Base::Document, "createTextNode"),
        ),
        ("createCDATASection", m_create_cdata_section),
        (
            "createComment",
            legacy_method(Base::Document, "createComment"),
        ),
        ("createProcessingInstruction", m_create_pi),
        ("importNode", m_import_node),
        ("adoptNode", m_adopt_node),
        ("createAttribute", m_create_attribute),
        ("createAttributeNS", m_create_attribute_ns),
        (
            "getElementById",
            legacy_method(Base::Document, "getElementById"),
        ),
        ("registerNodeClass", m_register_node_class),
        (
            "schemaValidate",
            legacy_method(Base::Document, "schemaValidate"),
        ),
        (
            "schemaValidateSource",
            legacy_method(Base::Document, "schemaValidateSource"),
        ),
        (
            "relaxNgValidate",
            legacy_method(Base::Document, "relaxNGValidate"),
        ),
        (
            "relaxNgValidateSource",
            legacy_method(Base::Document, "relaxNGValidateSource"),
        ),
        ("importLegacyNode", m_import_legacy_node),
    ]
}

pub fn xml_document_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        (
            "createEntityReference",
            legacy_method(Base::Document, "createEntityReference") as MethodFn,
        ),
        ("validate", legacy_method(Base::Document, "validate")),
        ("xinclude", m_xinclude),
        ("saveXml", m_save_xml),
        ("saveXmlFile", m_save_xml_file),
    ]
}

fn m_save_html(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let node = a.obj_of(vm, 0, "node", "Dom\\Node", true)?;
    let d = doc_node(vm, o)?;
    let n = match node {
        Some(no) => {
            let n = need_node(vm, &no)?;
            if tree::with(|t| t.nodes[n].doc) != Some(d) {
                dom_err(vm, WRONG_DOCUMENT_ERR);
                return Err(());
            }
            n
        }
        None => d,
    };
    let out = super::html_serializer::serialize_node(n);
    let enc = tree::with(|t| t.doc_info(d).and_then(|i| i.encoding.clone()));
    Ok(Value::String(PhpString::from_vec(
        crate::serialize::encode_output(&out, enc.as_deref()),
    )))
}

fn m_save_html_file(vm: &mut Vm, o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 1, 1)?;
    let file = a.path_str(vm, 0, "filename")?;
    if file.is_empty() {
        throw_error(
            vm,
            b"ValueError",
            &format!("{}(): Argument #1 ($filename) must not be empty", a.f),
        );
        return Err(());
    }
    let d = doc_node(vm, o)?;
    let out = super::html_serializer::serialize_node(d);
    let enc = tree::with(|t| t.doc_info(d).and_then(|i| i.encoding.clone()));
    let bytes = crate::serialize::encode_output(&out, enc.as_deref());
    let n = crate::io::write_resource_len(vm, &file, &bytes);
    if n < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(n))
}

pub fn html_document_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("saveXml", m_save_xml as MethodFn),
        ("saveXmlFile", m_save_xml_file),
        ("saveHtml", m_save_html),
        ("saveHtmlFile", m_save_html_file),
    ]
}

// ---------------------------------------------------------------------------
// Dom\Implementation

fn m_create_document_type(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 3, 3)?;
    let name = a.path_str(vm, 0, "qualifiedName")?;
    let public = a.path_str(vm, 1, "publicId")?;
    let system = a.path_str(vm, 2, "systemId")?;
    if !is_valid_qname(&name) {
        dom_err(vm, NAMESPACE_ERR);
        return Err(());
    }
    let n = tree::with(|t| {
        t.create_int_subset(
            None,
            &name,
            if public.is_empty() {
                None
            } else {
                Some(&public)
            },
            if system.is_empty() {
                None
            } else {
                Some(&system)
            },
        )
    });
    Ok(node_obj_detached_modern(vm, n))
}

fn m_create_document(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 2, 3)?;
    let uri = a.opt_path_str(vm, 0, "namespace")?;
    let qname = a.path_str(vm, 1, "qualifiedName")?;
    let dt = arg_node(vm, &a, 2, "doctype", "Dom\\DocumentType", true)?;
    let d = tree::with(|t| {
        let d = t.new_doc(Some("1.0"));
        if let Some(di) = t.doc_info_mut(d) {
            di.encoding = Some("UTF-8".to_string());
        }
        d
    });
    mapper_reset(d);
    let mut element = None;
    if !qname.is_empty() {
        match crate::dom::document::validate_and_extract(uri.as_deref(), &qname) {
            Ok((prefix, local)) => {
                element = Some(tree::with(|t| {
                    let ns = mapper_get_ns(t, d, prefix.as_deref(), uri.as_deref());
                    let e = t.new_element(Some(d), &local);
                    t.nodes[e].ns = ns;
                    e
                }));
            }
            Err(code) => {
                dom_err(vm, code);
                return Err(());
            }
        }
    }
    let r = modern_doc_obj(vm, d);
    tree::with(|t| {
        if let Some((_, dtn)) = dt {
            adopt_into(t, dtn, d);
            t.link_child_raw(d, dtn);
            t.nodes[dtn].doc = Some(d);
            t.nodes[dtn].flags &= !tree::F_MODERN;
            if let Some(di) = t.doc_info_mut(d) {
                di.int_subset = Some(dtn);
            }
        }
        if let Some(e) = element {
            t.link_child_raw(d, e);
        }
    });
    Ok(r)
}

fn m_create_html_document(vm: &mut Vm, _o: &Obj, a: &[Value]) -> R<Value> {
    let a = args(a);
    a.count(vm, 0, 1)?;
    let title = a.opt_path_str(vm, 0, "title")?;
    let d = tree::with(|t| {
        let d = t.new_html_doc();
        if let Some(di) = t.doc_info_mut(d) {
            // htmlNewDocNoDtD marks the document standalone
            di.standalone = 1;
            di.encoding = Some("UTF-8".to_string());
        }
        d
    });
    mapper_reset(d);
    tree::with(|t| {
        t.create_int_subset(Some(d), "html", None, None);
        let ns = mapper_get_ns(t, d, None, Some(XHTML_NS));
        let mk = |t: &mut Tree, name: &str| {
            let e = t.new_element(Some(d), name);
            t.nodes[e].ns = ns;
            e
        };
        let html = mk(t, "html");
        t.link_child_raw(d, html);
        let head = mk(t, "head");
        t.link_child_raw(html, head);
        if let Some(ti) = &title {
            let te = mk(t, "title");
            if !ti.is_empty() {
                let tx = t.new_text(Some(d), ti);
                t.link_child_raw(te, tx);
            }
            t.link_child_raw(head, te);
        }
        let body = mk(t, "body");
        t.link_child_raw(html, body);
    });
    Ok(modern_doc_obj(vm, d))
}

pub fn implementation_methods() -> Vec<(&'static str, MethodFn)> {
    vec![
        ("createDocumentType", m_create_document_type as MethodFn),
        ("createDocument", m_create_document),
        ("createHTMLDocument", m_create_html_document),
    ]
}
