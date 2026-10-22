//! ext/xmlwriter: a port of PHP's php_xmlwriter.c. The libxml2 text writer
//! it wraps is reimplemented in `textwriter.rs`.

mod textwriter;

use goro_core::object::{ClassEntry, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, Vm, VmError};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use textwriter::{EncKind, Encoder, MbConv, TextWriter, builtin_encoder, valid_name};

type R<T> = Result<T, VmError>;
type Obj = Rc<RefCell<PhpObject>>;

// ── Writer storage ──────────────────────────────────────────────────────────

enum Sink {
    /// openMemory/toMemory: PHP's smart_str output buffer.
    Memory(Vec<u8>),
    /// openUri on a local file.
    File(std::fs::File),
    /// openUri through a stream wrapper (owned: closed with the writer) or
    /// toStream (borrowed).
    Stream { res: Value, owned: bool },
}

struct PhpWriter {
    tw: TextWriter,
    sink: Sink,
}

struct Slot {
    owner: Weak<RefCell<PhpObject>>,
    /// None while uninitialized, or while an operation has it checked out.
    writer: Option<Box<PhpWriter>>,
    seq: u64,
}

thread_local! {
    /// Keyed by the address of the owning object's allocation; the Weak we
    /// hold keeps the allocation (and thus the address) reserved.
    static WRITERS: RefCell<HashMap<usize, Slot>> = RefCell::new(HashMap::new());
    static NEXT_SEQ: Cell<u64> = const { Cell::new(1) };
    /// Writers of freed objects whose output needs the VM to be flushed.
    #[allow(clippy::vec_box)]
    static DEFERRED: RefCell<Vec<Box<PhpWriter>>> = const { RefCell::new(Vec::new()) };
}

impl PhpWriter {
    /// Whether flushing needs the VM (PHP streams, mbstring conversion).
    fn needs_vm(&self) -> bool {
        matches!(self.sink, Sink::Stream { .. })
            || matches!(
                self.tw.out.encoder.as_ref().map(|e| &e.kind),
                Some(EncKind::Mb(_))
            )
    }
}

/// OBJECT_FREE_HOOK: the XMLWriter object went away — free its writer now
/// (xmlwriter_object_dtor), or as soon as the VM is available.
fn on_object_free(addr: usize) {
    let slot = WRITERS
        // (every object's release comes here: no hashing while there is
        // no writer)
        .try_with(|w| {
            w.try_borrow_mut()
                .ok()
                .and_then(|mut m| if m.is_empty() { None } else { m.remove(&addr) })
        })
        .ok()
        .flatten();
    if let Some(Slot {
        writer: Some(mut w),
        ..
    }) = slot
    {
        if w.needs_vm() {
            let _ = DEFERRED.try_with(|d| d.borrow_mut().push(w));
        } else {
            pump(None, &mut w, true);
        }
    }
}

/// Address of the PhpObject itself (what OBJECT_FREE_HOOK reports).
fn obj_key(o: &Obj) -> usize {
    o.as_ptr() as usize
}

fn take_writer(o: &Obj) -> Option<Box<PhpWriter>> {
    WRITERS.with(|w| {
        w.borrow_mut()
            .get_mut(&obj_key(o))
            .and_then(|s| s.writer.take())
    })
}

fn put_writer(o: &Obj, writer: Box<PhpWriter>) {
    WRITERS.with(|w| {
        let mut map = w.borrow_mut();
        let key = obj_key(o);
        match map.get_mut(&key) {
            Some(s) if s.owner.upgrade().is_some() => s.writer = Some(writer),
            _ => {
                let seq = NEXT_SEQ.with(|c| {
                    let v = c.get();
                    c.set(v + 1);
                    v
                });
                map.insert(
                    key,
                    Slot {
                        owner: Rc::downgrade(o),
                        writer: Some(writer),
                        seq,
                    },
                );
            }
        }
    });
}

/// Install a new writer on an object, freeing the previous one.
fn set_writer(vm: &mut Vm, o: &Obj, writer: Box<PhpWriter>) {
    if let Some(old) = take_writer(o) {
        close_writer(vm, old);
    }
    put_writer(o, writer);
}

/// Free writers whose object is gone (PHP frees them in the object dtor).
fn sweep(vm: &mut Vm) {
    let deferred: Vec<Box<PhpWriter>> = DEFERRED.with(|d| std::mem::take(&mut *d.borrow_mut()));
    for w in deferred {
        close_writer(vm, w);
    }
    let dead: Vec<Slot> = WRITERS.with(|w| {
        let mut map = w.borrow_mut();
        let keys: Vec<usize> = map
            .iter()
            .filter(|(_, s)| s.owner.strong_count() == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut v: Vec<Slot> = keys.into_iter().filter_map(|k| map.remove(&k)).collect();
        v.sort_by_key(|s| s.seq);
        v
    });
    for s in dead {
        if let Some(w) = s.writer {
            close_writer(vm, w);
        }
    }
}

/// End of request: every remaining writer is freed (flushed and closed).
fn shutdown(vm: &mut Vm) {
    sweep(vm);
    let mut all: Vec<Slot> = WRITERS.with(|w| w.borrow_mut().drain().map(|(_, s)| s).collect());
    all.sort_by_key(|s| std::cmp::Reverse(s.seq));
    for s in all {
        if let Some(w) = s.writer {
            close_writer(vm, w);
        }
    }
}

/// xmlFreeTextWriter: flush the output buffer, then close it.
fn close_writer(vm: &mut Vm, mut w: Box<PhpWriter>) {
    pump(Some(vm), &mut w, true);
    if let Sink::Stream { res, owned: true } = &w.sink {
        let res = res.clone();
        if let Value::Resource(id) = &res {
            if goro_core::value::is_open_resource(id.id()) {
                call_builtin(vm, b"fclose", &[res]);
            }
        }
    }
}

// ── Output plumbing (xmlOutputBuffer write callbacks) ───────────────────────

fn call_builtin(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    let r = f(vm, args);
    if vm.current_exception.is_some() {
        vm.current_exception = None;
        return None;
    }
    r.ok()
}

fn stream_is_valid(res: &Value) -> bool {
    match res {
        Value::Resource(id) => {
            if goro_core::value::is_registered_resource(id.id()) {
                goro_core::value::is_open_resource(id.id())
                    && goro_core::value::resource_type_of(id.id())
                        .map(|t| t == b"stream")
                        .unwrap_or(false)
            } else {
                // STDIN/STDOUT/STDERR are not in the registry.
                (1..=3).contains(&id.id())
            }
        }
        _ => false,
    }
}

fn sink_write(vm: Option<&mut Vm>, sink: &mut Sink, data: &[u8]) -> i64 {
    match sink {
        Sink::Memory(v) => {
            v.extend_from_slice(data);
            data.len() as i64
        }
        Sink::File(f) => {
            use std::io::Write;
            match f.write_all(data) {
                Ok(()) => data.len() as i64,
                Err(_) => -1,
            }
        }
        Sink::Stream { res, .. } => {
            let vm = match vm {
                Some(vm) => vm,
                None => return -1,
            };
            if !stream_is_valid(res) {
                return -1;
            }
            if data.is_empty() {
                return 0;
            }
            let res = res.clone();
            match call_builtin(
                vm,
                b"fwrite",
                &[res, Value::String(PhpString::from_bytes(data))],
            ) {
                Some(Value::Long(n)) if n >= 0 => n,
                _ => -1,
            }
        }
    }
}

fn s(v: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(v))
}

