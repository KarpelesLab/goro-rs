//! php-src's ext/tidy (tidy.c) over goro-tidy, a port of tidy-html5 5.8.0:
//! the tidy and tidyNode classes, the tidy_* functions, the TIDY_*
//! constants, the tidy.default_config / tidy.clean_output INI settings and
//! the ob_tidyhandler output handler.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::ob::{OB_OP_FINAL, OB_OP_START, ObHandler, ObKind};
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};
use goro_tidy::config::{ConfigCategory, OptionImpl, OptionType};
use goro_tidy::doc::NodeType;
use goro_tidy::{Doc, NodeId, OptId, TagId, tidylib};

mod zpp;

use zpp::{Arg, Func, Obj};

/// The extension name (`extension_loaded('tidy')`).
pub const NAME: &[u8] = b"tidy";

type R = Result<Value, VmError>;

// ---------------------------------------------------------------------------
// objects

/// PHPTidyDoc
pub struct PtDoc {
    pub doc: RefCell<Doc>,
    pub initialized: Cell<bool>,
}

/// PHPTidyObj: a tidy document (`node` None) or a tidyNode.
pub struct TidyData {
    pub ptdoc: Rc<PtDoc>,
    pub node: Option<NodeId>,
    /// The document generation the node belongs to (a new parse frees the
    /// tree; such nodes are dangling in php-src).
    pub generation: u64,
}

static DOC_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    cast: Some(doc_cast),
    ..NativeObjectHandlers::EMPTY
};

static NODE_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    cast: Some(node_cast),
    ..NativeObjectHandlers::EMPTY
};

fn data_of(o: &Obj) -> Option<Rc<TidyData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<TidyData>().ok()
}

fn obj_of(v: &Value) -> Option<Obj> {
    match v {
        Value::Object(o) => Some(o.clone()),
        Value::Reference(r) => obj_of(&r.borrow()),
        _ => None,
    }
}

/// FIX_BUFFER + `size - 1`: the output without its last byte.
fn drop_last(mut v: Vec<u8>) -> Vec<u8> {
    v.pop();
    v
}

fn vstr(b: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(b.as_ref()))
}

fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// tidy_doc_cast_handler()
fn doc_cast(o: &Obj, ty: u8) -> Option<Value> {
    match ty {
        0 => Some(Value::True),
        1 => Some(Value::Long(0)),
        2 => Some(Value::Double(0.0)),
        3 => {
            let d = data_of(o)?;
            let (_, out) = tidylib::save_buffer(&mut d.ptdoc.doc.borrow_mut());
            Some(vstr(drop_last(out)))
        }
        _ => None,
    }
}

/// The node of a node object, if it still belongs to the document.
fn live_node(d: &TidyData) -> Option<NodeId> {
    let n = d.node?;
    let doc = d.ptdoc.doc.borrow();
    if doc.generation == d.generation && (n.0 as usize) < doc.nodes.len() {
        Some(n)
    } else {
        None
    }
}

/// tidyNodeGetText(): the pretty-printed node, without the last byte
fn node_text(doc: &mut Doc, n: NodeId) -> Vec<u8> {
    if doc.lexer.is_none() {
        return Vec::new();
    }
    drop_last(tidylib::node_get_text(doc, n))
}

/// tidy_node_cast_handler()
fn node_cast(o: &Obj, ty: u8) -> Option<Value> {
    match ty {
        0 => Some(Value::True),
        1 => Some(Value::Long(0)),
        2 => Some(Value::Double(0.0)),
        3 => {
            let d = data_of(o)?;
            let s = match live_node(&d) {
                Some(n) => node_text(&mut d.ptdoc.doc.borrow_mut(), n),
                None => Vec::new(),
            };
            Some(vstr(s))
        }
        _ => None,
    }
}

thread_local! {
    /// TG(clean_output) (and its INI value at startup)
    static CLEAN_OUTPUT: Cell<bool> = const { Cell::new(false) };
}

