//! ext/xml: the expat-compatible SAX API (xml_parser_create, xml_parse,
//! xml_parse_into_struct, xml_set_*_handler...), following php-src's
//! ext/xml/xml.c on top of its libxml2 compatibility layer (compat.c), with
//! the push parser of `parser.rs` standing in for libxml2.

use crate::parser::{self, EntKind, PEntity, Parser, ParserState, SaxAttr, Sink};
use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

type Obj = Rc<RefCell<PhpObject>>;

const XML_MAXLEVEL: i64 = 255;

const OPT_CASE_FOLDING: i64 = 1;
const OPT_TARGET_ENCODING: i64 = 2;
const OPT_SKIP_TAGSTART: i64 = 3;
const OPT_SKIP_WHITE: i64 = 4;
const OPT_PARSE_HUGE: i64 = 5;

/// XML_ERROR_EXTERNAL_ENTITY_HANDLING
const XML_ERROR_EXTERNAL_ENTITY_HANDLING: i32 = 21;

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers::EMPTY;

// ---------------------------------------------------------------------------
// Parser state

/// A resolved handler (zend_fcall_info_cache).
#[derive(Clone)]
enum Handler {
    Callable(Value),
    /// A method looked up by name on the xml_set_object() object.
    Method {
        obj: Value,
        name: Vec<u8>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Start,
    End,
    CharData,
    Pi,
    Default,
    Unparsed,
    Notation,
    ExtEntity,
    StartNs,
    EndNs,
}

const ALL_SLOTS: [(Slot, &str); 10] = [
    (Slot::Start, "xml_set_element_handler()"),
    (Slot::End, "xml_set_element_handler()"),
    (Slot::CharData, "xml_set_character_data_handler()"),
    (Slot::Pi, "xml_set_processing_instruction_handler()"),
    (Slot::Default, "xml_set_default_handler()"),
    (Slot::Unparsed, "xml_set_unparsed_entity_decl_handler()"),
    (Slot::Notation, "xml_set_notation_decl_handler()"),
    (Slot::ExtEntity, "xml_set_external_entity_ref_handler()"),
    (Slot::StartNs, "xml_set_start_namespace_decl_handler()"),
    (Slot::EndNs, "xml_set_end_namespace_decl_handler()"),
];

/// Which compat-layer (C level) handlers are installed: XML_Set*Handler is
/// called by the setters even when the PHP handler is null.
#[derive(Default, Clone, Copy)]
struct Installed {
    element: bool,
    cdata: bool,
    pi: bool,
    default: bool,
    unparsed: bool,
    notation: bool,
    ext_entity: bool,
    start_ns: bool,
}

struct XmlState {
    handlers: [Option<Handler>; 10],
    installed: Installed,
    object: Option<Value>,
    /// The XMLParser object itself (not owned).
    index: Weak<RefCell<PhpObject>>,
    target_encoding: &'static str,
    case_folding: bool,
    skipwhite: bool,
    toffset: i64,
    parsehuge: bool,
    isparsing: bool,
    /// The function running the parser (for warnings from handlers).
    cur_fn: &'static str,
    ns_separator: Vec<u8>,
    // xml_parse_into_struct
    data: Option<Rc<RefCell<Value>>>,
    info: Option<Rc<RefCell<Value>>>,
    level: i64,
    curtag: i64,
    ctag_index: i64,
    ltags: Option<Vec<Option<Vec<u8>>>>,
    lastwasopen: bool,
    engine: Option<Box<Engine>>,
    /// Position / error code while the engine is checked out (parsing).
    live: (u32, u32, u64, i32),
}

fn slot_idx(s: Slot) -> usize {
    ALL_SLOTS.iter().position(|(x, _)| *x == s).unwrap()
}

impl XmlState {
    fn handler(&self, s: Slot) -> Option<Handler> {
        self.handlers[slot_idx(s)].clone()
    }
}

type State = RefCell<XmlState>;

/// The libxml2 parser context with its compat SAX handler.
struct Engine {
    parser: std::mem::ManuallyDrop<Parser<'static, CompatSink>>,
    sink: *mut CompatSink,
}

impl Engine {
    fn new(use_namespace: bool) -> Box<Engine> {
        let sink: &'static mut CompatSink = Box::leak(Box::new(CompatSink {
            ctx: None,
            use_namespace,
            failed: None,
        }));
        let ptr = sink as *mut CompatSink;
        // php_libxml_sanitize_parse_ctxt_options + XML_PARSE_OLDSAX | XML_PARSE_NOENT
        let mut p = Parser::new(sink, Vec::new(), None, parser::XML_PARSE_NOENT);
        p.st.sax1 = !use_namespace;
        p.st.sax_get_entity = true;
        // compat.c: parser->parser->wellFormed = 0
        p.st.well_formed = false;
        Box::new(Engine {
            parser: std::mem::ManuallyDrop::new(p),
            sink: ptr,
        })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self.parser);
            drop(Box::from_raw(self.sink));
        }
    }
}

fn state_of(v: &Value) -> Option<(Obj, Rc<State>)> {
    match v {
        Value::Object(o) => {
            let st = {
                let b = o.borrow();
                let n = b.native.as_ref()?;
                n.data.clone().downcast::<State>().ok()?
            };
            Some((o.clone(), st))
        }
        Value::Reference(r) => state_of(&r.borrow()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Encoding helpers (xml_utf8_decode)

fn xml_get_encoding(name: &[u8]) -> Option<&'static str> {
    ["ISO-8859-1", "US-ASCII", "UTF-8"]
        .into_iter()
        .find(|e| e.as_bytes().eq_ignore_ascii_case(name))
}

/// php_next_utf8_char-based decoding into the target encoding.
fn utf8_decode(s: &[u8], enc: &str) -> Vec<u8> {
    let max = match enc {
        "ISO-8859-1" => 0xFF,
        "US-ASCII" => 0x7F,
        _ => return s.to_vec(),
    };
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (c, len) = match parser::utf8_char(s, i) {
            Some((c, l)) => (c, l),
            None => (u32::MAX, 1),
        };
        i += len;
        out.push(if c > max { b'?' } else { c as u8 });
    }
    out
}

fn pstr(b: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(b))
}

fn str_or_false(s: Option<&str>, enc: &str) -> Value {
    match s {
        Some(s) => pstr(utf8_decode(s.as_bytes(), enc)),
        None => Value::False,
    }
}

// ---------------------------------------------------------------------------
// The compat SAX handler

struct CompatSink {
    /// Set while parsing: the PHP-level state and the VM.
    ctx: Option<(Rc<State>, *mut Vm)>,
    use_namespace: bool,
    /// A callback raised an exception: no more callbacks are made.
    failed: Option<VmError>,
}

impl CompatSink {
    fn state(&self) -> Option<Rc<State>> {
        self.ctx.as_ref().map(|(s, _)| s.clone())
    }

    /// The VM running the parse (set for the duration of xml_parse()).
    fn vm_ptr(&self) -> *mut Vm {
        self.ctx.as_ref().unwrap().1
    }

    fn sep(&self, st: &XmlState) -> Vec<u8> {
        st.ns_separator
            .first()
            .map(|b| vec![*b])
            .unwrap_or_default()
    }

    /// qualify_namespace
    fn qualify(&self, st: &XmlState, name: &str, uri: Option<&str>) -> Vec<u8> {
        match uri {
            Some(u) => {
                let mut q = u.as_bytes().to_vec();
                q.extend_from_slice(&self.sep(st));
                q.extend_from_slice(name.as_bytes());
                q
            }
            None => name.as_bytes().to_vec(),
        }
    }