fn mb_convert(vm: &mut Vm, name: &str, c: u32) -> Option<Vec<u8>> {
    let ch = char::from_u32(c)?;
    let utf8 = ch.to_string();
    match call_builtin(
        vm,
        b"mb_convert_encoding",
        &[s(utf8.as_bytes()), s(name.as_bytes()), s(b"UTF-8")],
    ) {
        Some(Value::String(out)) => {
            let b = out.as_bytes();
            // mbstring substitutes unmappable characters.
            if b.is_empty() || (b == b"?" && c != b'?' as u32) {
                None
            } else {
                Some(b.to_vec())
            }
        }
        _ => None,
    }
}

/// Deliver output to the sink: first the chunks xmlOutputBufferWrite
/// released, then (with `force`, i.e. xmlOutputBufferFlush) everything that
/// is left. Returns the result of the last write callback.
fn pump(mut vm: Option<&mut Vm>, w: &mut PhpWriter, force: bool) -> i64 {
    if w.tw.out.is_mb() {
        if let Some(vm) = vm.as_deref_mut() {
            let mut f = |name: &str, c: u32| mb_convert(vm, name, c);
            let mut mb: MbConv = Some(&mut f);
            w.tw.out.auto_flush(&mut mb);
            if force {
                w.tw.out.convert(&mut mb);
            }
        }
    } else if force {
        w.tw.out.convert(&mut None);
    }
    let mut ret = 0;
    let mut chunks = std::mem::take(&mut w.tw.out.ready);
    if force {
        chunks.push(w.tw.out.take_pending());
    }
    for data in chunks {
        if w.tw.out.error {
            return -1;
        }
        ret = sink_write(vm.as_deref_mut(), &mut w.sink, &data);
        let consumed = if ret < 0 {
            w.tw.out.error = true;
            0
        } else {
            (ret as usize).min(data.len())
        };
        if consumed < data.len() {
            // Keep what the callback did not take at the front of the queue.
            let rest = data[consumed..].to_vec();
            if w.tw.out.encoder.is_some() {
                let c = w.tw.out.conv.get_or_insert_with(Vec::new);
                let mut v = rest;
                v.extend_from_slice(c);
                *c = v;
            } else {
                let mut v = rest;
                v.extend_from_slice(&w.tw.out.buffer);
                w.tw.out.buffer = v;
            }
        }
    }
    ret
}

/// Resolve an encoding name the way xmlFindCharEncodingHandler would.
fn find_encoder(vm: &mut Vm, name: &[u8]) -> Option<Encoder> {
    if let Some(e) = builtin_encoder(name) {
        return Some(e);
    }
    if name.is_empty() || !vm.functions.contains_key(&b"mb_convert_encoding"[..]) {
        return None;
    }
    let n = String::from_utf8_lossy(name).into_owned();
    match call_builtin(
        vm,
        b"mb_convert_encoding",
        &[s(b"a"), s(n.as_bytes()), s(b"UTF-8")],
    ) {
        Some(Value::String(_)) => Some(Encoder {
            name: name.to_vec(),
            kind: EncKind::Mb(n),
            cache: HashMap::new(),
        }),
        _ => None,
    }
}

// ── Argument parsing (zend_parse_parameters) ────────────────────────────────

fn throw(vm: &mut Vm, class: &[u8], msg: &str) -> VmError {
    let exc = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg.to_string(),
        line: vm.current_line,
    }
}

struct Spec {
    method: &'static str,
    func: &'static str,
    params: &'static [&'static str],
    min: usize,
    is_static: bool,
    /// The procedural form takes the writer as its first argument.
    takes_writer: bool,
}

const fn sp(
    method: &'static str,
    func: &'static str,
    params: &'static [&'static str],
    min: usize,
) -> Spec {
    Spec {
        method,
        func,
        params,
        min,
        is_static: false,
        takes_writer: true,
    }
}