fn ini_str(vm: &Vm, key: &[u8]) -> Vec<u8> {
    vm.constants
        .get(key)
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

/// zend_ini_parse_bool()
fn ini_parse_bool(v: &[u8]) -> bool {
    let s = String::from_utf8_lossy(v).to_ascii_lowercase();
    match s.as_str() {
        "true" | "yes" | "on" => true,
        _ => {
            let t = s.trim_start();
            let digits: String = t
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '-')
                .collect();
            digits.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
    }
}

/// php_error_docref() prefix: the running function, or main()
fn docref_prefix(vm: &Vm) -> String {
    match vm.current_builtin_name() {
        Some(f) => format!("{}(): ", f),
        None => "main(): ".to_string(),
    }
}

fn warn(vm: &mut Vm, f: &Func, msg: &str) {
    f.warn(vm, msg);
}

/// php_tidy_load_config()
fn load_config(vm: &mut Vm, f: Option<&Func>, doc: &mut Doc, path: &[u8]) {
    let ret = tidylib::load_config(doc, path);
    let p = String::from_utf8_lossy(path);
    let prefix = match f {
        Some(f) => format!("{}(): ", f.name),
        None => docref_prefix(vm),
    };
    if ret < 0 {
        let line = vm.current_line;
        vm.emit_warning_at(
            &format!(
                "{}Could not load the Tidy configuration file \"{}\"",
                prefix, p
            ),
            line,
        );
    } else if ret > 0 {
        let line = vm.current_line;
        vm.emit_notice_at(
            &format!(
                "{}There were errors while parsing the Tidy configuration file \"{}\"",
                prefix, p
            ),
            line,
        );
    }
}

/// TIDY_SET_DEFAULT_CONFIG()
fn set_default_config(vm: &mut Vm, f: Option<&Func>, doc: &mut Doc) {
    let dc = ini_str(vm, b"tidy.default_config");
    if !dc.is_empty() {
        load_config(vm, f, doc, &dc);
    }
}

/// tidyCreate() + the PHP defaults (tidy_object_new for is_doc)
fn new_ptdoc(vm: &mut Vm, f: Option<&Func>) -> Rc<PtDoc> {
    let mut doc = Doc::new();
    tidylib::set_error_buffer(&mut doc);
    tidylib::opt_set_bool(&mut doc, OptId::ForceOutput, true);
    tidylib::opt_set_bool(&mut doc, OptId::Mark, false);
    set_default_config(vm, f, &mut doc);
    Rc::new(PtDoc {
        doc: RefCell::new(doc),
        initialized: Cell::new(false),
    })
}

fn attach_doc(o: &Obj, ptdoc: Rc<PtDoc>) {
    let d = Rc::new(TidyData {
        ptdoc,
        node: None,
        generation: 0,
    });
    o.borrow_mut().native = Some(NativeData {
        data: d as Rc<dyn Any>,
        handlers: &DOC_HANDLERS,
    });
}

/// tidy_object_new_doc(): the create_object hook of tidy (and subclasses)
fn create_doc_object(vm: &mut Vm, o: &Obj) {
    if data_of(o).is_some() {
        return;
    }
    let ptdoc = new_ptdoc(vm, None);
    attach_doc(o, ptdoc);
}

/// object_init_ex(return_value, tidy_ce_doc)
fn new_doc_object(vm: &mut Vm) -> Obj {
    let o = vm.spl_instantiate(b"tidy");
    vm.run_object_create_hook(&o);
    o
}

fn set_prop(o: &Obj, name: &[u8], v: Value) {
    o.borrow_mut().set_property(name.to_vec(), v);
}

/// tidy_doc_update_properties()
fn update_doc_properties(o: &Obj, d: &TidyData) {
    let mut doc = d.ptdoc.doc.borrow_mut();
    let (_, out) = tidylib::save_buffer(&mut doc);
    if !out.is_empty() {
        set_prop(o, b"value", vstr(drop_last(out)));
    }
    let eb = doc.errout.bytes();
    if !eb.is_empty() {
        let v = eb[..eb.len() - 1].to_vec();
        drop(doc);
        set_prop(o, b"errorBuffer", vstr(v));
    }
}

/// tidy_create_node_object() + tidy_add_node_default_properties()
fn create_node_object(vm: &mut Vm, ptdoc: &Rc<PtDoc>, node: NodeId) -> Value {
    let o = vm.spl_instantiate(b"tidyNode");
    let generation = ptdoc.doc.borrow().generation;
    let d = Rc::new(TidyData {
        ptdoc: ptdoc.clone(),
        node: Some(node),
        generation,
    });
    o.borrow_mut().native = Some(NativeData {
        data: d as Rc<dyn Any>,
        handlers: &NODE_HANDLERS,
    });

    let (value, name, ty, line, column, prop, id, attrs, first_child) = {
        let mut doc = ptdoc.doc.borrow_mut();
        let value = node_text(&mut doc, node);
        let name = doc[node].element.clone().unwrap_or_default();
        let ty = doc[node].type_;
        let line = doc[node].line;
        let column = doc[node].column;
        let prop = tidylib::node_is_prop(&doc, node);
        let id = match ty {
            NodeType::RootNode
            | NodeType::DocTypeTag
            | NodeType::TextNode
            | NodeType::CommentTag => None,
            _ => Some(tidylib::node_get_id(&doc, node) as i64),
        };
        let attrs: Vec<(Vec<u8>, Vec<u8>)> = doc
            .attr_list(node)
            .into_iter()
            .filter_map(|a| {
                let av = &doc[a];
                av.attribute
                    .clone()
                    .map(|n| (n, av.value.clone().unwrap_or_default()))
            })
            .collect();
        let has_attrs = doc[node].attributes.is_some();
        (
            value,
            name,
            ty,
            line,
            column,
            prop,
            id,
            if has_attrs { Some(attrs) } else { None },
            doc[node].content,
        )
    };
    set_prop(&o, b"value", vstr(value));
    set_prop(&o, b"name", vstr(name));
    set_prop(&o, b"type", Value::Long(ty as i64));
    set_prop(&o, b"line", Value::Long(line as i64));
    set_prop(&o, b"column", Value::Long(column as i64));
    set_prop(&o, b"proprietary", vbool(prop));
    set_prop(&o, b"id", id.map_or(Value::Null, Value::Long));
    let attribute = match attrs {
        Some(list) => {
            let mut a = PhpArray::new();
            for (k, v) in list {
                a.set(ArrayKey::String(PhpString::from_vec(k)), vstr(v));
            }
            Value::Array(Rc::new(RefCell::new(a)))
        }
        None => Value::Null,
    };
    set_prop(&o, b"attribute", attribute);
    let child = match first_child {
        Some(c) => {
            let mut a = PhpArray::new();
            let mut cur = Some(c);
            while let Some(n) = cur {
                let co = create_node_object(vm, ptdoc, n);
                a.push(co);
                cur = ptdoc.doc.borrow()[n].next;
            }
            Value::Array(Rc::new(RefCell::new(a)))
        }
        None => Value::Null,
    };
    set_prop(&o, b"child", child);
    Value::Object(o)
}

// ---------------------------------------------------------------------------
// configuration

/// php_tidy_set_tidy_opt()
fn set_tidy_opt(
    vm: &mut Vm,
    f: &Func,
    doc: &mut Doc,
    optname: &[u8],
    value: &Value,
    arg: usize,
) -> bool {
    let name = String::from_utf8_lossy(optname).into_owned();
    let Some(opt) = goro_tidy::config::lookup_option(optname) else {
        f.value_error(
            vm,
            arg,
            &format!("Unknown Tidy configuration option \"{}\"", name),
        );
        return false;
    };
    if opt.category == ConfigCategory::InternalCategory {
        f.value_error(
            vm,
            arg,
            &format!("Attempting to set read-only option \"{}\"", name),
        );
        return false;
    }
    let value = value.deref();
    let type_err = |vm: &mut Vm, s: &[u8]| {
        let m = format!(
            "{} option \"{}\" does not accept \"{}\" as a value",
            f.arg_prefix(arg),
            name,
            String::from_utf8_lossy(s)
        );
        zpp::throw(vm, b"TypeError", &m);
    };
    match opt.type_ {
        OptionType::String => {
            let s = vm.value_to_string(&value);
            if vm.current_exception.is_some() {
                return false;
            }
            let r = tidylib::opt_set_value(doc, opt.id, s.as_bytes());
            if !r {
                type_err(vm, s.as_bytes());
            }
            r
        }
        OptionType::Integer => {
            if matches!(value, Value::String(_) | Value::Object(_)) {
                let s = vm.value_to_string(&value);
                if vm.current_exception.is_some() {
                    return false;
                }
                let lval = match goro_core::value::parse_numeric_str(s.as_bytes(), true) {
                    Some((goro_core::value::NumStr::Long(l), _)) => Some(l),
                    Some((goro_core::value::NumStr::Double(d), _)) => {
                        Some(dval_to_lval_cap(d, s.as_bytes()))
                    }
                    None => None,
                };
                match lval {
                    Some(l) => tidylib::opt_set_int(doc, opt.id, l as u64),
                    None => {
                        let r = tidylib::opt_set_value(doc, opt.id, s.as_bytes());
                        if !r {
                            type_err(vm, s.as_bytes());
                        }
                        r
                    }
                }
            } else {
                let l = zval_get_long(vm, &value);
                tidylib::opt_set_int(doc, opt.id, l as u64)
            }
        }
        OptionType::Boolean => {
            let l = zval_get_long(vm, &value);
            // (Bool) lval, stored as ulong
            goro_tidy::config::set_option_int(doc, opt.id, (l as i32) as i64 as u64)
        }
    }
}

/// zval_get_long()
fn zval_get_long(vm: &mut Vm, v: &Value) -> i64 {
    match v {
        Value::Double(d) => {
            let line = vm.current_line;
            vm.float_to_int_cast(*d, line)
        }
        other => other.to_long(),
    }
}

/// zend_dval_to_lval_cap()
fn dval_to_lval_cap(d: f64, s: &[u8]) -> i64 {
    if !d.is_finite() {
        return 0;
    }
    if d >= 9.223_372_036_854_776e18 || d < -9.223_372_036_854_776e18 {
        let neg = s.iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'-');
        return if neg { i64::MIN } else { i64::MAX };
    }
    d as i64
}

/// _php_tidy_apply_config_array()
fn apply_config_array(
    vm: &mut Vm,
    f: &Func,
    doc: &mut Doc,
    ht: &Rc<RefCell<PhpArray>>,
    arg: usize,
) -> bool {
    let entries: Vec<(ArrayKey, Value)> = {
        let a = ht.borrow();
        let packed = !a.is_empty() && a.looks_packed();
        if packed {
            drop(a);
            let m = format!(
                "{} must be of type array with keys as string",
                f.arg_prefix(arg)
            );
            zpp::throw(vm, b"TypeError", &m);
            return false;
        }
        a.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    let mut has_failures = false;
    for (k, v) in entries {
        let name = match k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(_) => {
                let m = format!(
                    "{} must be of type array with keys as string",
                    f.arg_prefix(arg)
                );
                zpp::throw(vm, b"TypeError", &m);
                return false;
            }
        };
        has_failures = has_failures || !set_tidy_opt(vm, f, doc, &name, &v, arg);
    }
    !has_failures
}