    fn note_position(&self, p: &mut ParserState) {
        if let Some(st) = self.state() {
            let (l, c, b) = p.position();
            st.borrow_mut().live = (l, c, b, p.err_no);
        }
    }

    /// zend_call_known_fcc: no-op once an exception is pending.
    fn call(&mut self, h: &Handler, args: Vec<Value>) -> Option<Value> {
        if self.failed.is_some() {
            return None;
        }
        // SAFETY: the pointer is the VM that called xml_parse() and outlives it
        let vm = unsafe { &mut *self.vm_ptr() };
        let callable = match h {
            Handler::Callable(c) => c.clone(),
            Handler::Method { obj, name } => {
                let mut arr = PhpArray::new();
                arr.push(obj.clone());
                arr.push(pstr(name.clone()));
                Value::Array(Rc::new(RefCell::new(arr)))
            }
        };
        let r = match vm.functions.get(&b"call_user_func"[..]).copied() {
            Some(cuf) => {
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(callable);
                a.extend(args);
                cuf(vm, &a)
            }
            None => vm.call_callback(&callable, &args),
        };
        match r {
            Ok(v) => {
                if vm.current_exception.is_some() || vm.exit_requested {
                    self.failed = Some(VmError {
                        message: String::new(),
                        line: vm.current_line,
                    });
                    return None;
                }
                Some(v)
            }
            Err(e) => {
                self.failed = Some(e);
                None
            }
        }
    }

    fn index(&self) -> Value {
        match self.state().and_then(|s| s.borrow().index.upgrade()) {
            Some(o) => Value::Object(o),
            None => Value::Null,
        }
    }

    fn check_failed(&self, p: &mut ParserState) -> bool {
        if self.failed.is_some() {
            p.stop = true;
            return true;
        }
        false
    }

    // -- xml.c handlers ---------------------------------------------------

    fn decode_tag(st: &XmlState, tag: &[u8]) -> Vec<u8> {
        let mut s = utf8_decode(tag, st.target_encoding);
        if st.case_folding {
            s.make_ascii_uppercase();
        }
        s
    }

    fn stripped(tag: &[u8], off: i64) -> Vec<u8> {
        let off = off.max(0) as usize;
        if off >= tag.len() {
            if off == 0 { tag.to_vec() } else { Vec::new() }
        } else {
            tag[off..].to_vec()
        }
    }

    fn x_default(&mut self, s: &[u8]) {
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, enc) = {
            let b = st.borrow();
            (b.handler(Slot::Default), b.target_encoding)
        };
        if let Some(h) = h {
            let args = vec![self.index(), pstr(utf8_decode(s, enc))];
            self.call(&h, args);
        }
    }

    fn x_start_element(&mut self, name: &[u8], attrs: &[(Vec<u8>, String)]) {
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, tag_name, toffset, enc, attr_arr) = {
            let mut b = st.borrow_mut();
            b.level += 1;
            let tag_name = Self::decode_tag(&b, name);
            let mut arr = PhpArray::new();
            for (k, v) in attrs {
                let key = Self::decode_tag(&b, k);
                let val = utf8_decode(v.as_bytes(), b.target_encoding);
                arr.set(Vm::value_to_array_key(pstr(key)), pstr(val));
            }
            (
                b.handler(Slot::Start),
                tag_name,
                b.toffset,
                b.target_encoding,
                arr,
            )
        };
        let _ = enc;
        if let Some(h) = h {
            let args = vec![
                self.index(),
                pstr(Self::stripped(&tag_name, toffset)),
                Value::Array(Rc::new(RefCell::new(attr_arr.clone()))),
            ];
            self.call(&h, args);
        }
        if self.failed.is_some() {
            return;
        }
        let mut b = st.borrow_mut();
        if b.data.is_none() {
            return;
        }
        if b.level <= XML_MAXLEVEL {
            let stripped = Self::stripped(&tag_name, b.toffset);
            add_to_info(&mut b, &stripped);
            let mut tag = PhpArray::new();
            tag.set(skey("tag"), pstr(stripped));
            tag.set(skey("type"), pstr(b"open".to_vec()));
            tag.set(skey("level"), Value::Long(b.level));
            let lvl = b.level as usize;
            if let Some(lt) = b.ltags.as_mut() {
                lt[lvl - 1] = Some(tag_name.clone());
            }
            b.lastwasopen = true;
            if !attr_arr.is_empty() {
                tag.set(
                    skey("attributes"),
                    Value::Array(Rc::new(RefCell::new(attr_arr))),
                );
            }
            let data = b.data.clone().unwrap();
            if let Some(arr) = separated_array(&data) {
                let mut a = arr.borrow_mut();
                // nNextFreeElement is ZEND_LONG_MIN until an integer key exists.
                let k = a.next_free_key().max(0);
                if a.try_push(Value::Array(Rc::new(RefCell::new(tag)))).is_ok() {
                    b.ctag_index = k;
                }
            }
        } else if b.level == XML_MAXLEVEL + 1 {
            drop(b);
            self.depth_warning();
        }
    }

    fn x_end_element(&mut self, name: &[u8]) {
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, tag_name, toffset) = {
            let b = st.borrow();
            (b.handler(Slot::End), Self::decode_tag(&b, name), b.toffset)
        };
        if let Some(h) = h {
            let args = vec![self.index(), pstr(Self::stripped(&tag_name, toffset))];
            self.call(&h, args);
        }
        let mut b = st.borrow_mut();
        if b.data.is_some() && self.failed.is_none() {
            let data = b.data.clone().unwrap();
            if b.lastwasopen {
                if let Some(ctag) = ctag(&data, b.ctag_index) {
                    ctag.borrow_mut()
                        .set(skey("type"), pstr(b"complete".to_vec()));
                }
            } else {
                let stripped = Self::stripped(&tag_name, b.toffset);
                add_to_info(&mut b, &stripped);
                if let Some(arr) = separated_array(&data) {
                    let mut tag = PhpArray::new();
                    tag.set(skey("tag"), pstr(stripped));
                    tag.set(skey("type"), pstr(b"close".to_vec()));
                    tag.set(skey("level"), Value::Long(b.level));
                    let _ = arr
                        .borrow_mut()
                        .try_push(Value::Array(Rc::new(RefCell::new(tag))));
                }
            }
            b.lastwasopen = false;
        }
        let lvl = b.level;
        if (1..=XML_MAXLEVEL).contains(&lvl) {
            if let Some(lt) = b.ltags.as_mut() {
                lt[(lvl - 1) as usize] = None;
            }
        }
        b.level -= 1;
    }

    fn x_character_data(&mut self, s: &[u8]) {
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, enc) = {
            let b = st.borrow();
            (b.handler(Slot::CharData), b.target_encoding)
        };
        if let Some(h) = h {
            let args = vec![self.index(), pstr(utf8_decode(s, enc))];
            self.call(&h, args);
        }
        let mut b = st.borrow_mut();
        if b.data.is_none() || self.failed.is_some() {
            return;
        }
        let data = b.data.clone().unwrap();
        let decoded = utf8_decode(s, b.target_encoding);
        let mut doprint = false;
        if b.skipwhite {
            doprint = decoded.iter().any(|c| !matches!(c, b' ' | b'\t' | b'\n'));
        }
        if b.lastwasopen {
            let ctag = match ctag(&data, b.ctag_index) {
                Some(c) => c,
                None => return,
            };
            let mut c = ctag.borrow_mut();
            let k = skey("value");
            if let Some(Value::String(old)) = c.get(&k) {
                let mut v = old.as_bytes().to_vec();
                v.extend_from_slice(&decoded);
                c.set(k, pstr(v));
            } else if doprint || !b.skipwhite {
                c.set(k, pstr(decoded));
            }
        } else {
            let arr = match separated_array(&data) {
                Some(a) => a,
                None => return,
            };
            {
                let mut a = arr.borrow_mut();
                let last_key = a.keys().last().cloned();
                if let Some(lk) = last_key {
                    if let Some(Value::Array(cur)) = a.get(&lk).cloned() {
                        let is_cdata = matches!(cur.borrow().get(&skey("type")), Some(Value::String(t)) if t.as_bytes() == b"cdata");
                        if is_cdata {
                            let cur = separate_rc(cur);
                            a.set(lk, Value::Array(cur.clone()));
                            let mut c = cur.borrow_mut();
                            let k = skey("value");
                            if let Some(v) = c.get(&k).cloned() {
                                let mut nv = v.to_php_string().as_bytes().to_vec();
                                nv.extend_from_slice(&decoded);
                                c.set(k, pstr(nv));
                                return;
                            }
                        }
                    }
                }
            }
            if b.level <= XML_MAXLEVEL && b.level > 0 && (doprint || !b.skipwhite) {
                let lt = b
                    .ltags
                    .as_ref()
                    .and_then(|l| l[(b.level - 1) as usize].clone())
                    .unwrap_or_default();
                let stripped = Self::stripped(&lt, b.toffset);
                add_to_info(&mut b, &stripped);
                let mut tag = PhpArray::new();
                tag.set(skey("tag"), pstr(stripped));
                tag.set(skey("value"), pstr(decoded));
                tag.set(skey("type"), pstr(b"cdata".to_vec()));
                tag.set(skey("level"), Value::Long(b.level));
                let _ = arr
                    .borrow_mut()
                    .try_push(Value::Array(Rc::new(RefCell::new(tag))));
            } else if b.level == XML_MAXLEVEL + 1 {
                drop(b);
                self.depth_warning();
            }
        }
    }

    // -- compat.c handlers -------------------------------------------------

    /// php_error_docref from a handler: prefixed with the parsing function.
    fn depth_warning(&self) {
        let f = self
            .state()
            .map(|s| s.borrow().cur_fn)
            .unwrap_or("xml_parse");
        // SAFETY: as in call_handler
        let vm = unsafe { &mut *self.vm_ptr() };
        vm.emit_warning(&format!(
            "{}(): Maximum depth exceeded - Results truncated",
            f
        ));
    }

    fn installed(&self) -> Installed {
        self.state()
            .map(|s| s.borrow().installed)
            .unwrap_or_default()
    }

    /// cdata_handler
    fn cdata(&mut self, text: &str) {
        let inst = self.installed();
        if !inst.cdata {
            if inst.default {
                self.x_default(text.as_bytes());
            }
            return;
        }
        self.x_character_data(text.as_bytes());
    }

    /// start_element_emit_default: the raw start tag from the input.
    fn emit_default_start(&mut self, p: &mut ParserState) {
        if !self.installed().default {
            return;
        }
        let raw = {
            let inp = match p.inputs.last() {
                Some(i) => i,
                None => return,
            };
            let d = &inp.data;
            let end = inp.pos.min(d.len().saturating_sub(1));
            let mut cur = end;
            while cur > 0 && d[cur] != b'<' {
                cur -= 1;
            }
            let mut t = d[cur..=end].to_vec();
            if d.get(end) == Some(&b'/') {
                let n = t.len();
                t[n - 1] = b'>';
            }
            t
        };
        self.x_default(&raw);
    }
}