static SPECS: &[Spec] = &[
    Spec {
        method: "openUri",
        func: "xmlwriter_open_uri",
        params: &["uri"],
        min: 1,
        is_static: false,
        takes_writer: false,
    },
    Spec {
        method: "toUri",
        func: "",
        params: &["uri"],
        min: 1,
        is_static: true,
        takes_writer: false,
    },
    Spec {
        method: "openMemory",
        func: "xmlwriter_open_memory",
        params: &[],
        min: 0,
        is_static: false,
        takes_writer: false,
    },
    Spec {
        method: "toMemory",
        func: "",
        params: &[],
        min: 0,
        is_static: true,
        takes_writer: false,
    },
    Spec {
        method: "toStream",
        func: "",
        params: &["stream"],
        min: 1,
        is_static: true,
        takes_writer: false,
    },
    sp("setIndent", "xmlwriter_set_indent", &["enable"], 1),
    sp(
        "setIndentString",
        "xmlwriter_set_indent_string",
        &["indentation"],
        1,
    ),
    sp("startComment", "xmlwriter_start_comment", &[], 0),
    sp("endComment", "xmlwriter_end_comment", &[], 0),
    sp("startAttribute", "xmlwriter_start_attribute", &["name"], 1),
    sp("endAttribute", "xmlwriter_end_attribute", &[], 0),
    sp(
        "writeAttribute",
        "xmlwriter_write_attribute",
        &["name", "value"],
        2,
    ),
    sp(
        "startAttributeNs",
        "xmlwriter_start_attribute_ns",
        &["prefix", "name", "namespace"],
        3,
    ),
    sp(
        "writeAttributeNs",
        "xmlwriter_write_attribute_ns",
        &["prefix", "name", "namespace", "value"],
        4,
    ),
    sp("startElement", "xmlwriter_start_element", &["name"], 1),
    sp("endElement", "xmlwriter_end_element", &[], 0),
    sp("fullEndElement", "xmlwriter_full_end_element", &[], 0),
    sp(
        "startElementNs",
        "xmlwriter_start_element_ns",
        &["prefix", "name", "namespace"],
        3,
    ),
    sp(
        "writeElement",
        "xmlwriter_write_element",
        &["name", "content"],
        1,
    ),
    sp(
        "writeElementNs",
        "xmlwriter_write_element_ns",
        &["prefix", "name", "namespace", "content"],
        3,
    ),
    sp("startPi", "xmlwriter_start_pi", &["target"], 1),
    sp("endPi", "xmlwriter_end_pi", &[], 0),
    sp("writePi", "xmlwriter_write_pi", &["target", "content"], 2),
    sp("startCdata", "xmlwriter_start_cdata", &[], 0),
    sp("endCdata", "xmlwriter_end_cdata", &[], 0),
    sp("writeCdata", "xmlwriter_write_cdata", &["content"], 1),
    sp("text", "xmlwriter_text", &["content"], 1),
    sp("writeRaw", "xmlwriter_write_raw", &["content"], 1),
    sp(
        "startDocument",
        "xmlwriter_start_document",
        &["version", "encoding", "standalone"],
        0,
    ),
    sp("endDocument", "xmlwriter_end_document", &[], 0),
    sp("writeComment", "xmlwriter_write_comment", &["content"], 1),
    sp(
        "startDtd",
        "xmlwriter_start_dtd",
        &["qualifiedName", "publicId", "systemId"],
        1,
    ),
    sp("endDtd", "xmlwriter_end_dtd", &[], 0),
    sp(
        "writeDtd",
        "xmlwriter_write_dtd",
        &["name", "publicId", "systemId", "content"],
        1,
    ),
    sp(
        "startDtdElement",
        "xmlwriter_start_dtd_element",
        &["qualifiedName"],
        1,
    ),
    sp("endDtdElement", "xmlwriter_end_dtd_element", &[], 0),
    sp(
        "writeDtdElement",
        "xmlwriter_write_dtd_element",
        &["name", "content"],
        2,
    ),
    sp(
        "startDtdAttlist",
        "xmlwriter_start_dtd_attlist",
        &["name"],
        1,
    ),
    sp("endDtdAttlist", "xmlwriter_end_dtd_attlist", &[], 0),
    sp(
        "writeDtdAttlist",
        "xmlwriter_write_dtd_attlist",
        &["name", "content"],
        2,
    ),
    sp(
        "startDtdEntity",
        "xmlwriter_start_dtd_entity",
        &["name", "isParam"],
        2,
    ),
    sp("endDtdEntity", "xmlwriter_end_dtd_entity", &[], 0),
    sp(
        "writeDtdEntity",
        "xmlwriter_write_dtd_entity",
        &[
            "name",
            "content",
            "isParam",
            "publicId",
            "systemId",
            "notationData",
        ],
        2,
    ),
    sp("outputMemory", "xmlwriter_output_memory", &["flush"], 0),
    sp("flush", "xmlwriter_flush", &["empty"], 0),
];

fn spec_by_method(lower: &[u8]) -> Option<&'static Spec> {
    SPECS
        .iter()
        .find(|s| s.method.as_bytes().eq_ignore_ascii_case(lower))
}

struct Ctx {
    fname: String,
    is_method: bool,
    spec: &'static Spec,
}

impl Ctx {
    /// Displayed argument number of method-level parameter `i`.
    fn argno(&self, i: usize) -> usize {
        i + if self.is_method || !self.spec.takes_writer {
            1
        } else {
            2
        }
    }

    /// zend_argument_value_error(n, ...) as used by php_xmlwriter.c, which
    /// counts the writer even for methods (the name is looked up in the
    /// active function's own arginfo).
    fn value_error(&self, vm: &mut Vm, n: usize, msg: &str) -> VmError {
        let name = if self.is_method || !self.spec.takes_writer {
            self.spec.params.get(n - 1).copied()
        } else if n == 1 {
            Some("writer")
        } else {
            self.spec.params.get(n - 2).copied()
        };
        let named = name.map(|p| format!(" (${})", p)).unwrap_or_default();
        let m = format!("{}(): Argument #{}{} {}", self.fname, n, named, msg);
        throw(vm, b"ValueError", &m)
    }