/// php_tidy_apply_config(): Ok(()) is SUCCESS
fn apply_config(vm: &mut Vm, f: &Func, doc: &mut Doc, cfg: &Arg, arg: usize) -> bool {
    match cfg {
        Arg::Arr(ht) => apply_config_array(vm, f, doc, ht, arg),
        Arg::Str(s) => {
            let p = String::from_utf8_lossy(s).into_owned();
            if !vm.check_open_basedir(&f.name, &p) {
                return false;
            }
            load_config(vm, Some(f), doc, s);
            true
        }
        _ => true,
    }
}

/// php_tidy_parse_string()
fn parse_string(
    vm: &mut Vm,
    f: &Func,
    o: &Obj,
    d: &TidyData,
    input: Vec<u8>,
    enc: Option<&[u8]>,
) -> bool {
    if let Some(enc) = enc {
        let r = tidylib::set_char_encoding(&mut d.ptdoc.doc.borrow_mut(), enc);
        if r < 0 {
            warn(
                vm,
                f,
                &format!(
                    "Could not set encoding \"{}\"",
                    String::from_utf8_lossy(enc)
                ),
            );
            return false;
        }
    }
    d.ptdoc.initialized.set(true);
    let r = tidylib::parse_buffer(&mut d.ptdoc.doc.borrow_mut(), input);
    if r < 0 {
        let eb = d.ptdoc.doc.borrow().errout.bytes().to_vec();
        warn(vm, f, &String::from_utf8_lossy(&eb));
        return false;
    }
    update_doc_properties(o, d);
    true
}

/// php_tidy_file_to_mem(): None when the file cannot be opened
fn file_to_mem(
    vm: &mut Vm,
    filename: &[u8],
    use_include_path: bool,
) -> Result<Option<Vec<u8>>, VmError> {
    let mut path = filename.to_vec();
    if filename.is_empty() {
        zpp::throw(vm, b"ValueError", "Path must not be empty");
        return Ok(None);
    }
    if use_include_path {
        if let Some(p) = resolve_include_path(vm, filename) {
            path = p;
        }
    }
    goro_ext_standard::native_streams::read_whole_url(vm, &path, false)
}

/// The include_path lookup of a relative path (USE_PATH).
fn resolve_include_path(vm: &Vm, filename: &[u8]) -> Option<Vec<u8>> {
    if filename.first() == Some(&b'/') || filename.windows(3).any(|w| w == b"://") {
        return None;
    }
    let ip = ini_str(vm, b"include_path");
    for dir in ip.split(|&c| c == b':') {
        if dir.is_empty() {
            continue;
        }
        let mut p = dir.to_vec();
        if p.last() != Some(&b'/') {
            p.push(b'/');
        }
        p.extend_from_slice(filename);
        if std::path::Path::new(&*String::from_utf8_lossy(&p)).is_file() {
            return Some(p);
        }
    }
    None
}

/// The size check done before loading a file: ZEND_SIZE_T_UINT_OVFL of
/// the content (files above 4 GiB are not read into memory).
fn file_too_large(filename: &[u8]) -> bool {
    std::fs::metadata(&*String::from_utf8_lossy(filename))
        .map(|m| m.is_file() && m.len() > u32::MAX as u64)
        .unwrap_or(false)
}

fn cannot_load_msg(filename: &[u8], use_include_path: bool) -> String {
    format!(
        "Cannot load \"{}\" into memory{}",
        String::from_utf8_lossy(filename),
        if use_include_path {
            " (using include path)"
        } else {
            ""
        }
    )
}

// ---------------------------------------------------------------------------
// the functions

fn tidy_arg(args: &[Arg], i: usize) -> Option<(Obj, Rc<TidyData>)> {
    match args.get(i) {
        Some(Arg::Any(v)) => {
            let o = obj_of(v)?;
            let d = data_of(&o)?;
            Some((o, d))
        }
        _ => None,
    }
}

/// The tidy object of a function (first argument) or method ($this).
fn fetch(
    vm: &mut Vm,
    f: &Func,
    this: Option<&Value>,
    args: &[Value],
    spec: &str,
) -> Result<(Obj, Rc<TidyData>, Vec<Arg>), ()> {
    match this.and_then(obj_of) {
        Some(o) => {
            let a = zpp::parse(vm, f, spec, args)?;
            let d = match data_of(&o) {
                Some(d) => d,
                None => {
                    let pt = new_ptdoc(vm, None);
                    attach_doc(&o, pt);
                    data_of(&o).unwrap()
                }
            };
            Ok((o, d, a))
        }
        None => {
            let mut a = zpp::parse(vm, f, &format!("O{}", spec), args)?;
            let (o, d) = tidy_arg(&a, 0).ok_or(())?;
            a.remove(0);
            Ok((o, d, a))
        }
    }
}

fn func_of(this: Option<&Value>, fname: &str, method: &str) -> Func {
    if this.is_some() {
        Func::method("tidy", method)
    } else {
        Func::function(fname)
    }
}

fn opt_arg(a: &[Arg], i: usize) -> Option<Vec<u8>> {
    match a.get(i) {
        Some(Arg::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn bool_arg(a: &[Arg], i: usize) -> bool {
    matches!(a.get(i), Some(Arg::Bool(true)))
}

fn cfg_arg(a: &[Arg], i: usize) -> Arg {
    a.get(i).cloned().unwrap_or(Arg::Absent)
}

/// tidy_parse_string()
fn tidy_parse_string(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("tidy_parse_string");
    let Ok(a) = zpp::parse(vm, &f, "S|C!s!", args) else {
        return Ok(Value::Null);
    };
    let input = opt_arg(&a, 0).unwrap_or_default();
    if input.len() > u32::MAX as usize {
        f.value_error(vm, 1, "is too long");
        return Ok(Value::Null);
    }
    let o = new_doc_object(vm);
    let d = data_of(&o).unwrap();
    let ok = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        apply_config(vm, &f, &mut doc, &cfg_arg(&a, 1), 2)
    };
    if !ok || vm.current_exception.is_some() {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    }
    let enc = opt_arg(&a, 2);
    if !parse_string(vm, &f, &o, &d, input, enc.as_deref()) {
        return Ok(Value::False);
    }
    Ok(Value::Object(o))
}

/// tidy_get_error_buffer()
fn tidy_get_error_buffer(vm: &mut Vm, args: &[Value]) -> R {
    get_error_buffer(vm, None, args)
}

fn get_error_buffer(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = Func::function("tidy_get_error_buffer");
    let Ok((_, d, _)) = fetch(vm, &f, this, args, "") else {
        return Ok(Value::Null);
    };
    let doc = d.ptdoc.doc.borrow();
    let eb = doc.errout.bytes();
    if eb.is_empty() {
        return Ok(Value::False);
    }
    Ok(vstr(&eb[..eb.len() - 1]))
}

/// tidy_get_output()
fn tidy_get_output(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("tidy_get_output");
    let Ok((_, d, _)) = fetch(vm, &f, None, args, "") else {
        return Ok(Value::Null);
    };
    let (_, out) = tidylib::save_buffer(&mut d.ptdoc.doc.borrow_mut());
    Ok(vstr(drop_last(out)))
}

/// tidy_parse_file()
fn tidy_parse_file(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("tidy_parse_file");
    let Ok(a) = zpp::parse(vm, &f, "p|C!s!b", args) else {
        return Ok(Value::Null);
    };
    let inputfile = opt_arg(&a, 0).unwrap_or_default();
    let uip = bool_arg(&a, 3);
    if file_too_large(&inputfile) {
        zpp::throw(vm, b"ValueError", "File content is too long");
        return Ok(Value::Null);
    }
    let contents = match file_to_mem(vm, &inputfile, uip)? {
        Some(c) => c,
        None => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            warn(vm, &f, &cannot_load_msg(&inputfile, uip));
            return Ok(Value::False);
        }
    };
    let o = new_doc_object(vm);
    let d = data_of(&o).unwrap();
    let ok = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        apply_config(vm, &f, &mut doc, &cfg_arg(&a, 1), 2)
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let enc = opt_arg(&a, 2);
    if !ok || !parse_string(vm, &f, &o, &d, contents, enc.as_deref()) {
        return Ok(Value::False);
    }
    Ok(Value::Object(o))
}

/// tidy_clean_repair()
fn tidy_clean_repair(vm: &mut Vm, args: &[Value]) -> R {
    clean_repair(vm, None, args)
}

fn clean_repair(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = func_of(this, "tidy_clean_repair", "cleanRepair");
    let Ok((o, d, _)) = fetch(vm, &f, this, args, "") else {
        return Ok(Value::Null);
    };
    let r = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        if doc.lexer.is_none() {
            // tidyCleanAndRepair() on a document never parsed
            tidylib::status(&doc)
        } else {
            tidylib::clean_and_repair(&mut doc)
        }
    };
    if r >= 0 {
        update_doc_properties(&o, &d);
        return Ok(Value::True);
    }
    Ok(Value::False)
}