fn skey(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

fn separate_rc(a: Rc<RefCell<PhpArray>>) -> Rc<RefCell<PhpArray>> {
    if Rc::strong_count(&a) > 1 {
        Rc::new(RefCell::new(a.borrow().clone()))
    } else {
        a
    }
}

/// xml_get_separated_data: the array behind the by-reference argument.
fn separated_array(r: &Rc<RefCell<Value>>) -> Option<Rc<RefCell<PhpArray>>> {
    let mut v = r.borrow_mut();
    let inner: &mut Value = &mut v;
    if let Value::Array(a) = inner {
        let s = separate_rc(a.clone());
        *a = s.clone();
        return Some(s);
    }
    None
}

/// xml_get_ctag
fn ctag(data: &Rc<RefCell<Value>>, idx: i64) -> Option<Rc<RefCell<PhpArray>>> {
    let arr = separated_array(data)?;
    let mut a = arr.borrow_mut();
    let k = ArrayKey::Int(idx);
    let cur = match a.get(&k)? {
        Value::Array(c) => c.clone(),
        Value::Reference(r) => {
            if let Value::Array(c) = &*r.borrow() {
                return Some(c.clone());
            }
            return None;
        }
        _ => return None,
    };
    let s = separate_rc(cur);
    a.set(k, Value::Array(s.clone()));
    Some(s)
}

/// xml_add_to_info
fn add_to_info(b: &mut XmlState, name: &[u8]) {
    let info = match &b.info {
        Some(i) => i.clone(),
        None => return,
    };
    let arr = match separated_array(&info) {
        Some(a) => a,
        None => return,
    };
    {
        let mut a = arr.borrow_mut();
        let key = Vm::value_to_array_key(pstr(name.to_vec()));
        let list = match a.get(&key) {
            Some(Value::Array(l)) => separate_rc(l.clone()),
            _ => Rc::new(RefCell::new(PhpArray::new())),
        };
        list.borrow_mut().push(Value::Long(b.curtag));
        a.set(key, Value::Array(list));
    }
    b.curtag += 1;
}

impl Sink for CompatSink {
    fn start_element(
        &mut self,
        p: &mut ParserState,
        localname: &str,
        _prefix: Option<&str>,
        uri: Option<&str>,
        nsdecls: &[(Option<String>, String)],
        attrs: &[SaxAttr],
        _nb_defaulted: usize,
        _line: u32,
    ) {
        if self.check_failed(p) {
            return;
        }
        self.note_position(p);
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let inst = st.borrow().installed;
        if self.use_namespace {
            // start_element_handler_ns
            if !nsdecls.is_empty() && inst.start_ns {
                let (h, enc) = {
                    let b = st.borrow();
                    (b.handler(Slot::StartNs), b.target_encoding)
                };
                for (pfx, u) in nsdecls {
                    if let Some(h) = &h {
                        let args = vec![
                            self.index(),
                            str_or_false(pfx.as_deref(), enc),
                            str_or_false(Some(u), enc),
                        ];
                        self.call(h, args);
                    }
                }
            }
            if !inst.element {
                self.emit_default_start(p);
                return;
            }
            let (name, attrs) = {
                let b = st.borrow();
                let name = self.qualify(&b, localname, uri);
                let attrs: Vec<(Vec<u8>, String)> = attrs
                    .iter()
                    .map(|a| {
                        let n = if a.prefix.is_some() {
                            self.qualify(&b, &a.localname, a.uri.as_deref())
                        } else {
                            a.localname.as_bytes().to_vec()
                        };
                        (n, a.value.clone())
                    })
                    .collect();
                (name, attrs)
            };
            self.x_start_element(&name, &attrs);
        } else {
            // start_element_handler (SAX1)
            if !inst.element {
                self.emit_default_start(p);
                return;
            }
            let attrs: Vec<(Vec<u8>, String)> = attrs
                .iter()
                .map(|a| (a.localname.as_bytes().to_vec(), a.value.clone()))
                .collect();
            self.x_start_element(localname.as_bytes(), &attrs);
        }
        self.check_failed(p);
    }

    fn end_element(
        &mut self,
        p: &mut ParserState,
        localname: &str,
        prefix: Option<&str>,
        uri: Option<&str>,
    ) {
        if localname.starts_with('\u{0}') || self.check_failed(p) {
            return;
        }
        self.note_position(p);
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let inst = st.borrow().installed;
        if !inst.element {
            if inst.default {
                let s = match (self.use_namespace, prefix) {
                    (true, Some(pf)) => format!("</{}:{}>", pf, localname),
                    _ => format!("</{}>", localname),
                };
                self.x_default(s.as_bytes());
            }
            return;
        }
        let name = if self.use_namespace {
            self.qualify(&st.borrow(), localname, uri)
        } else {
            localname.as_bytes().to_vec()
        };
        self.x_end_element(&name);
        self.check_failed(p);
    }

    fn characters(&mut self, p: &mut ParserState, text: &str) {
        if self.check_failed(p) {
            return;
        }
        self.note_position(p);
        self.cdata(text);
        self.check_failed(p);
    }

    fn cdata_block(&mut self, p: &mut ParserState, text: &str) {
        self.characters(p, text);
    }

    fn comment(&mut self, p: &mut ParserState, text: &str) {
        if self.check_failed(p) {
            return;
        }
        if self.installed().default {
            self.note_position(p);
            let s = format!("<!--{}-->", text);
            self.x_default(s.as_bytes());
        }
        self.check_failed(p);
    }

    fn pi(&mut self, p: &mut ParserState, target: &str, data: Option<&str>) {
        if self.check_failed(p) {
            return;
        }
        self.note_position(p);
        let inst = self.installed();
        if !inst.pi {
            if inst.default {
                let s = format!("<?{} {}?>", target, data.unwrap_or("(null)"));
                self.x_default(s.as_bytes());
            }
            return;
        }
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, enc) = {
            let b = st.borrow();
            (b.handler(Slot::Pi), b.target_encoding)
        };
        if let Some(h) = h {
            let args = vec![
                self.index(),
                str_or_false(Some(target), enc),
                str_or_false(data, enc),
            ];
            self.call(&h, args);
        }
        self.check_failed(p);
    }

    fn unparsed_entity_decl(
        &mut self,
        name: &str,
        public: Option<&str>,
        system: Option<&str>,
        notation: &str,
    ) {
        if self.failed.is_some() || !self.installed().unparsed {
            return;
        }
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, enc) = {
            let b = st.borrow();
            (b.handler(Slot::Unparsed), b.target_encoding)
        };
        if let Some(h) = h {
            let args = vec![
                self.index(),
                str_or_false(Some(name), enc),
                Value::False,
                str_or_false(system, enc),
                str_or_false(public, enc),
                str_or_false(Some(notation), enc),
            ];
            self.call(&h, args);
        }
    }

    fn notation_decl(&mut self, name: &str, public: Option<&str>, system: Option<&str>) {
        if self.failed.is_some() || !self.installed().notation {
            return;
        }
        let st = match self.state() {
            Some(s) => s,
            None => return,
        };
        let (h, enc) = {
            let b = st.borrow();
            (b.handler(Slot::Notation), b.target_encoding)
        };
        if let Some(h) = h {
            let args = vec![
                self.index(),
                str_or_false(Some(name), enc),
                Value::False,
                str_or_false(system, enc),
                str_or_false(public, enc),
            ];
            self.call(&h, args);
        }
    }

    /// compat.c get_entity
    fn get_entity(
        &mut self,
        p: &mut ParserState,
        name: &str,
        ent: Option<&PEntity>,
        in_content: bool,
    ) {
        if self.check_failed(p) {
            return;
        }
        if ent.is_some() && !in_content {
            return;
        }
        self.note_position(p);
        let inst = self.installed();
        let internal = match ent {
            None => true,
            Some(e) => matches!(
                e.kind,
                EntKind::InternalGeneral | EntKind::InternalParameter | EntKind::Predefined
            ),
        };
        if internal {
            let predefined = ent.map(|e| e.kind == EntKind::Predefined).unwrap_or(false);
            if inst.default && !(predefined && inst.cdata) {
                let s = format!("&{};", name);
                self.x_default(s.as_bytes());
            } else if inst.cdata {
                if let Some(e) = ent {
                    let c = e.content.clone();
                    self.x_character_data(c.as_bytes());
                }
            }
        } else if let Some(e) = ent {
            if e.kind == EntKind::ExternalGeneralParsed && inst.ext_entity {
                // external_entity_ref_handler
                let st = match self.state() {
                    Some(s) => s,
                    None => return,
                };
                let (h, enc) = {
                    let b = st.borrow();
                    (b.handler(Slot::ExtEntity), b.target_encoding)
                };
                let ret = match h {
                    None => 0,
                    Some(h) => {
                        let args = vec![
                            self.index(),
                            str_or_false(Some(name), enc),
                            pstr(Vec::new()),
                            str_or_false(e.system.as_deref(), enc),
                            str_or_false(e.public.as_deref(), enc),
                        ];
                        match self.call(&h, args) {
                            Some(v) => v.to_long(),
                            None => 0,
                        }
                    }
                };
                if ret == 0 {
                    p.disable_sax = 2;
                    p.err_no = XML_ERROR_EXTERNAL_ENTITY_HANDLING;
                }
            }
        }
        self.check_failed(p);
    }
}