    fn type_error(&self, vm: &mut Vm, i: usize, expected: &str, v: &Value) -> VmError {
        let m = format!(
            "{}(): Argument #{} (${}) must be of type {}, {} given",
            self.fname,
            self.argno(i),
            self.spec.params[i],
            expected,
            Vm::value_type_name(v)
        );
        throw(vm, b"TypeError", &m)
    }

    fn null_deprecated(&self, vm: &mut Vm, i: usize, ty: &str) {
        let m = format!(
            "{}(): Passing null to parameter #{} (${}) of type {} is deprecated",
            self.fname,
            self.argno(i),
            self.spec.params[i],
            ty
        );
        vm.emit_deprecated(&m);
    }

    /// Z_PARAM_STRING / "s" ("s!" when nullable). Absent → None.
    fn str_arg(&self, vm: &mut Vm, args: &[Value], i: usize, nullable: bool) -> R<Option<Vec<u8>>> {
        let v = match args.get(i) {
            None | Some(Value::Undef) => return Ok(None),
            Some(v) => v.deref(),
        };
        let ty = if nullable { "?string" } else { "string" };
        match &v {
            Value::Null | Value::Undef => {
                if nullable {
                    Ok(None)
                } else if vm.caller_strict_types {
                    Err(self.type_error(vm, i, ty, &v))
                } else {
                    self.null_deprecated(vm, i, "string");
                    Ok(Some(Vec::new()))
                }
            }
            Value::String(st) => Ok(Some(st.as_bytes().to_vec())),
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                if vm.caller_strict_types {
                    Err(self.type_error(vm, i, ty, &v))
                } else {
                    Ok(Some(v.to_php_string().as_bytes().to_vec()))
                }
            }
            Value::Object(o) => {
                let lower = o.borrow().class_name.to_ascii_lowercase();
                if vm.class_implements_interface(&lower, b"stringable") {
                    if let Some(r) = vm.call_object_method(&v, b"__toString", &[]) {
                        return Ok(Some(r.to_php_string().as_bytes().to_vec()));
                    }
                    if vm.current_exception.is_some() {
                        return Err(VmError {
                            message: String::new(),
                            line: vm.current_line,
                        });
                    }
                }
                Err(self.type_error(vm, i, ty, &v))
            }
            _ => Err(self.type_error(vm, i, ty, &v)),
        }
    }

    /// Z_PARAM_PATH / "p": a string without NUL bytes.
    fn path_arg(
        &self,
        vm: &mut Vm,
        args: &[Value],
        i: usize,
        nullable: bool,
    ) -> R<Option<Vec<u8>>> {
        let r = self.str_arg(vm, args, i, nullable)?;
        if let Some(b) = &r {
            if b.contains(&0) {
                let m = format!(
                    "{}(): Argument #{} (${}) must not contain any null bytes",
                    self.fname,
                    self.argno(i),
                    self.spec.params[i]
                );
                return Err(throw(vm, b"ValueError", &m));
            }
        }
        Ok(r)
    }

    fn bool_arg(&self, vm: &mut Vm, args: &[Value], i: usize) -> R<Option<bool>> {
        let v = match args.get(i) {
            None | Some(Value::Undef) => return Ok(None),
            Some(v) => v.deref(),
        };
        match &v {
            Value::True => Ok(Some(true)),
            Value::False => Ok(Some(false)),
            Value::Null | Value::Undef => {
                if vm.caller_strict_types {
                    return Err(self.type_error(vm, i, "bool", &v));
                }
                self.null_deprecated(vm, i, "bool");
                Ok(Some(false))
            }
            Value::Long(_) | Value::Double(_) | Value::String(_) => {
                if vm.caller_strict_types {
                    return Err(self.type_error(vm, i, "bool", &v));
                }
                Ok(Some(v.is_truthy()))
            }
            _ => Err(self.type_error(vm, i, "bool", &v)),
        }
    }
}

/// libxml2 receives C strings: everything after a NUL byte is invisible.
fn cstr(v: Vec<u8>) -> Vec<u8> {
    match v.iter().position(|&b| b == 0) {
        Some(p) => v[..p].to_vec(),
        None => v,
    }
}

fn count_args(args: &[Value]) -> usize {
    args.iter()
        .rposition(|a| !matches!(a, Value::Undef))
        .map(|p| p + 1)
        .unwrap_or(0)
}

fn check_count(vm: &mut Vm, fname: &str, n: usize, min: usize, max: usize) -> R<()> {
    if n >= min && n <= max {
        return Ok(());
    }
    let (kind, lim) = if min == max {
        ("exactly", min)
    } else if n < min {
        ("at least", min)
    } else {
        ("at most", max)
    };
    let m = format!(
        "{}() expects {} {} argument{}, {} given",
        fname,
        kind,
        lim,
        if lim == 1 { "" } else { "s" },
        n
    );
    Err(throw(vm, b"ArgumentCountError", &m))
}

// ── Object helpers ──────────────────────────────────────────────────────────

fn is_xmlwriter_class(vm: &Vm, class_lower: &[u8]) -> bool {
    class_lower == b"xmlwriter" || vm.is_subclass_of(class_lower, b"xmlwriter")
}

fn new_object(vm: &mut Vm, class: &[u8]) -> Obj {
    let lower = class.to_ascii_lowercase();
    let display = vm
        .classes
        .get(&lower)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| class.to_vec());
    let id = vm.alloc_object_id();
    let mut obj = PhpObject::new(display, id);
    vm.apply_class_property_defaults(&mut obj, &lower);
    Rc::new(RefCell::new(obj))
}

fn has_constructor(vm: &Vm, class_lower: &[u8]) -> bool {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        let ce = match vm.classes.get(&cur) {
            Some(c) => c,
            None => return false,
        };
        if ce.methods.contains_key(&b"__construct"[..]) {
            return true;
        }
        match &ce.parent {
            Some(p) => cur = p.to_ascii_lowercase(),
            None => return false,
        }
    }
    false
}

