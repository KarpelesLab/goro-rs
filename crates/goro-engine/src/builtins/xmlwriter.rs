//! ext/xmlwriter (PHP manual: XMLWriter): the `XMLWriter` class and the
//! procedural `xmlwriter_*` functions on goro-xml's writer
//! ([`goro_xml::writer`]), writing to memory, a URI or a stream. API from
//! Reflection on PHP 8.5; argument numbering of the name ValueErrors and
//! flush results observed on PHP 8.5 and in the ext/xmlwriter tests.

use goro_runtime::{ObjRef, Value};
use goro_xml::writer::{WriterError, XmlWriter};

use crate::class::{DisplayName, NativeHooks};
use crate::func::{Args, NativeFn};
use crate::registry::Module;
use crate::vm::{R, Vm};

/// `(method, function, signature => handler)`: each handler serves the
/// method and the procedural function.
macro_rules! table {
    ($( $oo:literal, $proc:literal, $sig:literal => $f:ident; )*) => {
        static FUNCTIONS: &[(&str, NativeFn)] = &[ $( ($sig, $f), )* ];
        static METHODS: &[(&str, NativeFn)] = &[
            $( ($oo, $f), )*
            ("xmlwriter::touri", to_uri),
            ("xmlwriter::tomemory", to_memory),
            ("xmlwriter::tostream", to_stream),
        ];
    };
}