/// php_tidy_quick_repair()
fn quick_repair(vm: &mut Vm, f: &Func, args: &[Value], is_file: bool) -> R {
    let spec = if is_file { "p|C!sb" } else { "S|C!s" };
    let Ok(a) = zpp::parse(vm, f, spec, args) else {
        return Ok(Value::Null);
    };
    let arg1 = opt_arg(&a, 0).unwrap_or_default();
    let data = if is_file {
        let uip = bool_arg(&a, 3);
        if file_too_large(&arg1) {
            f.value_error(vm, 1, "File content is too long");
            return Ok(Value::Null);
        }
        match file_to_mem(vm, &arg1, uip)? {
            Some(c) => c,
            None => {
                return Ok(if vm.current_exception.is_some() {
                    Value::Null
                } else {
                    Value::False
                });
            }
        }
    } else {
        if arg1.len() > u32::MAX as usize {
            f.value_error(vm, 1, "is too long");
            return Ok(Value::Null);
        }
        arg1
    };

    let mut doc = Doc::new();
    tidylib::set_error_buffer(&mut doc);
    tidylib::opt_set_bool(&mut doc, OptId::ForceOutput, true);
    tidylib::opt_set_bool(&mut doc, OptId::Mark, false);
    set_default_config(vm, Some(f), &mut doc);

    // RETVAL_FALSE on a configuration / encoding failure, but the
    // document is parsed and repaired (and returned) anyway
    let config_ok = apply_config(vm, f, &mut doc, &cfg_arg(&a, 1), 2);
    if let Some(enc) = opt_arg(&a, 2).filter(|e| config_ok && !e.is_empty()) {
        if tidylib::set_char_encoding(&mut doc, &enc) < 0 {
            warn(
                vm,
                f,
                &format!(
                    "Could not set encoding \"{}\"",
                    String::from_utf8_lossy(&enc)
                ),
            );
        }
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let retval = if tidylib::parse_buffer(&mut doc, data) < 0 {
        let eb = doc.errout.bytes().to_vec();
        warn(vm, f, &String::from_utf8_lossy(&eb));
        Value::False
    } else if tidylib::clean_and_repair(&mut doc) >= 0 {
        let (_, out) = tidylib::save_buffer(&mut doc);
        vstr(drop_last(out))
    } else {
        Value::False
    };
    Ok(retval)
}

/// tidy_repair_string()
fn tidy_repair_string(vm: &mut Vm, args: &[Value]) -> R {
    quick_repair(vm, &Func::function("tidy_repair_string"), args, false)
}

/// tidy_repair_file()
fn tidy_repair_file(vm: &mut Vm, args: &[Value]) -> R {
    quick_repair(vm, &Func::function("tidy_repair_file"), args, true)
}

/// tidy_diagnose()
fn tidy_diagnose(vm: &mut Vm, args: &[Value]) -> R {
    diagnose(vm, None, args)
}

fn diagnose(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = func_of(this, "tidy_diagnose", "diagnose");
    let Ok((o, d, _)) = fetch(vm, &f, this, args, "") else {
        return Ok(Value::Null);
    };
    if d.ptdoc.initialized.get() && tidylib::run_diagnostics(&mut d.ptdoc.doc.borrow_mut()) >= 0 {
        update_doc_properties(&o, &d);
        return Ok(Value::True);
    }
    Ok(Value::False)
}

/// tidy_get_release()
fn tidy_get_release(vm: &mut Vm, args: &[Value]) -> R {
    let f = Func::function("tidy_get_release");
    if zpp::parse(vm, &f, "", args).is_err() {
        return Ok(Value::Null);
    }
    Ok(vstr(goro_tidy::message::release_date()))
}

/// tidy_get_opt_doc()
fn tidy_get_opt_doc(vm: &mut Vm, args: &[Value]) -> R {
    get_opt_doc(vm, None, args)
}

fn get_opt_doc(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = func_of(this, "tidy_get_opt_doc", "getOptDoc");
    let Ok((_, _, a)) = fetch(vm, &f, this, args, "s") else {
        return Ok(Value::Null);
    };
    let optname = opt_arg(&a, 0).unwrap_or_default();
    let Some(opt) = goro_tidy::config::lookup_option(&optname) else {
        let n = if this.is_some() { 1 } else { 2 };
        f.value_error(
            vm,
            n,
            &format!(
                "is an invalid configuration option, \"{}\" given",
                String::from_utf8_lossy(&optname)
            ),
        );
        return Ok(Value::Null);
    };
    Ok(match tidylib::opt_get_doc(opt.id) {
        Some(s) => vstr(s),
        None => Value::False,
    })
}

/// php_tidy_get_opt_val()
fn opt_val(doc: &Doc, opt: &'static OptionImpl) -> Value {
    match opt.type_ {
        OptionType::String => vstr(tidylib::opt_get_value(doc, opt.id).unwrap_or_default()),
        OptionType::Integer => Value::Long(tidylib::opt_get_int(doc, opt.id) as i64),
        OptionType::Boolean => vbool(tidylib::opt_get_bool(doc, opt.id)),
    }
}

/// tidy_get_config()
fn tidy_get_config(vm: &mut Vm, args: &[Value]) -> R {
    get_config(vm, None, args)
}

fn get_config(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = func_of(this, "tidy_get_config", "getConfig");
    let Ok((_, d, _)) = fetch(vm, &f, this, args, "") else {
        return Ok(Value::Null);
    };
    let doc = d.ptdoc.doc.borrow();
    let mut a = PhpArray::new();
    for opt in goro_tidy::config::get_option_list(&doc) {
        a.set(
            ArrayKey::String(PhpString::from_bytes(opt.name.as_bytes())),
            opt_val(&doc, opt),
        );
    }
    Ok(Value::Array(Rc::new(RefCell::new(a))))
}

macro_rules! simple_doc_fn {
    ($fname:ident, $impl:ident, $cname:expr, $mname:expr, $body:expr, $needs_init:expr) => {
        fn $fname(vm: &mut Vm, args: &[Value]) -> R {
            $impl(vm, None, args)
        }
        fn $impl(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
            let f = func_of(this, $cname, $mname);
            let Ok((_, d, _)) = fetch(vm, &f, this, args, "") else {
                return Ok(Value::Null);
            };
            if $needs_init && !d.ptdoc.initialized.get() {
                zpp::throw(vm, b"Error", "tidy object is not initialized");
                return Ok(Value::Null);
            }
            let doc = d.ptdoc.doc.borrow();
            let body: fn(&Doc) -> Value = $body;
            Ok(body(&doc))
        }
    };
}

simple_doc_fn!(
    tidy_get_status,
    get_status,
    "tidy_get_status",
    "getStatus",
    |d| Value::Long(tidylib::status(d) as i64),
    false
);
simple_doc_fn!(
    tidy_get_html_ver,
    get_html_ver,
    "tidy_get_html_ver",
    "getHtmlVer",
    |d| Value::Long(tidylib::detected_html_version(d) as i64),
    true
);
simple_doc_fn!(
    tidy_is_xhtml,
    is_xhtml,
    "tidy_is_xhtml",
    "isXhtml",
    |d| vbool(tidylib::detected_xhtml(d)),
    true
);
simple_doc_fn!(
    tidy_is_xml,
    is_xml,
    "tidy_is_xml",
    "isXml",
    |d| vbool(tidylib::detected_generic_xml(d)),
    true
);
simple_doc_fn!(
    tidy_error_count,
    error_count,
    "tidy_error_count",
    "errorCount",
    |d| Value::Long(d.errors as i64),
    false
);
simple_doc_fn!(
    tidy_warning_count,
    warning_count,
    "tidy_warning_count",
    "warningCount",
    |d| Value::Long(d.warnings as i64),
    false
);
simple_doc_fn!(
    tidy_access_count,
    access_count,
    "tidy_access_count",
    "accessCount",
    |d| Value::Long(d.access_errors as i64),
    false
);
simple_doc_fn!(
    tidy_config_count,
    config_count,
    "tidy_config_count",
    "configCount",
    |d| Value::Long(d.option_errors as i64),
    false
);

/// tidy_getopt()
fn tidy_getopt(vm: &mut Vm, args: &[Value]) -> R {
    getopt(vm, None, args)
}

fn getopt(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = func_of(this, "tidy_getopt", "getOpt");
    let Ok((_, d, a)) = fetch(vm, &f, this, args, "s") else {
        return Ok(Value::Null);
    };
    let optname = opt_arg(&a, 0).unwrap_or_default();
    let Some(opt) = goro_tidy::config::lookup_option(&optname) else {
        let n = if this.is_some() { 1 } else { 2 };
        f.value_error(
            vm,
            n,
            &format!(
                "is an invalid configuration option, \"{}\" given",
                String::from_utf8_lossy(&optname)
            ),
        );
        return Ok(Value::Null);
    };
    let doc = d.ptdoc.doc.borrow();
    Ok(opt_val(&doc, opt))
}

#[derive(Clone, Copy)]
enum BaseNode {
    Root,
    Html,
    Head,
    Body,
}

/// php_tidy_create_node()
fn create_base_node(
    vm: &mut Vm,
    this: Option<&Value>,
    args: &[Value],
    which: BaseNode,
    fname: &str,
    mname: &str,
) -> R {
    let f = func_of(this, fname, mname);
    let Ok((_, d, _)) = fetch(vm, &f, this, args, "") else {
        return Ok(Value::Null);
    };
    let node = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        match which {
            BaseNode::Root => Some(doc.root),
            BaseNode::Html | BaseNode::Head | BaseNode::Body if doc.lexer.is_none() => None,
            BaseNode::Html => tidylib::get_html(&mut doc),
            BaseNode::Head => tidylib::get_head(&mut doc),
            BaseNode::Body => tidylib::get_body(&mut doc),
        }
    };
    match node {
        Some(n) => Ok(create_node_object(vm, &d.ptdoc, n)),
        None => Ok(Value::Null),
    }
}