// ---------------------------------------------------------------------------
// Argument helpers

fn throw(vm: &mut Vm, class: &[u8], msg: String) -> VmError {
    let e = vm.create_exception(class, &msg, vm.current_line);
    vm.current_exception = Some(e);
    let cls = String::from_utf8_lossy(class).into_owned();
    VmError {
        message: format!("Uncaught {}: {}", cls, msg),
        line: vm.current_line,
    }
}

fn zend_type_name(v: &Value) -> String {
    match v {
        Value::Null | Value::Undef => "null".into(),
        Value::True | Value::False => "bool".into(),
        Value::Long(_) => "int".into(),
        Value::Double(_) => "float".into(),
        Value::String(_) => "string".into(),
        Value::Array(_) => "array".into(),
        Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
        Value::Reference(r) => zend_type_name(&r.borrow()),
        _ => v.type_name().into(),
    }
}

fn deref(v: &Value) -> Value {
    match v {
        Value::Reference(r) => r.borrow().clone(),
        _ => v.clone(),
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).map(deref).unwrap_or(Value::Null)
}

fn get_parser(vm: &mut Vm, f: &str, args: &[Value]) -> Result<(Obj, Rc<State>), VmError> {
    let v = arg(args, 0);
    match state_of(&v) {
        Some(x) => Ok(x),
        None => Err(throw(
            vm,
            b"TypeError",
            format!(
                "{}(): Argument #1 ($parser) must be of type XMLParser, {} given",
                f,
                zend_type_name(&v)
            ),
        )),
    }
}

fn is_callable(vm: &mut Vm, v: &Value) -> bool {
    vm.zend_is_callable_ex(v, 0).ok
}