table! {
    "xmlwriter::openuri", "xmlwriter_open_uri", "function xmlwriter_open_uri(string $uri): XMLWriter|false" => open_uri;
    "xmlwriter::openmemory", "xmlwriter_open_memory", "function xmlwriter_open_memory(): XMLWriter|false" => open_memory;
    "xmlwriter::setindent", "xmlwriter_set_indent", "function xmlwriter_set_indent(XMLWriter $writer, bool $enable): bool" => set_indent;
    "xmlwriter::setindentstring", "xmlwriter_set_indent_string", "function xmlwriter_set_indent_string(XMLWriter $writer, string $indentation): bool" => set_indent_string;
    "xmlwriter::startcomment", "xmlwriter_start_comment", "function xmlwriter_start_comment(XMLWriter $writer): bool" => start_comment;
    "xmlwriter::endcomment", "xmlwriter_end_comment", "function xmlwriter_end_comment(XMLWriter $writer): bool" => end_comment;
    "xmlwriter::startattribute", "xmlwriter_start_attribute", "function xmlwriter_start_attribute(XMLWriter $writer, string $name): bool" => start_attribute;
    "xmlwriter::endattribute", "xmlwriter_end_attribute", "function xmlwriter_end_attribute(XMLWriter $writer): bool" => end_attribute;
    "xmlwriter::writeattribute", "xmlwriter_write_attribute", "function xmlwriter_write_attribute(XMLWriter $writer, string $name, string $value): bool" => write_attribute;
    "xmlwriter::startattributens", "xmlwriter_start_attribute_ns", "function xmlwriter_start_attribute_ns(XMLWriter $writer, ?string $prefix, string $name, ?string $namespace): bool" => start_attribute_ns;
    "xmlwriter::writeattributens", "xmlwriter_write_attribute_ns", "function xmlwriter_write_attribute_ns(XMLWriter $writer, ?string $prefix, string $name, ?string $namespace, string $value): bool" => write_attribute_ns;
    "xmlwriter::startelement", "xmlwriter_start_element", "function xmlwriter_start_element(XMLWriter $writer, string $name): bool" => start_element;
    "xmlwriter::endelement", "xmlwriter_end_element", "function xmlwriter_end_element(XMLWriter $writer): bool" => end_element;
    "xmlwriter::fullendelement", "xmlwriter_full_end_element", "function xmlwriter_full_end_element(XMLWriter $writer): bool" => full_end_element;
    "xmlwriter::startelementns", "xmlwriter_start_element_ns", "function xmlwriter_start_element_ns(XMLWriter $writer, ?string $prefix, string $name, ?string $namespace): bool" => start_element_ns;
    "xmlwriter::writeelement", "xmlwriter_write_element", "function xmlwriter_write_element(XMLWriter $writer, string $name, ?string $content = null): bool" => write_element;
    "xmlwriter::writeelementns", "xmlwriter_write_element_ns", "function xmlwriter_write_element_ns(XMLWriter $writer, ?string $prefix, string $name, ?string $namespace, ?string $content = null): bool" => write_element_ns;
    "xmlwriter::startpi", "xmlwriter_start_pi", "function xmlwriter_start_pi(XMLWriter $writer, string $target): bool" => start_pi;
    "xmlwriter::endpi", "xmlwriter_end_pi", "function xmlwriter_end_pi(XMLWriter $writer): bool" => end_pi;
    "xmlwriter::writepi", "xmlwriter_write_pi", "function xmlwriter_write_pi(XMLWriter $writer, string $target, string $content): bool" => write_pi;
    "xmlwriter::startcdata", "xmlwriter_start_cdata", "function xmlwriter_start_cdata(XMLWriter $writer): bool" => start_cdata;
    "xmlwriter::endcdata", "xmlwriter_end_cdata", "function xmlwriter_end_cdata(XMLWriter $writer): bool" => end_cdata;
    "xmlwriter::writecdata", "xmlwriter_write_cdata", "function xmlwriter_write_cdata(XMLWriter $writer, string $content): bool" => write_cdata;
    "xmlwriter::text", "xmlwriter_text", "function xmlwriter_text(XMLWriter $writer, string $content): bool" => text;
    "xmlwriter::writeraw", "xmlwriter_write_raw", "function xmlwriter_write_raw(XMLWriter $writer, string $content): bool" => write_raw;
    "xmlwriter::startdocument", "xmlwriter_start_document", "function xmlwriter_start_document(XMLWriter $writer, ?string $version = \"1.0\", ?string $encoding = null, ?string $standalone = null): bool" => start_document;
    "xmlwriter::enddocument", "xmlwriter_end_document", "function xmlwriter_end_document(XMLWriter $writer): bool" => end_document;
    "xmlwriter::writecomment", "xmlwriter_write_comment", "function xmlwriter_write_comment(XMLWriter $writer, string $content): bool" => write_comment;
    "xmlwriter::startdtd", "xmlwriter_start_dtd", "function xmlwriter_start_dtd(XMLWriter $writer, string $qualifiedName, ?string $publicId = null, ?string $systemId = null): bool" => start_dtd;
    "xmlwriter::enddtd", "xmlwriter_end_dtd", "function xmlwriter_end_dtd(XMLWriter $writer): bool" => end_dtd;
    "xmlwriter::writedtd", "xmlwriter_write_dtd", "function xmlwriter_write_dtd(XMLWriter $writer, string $name, ?string $publicId = null, ?string $systemId = null, ?string $content = null): bool" => write_dtd;
    "xmlwriter::startdtdelement", "xmlwriter_start_dtd_element", "function xmlwriter_start_dtd_element(XMLWriter $writer, string $qualifiedName): bool" => start_dtd_element;
    "xmlwriter::enddtdelement", "xmlwriter_end_dtd_element", "function xmlwriter_end_dtd_element(XMLWriter $writer): bool" => end_dtd_element;
    "xmlwriter::writedtdelement", "xmlwriter_write_dtd_element", "function xmlwriter_write_dtd_element(XMLWriter $writer, string $name, string $content): bool" => write_dtd_element;
    "xmlwriter::startdtdattlist", "xmlwriter_start_dtd_attlist", "function xmlwriter_start_dtd_attlist(XMLWriter $writer, string $name): bool" => start_dtd_attlist;
    "xmlwriter::enddtdattlist", "xmlwriter_end_dtd_attlist", "function xmlwriter_end_dtd_attlist(XMLWriter $writer): bool" => end_dtd_attlist;
    "xmlwriter::writedtdattlist", "xmlwriter_write_dtd_attlist", "function xmlwriter_write_dtd_attlist(XMLWriter $writer, string $name, string $content): bool" => write_dtd_attlist;
    "xmlwriter::startdtdentity", "xmlwriter_start_dtd_entity", "function xmlwriter_start_dtd_entity(XMLWriter $writer, string $name, bool $isParam): bool" => start_dtd_entity;
    "xmlwriter::enddtdentity", "xmlwriter_end_dtd_entity", "function xmlwriter_end_dtd_entity(XMLWriter $writer): bool" => end_dtd_entity;
    "xmlwriter::writedtdentity", "xmlwriter_write_dtd_entity", "function xmlwriter_write_dtd_entity(XMLWriter $writer, string $name, string $content, bool $isParam = false, ?string $publicId = null, ?string $systemId = null, ?string $notationData = null): bool" => write_dtd_entity;
    "xmlwriter::outputmemory", "xmlwriter_output_memory", "function xmlwriter_output_memory(XMLWriter $writer, bool $flush = true): string" => output_memory;
    "xmlwriter::flush", "xmlwriter_flush", "function xmlwriter_flush(XMLWriter $writer, bool $empty = true): string|int" => flush;
}