fn tidy_get_root(vm: &mut Vm, args: &[Value]) -> R {
    create_base_node(vm, None, args, BaseNode::Root, "tidy_get_root", "root")
}

fn tidy_get_html(vm: &mut Vm, args: &[Value]) -> R {
    create_base_node(vm, None, args, BaseNode::Html, "tidy_get_html", "html")
}

fn tidy_get_head(vm: &mut Vm, args: &[Value]) -> R {
    create_base_node(vm, None, args, BaseNode::Head, "tidy_get_head", "head")
}

fn tidy_get_body(vm: &mut Vm, args: &[Value]) -> R {
    create_base_node(vm, None, args, BaseNode::Body, "tidy_get_body", "body")
}

// ---------------------------------------------------------------------------
// the tidy methods

/// tidy::__construct()
fn m_construct(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = Func::method("tidy", "__construct");
    let Some(o) = this.and_then(obj_of) else {
        return Ok(Value::Null);
    };
    let Ok(a) = zpp::parse(vm, &f, "|p!C!s!b", args) else {
        return Ok(Value::Null);
    };
    let d = match data_of(&o) {
        Some(d) => d,
        None => {
            create_doc_object(vm, &o);
            data_of(&o).unwrap()
        }
    };
    let Some(inputfile) = opt_arg(&a, 0) else {
        return Ok(Value::Null);
    };
    let uip = bool_arg(&a, 3);
    if file_too_large(&inputfile) {
        zpp::throw(vm, b"ValueError", "File content is too long");
        return Ok(Value::Null);
    }
    let contents = match file_to_mem(vm, &inputfile, uip)? {
        Some(c) => c,
        None => {
            if vm.current_exception.is_none() {
                zpp::throw(vm, b"Exception", &cannot_load_msg(&inputfile, uip));
            }
            return Ok(Value::Null);
        }
    };
    // zend_replace_error_handling(EH_THROW): warnings become exceptions
    let saved = vm.warning_capture.replace(Vec::new());
    let ok = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        apply_config(vm, &f, &mut doc, &cfg_arg(&a, 1), 2)
    };
    let captured = std::mem::replace(&mut vm.warning_capture, saved).unwrap_or_default();
    if let Some(w) = captured.first() {
        if vm.current_exception.is_none() {
            zpp::throw(vm, b"Exception", w);
        }
        return Ok(Value::Null);
    }
    if !ok || vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let enc = opt_arg(&a, 2);
    parse_string(vm, &f, &o, &d, contents, enc.as_deref());
    Ok(Value::Null)
}

/// tidy::parseFile()
fn m_parse_file(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = Func::method("tidy", "parseFile");
    let Ok((o, d, a)) = fetch(vm, &f, this, args, "p|C!s!b") else {
        return Ok(Value::Null);
    };
    let inputfile = opt_arg(&a, 0).unwrap_or_default();
    let uip = bool_arg(&a, 3);
    if file_too_large(&inputfile) {
        zpp::throw(vm, b"ValueError", "File content is too long");
        return Ok(Value::Null);
    }
    let contents = match file_to_mem(vm, &inputfile, uip)? {
        Some(c) => c,
        None => {
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            warn(vm, &f, &cannot_load_msg(&inputfile, uip));
            return Ok(Value::False);
        }
    };
    let ok = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        apply_config(vm, &f, &mut doc, &cfg_arg(&a, 1), 2)
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let enc = opt_arg(&a, 2);
    Ok(vbool(
        ok && parse_string(vm, &f, &o, &d, contents, enc.as_deref()),
    ))
}