/// xml_writer_create_static: instantiate the called class (running its
/// constructor) and attach the writer.
fn create_static(vm: &mut Vm, called: &[u8], writer: Box<PhpWriter>) -> R<Value> {
    let o = new_object(vm, called);
    let v = Value::Object(o.clone());
    if has_constructor(vm, &called.to_ascii_lowercase()) {
        vm.call_object_method(&v, b"__construct", &[]);
        if vm.current_exception.is_some() {
            return Err(VmError {
                message: String::new(),
                line: vm.current_line,
            });
        }
    }
    set_writer(vm, &o, writer);
    Ok(v)
}

// ── File path resolution (_xmlwriter_get_valid_file_path) ───────────────────

const MAXPATHLEN: usize = 4096;

fn has_uri_scheme(s: &[u8]) -> bool {
    if s.is_empty() || !s[0].is_ascii_alphabetic() {
        return false;
    }
    for &c in &s[1..] {
        if c == b':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == b'+' || c == b'-' || c == b'.') {
            return false;
        }
    }
    false
}

/// expand_filepath: make absolute and normalize `.`/`..` lexically.
fn expand_filepath(p: &[u8]) -> Option<Vec<u8>> {
    let mut full: Vec<u8> = if p.first() == Some(&b'/') {
        p.to_vec()
    } else {
        let cwd = std::env::current_dir().ok()?;
        let mut c = cwd.to_string_lossy().into_owned().into_bytes();
        c.push(b'/');
        c.extend_from_slice(p);
        c
    };
    let mut parts: Vec<&[u8]> = Vec::new();
    for comp in full.split(|&b| b == b'/') {
        match comp {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    let mut out = Vec::new();
    for c in parts {
        out.push(b'/');
        out.extend_from_slice(c);
    }
    if out.is_empty() {
        out.push(b'/');
    }
    full = out;
    if full.len() >= MAXPATHLEN {
        return None;
    }
    Some(full)
}

/// zend_dirname.
fn dirname(p: &[u8]) -> Vec<u8> {
    let mut end = p.len();
    while end > 1 && p[end - 1] == b'/' {
        end -= 1;
    }
    match p[..end].iter().rposition(|&b| b == b'/') {
        None => b".".to_vec(),
        Some(0) => b"/".to_vec(),
        Some(i) => {
            let mut j = i;
            while j > 1 && p[j - 1] == b'/' {
                j -= 1;
            }
            p[..j].to_vec()
        }
    }
}

/// Returns the path to open and whether it is a local file (as opposed to
/// a URI handed to a stream wrapper).
fn resolve_uri(source: &[u8]) -> Option<(Vec<u8>, bool)> {
    let mut src = source;
    let scheme = has_uri_scheme(source);
    let mut is_file = false;
    if scheme {
        let lower = source.to_ascii_lowercase();
        if lower.starts_with(b"file:///") {
            if source.len() == 8 {
                return None;
            }
            is_file = true;
            src = &source[7..];
        } else if lower.starts_with(b"file://localhost/") {
            if source.len() == 17 {
                return None;
            }
            is_file = true;
            src = &source[16..];
        }
    }
    if scheme && !is_file {
        return Some((source.to_vec(), false));
    }
    let path_str = String::from_utf8_lossy(src).into_owned();
    let resolved = match std::fs::canonicalize(&path_str) {
        Ok(p) if p.as_os_str().len() < MAXPATHLEN => p.to_string_lossy().into_owned().into_bytes(),
        _ => expand_filepath(src)?,
    };
    let dir = dirname(src);
    if !dir.is_empty() && std::fs::metadata(String::from_utf8_lossy(&dir).as_ref()).is_err() {
        return None;
    }
    Some((resolved, true))
}

/// The output side of php_libxml_output_buffer_create_filename ("wb").
fn open_uri_sink(vm: &mut Vm, ctx: &Ctx, path: &[u8], local: bool) -> Option<Sink> {
    if !local {
        return match call_builtin(vm, b"fopen", &[s(path), s(b"wb")]) {
            Some(v @ Value::Resource(_)) => Some(Sink::Stream {
                res: v,
                owned: true,
            }),
            _ => None,
        };
    }
    let p = String::from_utf8_lossy(path).into_owned();
    match std::fs::File::create(&p) {
        Ok(f) => Some(Sink::File(f)),
        Err(e) => {
            let mut msg = e.to_string();
            if let Some(i) = msg.find(" (os error") {
                msg.truncate(i);
            }
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                ctx.fname, p, msg
            ));
            None
        }
    }
}

// ── Dispatch ────────────────────────────────────────────────────────────────

fn name_check(vm: &mut Vm, ctx: &Ctx, n: usize, subject: &str, name: &[u8]) -> R<()> {
    if valid_name(name) {
        return Ok(());
    }
    let msg = format!(
        "must be a valid {}, \"{}\" given",
        subject,
        String::from_utf8_lossy(name)
    );
    Err(ctx.value_error(vm, n, &msg))
}

/// Run one operation of the writer attached to `o`.
fn with_writer(
    vm: &mut Vm,
    _ctx: &Ctx,
    o: &Obj,
    f: impl FnOnce(&mut Vm, &mut PhpWriter) -> R<Value>,
) -> R<Value> {
    let mut w = match take_writer(o) {
        Some(w) => w,
        None => {
            return Err(throw(
                vm,
                b"Error",
                "Invalid or uninitialized XMLWriter object",
            ));
        }
    };
    let r = f(vm, &mut w);
    let errors = std::mem::take(&mut w.tw.errors);
    let mut r = r;
    if w.tw.flush_requested {
        w.tw.flush_requested = false;
        if pump(Some(vm), &mut w, true) < 0 {
            if let Ok(Value::True) = r {
                r = Ok(Value::False);
            }
        }
    } else {
        pump(Some(vm), &mut w, false);
    }
    put_writer(o, w);
    // libxml2 raises writer errors without a channel: they only update
    // xmlLastError and never reach PHP's error handler.
    drop(errors);
    r
}