const CLASS: &str = r#"<?php
class XMLWriter {
    public function openUri(string $uri): bool {}
    public static function toUri(string $uri): static {}
    public function openMemory(): bool {}
    public static function toMemory(): static {}
    public static function toStream($stream): static {}
    public function setIndent(bool $enable): bool {}
    public function setIndentString(string $indentation): bool {}
    public function startComment(): bool {}
    public function endComment(): bool {}
    public function startAttribute(string $name): bool {}
    public function endAttribute(): bool {}
    public function writeAttribute(string $name, string $value): bool {}
    public function startAttributeNs(?string $prefix, string $name, ?string $namespace): bool {}
    public function writeAttributeNs(?string $prefix, string $name, ?string $namespace, string $value): bool {}
    public function startElement(string $name): bool {}
    public function endElement(): bool {}
    public function fullEndElement(): bool {}
    public function startElementNs(?string $prefix, string $name, ?string $namespace): bool {}
    public function writeElement(string $name, ?string $content = null): bool {}
    public function writeElementNs(?string $prefix, string $name, ?string $namespace, ?string $content = null): bool {}
    public function startPi(string $target): bool {}
    public function endPi(): bool {}
    public function writePi(string $target, string $content): bool {}
    public function startCdata(): bool {}
    public function endCdata(): bool {}
    public function writeCdata(string $content): bool {}
    public function text(string $content): bool {}
    public function writeRaw(string $content): bool {}
    public function startDocument(?string $version = "1.0", ?string $encoding = null, ?string $standalone = null): bool {}
    public function endDocument(): bool {}
    public function writeComment(string $content): bool {}
    public function startDtd(string $qualifiedName, ?string $publicId = null, ?string $systemId = null): bool {}
    public function endDtd(): bool {}
    public function writeDtd(string $name, ?string $publicId = null, ?string $systemId = null, ?string $content = null): bool {}
    public function startDtdElement(string $qualifiedName): bool {}
    public function endDtdElement(): bool {}
    public function writeDtdElement(string $name, string $content): bool {}
    public function startDtdAttlist(string $name): bool {}
    public function endDtdAttlist(): bool {}
    public function writeDtdAttlist(string $name, string $content): bool {}
    public function startDtdEntity(string $name, bool $isParam): bool {}
    public function endDtdEntity(): bool {}
    public function writeDtdEntity(string $name, string $content, bool $isParam = false, ?string $publicId = null, ?string $systemId = null, ?string $notationData = null): bool {}
    public function outputMemory(bool $flush = true): string {}
    public function flush(bool $empty = true): string|int {}
}"#;

pub static MODULE: Module = Module {
    name: "xmlwriter",
    functions: FUNCTIONS,
    classes: &[("xmlwriter", CLASS)],
    methods: METHODS,
    hooks: &[("xmlwriter", hooks)],
    ..Module::EMPTY
};

// ------------------------------------------------------------------ state

enum Out {
    Memory,
    /// A stream resource (opened for a URI, or given to `toStream()`).
    Stream(Value),
}