/// php_xml_check_string_method_arg; `arg_num` 0 means "don't report".
fn check_string_method(
    vm: &mut Vm,
    f: &str,
    arg_num: usize,
    pname: &str,
    object: Option<&Value>,
    method: &[u8],
) -> Result<Option<Handler>, VmError> {
    if method.is_empty() {
        return Ok(None);
    }
    let obj = match object {
        Some(o) => o.clone(),
        None => {
            return Err(throw(
                vm,
                b"ValueError",
                format!(
                    "{}(): Argument #{} (${}) an object must be set via xml_set_object() to be able to lookup method",
                    f, arg_num, pname
                ),
            ));
        }
    };
    match find_method(vm, &obj, method) {
        Some(name) => Ok(Some(Handler::Method { obj, name })),
        None => {
            let cname = match &obj {
                Value::Object(o) => goro_core::value::display_class_name(&o.borrow().class_name),
                _ => String::new(),
            };
            Err(throw(
                vm,
                b"ValueError",
                format!(
                    "{}(): Argument #{} (${}) method {}::{}() does not exist",
                    f,
                    arg_num,
                    pname,
                    cname,
                    String::from_utf8_lossy(method)
                ),
            ))
        }
    }
}

/// The declared name of `method` in the object's class, if it exists.
fn find_method(vm: &Vm, obj: &Value, method: &[u8]) -> Option<Vec<u8>> {
    let o = match obj {
        Value::Object(o) => o,
        _ => return None,
    };
    let lower = method.to_ascii_lowercase();
    let mut cls = o.borrow().class_name.to_ascii_lowercase();
    for _ in 0..64 {
        let ce = vm.classes.get(&cls)?;
        if let Some(m) = ce.methods.get(&lower) {
            return Some(if m.name.is_empty() {
                method.to_vec()
            } else {
                m.name.clone()
            });
        }
        cls = ce.parent.as_ref()?.to_ascii_lowercase();
    }
    None
}

enum Parsed {
    Unset,
    Fcc(Handler),
    Str(Vec<u8>),
    Bad,
}

fn classify(vm: &mut Vm, v: &Value) -> Parsed {
    match v {
        Value::Null | Value::Undef => Parsed::Unset,
        _ if is_callable(vm, v) => Parsed::Fcc(Handler::Callable(v.clone())),
        Value::String(s) => Parsed::Str(s.as_bytes().to_vec()),
        _ => Parsed::Bad,
    }
}

fn deprecated_string(vm: &mut Vm, f: &str) {
    vm.emit_deprecated(&format!(
        "{}(): Passing non-callable strings is deprecated since 8.4",
        f
    ));
}

// ---------------------------------------------------------------------------
// Functions

fn create_impl(vm: &mut Vm, args: &[Value], ns: bool) -> Result<Value, VmError> {
    let f = if ns {
        "xml_parser_create_ns"
    } else {
        "xml_parser_create"
    };
    let enc_arg = arg(args, 0);
    let target: &'static str = match &enc_arg {
        Value::Null | Value::Undef => "UTF-8",
        v => {
            let s = vm.value_to_string_checked(v)?;
            let b = s.as_bytes();
            if b.is_empty() {
                "UTF-8"
            } else if let Some(e) = xml_get_encoding(b) {
                e
            } else {
                return Err(throw(
                    vm,
                    b"ValueError",
                    format!(
                        "{}(): Argument #1 ($encoding) is not a supported source encoding",
                        f
                    ),
                ));
            }
        }
    };
    let sep = if ns {
        match args.get(1).map(deref) {
            None | Some(Value::Undef) => b":".to_vec(),
            Some(v) => vm.value_to_string_checked(&v)?.as_bytes().to_vec(),
        }
    } else {
        Vec::new()
    };
    let id = vm.next_object_id();
    let obj = Rc::new(RefCell::new(PhpObject::new(b"XMLParser".to_vec(), id)));
    let state = XmlState {
        handlers: Default::default(),
        installed: Installed::default(),
        object: None,
        index: Rc::downgrade(&obj),
        target_encoding: target,
        case_folding: true,
        skipwhite: false,
        toffset: 0,
        parsehuge: false,
        isparsing: false,
        cur_fn: "xml_parse",
        ns_separator: sep,
        data: None,
        info: None,
        level: 0,
        curtag: 0,
        ctag_index: 0,
        ltags: None,
        lastwasopen: false,
        engine: Some(Engine::new(ns)),
        live: (1, 1, 0, 0),
    };
    let data: Rc<dyn std::any::Any> = Rc::new(RefCell::new(state));
    obj.borrow_mut().native = Some(NativeData {
        data,
        handlers: &HANDLERS,
    });
    Ok(Value::Object(obj))
}

fn xml_parser_create(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    create_impl(vm, args, false)
}

fn xml_parser_create_ns(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    create_impl(vm, args, true)
}

/// xml_parse_helper
fn parse_helper(
    vm: &mut Vm,
    st: &Rc<State>,
    data: &[u8],
    is_final: bool,
    f: &'static str,
) -> Result<i64, VmError> {
    let mut engine = {
        let mut b = st.borrow_mut();
        b.cur_fn = f;
        let huge = b.parsehuge;
        let mut e = match b.engine.take() {
            Some(e) => e,
            None => return Ok(0),
        };
        if huge {
            e.parser.st.options |= parser::XML_PARSE_HUGE;
        } else {
            e.parser.st.options &= !parser::XML_PARSE_HUGE;
        }
        b.isparsing = true;
        e
    };
    engine.parser.sink.ctx = Some((st.clone(), vm as *mut Vm));
    engine.parser.sink.failed = None;
    let err = engine.parser.parse_chunk(data, is_final);
    let ret = if err != 0 {
        0
    } else {
        (engine.parser.st.last_err_level <= parser::ERR_WARNING) as i64
    };
    engine.parser.sink.ctx = None;
    let failed = engine.parser.sink.failed.take();
    // libxml2 errors of ext/xml's parser reach xmlLastError and, with
    // libxml_use_internal_errors(), the error list; never warnings
    let errs = std::mem::take(&mut engine.parser.st.errors);
    if !errs.is_empty() {
        crate::libxml::report_ex(vm, f, &errs, crate::libxml::Style::Generic, &|_| true);
    }
    let mut b = st.borrow_mut();
    b.isparsing = false;
    let (l, c, bi) = engine.parser.st.position();
    b.live = (l, c, bi, engine.parser.st.err_no);
    b.engine = Some(engine);
    drop(b);
    match failed {
        Some(e) => {
            if vm.current_exception.is_some() || !e.message.is_empty() {
                Err(e)
            } else {
                Ok(ret)
            }
        }
        None => Ok(ret),
    }
}

fn data_arg(vm: &mut Vm, args: &[Value], i: usize) -> Result<Vec<u8>, VmError> {
    let v = arg(args, i);
    Ok(vm.value_to_string_checked(&v)?.as_bytes().to_vec())
}

fn xml_parse(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, "xml_parse", args)?;
    let data = data_arg(vm, args, 1)?;
    let is_final = arg(args, 2).is_truthy();
    if st.borrow().isparsing {
        return Err(throw(
            vm,
            b"Error",
            "Parser must not be called recursively".into(),
        ));
    }
    let r = parse_helper(vm, &st, &data, is_final, "xml_parse")?;
    Ok(Value::Long(r))
}