fn b(ret: i32) -> Value {
    if ret != -1 { Value::True } else { Value::False }
}

fn dispatch(vm: &mut Vm, ctx: &Ctx, this: Option<Obj>, called: &[u8], args: &[Value]) -> R<Value> {
    sweep(vm);
    let m = ctx.spec.method;
    match m {
        "openUri" | "toUri" => {
            let uri = ctx.path_arg(vm, args, 0, false)?.unwrap_or_default();
            if uri.is_empty() {
                return Err(ctx.value_error(vm, 1, "must not be empty"));
            }
            let (path, local) = match resolve_uri(&uri) {
                Some(p) => p,
                None => {
                    if m == "toUri" {
                        return Err(ctx.value_error(vm, 1, "must resolve to a valid file path"));
                    }
                    vm.emit_warning(&format!("{}(): Unable to resolve file path", ctx.fname));
                    return Ok(Value::False);
                }
            };
            let sink = match open_uri_sink(vm, ctx, &path, local) {
                Some(r) => r,
                None => {
                    if m == "toUri" {
                        return Err(throw(vm, b"Error", "Could not construct libxml writer"));
                    }
                    return Ok(Value::False);
                }
            };
            let w = Box::new(PhpWriter {
                tw: TextWriter::new(false),
                sink,
            });
            if m == "toUri" {
                return create_static(vm, called, w);
            }
            attach_new(vm, this, w)
        }
        "openMemory" | "toMemory" => {
            let w = Box::new(PhpWriter {
                tw: TextWriter::new(false),
                sink: Sink::Memory(Vec::new()),
            });
            if m == "toMemory" {
                return create_static(vm, called, w);
            }
            attach_new(vm, this, w)
        }
        "toStream" => {
            let v = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
            if !matches!(v, Value::Resource(_)) {
                return Err(ctx.type_error(vm, 0, "resource", &v));
            }
            if !stream_is_valid(&v) {
                return Err(throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): supplied resource is not a valid stream resource",
                        ctx.fname
                    ),
                ));
            }
            let w = Box::new(PhpWriter {
                tw: TextWriter::new(false),
                sink: Sink::Stream {
                    res: v,
                    owned: false,
                },
            });
            create_static(vm, called, w)
        }
        _ => {
            let o = match this {
                Some(o) => o,
                None => {
                    return Err(throw(
                        vm,
                        b"Error",
                        "Invalid or uninitialized XMLWriter object",
                    ));
                }
            };
            writer_op(vm, ctx, &o, args)
        }
    }
}

fn attach_new(vm: &mut Vm, this: Option<Obj>, w: Box<PhpWriter>) -> R<Value> {
    match this {
        Some(o) => {
            set_writer(vm, &o, w);
            Ok(Value::True)
        }
        None => {
            let o = new_object(vm, b"XMLWriter");
            set_writer(vm, &o, w);
            Ok(Value::Object(o))
        }
    }
}