struct WriterData {
    w: XmlWriter,
    out: Out,
}

/// The native data: `None` until opened.
#[derive(Default)]
struct WriterObj(Option<WriterData>);

impl goro_runtime::NativeData for WriterObj {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        if let Some(WriterData {
            out: Out::Stream(v),
            ..
        }) = &self.0
        {
            visit(v);
        }
    }
}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|_, o| {
            *o.obj().native.borrow_mut() = Some(Box::new(WriterObj(None)));
        }),
        clone: Some(|_, _, to| {
            *to.obj().native.borrow_mut() = Some(Box::new(WriterObj(None)));
        }),
        destroy: Some(|vm, o| {
            let _ = write_out(vm, o);
        }),
        ..Default::default()
    }
}

fn with<T>(o: &ObjRef, f: impl FnOnce(&mut WriterObj) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<WriterObj>().map(f)
}

/// The writer object and the index of the first method argument (1 for
/// the procedural forms).
fn target(a: &Args) -> (ObjRef, usize) {
    match &a.this {
        Some(o) => (o.clone(), 0),
        None => (a.obj(0).expect("XMLWriter argument"), 1),
    }
}

fn s(a: &Args, i: usize) -> String {
    String::from_utf8_lossy(a.bytes(i)).into_owned()
}

fn os(a: &Args, i: usize) -> Option<String> {
    a.opt_str(i)
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
}

/// How invalid names are reported.
#[derive(Clone, Copy)]
enum Check {
    None,
    /// ValueError "must be a valid <what>" for the name at OO index `pos`.
    Name(usize, &'static str),
}

/// Runs a writer call: the uninitialized-object error, the name
/// ValueError, false for calls not allowed in the current state.
fn run(
    vm: &mut Vm,
    a: &Args,
    check: Check,
    f: impl FnOnce(&mut XmlWriter, usize) -> Result<(), WriterError>,
) -> R<Value> {
    let (o, base) = target(a);
    let r = with(&o, |d| d.0.as_mut().map(|d| f(&mut d.w, base))).flatten();
    match r {
        None => vm.throw("Error", "Invalid or uninitialized XMLWriter object"),
        Some(Ok(())) => Ok(Value::Bool(true)),
        Some(Err(WriterError::InvalidName)) => match check {
            Check::Name(pos, what) => {
                // "Argument #N" counts the procedural form's arguments; the
                // name in parentheses is the called form's parameter at
                // that position (observed on PHP 8.5).
                let n = pos + 2;
                let pname = vm.params_of(a.func).get(pos + 1).map(|p| p.name.dname());
                let given = a.str(base + pos).to_string_lossy();
                let arg = match pname {
                    Some(p) => format!("Argument #{n} (${p})"),
                    None => format!("Argument #{n}"),
                };
                vm.fthrow(
                    a,
                    "ValueError",
                    &format!("{arg} must be a valid {what}, \"{given}\" given"),
                )
            }
            Check::None => Ok(Value::Bool(false)),
        },
        Some(Err(_)) => Ok(Value::Bool(false)),
    }
}

// ------------------------------------------------------------------ output

/// Writes the buffered output to the stream target. The bytes written,
/// or -1 when the stream is gone.
fn write_out(vm: &mut Vm, o: &ObjRef) -> R<i64> {
    let Some(Some((bytes, res))) = with(o, |d| {
        d.0.as_mut().and_then(|d| match &d.out {
            Out::Stream(r) => Some((d.w.flush(true), r.clone())),
            Out::Memory => None,
        })
    }) else {
        return Ok(0);
    };
    let Value::Resource(r) = &res else {
        return Ok(-1);
    };
    let data = r.res().data.borrow();
    let Some(cell) = data
        .as_ref()
        .and_then(|d| d.downcast_ref::<std::cell::RefCell<super::file::Stream>>())
    else {
        return Ok(-1);
    };
    let cell: *const std::cell::RefCell<super::file::Stream> = cell;
    drop(data);
    if bytes.is_empty() {
        return Ok(0);
    }
    // SAFETY: the resource is held by `res` for the call.
    let mut st = unsafe { (*cell).borrow_mut() };
    match st.write_all(vm, &bytes) {
        Ok(n) => Ok(n as i64),
        Err(_) => Ok(-1),
    }
}

fn flush(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, base) = target(a);
    let empty = a.bool_or(base, true);
    let mem = with(&o, |d| {
        d.0.as_mut().map(|d| match d.out {
            Out::Memory => Some(d.w.flush(empty)),
            Out::Stream(_) => None,
        })
    })
    .flatten();
    match mem {
        None => vm.throw("Error", "Invalid or uninitialized XMLWriter object"),
        Some(Some(b)) => Ok(Value::from(b.as_slice())),
        Some(None) => Ok(Value::Int(write_out(vm, &o)?)),
    }
}