fn xml_parse_into_struct(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, "xml_parse_into_struct", args)?;
    let data = data_arg(vm, args, 1)?;
    if st.borrow().isparsing {
        vm.emit_warning("xml_parse_into_struct(): Parser must not be called recursively");
        return Ok(Value::False);
    }
    let as_ref = |v: Option<&Value>| -> Option<Rc<RefCell<Value>>> {
        match v {
            Some(Value::Reference(r)) => Some(r.clone()),
            _ => None,
        }
    };
    let values = as_ref(args.get(2)).unwrap_or_else(|| Rc::new(RefCell::new(Value::Null)));
    let info = as_ref(args.get(3));
    if let Some(i) = &info {
        *i.borrow_mut() = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
    }
    *values.borrow_mut() = Value::Array(Rc::new(RefCell::new(PhpArray::new())));
    {
        let mut b = st.borrow_mut();
        b.data = Some(values);
        if info.is_some() {
            b.info = info;
        }
        b.level = 0;
        b.ltags = Some(vec![None; XML_MAXLEVEL as usize]);
        b.installed.element = true;
        b.installed.cdata = true;
    }
    let r = parse_helper(vm, &st, &data, true, "xml_parse_into_struct")?;
    Ok(Value::Long(r))
}

fn set_handler(st: &Rc<State>, slot: Slot, h: Option<Handler>) {
    let mut b = st.borrow_mut();
    b.handlers[slot_idx(slot)] = h;
    let inst = &mut b.installed;
    match slot {
        Slot::Start | Slot::End => inst.element = true,
        Slot::CharData => inst.cdata = true,
        Slot::Pi => inst.pi = true,
        Slot::Default => inst.default = true,
        Slot::Unparsed => inst.unparsed = true,
        Slot::Notation => inst.notation = true,
        Slot::ExtEntity => inst.ext_entity = true,
        Slot::StartNs => inst.start_ns = true,
        Slot::EndNs => {}
    }
}

/// php_xml_set_handler_parse_callable + XML_SET_HANDLER_PHP_FUNCTION
fn generic_set_handler(vm: &mut Vm, args: &[Value], f: &str, slot: Slot) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, f, args)?;
    let v = arg(args, 1);
    let h = match classify(vm, &v) {
        Parsed::Unset => None,
        Parsed::Fcc(h) => Some(h),
        Parsed::Str(s) => {
            deprecated_string(vm, f);
            if vm.current_exception.is_some() {
                return Err(VmError {
                    message: String::new(),
                    line: vm.current_line,
                });
            }
            let obj = st.borrow().object.clone();
            check_string_method(vm, f, 2, "handler", obj.as_ref(), &s)?
        }
        Parsed::Bad => {
            return Err(throw(
                vm,
                b"TypeError",
                format!(
                    "{}(): Argument #2 ($handler) must be of type callable|string|null",
                    f
                ),
            ));
        }
    };
    set_handler(&st, slot, h);
    Ok(Value::True)
}

fn xml_set_element_handler(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "xml_set_element_handler";
    let (_o, st) = get_parser(vm, f, args)?;
    let sv = arg(args, 1);
    let ev = arg(args, 2);
    let s = classify(vm, &sv);
    let e = classify(vm, &ev);
    let obj = st.borrow().object.clone();
    let (sh, eh) = match (s, e) {
        (Parsed::Unset, Parsed::Unset) => (None, None),
        (Parsed::Unset, Parsed::Fcc(b)) => (None, Some(b)),
        (Parsed::Fcc(a), Parsed::Unset) => (Some(a), None),
        (Parsed::Fcc(a), Parsed::Fcc(b)) => (Some(a), Some(b)),
        (a @ (Parsed::Unset | Parsed::Fcc(_)), Parsed::Str(es)) => {
            deprecated_string(vm, f);
            let eh = check_string_method(vm, f, 3, "end_handler", obj.as_ref(), &es)?;
            (
                if let Parsed::Fcc(h) = a {
                    Some(h)
                } else {
                    None
                },
                eh,
            )
        }
        (Parsed::Str(ss), b @ (Parsed::Unset | Parsed::Fcc(_))) => {
            deprecated_string(vm, f);
            let sh = check_string_method(vm, f, 2, "start_handler", obj.as_ref(), &ss)?;
            (
                sh,
                if let Parsed::Fcc(h) = b {
                    Some(h)
                } else {
                    None
                },
            )
        }
        (Parsed::Str(ss), Parsed::Str(es)) => {
            deprecated_string(vm, f);
            let sh = check_string_method(vm, f, 2, "start_handler", obj.as_ref(), &ss)?;
            let eh = check_string_method(vm, f, 3, "end_handler", obj.as_ref(), &es)?;
            (sh, eh)
        }
        _ => {
            if !matches!(sv, Value::Null | Value::Undef | Value::String(_)) {
                return Err(throw(
                    vm,
                    b"TypeError",
                    format!(
                        "{}(): Argument #2 ($start_handler) must be of type callable|string|null",
                        f
                    ),
                ));
            }
            return Err(throw(
                vm,
                b"TypeError",
                format!(
                    "{}(): Argument #3 ($end_handler) must be of type callable|string|null",
                    f
                ),
            ));
        }
    };
    set_handler(&st, Slot::Start, sh);
    set_handler(&st, Slot::End, eh);
    Ok(Value::True)
}

macro_rules! handler_fn {
    ($fn_name:ident, $php:expr, $slot:expr) => {
        fn $fn_name(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
            generic_set_handler(vm, args, $php, $slot)
        }
    };
}

handler_fn!(
    xml_set_character_data_handler,
    "xml_set_character_data_handler",
    Slot::CharData
);
handler_fn!(
    xml_set_processing_instruction_handler,
    "xml_set_processing_instruction_handler",
    Slot::Pi
);
handler_fn!(
    xml_set_default_handler,
    "xml_set_default_handler",
    Slot::Default
);
handler_fn!(
    xml_set_unparsed_entity_decl_handler,
    "xml_set_unparsed_entity_decl_handler",
    Slot::Unparsed
);
handler_fn!(
    xml_set_notation_decl_handler,
    "xml_set_notation_decl_handler",
    Slot::Notation
);
handler_fn!(
    xml_set_external_entity_ref_handler,
    "xml_set_external_entity_ref_handler",
    Slot::ExtEntity
);
handler_fn!(
    xml_set_start_namespace_decl_handler,
    "xml_set_start_namespace_decl_handler",
    Slot::StartNs
);
handler_fn!(
    xml_set_end_namespace_decl_handler,
    "xml_set_end_namespace_decl_handler",
    Slot::EndNs
);