fn writer_op(vm: &mut Vm, ctx: &Ctx, o: &Obj, args: &[Value]) -> R<Value> {
    let m = ctx.spec.method;
    // Argument parsing happens before the writer is looked up.
    macro_rules! st {
        ($i:expr) => {
            cstr(ctx.str_arg(vm, args, $i, false)?.unwrap_or_default())
        };
    }
    macro_rules! nst {
        ($i:expr) => {
            ctx.str_arg(vm, args, $i, true)?.map(cstr)
        };
    }
    match m {
        "setIndent" => {
            let e = ctx.bool_arg(vm, args, 0)?.unwrap_or(false);
            with_writer(vm, ctx, o, |_, w| Ok(bv(w.tw.set_indent(e) == 0)))
        }
        "setIndentString" => {
            let v = st!(0);
            with_writer(vm, ctx, o, |_, w| Ok(bv(w.tw.set_indent_string(&v) == 0)))
        }
        "startComment" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.start_comment()))),
        "endComment" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_comment()))),
        "startAttribute" | "startElement" | "startPi" | "startDtdElement" | "startDtdAttlist" => {
            let name = st!(0);
            with_writer(vm, ctx, o, |vm, w| {
                let subject = match m {
                    "startAttribute" => "attribute name",
                    "startPi" => "PI target",
                    _ => "element name",
                };
                name_check(vm, ctx, 2, subject, &name)?;
                Ok(b(match m {
                    "startAttribute" => w.tw.start_attribute(&name),
                    "startElement" => w.tw.start_element(&name),
                    "startPi" => w.tw.start_pi(&name),
                    "startDtdElement" => w.tw.start_dtd_element(&name),
                    _ => w.tw.start_dtd_attlist(&name),
                }))
            })
        }
        "endAttribute" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_attribute()))),
        "writeAttribute" | "writePi" | "writeDtdElement" | "writeDtdAttlist" => {
            let name = st!(0);
            let content = st!(1);
            with_writer(vm, ctx, o, |vm, w| {
                let subject = match m {
                    "writeAttribute" => "attribute name",
                    "writePi" => "PI target",
                    _ => "element name",
                };
                name_check(vm, ctx, 2, subject, &name)?;
                Ok(b(match m {
                    "writeAttribute" => w.tw.write_attribute(&name, &content),
                    "writePi" => w.tw.write_pi(&name, &content),
                    "writeDtdElement" => w.tw.write_dtd_element(&name, &content),
                    _ => w.tw.write_dtd_attlist(&name, &content),
                }))
            })
        }
        "startAttributeNs" | "startElementNs" => {
            let prefix = nst!(0);
            let name = st!(1);
            let uri = nst!(2);
            with_writer(vm, ctx, o, |vm, w| {
                let attr = m == "startAttributeNs";
                name_check(
                    vm,
                    ctx,
                    3,
                    if attr {
                        "attribute name"
                    } else {
                        "element name"
                    },
                    &name,
                )?;
                Ok(b(if attr {
                    w.tw.start_attribute_ns(prefix.as_deref(), &name, uri.as_deref())
                } else {
                    w.tw.start_element_ns(prefix.as_deref(), &name, uri.as_deref())
                }))
            })
        }
        "writeAttributeNs" => {
            let prefix = nst!(0);
            let name = st!(1);
            let uri = nst!(2);
            let content = st!(3);
            with_writer(vm, ctx, o, |vm, w| {
                name_check(vm, ctx, 3, "attribute name", &name)?;
                Ok(b(w.tw.write_attribute_ns(
                    prefix.as_deref(),
                    &name,
                    uri.as_deref(),
                    &content,
                )))
            })
        }
        "endElement" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_element()))),
        "fullEndElement" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.full_end_element()))),
        "writeElement" => {
            let name = st!(0);
            let content = nst!(1);
            with_writer(vm, ctx, o, |vm, w| {
                name_check(vm, ctx, 2, "element name", &name)?;
                Ok(match content {
                    None => {
                        if w.tw.start_element(&name) == -1 {
                            Value::False
                        } else {
                            b(w.tw.end_element())
                        }
                    }
                    Some(c) => b(w.tw.write_element(&name, Some(&c))),
                })
            })
        }
        "writeElementNs" => {
            let prefix = nst!(0);
            let name = st!(1);
            let uri = nst!(2);
            let content = nst!(3);
            with_writer(vm, ctx, o, |vm, w| {
                name_check(vm, ctx, 3, "element name", &name)?;
                Ok(match content {
                    None => {
                        if w.tw
                            .start_element_ns(prefix.as_deref(), &name, uri.as_deref())
                            == -1
                        {
                            Value::False
                        } else {
                            b(w.tw.end_element())
                        }
                    }
                    Some(c) => {
                        b(w.tw.write_element_ns(prefix.as_deref(), &name, uri.as_deref(), &c))
                    }
                })
            })
        }
        "endPi" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_pi()))),
        "startCdata" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.start_cdata()))),
        "endCdata" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_cdata()))),
        "writeCdata" | "text" | "writeRaw" | "writeComment" => {
            let c = st!(0);
            with_writer(vm, ctx, o, |_, w| {
                Ok(b(match m {
                    "writeCdata" => w.tw.write_cdata(&c),
                    "text" => w.tw.write_string(&c),
                    "writeRaw" => w.tw.write_raw(&c),
                    _ => w.tw.write_comment(&c),
                }))
            })
        }
        "startDocument" => {
            let version = nst!(0);
            let encoding = ctx.path_arg(vm, args, 1, true)?;
            let standalone = nst!(2);
            with_writer(vm, ctx, o, |vm, w| {
                let enc = encoding.map(|e| find_encoder(vm, &e));
                Ok(b(w.tw.start_document(
                    version.as_deref(),
                    enc,
                    standalone.as_deref(),
                )))
            })
        }
        "endDocument" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_document()))),
        "startDtd" => {
            let name = st!(0);
            let pubid = nst!(1);
            let sysid = nst!(2);
            with_writer(vm, ctx, o, |_, w| {
                Ok(b(w.tw.start_dtd(&name, pubid.as_deref(), sysid.as_deref())))
            })
        }
        "endDtd" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_dtd()))),
        "writeDtd" => {
            let name = st!(0);
            let pubid = nst!(1);
            let sysid = nst!(2);
            let subset = nst!(3);
            with_writer(vm, ctx, o, |_, w| {
                Ok(b(w.tw.write_dtd(
                    &name,
                    pubid.as_deref(),
                    sysid.as_deref(),
                    subset.as_deref(),
                )))
            })
        }
        "endDtdElement" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_dtd_element()))),
        "endDtdAttlist" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_dtd_attlist()))),
        "startDtdEntity" => {
            let name = st!(0);
            let pe = ctx.bool_arg(vm, args, 1)?.unwrap_or(false);
            with_writer(vm, ctx, o, |vm, w| {
                name_check(vm, ctx, 2, "attribute name", &name)?;
                Ok(b(w.tw.start_dtd_entity(pe, &name)))
            })
        }
        "endDtdEntity" => with_writer(vm, ctx, o, |_, w| Ok(b(w.tw.end_dtd_entity()))),
        "writeDtdEntity" => {
            let name = st!(0);
            let content = st!(1);
            let pe = ctx.bool_arg(vm, args, 2)?.unwrap_or(false);
            let pubid = nst!(3);
            let sysid = nst!(4);
            let ndata = nst!(5);
            with_writer(vm, ctx, o, |vm, w| {
                name_check(vm, ctx, 2, "element name", &name)?;
                Ok(b(w.tw.write_dtd_entity(
                    pe,
                    &name,
                    pubid.as_deref(),
                    sysid.as_deref(),
                    ndata.as_deref(),
                    &content,
                )))
            })
        }
        "outputMemory" | "flush" => {
            let empty = ctx.bool_arg(vm, args, 0)?.unwrap_or(true);
            let force_string = m == "outputMemory";
            let mut w = match take_writer(o) {
                Some(w) => w,
                None => {
                    return Err(throw(
                        vm,
                        b"Error",
                        "Invalid or uninitialized XMLWriter object",
                    ));
                }
            };
            let r = if force_string && !matches!(w.sink, Sink::Memory(_)) {
                s(b"")
            } else {
                let bytes = pump(Some(vm), &mut w, true);
                match &mut w.sink {
                    Sink::Memory(out) => {
                        if empty {
                            s(&std::mem::take(out))
                        } else {
                            s(out)
                        }
                    }
                    _ => Value::Long(bytes),
                }
            };
            put_writer(o, w);
            Ok(r)
        }
        _ => Ok(Value::Null),
    }
}