fn output_memory(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, base) = target(a);
    let fl = a.bool_or(base, true);
    let mem = with(&o, |d| {
        d.0.as_mut().map(|d| match d.out {
            Out::Memory => Some(d.w.flush(fl)),
            Out::Stream(_) => None,
        })
    })
    .flatten();
    match mem {
        None => vm.throw("Error", "Invalid or uninitialized XMLWriter object"),
        Some(Some(b)) => Ok(Value::from(b.as_slice())),
        Some(None) => {
            write_out(vm, &o)?;
            Ok(Value::from(&b""[..]))
        }
    }
}

// ------------------------------------------------------------------ opening

fn class_of_static(vm: &mut Vm, a: &Args) -> std::rc::Rc<crate::class::Class> {
    let p = if a.static_class.is_null() {
        a.func.scope.get()
    } else {
        a.static_class
    };
    vm.rc_class(p)
}

/// A new writer object of the static class, its constructor run.
fn new_static(vm: &mut Vm, a: &Args) -> R<ObjRef> {
    let cls = class_of_static(vm, a);
    let o = vm.instantiate(&cls)?;
    if cls.find_method(b"__construct").is_some() {
        vm.call_method(&o, b"__construct", Vec::new())?;
    }
    Ok(o)
}

fn new_writer() -> XmlWriter {
    XmlWriter::new()
}

fn set_target(o: &ObjRef, out: Out) {
    with(o, |d| {
        d.0 = Some(WriterData {
            w: new_writer(),
            out,
        })
    });
}

/// Opens `uri` for writing: the stream, or None after the warning.
fn open_uri_stream(vm: &mut Vm, a: &Args, uri: &[u8]) -> R<Option<Value>> {
    if uri.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($uri) must not be empty");
    }
    let path: &[u8] = if let Some(p) = uri.strip_prefix(b"file://localhost") {
        p
    } else if let Some(p) = uri.strip_prefix(b"file://") {
        p
    } else {
        uri
    };
    let plain = !path.windows(3).any(|w| w == b"://");
    let ok_path = !plain || {
        let fp = vm.fs_path(path);
        !path.ends_with(b"/")
            && !fp.is_dir()
            && fp
                .parent()
                .is_none_or(|d| d.as_os_str().is_empty() || d.is_dir())
    };
    let st = if ok_path {
        super::file::open_stream(vm, a, path, b"wb", true)?
    } else {
        None
    };
    match st {
        Some(s) => Ok(Some(Value::Resource(super::file::new_stream_res(s)))),
        None => {
            vm.fwarn(a, "Unable to resolve file path")?;
            Ok(None)
        }
    }
}

fn open_uri(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let uri = a.str(0);
    let Some(st) = open_uri_stream(vm, a, uri.as_bytes())? else {
        return Ok(Value::Bool(false));
    };
    match &a.this {
        Some(o) => {
            let o = o.clone();
            set_target(&o, Out::Stream(st));
            Ok(Value::Bool(true))
        }
        None => {
            let cls = vm
                .find_class(b"XMLWriter", false)?
                .expect("XMLWriter is declared");
            let o = vm.instantiate_raw(&cls)?;
            set_target(&o, Out::Stream(st));
            Ok(Value::Object(o))
        }
    }
}