/// tidy::parseString()
fn m_parse_string(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    let f = Func::method("tidy", "parseString");
    let Ok((o, d, a)) = fetch(vm, &f, this, args, "S|C!s!") else {
        return Ok(Value::Null);
    };
    let input = opt_arg(&a, 0).unwrap_or_default();
    if input.len() > u32::MAX as usize {
        f.value_error(vm, 1, "is too long");
        return Ok(Value::Null);
    }
    let ok = {
        let mut doc = d.ptdoc.doc.borrow_mut();
        apply_config(vm, &f, &mut doc, &cfg_arg(&a, 1), 2)
    };
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let enc = opt_arg(&a, 2);
    Ok(vbool(
        ok && parse_string(vm, &f, &o, &d, input, enc.as_deref()),
    ))
}

type MethodImpl = fn(&mut Vm, Option<&Value>, &[Value]) -> R;

fn m_repair_string(vm: &mut Vm, _this: Option<&Value>, args: &[Value]) -> R {
    quick_repair(vm, &Func::method("tidy", "repairString"), args, false)
}

fn m_repair_file(vm: &mut Vm, _this: Option<&Value>, args: &[Value]) -> R {
    quick_repair(vm, &Func::method("tidy", "repairFile"), args, true)
}

fn m_get_release(vm: &mut Vm, _this: Option<&Value>, args: &[Value]) -> R {
    let f = Func::method("tidy", "getRelease");
    if zpp::parse(vm, &f, "", args).is_err() {
        return Ok(Value::Null);
    }
    Ok(vstr(goro_tidy::message::release_date()))
}

fn m_root(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    create_base_node(vm, this, args, BaseNode::Root, "tidy_get_root", "root")
}

fn m_html(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    create_base_node(vm, this, args, BaseNode::Html, "tidy_get_html", "html")
}

fn m_head(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    create_base_node(vm, this, args, BaseNode::Head, "tidy_get_head", "head")
}

fn m_body(vm: &mut Vm, this: Option<&Value>, args: &[Value]) -> R {
    create_base_node(vm, this, args, BaseNode::Body, "tidy_get_body", "body")
}

const TIDY_METHODS: &[(&str, MethodImpl)] = &[
    ("__construct", m_construct),
    ("getOpt", getopt),
    ("cleanRepair", clean_repair),
    ("parseFile", m_parse_file),
    ("parseString", m_parse_string),
    ("repairString", m_repair_string),
    ("repairFile", m_repair_file),
    ("diagnose", diagnose),
    ("getRelease", m_get_release),
    ("getConfig", get_config),
    ("getStatus", get_status),
    ("getHtmlVer", get_html_ver),
    ("getOptDoc", get_opt_doc),
    ("isXhtml", is_xhtml),
    ("isXml", is_xml),
    ("root", m_root),
    ("head", m_head),
    ("html", m_html),
    ("body", m_body),
];

const STATIC_METHODS: &[&str] = &["repairString", "repairFile"];

fn find_method<'a>(
    table: &'a [(&'static str, MethodImpl)],
    m: &[u8],
) -> Option<&'a (&'static str, MethodImpl)> {
    table
        .iter()
        .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(m))
}

fn tidy_has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    match find_method(TIDY_METHODS, method) {
        Some(m) => !is_static || STATIC_METHODS.contains(&m.0),
        None => false,
    }
}

fn throw_r(vm: &mut Vm, r: R) -> Value {
    match r {
        Ok(v) => v,
        Err(e) => {
            if vm.current_exception.is_none() {
                zpp::throw(vm, b"Error", &e.message);
            }
            Value::Null
        }
    }
}

fn tidy_call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(&(name, imp)) = find_method(TIDY_METHODS, method) else {
        return Value::Null;
    };
    if this.is_none() && !STATIC_METHODS.contains(&name) {
        zpp::throw(
            vm,
            b"Error",
            &format!(
                "Non-static method tidy::{}() cannot be called statically",
                name
            ),
        );
        return Value::Null;
    }
    let this = if STATIC_METHODS.contains(&name) {
        None
    } else {
        this
    };
    let r = imp(vm, this, args);
    throw_r(vm, r)
}

fn param_names_of(class: &'static [u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, m) = goro_core::stubs::find_method(class, method)?;
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<Vec<u8>, &'static [&'static str]>> =
            RefCell::new(std::collections::HashMap::new());
    }
    let mut key = class.to_vec();
    key.extend_from_slice(b"::");
    key.extend_from_slice(&method.to_ascii_lowercase());
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return Some(v);
    }
    let names: Vec<&'static str> = m.args.iter().map(|a| a.name).collect();
    let leaked: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    CACHE.with(|c| c.borrow_mut().insert(key, leaked));
    Some(leaked)
}

fn tidy_param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    param_names_of(b"tidy", method)
}

// ---------------------------------------------------------------------------
// tidyNode methods

fn node_data(this: Option<&Value>) -> Option<Rc<TidyData>> {
    data_of(&this.and_then(obj_of)?)
}

const NODE_METHODS: &[&str] = &[
    "hasChildren",
    "hasSiblings",
    "isComment",
    "isHtml",
    "isText",
    "isJste",
    "isAsp",
    "isPhp",
    "getParent",
    "getPreviousSibling",
    "getNextSibling",
    "__construct",
];

fn node_has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    !is_static
        && NODE_METHODS
            .iter()
            .any(|m| m.as_bytes().eq_ignore_ascii_case(method))
}

fn node_param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    param_names_of(b"tidyNode", method)
}

fn node_call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(&name) = NODE_METHODS
        .iter()
        .find(|m| m.as_bytes().eq_ignore_ascii_case(method))
    else {
        return Value::Null;
    };
    if name == "__construct" {
        // private function __construct(): only callable from the class
        let msg = match vm.current_class_scope() {
            Some(s) if s.eq_ignore_ascii_case(b"tidyNode") => {
                "You should not create a tidyNode manually".to_string()
            }
            Some(s) => format!(
                "Call to private tidyNode::__construct() from scope {}",
                goro_core::value::display_class_name(&s)
            ),
            None => "Call to private tidyNode::__construct() from global scope".to_string(),
        };
        zpp::throw(vm, b"Error", &msg);
        return Value::Null;
    }
    let f = Func::method("tidyNode", name);
    // ZEND_PARSE_PARAMETERS_NONE()
    if zpp::parse(vm, &f, "", args).is_err() {
        return Value::Null;
    }
    let Some(d) = node_data(this) else {
        return Value::Null;
    };
    let node = live_node(&d);
    let (ty, content, next, prev, parent) = {
        let doc = d.ptdoc.doc.borrow();
        match node {
            Some(n) => (
                Some(doc[n].type_),
                doc[n].content,
                doc[n].next,
                doc[n].prev,
                doc[n].parent,
            ),
            None => (None, None, None, None, None),
        }
    };
    match name {
        "hasChildren" => vbool(content.is_some()),
        "hasSiblings" => vbool(next.is_some()),
        "isComment" => vbool(ty == Some(NodeType::CommentTag)),
        "isHtml" => vbool(matches!(
            ty,
            Some(NodeType::StartTag | NodeType::EndTag | NodeType::StartEndTag)
        )),
        "isText" => vbool(ty == Some(NodeType::TextNode)),
        "isJste" => vbool(ty == Some(NodeType::JsteTag)),
        "isAsp" => vbool(ty == Some(NodeType::AspTag)),
        "isPhp" => vbool(ty == Some(NodeType::PhpTag)),
        "getParent" => parent.map_or(Value::Null, |p| create_node_object(vm, &d.ptdoc, p)),
        "getPreviousSibling" => prev.map_or(Value::Null, |p| create_node_object(vm, &d.ptdoc, p)),
        "getNextSibling" => next.map_or(Value::Null, |p| create_node_object(vm, &d.ptdoc, p)),
        _ => Value::Null,
    }
}