fn bv(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

// ── Entry points ────────────────────────────────────────────────────────────

fn function_entry(vm: &mut Vm, args: &[Value], method: &str) -> R<Value> {
    let spec = spec_by_method(method.as_bytes()).expect("xmlwriter spec");
    let ctx = Ctx {
        fname: spec.func.to_string(),
        is_method: false,
        spec,
    };
    let extra = if spec.takes_writer { 1 } else { 0 };
    check_count(
        vm,
        &ctx.fname,
        count_args(args),
        spec.min + extra,
        spec.params.len() + extra,
    )?;
    if !spec.takes_writer {
        return dispatch(vm, &ctx, None, b"XMLWriter", args);
    }
    let w = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let o = match &w {
        Value::Object(o) if is_xmlwriter_class(vm, &o.borrow().class_name.to_ascii_lowercase()) => {
            o.clone()
        }
        _ => {
            let m = format!(
                "{}(): Argument #1 ($writer) must be of type XMLWriter, {} given",
                ctx.fname,
                Vm::value_type_name(&w)
            );
            return Err(throw(vm, b"TypeError", &m));
        }
    };
    dispatch(vm, &ctx, Some(o), b"XMLWriter", &args[1..])
}

fn has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    match spec_by_method(method) {
        Some(s) => s.is_static || !is_static,
        None => false,
    }
}

fn call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let spec = match spec_by_method(method) {
        Some(s) => s,
        None => return Value::Null,
    };
    let ctx = Ctx {
        fname: format!("XMLWriter::{}", spec.method),
        is_method: true,
        spec,
    };
    // The engine names the frame after the lowercased native class.
    for sep in ["->", "::"] {
        let canonical = format!("XMLWriter{}{}", sep, spec.method);
        if let Some(top) = vm.call_stack.last_mut() {
            if top.0.eq_ignore_ascii_case(&canonical) {
                top.0 = canonical;
            }
        }
    }
    let this_obj = this.and_then(|t| match t.deref() {
        Value::Object(o) => Some(o),
        _ => None,
    });
    let called_class: Vec<u8> = match &this_obj {
        Some(o) if spec.is_static => o.borrow().class_name.clone(),
        _ => called.to_vec(),
    };
    let r = check_count(
        vm,
        &ctx.fname,
        count_args(args),
        spec.min,
        spec.params.len(),
    )
    .and_then(|_| {
        dispatch(
            vm,
            &ctx,
            if spec.is_static { None } else { this_obj },
            &called_class,
            args,
        )
    });
    r.unwrap_or(Value::Null)
}

fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    spec_by_method(method).map(|s| s.params)
}

macro_rules! procedural {
    ($($f:ident => $m:literal),* $(,)?) => {
        $( fn $f(vm: &mut Vm, args: &[Value]) -> R<Value> { function_entry(vm, args, $m) } )*
        fn register_functions(vm: &mut Vm) {
            $(
                {
                    let spec = spec_by_method($m.as_bytes()).unwrap();
                    let mut names: Vec<&[u8]> = Vec::new();
                    if spec.takes_writer {
                        names.push(b"writer");
                    }
                    names.extend(spec.params.iter().map(|p| p.as_bytes()));
                    vm.register_function_with_params(spec.func.as_bytes(), $f, &names);
                }
            )*
        }
    };
}

procedural! {
    f_open_uri => "openUri",
    f_open_memory => "openMemory",
    f_set_indent => "setIndent",
    f_set_indent_string => "setIndentString",
    f_start_comment => "startComment",
    f_end_comment => "endComment",
    f_start_attribute => "startAttribute",
    f_end_attribute => "endAttribute",
    f_write_attribute => "writeAttribute",
    f_start_attribute_ns => "startAttributeNs",
    f_write_attribute_ns => "writeAttributeNs",
    f_start_element => "startElement",
    f_end_element => "endElement",
    f_full_end_element => "fullEndElement",
    f_start_element_ns => "startElementNs",
    f_write_element => "writeElement",
    f_write_element_ns => "writeElementNs",
    f_start_pi => "startPi",
    f_end_pi => "endPi",
    f_write_pi => "writePi",
    f_start_cdata => "startCdata",
    f_end_cdata => "endCdata",
    f_write_cdata => "writeCdata",
    f_text => "text",
    f_write_raw => "writeRaw",
    f_start_document => "startDocument",
    f_end_document => "endDocument",
    f_write_comment => "writeComment",
    f_start_dtd => "startDtd",
    f_end_dtd => "endDtd",
    f_write_dtd => "writeDtd",
    f_start_dtd_element => "startDtdElement",
    f_end_dtd_element => "endDtdElement",
    f_write_dtd_element => "writeDtdElement",
    f_start_dtd_attlist => "startDtdAttlist",
    f_end_dtd_attlist => "endDtdAttlist",
    f_write_dtd_attlist => "writeDtdAttlist",
    f_start_dtd_entity => "startDtdEntity",
    f_end_dtd_entity => "endDtdEntity",
    f_write_dtd_entity => "writeDtdEntity",
    f_output_memory => "outputMemory",
    f_flush => "flush",
}

pub fn register(vm: &mut Vm) {
    // A fresh VM: forget writers of a previous request on this thread.
    WRITERS.with(|w| w.borrow_mut().clear());
    DEFERRED.with(|d| d.borrow_mut().clear());
    goro_core::object::OBJECT_FREE_HOOK.with(|h| h.set(Some(on_object_free)));
    vm.register_extension(b"xmlwriter");
    register_functions(vm);
    vm.classes.insert(
        b"xmlwriter".to_vec(),
        ClassEntry::new(b"XMLWriter".to_vec()),
    );
    let h = NativeClassHandler {
        has_method,
        call: call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: None,
        param_names: Some(param_names),
    };
    vm.native_class_handlers.insert(b"xmlwriter".to_vec(), h);
    vm.shutdown_hooks.push(shutdown);
}