fn open_memory(vm: &mut Vm, a: &mut Args) -> R<Value> {
    match &a.this {
        Some(o) => {
            let o = o.clone();
            set_target(&o, Out::Memory);
            Ok(Value::Bool(true))
        }
        None => {
            let cls = vm
                .find_class(b"XMLWriter", false)?
                .expect("XMLWriter is declared");
            let o = vm.instantiate_raw(&cls)?;
            set_target(&o, Out::Memory);
            Ok(Value::Object(o))
        }
    }
}

fn to_memory(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = new_static(vm, a)?;
    set_target(&o, Out::Memory);
    Ok(Value::Object(o))
}

fn to_uri(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let uri = a.str(0);
    let Some(st) = open_uri_stream(vm, a, uri.as_bytes())? else {
        return vm.fthrow(a, "Error", "Unable to resolve file path");
    };
    let o = new_static(vm, a)?;
    set_target(&o, Out::Stream(st));
    Ok(Value::Object(o))
}

fn to_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let valid = match &v {
        Value::Resource(r) => r.res().data.borrow().as_ref().is_some_and(|d| {
            d.downcast_ref::<std::cell::RefCell<super::file::Stream>>()
                .is_some()
        }),
        _ => {
            return vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #1 ($stream) must be of type resource, {} given",
                    crate::class::TypeName::tname(&v)
                ),
            );
        }
    };
    if !valid {
        return vm.fthrow(
            a,
            "TypeError",
            "supplied resource is not a valid stream resource",
        );
    }
    let o = new_static(vm, a)?;
    set_target(&o, Out::Stream(v));
    Ok(Value::Object(o))
}

// ------------------------------------------------------------------ calls

fn set_indent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let on = a.bool(b);
    run(vm, a, Check::None, |w, _| {
        w.set_indent(on);
        Ok(())
    })
}

fn set_indent_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let st = s(a, b);
    run(vm, a, Check::None, |w, _| {
        w.set_indent_string(&st);
        Ok(())
    })
}

macro_rules! simple {
    ($name:ident, $m:ident) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            run(vm, a, Check::None, |w, _| w.$m())
        }
    };
}

simple!(start_comment, start_comment);
simple!(end_comment, end_comment);
simple!(end_attribute, end_attribute);
simple!(end_element, end_element);
simple!(full_end_element, full_end_element);
simple!(end_pi, end_pi);
simple!(start_cdata, start_cdata);
simple!(end_cdata, end_cdata);
simple!(end_document, end_document);
simple!(end_dtd, end_dtd);
simple!(end_dtd_element, end_dtd_element);
simple!(end_dtd_attlist, end_dtd_attlist);
simple!(end_dtd_entity, end_dtd_entity);

const ELEMENT: &str = "element name";
const ATTRIBUTE: &str = "attribute name";
const PI: &str = "PI target";

fn start_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let n = s(a, b);
    run(vm, a, Check::Name(0, ATTRIBUTE), |w, _| {
        w.start_attribute(&n)
    })
}

fn write_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, v) = (s(a, b), s(a, b + 1));
    run(vm, a, Check::Name(0, ATTRIBUTE), |w, _| {
        w.write_attribute(&n, &v)
    })
}

fn start_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (p, n, u) = (os(a, b), s(a, b + 1), os(a, b + 2));
    run(vm, a, Check::Name(1, ATTRIBUTE), |w, _| {
        w.start_attribute_ns(p.as_deref(), &n, u.as_deref())
    })
}

fn write_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (p, n, u, v) = (os(a, b), s(a, b + 1), os(a, b + 2), s(a, b + 3));
    run(vm, a, Check::Name(1, ATTRIBUTE), |w, _| {
        w.write_attribute_ns(p.as_deref(), &n, u.as_deref(), &v)
    })
}

fn start_element(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let n = s(a, b);
    run(vm, a, Check::Name(0, ELEMENT), |w, _| w.start_element(&n))
}