// ---------------------------------------------------------------------------
// output handler and INI

/// php_tidy_output_handler()
fn output_handler(vm: &mut Vm, input: &[u8], op: i64) -> Option<Vec<u8>> {
    if !(CLEAN_OUTPUT.with(|c| c.get()) && (op & OB_OP_START) != 0 && (op & OB_OP_FINAL) != 0) {
        return None;
    }
    if input.len() > u32::MAX as usize {
        let p = docref_prefix(vm);
        vm.emit_warning(&format!("{}Input string is too long", p));
        return None;
    }
    let mut doc = Doc::new();
    tidylib::set_error_buffer(&mut doc);
    tidylib::opt_set_bool(&mut doc, OptId::ForceOutput, true);
    tidylib::opt_set_bool(&mut doc, OptId::Mark, false);
    set_default_config(vm, None, &mut doc);
    if tidylib::parse_buffer(&mut doc, input.to_vec()) >= 0
        && tidylib::clean_and_repair(&mut doc) >= 0
    {
        let (_, out) = tidylib::save_buffer(&mut doc);
        return Some(drop_last(out));
    }
    None
}

/// php_tidy_output_handler_init()
fn output_handler_init(
    vm: &mut Vm,
    name: &[u8],
    chunk_size: usize,
    flags: i64,
) -> Option<ObHandler> {
    if chunk_size != 0 {
        let p = docref_prefix(vm);
        vm.emit_warning(&format!("{}Cannot use a chunk size for ob_tidyhandler", p));
        return None;
    }
    CLEAN_OUTPUT.with(|c| c.set(true));
    Some(Vm::ob_make_handler(
        name.to_vec(),
        ObKind::InternalFallible(output_handler),
        chunk_size,
        flags,
    ))
}

/// php_tidy_clean_output_start()
fn clean_output_start(vm: &mut Vm) {
    if CLEAN_OUTPUT.with(|c| c.get()) {
        let h = output_handler_init(vm, b"ob_tidyhandler", 0, goro_core::ob::OBH_STDFLAGS);
        if h.is_some() {
            goro_ext_standard::ob::output_handler_start(vm, h);
        }
    }
}

/// PHP_RINIT_FUNCTION(tidy)
fn request_startup(vm: &mut Vm) {
    let v = ini_parse_bool(&ini_str(vm, b"tidy.clean_output"));
    CLEAN_OUTPUT.with(|c| c.set(v));
    clean_output_start(vm);
}

/// PHP_RSHUTDOWN_FUNCTION(tidy)
fn request_shutdown(vm: &mut Vm) {
    let v = ini_parse_bool(&ini_str(vm, b"tidy.clean_output"));
    CLEAN_OUTPUT.with(|c| c.set(v));
}

/// php_tidy_set_clean_output() at runtime
fn ini_set_clean_output(vm: &mut Vm, _key: &[u8], value: &Value) -> bool {
    let v = ini_parse_bool(value.to_php_string().as_bytes());
    let p = docref_prefix(vm);
    if v && vm.output_start.is_some() {
        vm.emit_warning(&format!(
            "{}Cannot enable tidy.clean_output - there has already been output",
            p
        ));
        return false;
    }
    if vm.headers_sent {
        vm.emit_warning(&format!(
            "{}Cannot change tidy.clean_output - headers already sent",
            p
        ));
        return false;
    }
    CLEAN_OUTPUT.with(|c| c.set(v));
    if v && !vm.ob_handler_started(b"ob_tidyhandler") {
        clean_output_start(vm);
    }
    true
}

/// tidy.default_config is PHP_INI_SYSTEM
fn ini_set_default_config(_vm: &mut Vm, _key: &[u8], _value: &Value) -> bool {
    false
}

/// PHP_MINFO_FUNCTION(tidy)
fn minfo(vm: &mut Vm) -> String {
    let dc = String::from_utf8_lossy(&ini_str(vm, b"tidy.default_config")).into_owned();
    let co = ini_parse_bool(&ini_str(vm, b"tidy.clean_output"));
    let dcv = if dc.is_empty() {
        "no value".to_string()
    } else {
        dc
    };
    format!(
        "\ntidy\n\nTidy support => enabled\nlibTidy Version => {}\nlibTidy Release => {}\n\nDirective => Local Value => Master Value\ntidy.clean_output => {} => {}\ntidy.default_config => {} => {}\n",
        goro_tidy::message::library_version(),
        goro_tidy::message::release_date(),
        if co { "On" } else { "Off" },
        if co { "On" } else { "Off" },
        dcv,
        dcv
    )
}

// ---------------------------------------------------------------------------
// registration

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("tidy_parse_string", tidy_parse_string),
    ("tidy_get_error_buffer", tidy_get_error_buffer),
    ("tidy_get_output", tidy_get_output),
    ("tidy_parse_file", tidy_parse_file),
    ("tidy_clean_repair", tidy_clean_repair),
    ("tidy_repair_string", tidy_repair_string),
    ("tidy_repair_file", tidy_repair_file),
    ("tidy_diagnose", tidy_diagnose),
    ("tidy_get_release", tidy_get_release),
    ("tidy_get_opt_doc", tidy_get_opt_doc),
    ("tidy_get_config", tidy_get_config),
    ("tidy_get_status", tidy_get_status),
    ("tidy_get_html_ver", tidy_get_html_ver),
    ("tidy_is_xhtml", tidy_is_xhtml),
    ("tidy_is_xml", tidy_is_xml),
    ("tidy_error_count", tidy_error_count),
    ("tidy_warning_count", tidy_warning_count),
    ("tidy_access_count", tidy_access_count),
    ("tidy_config_count", tidy_config_count),
    ("tidy_getopt", tidy_getopt),
    ("tidy_get_root", tidy_get_root),
    ("tidy_get_html", tidy_get_html),
    ("tidy_get_head", tidy_get_head),
    ("tidy_get_body", tidy_get_body),
];

const NODETYPES: &[(&str, NodeType)] = &[
    ("TIDY_NODETYPE_ROOT", NodeType::RootNode),
    ("TIDY_NODETYPE_DOCTYPE", NodeType::DocTypeTag),
    ("TIDY_NODETYPE_COMMENT", NodeType::CommentTag),
    ("TIDY_NODETYPE_PROCINS", NodeType::ProcInsTag),
    ("TIDY_NODETYPE_TEXT", NodeType::TextNode),
    ("TIDY_NODETYPE_START", NodeType::StartTag),
    ("TIDY_NODETYPE_END", NodeType::EndTag),
    ("TIDY_NODETYPE_STARTEND", NodeType::StartEndTag),
    ("TIDY_NODETYPE_CDATA", NodeType::CDATATag),
    ("TIDY_NODETYPE_SECTION", NodeType::SectionTag),
    ("TIDY_NODETYPE_ASP", NodeType::AspTag),
    ("TIDY_NODETYPE_JSTE", NodeType::JsteTag),
    ("TIDY_NODETYPE_PHP", NodeType::PhpTag),
    ("TIDY_NODETYPE_XMLDECL", NodeType::XmlDecl),
];