fn same_object(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

fn xml_set_object(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "xml_set_object";
    let (_o, st) = get_parser(vm, f, args)?;
    let new_this = arg(args, 1);
    if !matches!(new_this, Value::Object(_)) {
        return Err(throw(
            vm,
            b"TypeError",
            format!(
                "{}(): Argument #2 ($object) must be of type object, {} given",
                f,
                zend_type_name(&new_this)
            ),
        ));
    }
    vm.emit_deprecated("Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions");
    let cur = st.borrow().object.clone();
    if let Some(cur) = cur {
        for (i, (_slot, setter)) in ALL_SLOTS.iter().enumerate() {
            let h = st.borrow().handlers[i].clone();
            if let Some(Handler::Method { obj, name }) = h {
                if same_object(&obj, &cur) {
                    match find_method(vm, &new_this, &name) {
                        Some(n) => {
                            st.borrow_mut().handlers[i] = Some(Handler::Method {
                                obj: new_this.clone(),
                                name: n,
                            });
                        }
                        None => {
                            st.borrow_mut().handlers[i] = None;
                            let cname = match &new_this {
                                Value::Object(o) => {
                                    goro_core::value::display_class_name(&o.borrow().class_name)
                                }
                                _ => String::new(),
                            };
                            return Err(throw(
                                vm,
                                b"ValueError",
                                format!(
                                    "{}(): Argument #2 ($object) cannot safely swap to object of class {} as method \"{}\" does not exist, which was set via {}",
                                    f,
                                    cname,
                                    String::from_utf8_lossy(&name),
                                    setter
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
    st.borrow_mut().object = Some(new_this);
    Ok(Value::True)
}

fn xml_get_error_code(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, "xml_get_error_code", args)?;
    let b = st.borrow();
    let code = match &b.engine {
        Some(e) => e.parser.st.err_no,
        None => b.live.3,
    };
    Ok(Value::Long(code as i64))
}

const ERROR_MAPPING: [&str; 102] = [
    "No error",
    "No memory",
    "Invalid document start",
    "Empty document",
    "Not well-formed (invalid token)",
    "Invalid document end",
    "Invalid hexadecimal character reference",
    "Invalid decimal character reference",
    "Invalid character reference",
    "Invalid character",
    "XML_ERR_CHARREF_AT_EOF",
    "XML_ERR_CHARREF_IN_PROLOG",
    "XML_ERR_CHARREF_IN_EPILOG",
    "XML_ERR_CHARREF_IN_DTD",
    "XML_ERR_ENTITYREF_AT_EOF",
    "XML_ERR_ENTITYREF_IN_PROLOG",
    "XML_ERR_ENTITYREF_IN_EPILOG",
    "XML_ERR_ENTITYREF_IN_DTD",
    "PEReference at end of document",
    "PEReference in prolog",
    "PEReference in epilog",
    "PEReference: forbidden within markup decl in internal subset",
    "XML_ERR_ENTITYREF_NO_NAME",
    "EntityRef: expecting ';'",
    "PEReference: no name",
    "PEReference: expecting ';'",
    "Undeclared entity error",
    "Undeclared entity warning",
    "Unparsed Entity",
    "XML_ERR_ENTITY_IS_EXTERNAL",
    "XML_ERR_ENTITY_IS_PARAMETER",
    "Unknown encoding",
    "Unsupported encoding",
    "String not started expecting ' or \"",
    "String not closed expecting \" or '",
    "Namespace declaration error",
    "EntityValue: \" or ' expected",
    "EntityValue: \" or ' expected",
    "< in attribute",
    "Attribute not started",
    "Attribute not finished",
    "Attribute without value",
    "Attribute redefined",
    "SystemLiteral \" or ' expected",
    "SystemLiteral \" or ' expected",
    "Comment not finished",
    "Processing Instruction not started",
    "Processing Instruction not finished",
    "NOTATION: Name expected here",
    "'>' required to close NOTATION declaration",
    "'(' required to start ATTLIST enumeration",
    "'(' required to start ATTLIST enumeration",
    "MixedContentDecl : '|' or ')*' expected",
    "XML_ERR_MIXED_NOT_FINISHED",
    "ELEMENT in DTD not started",
    "ELEMENT in DTD not finished",
    "XML declaration not started",
    "XML declaration not finished",
    "XML_ERR_CONDSEC_NOT_STARTED",
    "XML conditional section not closed",
    "Content error in the external subset",
    "DOCTYPE not finished",
    "Sequence ']]>' not allowed in content",
    "CDATA not finished",
    "Reserved XML Name",
    "Space required",
    "XML_ERR_SEPARATOR_REQUIRED",
    "NmToken expected in ATTLIST enumeration",
    "XML_ERR_NAME_REQUIRED",
    "MixedContentDecl : '#PCDATA' expected",
    "SYSTEM or PUBLIC, the URI is missing",
    "PUBLIC, the Public Identifier is missing",
    "< required",
    "> required",
    "</ required",
    "= required",
    "Mismatched tag",
    "Tag not finished",
    "standalone accepts only 'yes' or 'no'",
    "Invalid XML encoding name",
    "Comment must not contain '--' (double-hyphen)",
    "Invalid encoding",
    "external parsed entities cannot be standalone",
    "XML conditional section '[' expected",
    "Entity value required",
    "chunk is not well balanced",
    "extra content at the end of well balanced chunk",
    "XML_ERR_ENTITY_CHAR_ERROR",
    "PEReferences forbidden in internal subset",
    "Detected an entity reference loop",
    "XML_ERR_ENTITY_BOUNDARY",
    "Invalid URI",
    "Fragment not allowed",
    "XML_WAR_CATALOG_PI",
    "XML_ERR_NO_DTD",
    "conditional section INCLUDE or IGNORE keyword expected",
    "Version in XML Declaration missing",
    "XML_WAR_UNKNOWN_VERSION",
    "XML_WAR_LANG_VALUE",
    "XML_WAR_NS_URI",
    "XML_WAR_NS_URI_RELATIVE",
    "Missing encoding in text declaration",
];

fn xml_error_string(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let code = arg(args, 0).to_long();
    let s = if code < 0 || code as usize >= ERROR_MAPPING.len() {
        "Unknown"
    } else {
        ERROR_MAPPING[code as usize]
    };
    Ok(pstr(s.as_bytes().to_vec()))
}

fn position(st: &Rc<State>) -> (u32, u32, u64) {
    let mut b = st.borrow_mut();
    match b.engine.as_mut() {
        Some(e) => e.parser.st.position(),
        None => (b.live.0, b.live.1, b.live.2),
    }
}

fn xml_get_current_line_number(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, "xml_get_current_line_number", args)?;
    Ok(Value::Long(position(&st).0 as i64))
}

fn xml_get_current_column_number(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, "xml_get_current_column_number", args)?;
    Ok(Value::Long(position(&st).1 as i64))
}

fn xml_get_current_byte_index(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let (_o, st) = get_parser(vm, "xml_get_current_byte_index", args)?;
    Ok(Value::Long(position(&st).2 as i64))
}

fn xml_parser_free(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated(
        "Function xml_parser_free() is deprecated since 8.5, as it has no effect since PHP 8.0",
    );
    let (_o, st) = get_parser(vm, "xml_parser_free", args)?;
    if st.borrow().isparsing {
        vm.emit_warning("xml_parser_free(): Parser cannot be freed while it is parsing");
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn xml_parser_set_option(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "xml_parser_set_option";
    let (_o, st) = get_parser(vm, f, args)?;
    let opt = arg(args, 1).to_long();
    let value = arg(args, 2);
    if !matches!(
        value,
        Value::True | Value::False | Value::Long(_) | Value::String(_)
    ) {
        vm.emit_warning(&format!(
            "{}(): Argument #3 ($value) must be of type string|int|bool, {} given",
            f,
            zend_type_name(&value)
        ));
    }
    match opt {
        OPT_CASE_FOLDING => st.borrow_mut().case_folding = value.is_truthy(),
        OPT_SKIP_WHITE => st.borrow_mut().skipwhite = value.is_truthy(),
        OPT_PARSE_HUGE => {
            if st.borrow().isparsing {
                return Err(throw(
                    vm,
                    b"Error",
                    "Cannot change option XML_OPTION_PARSE_HUGE while parsing".into(),
                ));
            }
            st.borrow_mut().parsehuge = value.is_truthy();
        }
        OPT_SKIP_TAGSTART => {
            let v = match &value {
                Value::Object(o) => {
                    let cname = goro_core::value::display_class_name(&o.borrow().class_name);
                    vm.emit_warning(&format!(
                        "Object of class {} could not be converted to int",
                        cname
                    ));
                    1
                }
                v => v.to_long(),
            };
            if !(0..=i32::MAX as i64).contains(&v) {
                vm.emit_warning(&format!("{}(): Argument #3 ($value) must be between 0 and {} for option XML_OPTION_SKIP_TAGSTART", f, i32::MAX));
                return Ok(Value::False);
            }
            st.borrow_mut().toffset = v;
        }
        OPT_TARGET_ENCODING => {
            let s = if let Value::Array(_) = value {
                vm.emit_warning("Array to string conversion");
                PhpString::from_bytes(b"Array")
            } else {
                vm.value_to_string_checked(&value)?
            };
            match xml_get_encoding(s.as_bytes()) {
                Some(e) => st.borrow_mut().target_encoding = e,
                None => {
                    return Err(throw(
                        vm,
                        b"ValueError",
                        format!(
                            "{}(): Argument #3 ($value) is not a supported target encoding",
                            f
                        ),
                    ));
                }
            }
        }
        _ => {
            return Err(throw(
                vm,
                b"ValueError",
                format!(
                    "{}(): Argument #2 ($option) must be a XML_OPTION_* constant",
                    f
                ),
            ));
        }
    }
    Ok(Value::True)
}

fn xml_parser_get_option(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let f = "xml_parser_get_option";
    let (_o, st) = get_parser(vm, f, args)?;
    let opt = arg(args, 1).to_long();
    let b = st.borrow();
    Ok(match opt {
        OPT_CASE_FOLDING => {
            if b.case_folding {
                Value::True
            } else {
                Value::False
            }
        }
        OPT_SKIP_TAGSTART => Value::Long(b.toffset),
        OPT_SKIP_WHITE => {
            if b.skipwhite {
                Value::True
            } else {
                Value::False
            }
        }
        OPT_PARSE_HUGE => {
            if b.parsehuge {
                Value::True
            } else {
                Value::False
            }
        }
        OPT_TARGET_ENCODING => pstr(b.target_encoding.as_bytes().to_vec()),
        _ => {
            drop(b);
            return Err(throw(
                vm,
                b"ValueError",
                format!(
                    "{}(): Argument #2 ($option) must be a XML_OPTION_* constant",
                    f
                ),
            ));
        }
    })
}

// ---------------------------------------------------------------------------
// Registration

pub fn register(vm: &mut Vm) {
    let fns: &[(&[u8], goro_core::vm::BuiltinFn, &[&[u8]])] = &[
        (b"xml_parser_create", xml_parser_create, &[b"encoding"]),
        (
            b"xml_parser_create_ns",
            xml_parser_create_ns,
            &[b"encoding", b"separator"],
        ),
        (b"xml_set_object", xml_set_object, &[b"parser", b"object"]),
        (
            b"xml_set_element_handler",
            xml_set_element_handler,
            &[b"parser", b"start_handler", b"end_handler"],
        ),
        (
            b"xml_set_character_data_handler",
            xml_set_character_data_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_processing_instruction_handler",
            xml_set_processing_instruction_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_default_handler",
            xml_set_default_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_unparsed_entity_decl_handler",
            xml_set_unparsed_entity_decl_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_notation_decl_handler",
            xml_set_notation_decl_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_external_entity_ref_handler",
            xml_set_external_entity_ref_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_start_namespace_decl_handler",
            xml_set_start_namespace_decl_handler,
            &[b"parser", b"handler"],
        ),
        (
            b"xml_set_end_namespace_decl_handler",
            xml_set_end_namespace_decl_handler,
            &[b"parser", b"handler"],
        ),
        (b"xml_parse", xml_parse, &[b"parser", b"data", b"is_final"]),
        (
            b"xml_parse_into_struct",
            xml_parse_into_struct,
            &[b"parser", b"data", b"values", b"index"],
        ),
        (b"xml_get_error_code", xml_get_error_code, &[b"parser"]),
        (b"xml_error_string", xml_error_string, &[b"error_code"]),
        (
            b"xml_get_current_line_number",
            xml_get_current_line_number,
            &[b"parser"],
        ),
        (
            b"xml_get_current_column_number",
            xml_get_current_column_number,
            &[b"parser"],
        ),
        (
            b"xml_get_current_byte_index",
            xml_get_current_byte_index,
            &[b"parser"],
        ),
        (b"xml_parser_free", xml_parser_free, &[b"parser"]),
        (
            b"xml_parser_set_option",
            xml_parser_set_option,
            &[b"parser", b"option", b"value"],
        ),
        (
            b"xml_parser_get_option",
            xml_parser_get_option,
            &[b"parser", b"option"],
        ),
    ];
    for (name, f, params) in fns {
        vm.register_function_with_params(name, *f, params);
    }

    let errors: [&[u8]; 22] = [
        b"XML_ERROR_NONE",
        b"XML_ERROR_NO_MEMORY",
        b"XML_ERROR_SYNTAX",
        b"XML_ERROR_NO_ELEMENTS",
        b"XML_ERROR_INVALID_TOKEN",
        b"XML_ERROR_UNCLOSED_TOKEN",
        b"XML_ERROR_PARTIAL_CHAR",
        b"XML_ERROR_TAG_MISMATCH",
        b"XML_ERROR_DUPLICATE_ATTRIBUTE",
        b"XML_ERROR_JUNK_AFTER_DOC_ELEMENT",
        b"XML_ERROR_PARAM_ENTITY_REF",
        b"XML_ERROR_UNDEFINED_ENTITY",
        b"XML_ERROR_RECURSIVE_ENTITY_REF",
        b"XML_ERROR_ASYNC_ENTITY",
        b"XML_ERROR_BAD_CHAR_REF",
        b"XML_ERROR_BINARY_ENTITY_REF",
        b"XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF",
        b"XML_ERROR_MISPLACED_XML_PI",
        b"XML_ERROR_UNKNOWN_ENCODING",
        b"XML_ERROR_INCORRECT_ENCODING",
        b"XML_ERROR_UNCLOSED_CDATA_SECTION",
        b"XML_ERROR_EXTERNAL_ENTITY_HANDLING",
    ];
    for (i, n) in errors.iter().enumerate() {
        vm.constants.insert(n.to_vec(), Value::Long(i as i64));
    }
    for (n, v) in [
        (&b"XML_OPTION_CASE_FOLDING"[..], OPT_CASE_FOLDING),
        (b"XML_OPTION_TARGET_ENCODING", OPT_TARGET_ENCODING),
        (b"XML_OPTION_SKIP_TAGSTART", OPT_SKIP_TAGSTART),
        (b"XML_OPTION_SKIP_WHITE", OPT_SKIP_WHITE),
        (b"XML_OPTION_PARSE_HUGE", OPT_PARSE_HUGE),
    ] {
        vm.constants.insert(n.to_vec(), Value::Long(v));
    }
    vm.constants
        .insert(b"XML_SAX_IMPL".to_vec(), pstr(b"libxml".to_vec()));

    // XMLParser: final, not constructible, not serializable, not cloneable.
    let mut ce = goro_core::object::ClassEntry::new(b"XMLParser".to_vec());
    ce.is_final = true;
    vm.classes.insert(b"xmlparser".to_vec(), ce);
}