fn start_element_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (p, n, u) = (os(a, b), s(a, b + 1), os(a, b + 2));
    run(vm, a, Check::Name(1, ELEMENT), |w, _| {
        w.start_element_ns(p.as_deref(), &n, u.as_deref())
    })
}

fn write_element(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, c) = (s(a, b), os(a, b + 1));
    run(vm, a, Check::Name(0, ELEMENT), |w, _| {
        w.write_element(&n, c.as_deref())
    })
}

fn write_element_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (p, n, u, c) = (os(a, b), s(a, b + 1), os(a, b + 2), os(a, b + 3));
    run(vm, a, Check::Name(1, ELEMENT), |w, _| {
        w.write_element_ns(p.as_deref(), &n, u.as_deref(), c.as_deref())
    })
}

fn start_pi(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let n = s(a, b);
    run(vm, a, Check::Name(0, PI), |w, _| w.start_pi(&n))
}

fn write_pi(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, c) = (s(a, b), s(a, b + 1));
    run(vm, a, Check::Name(0, PI), |w, _| w.write_pi(&n, &c))
}

fn write_cdata(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let c = s(a, b);
    run(vm, a, Check::None, |w, _| w.write_cdata(&c))
}

fn text(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let c = s(a, b);
    run(vm, a, Check::None, |w, _| w.text(&c))
}

fn write_raw(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let c = s(a, b);
    run(vm, a, Check::None, |w, _| w.write_raw(&c))
}

fn write_comment(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let c = s(a, b);
    run(vm, a, Check::None, |w, _| w.write_comment(&c))
}

fn start_document(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let v = if a.has(b) {
        os(a, b)
    } else {
        Some("1.0".to_string())
    };
    let (e, sa) = (os(a, b + 1), os(a, b + 2));
    run(vm, a, Check::None, |w, _| {
        w.start_document(v.as_deref(), e.as_deref(), sa.as_deref())
    })
}

fn start_dtd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, p, sy) = (s(a, b), os(a, b + 1), os(a, b + 2));
    run(vm, a, Check::None, |w, _| {
        w.start_dtd(&n, p.as_deref(), sy.as_deref())
    })
}

fn write_dtd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, p, sy, c) = (s(a, b), os(a, b + 1), os(a, b + 2), os(a, b + 3));
    run(vm, a, Check::None, |w, _| {
        w.write_dtd(&n, p.as_deref(), sy.as_deref(), c.as_deref())
    })
}

fn start_dtd_element(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let n = s(a, b);
    run(vm, a, Check::Name(0, ELEMENT), |w, _| {
        w.start_dtd_element(&n)
    })
}

fn write_dtd_element(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, c) = (s(a, b), s(a, b + 1));
    run(vm, a, Check::Name(0, ELEMENT), |w, _| {
        w.write_dtd_element(&n, &c)
    })
}

fn start_dtd_attlist(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let n = s(a, b);
    run(vm, a, Check::Name(0, ELEMENT), |w, _| {
        w.start_dtd_attlist(&n)
    })
}

fn write_dtd_attlist(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, c) = (s(a, b), s(a, b + 1));
    run(vm, a, Check::Name(0, ELEMENT), |w, _| {
        w.write_dtd_attlist(&n, &c)
    })
}

fn start_dtd_entity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, p) = (s(a, b), a.bool(b + 1));
    // "attribute name" here (observed on PHP 8.5).
    run(vm, a, Check::Name(0, ATTRIBUTE), |w, _| {
        w.start_dtd_entity(&n, p)
    })
}

fn write_dtd_entity(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, b) = target(a);
    let (n, c, p) = (s(a, b), s(a, b + 1), a.bool(b + 2));
    let (pu, sy, nd) = (os(a, b + 3), os(a, b + 4), os(a, b + 5));
    run(vm, a, Check::Name(0, ELEMENT), |w, _| {
        if pu.is_some() || sy.is_some() {
            w.write_dtd_external_entity(&n, p, pu.as_deref(), sy.as_deref(), nd.as_deref())
        } else {
            w.write_dtd_entity(&n, &c, p)
        }
    })
}