/// The TIDY_TAG_* constants of tidy.stub.php, in its order.
const TAGS: &[TagId] = &[
    TagId::UNKNOWN,
    TagId::A,
    TagId::ABBR,
    TagId::ACRONYM,
    TagId::ADDRESS,
    TagId::ALIGN,
    TagId::APPLET,
    TagId::AREA,
    TagId::B,
    TagId::BASE,
    TagId::BASEFONT,
    TagId::BDO,
    TagId::BGSOUND,
    TagId::BIG,
    TagId::BLINK,
    TagId::BLOCKQUOTE,
    TagId::BODY,
    TagId::BR,
    TagId::BUTTON,
    TagId::CAPTION,
    TagId::CENTER,
    TagId::CITE,
    TagId::CODE,
    TagId::COL,
    TagId::COLGROUP,
    TagId::COMMENT,
    TagId::DD,
    TagId::DEL,
    TagId::DFN,
    TagId::DIR,
    TagId::DIV,
    TagId::DL,
    TagId::DT,
    TagId::EM,
    TagId::EMBED,
    TagId::FIELDSET,
    TagId::FONT,
    TagId::FORM,
    TagId::FRAME,
    TagId::FRAMESET,
    TagId::H1,
    TagId::H2,
    TagId::H3,
    TagId::H4,
    TagId::H5,
    TagId::H6,
    TagId::HEAD,
    TagId::HR,
    TagId::HTML,
    TagId::I,
    TagId::IFRAME,
    TagId::ILAYER,
    TagId::IMG,
    TagId::INPUT,
    TagId::INS,
    TagId::ISINDEX,
    TagId::KBD,
    TagId::KEYGEN,
    TagId::LABEL,
    TagId::LAYER,
    TagId::LEGEND,
    TagId::LI,
    TagId::LINK,
    TagId::LISTING,
    TagId::MAP,
    TagId::MARQUEE,
    TagId::MENU,
    TagId::META,
    TagId::MULTICOL,
    TagId::NOBR,
    TagId::NOEMBED,
    TagId::NOFRAMES,
    TagId::NOLAYER,
    TagId::NOSAVE,
    TagId::NOSCRIPT,
    TagId::OBJECT,
    TagId::OL,
    TagId::OPTGROUP,
    TagId::OPTION,
    TagId::P,
    TagId::PARAM,
    TagId::PLAINTEXT,
    TagId::PRE,
    TagId::Q,
    TagId::RB,
    TagId::RBC,
    TagId::RP,
    TagId::RT,
    TagId::RTC,
    TagId::RUBY,
    TagId::S,
    TagId::SAMP,
    TagId::SCRIPT,
    TagId::SELECT,
    TagId::SERVER,
    TagId::SERVLET,
    TagId::SMALL,
    TagId::SPACER,
    TagId::SPAN,
    TagId::STRIKE,
    TagId::STRONG,
    TagId::STYLE,
    TagId::SUB,
    TagId::SUP,
    TagId::TABLE,
    TagId::TBODY,
    TagId::TD,
    TagId::TEXTAREA,
    TagId::TFOOT,
    TagId::TH,
    TagId::THEAD,
    TagId::TITLE,
    TagId::TR,
    TagId::TT,
    TagId::U,
    TagId::UL,
    TagId::VAR,
    TagId::WBR,
    TagId::XMP,
    TagId::ARTICLE,
    TagId::ASIDE,
    TagId::AUDIO,
    TagId::BDI,
    TagId::CANVAS,
    TagId::COMMAND,
    TagId::DATALIST,
    TagId::DETAILS,
    TagId::DIALOG,
    TagId::FIGCAPTION,
    TagId::FIGURE,
    TagId::FOOTER,
    TagId::HEADER,
    TagId::HGROUP,
    TagId::MAIN,
    TagId::MARK,
    TagId::MENUITEM,
    TagId::METER,
    TagId::NAV,
    TagId::OUTPUT,
    TagId::PROGRESS,
    TagId::SECTION,
    TagId::SOURCE,
    TagId::SUMMARY,
    TagId::TEMPLATE,
    TagId::TIME,
    TagId::TRACK,
    TagId::VIDEO,
];

/// PHP_MINIT_FUNCTION(tidy)
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);

    for (k, d) in [
        (&b"tidy.default_config"[..], &b""[..]),
        (b"tidy.clean_output", b"0"),
    ] {
        vm.constants
            .entry(k.to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(d)));
    }
    vm.ini_set_hooks
        .insert(b"tidy.clean_output".to_vec(), ini_set_clean_output);
    vm.ini_set_hooks
        .insert(b"tidy.default_config".to_vec(), ini_set_default_config);

    // classes
    for (name, handler) in [
        (
            &b"tidy"[..],
            NativeClassHandler {
                has_method: tidy_has_method,
                call: tidy_call,
                prop_added: None,
                prop_write: None,
                compare_check: None,
                prop_read: None,
                param_names: Some(tidy_param_names),
            },
        ),
        (
            &b"tidyNode"[..],
            NativeClassHandler {
                has_method: node_has_method,
                call: node_call,
                prop_added: None,
                prop_write: None,
                compare_check: None,
                prop_read: None,
                param_names: Some(node_param_names),
            },
        ),
    ] {
        let mut ce = match goro_core::stubs::class_entry(vm, name) {
            Some(stub) => stub.clone(),
            None => goro_core::object::ClassEntry::new(name.to_vec()),
        };
        // the methods are served by the native class handler (tidyNode's
        // private constructor stays declared for the visibility check)
        ce.methods
            .retain(|k, _| name == b"tidyNode" && k.as_slice() == b"__construct");
        if name == b"tidyNode" {
            ce.is_final = true;
        }
        // internal classes allow dynamic properties
        ce.allow_dynamic_properties = true;
        let lower = name.to_ascii_lowercase();
        vm.classes.insert(lower.clone(), ce);
        vm.uncloneable_classes.push(lower.clone());
        vm.native_class_handlers.insert(lower, handler);
    }
    vm.object_create_handlers
        .insert(b"tidy".to_vec(), create_doc_object);

    // functions
    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
    }

    // constants
    let mut names: Vec<Vec<u8>> = Vec::new();
    for &(n, t) in NODETYPES {
        vm.constants
            .insert(n.as_bytes().to_vec(), Value::Long(t as i64));
        names.push(n.as_bytes().to_vec());
    }
    for &t in TAGS {
        let n = format!("TIDY_TAG_{:?}", t);
        vm.constants
            .insert(n.as_bytes().to_vec(), Value::Long(t as i64));
        names.push(n.into_bytes());
    }
    vm.module_constants.push((NAME.to_vec(), names));

    goro_ext_standard::ob::register_output_handler_alias(b"ob_tidyhandler", output_handler_init);
    vm.request_startup_hooks.push(request_startup);
    vm.request_shutdown_hooks.push(request_shutdown);
}

/// The PhpObject type (for tests of the native data).
pub fn is_tidy_object(o: &PhpObject) -> bool {
    o.native
        .as_ref()
        .is_some_and(|n| n.data.clone().downcast::<TidyData>().is_ok())
}
