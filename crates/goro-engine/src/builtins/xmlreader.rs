//! ext/xmlreader (PHP manual: XMLReader): the `XMLReader` pull parser on
//! goro-xml's reader ([`goro_xml::reader`]). API from Reflection on PHP
//! 8.5 (virtual properties, `open()`/`XML()` callable statically or on an
//! instance); error messages and states from the ext/xmlreader tests and
//! observations on PHP 8.5.

use super::dom::document::{SchemaLang, load_schema};
use goro_runtime::{ObjRef, Value};
use goro_xml::reader::XmlReader;

use super::libxml::{self, Style};
use crate::class::NativeHooks;
use crate::func::{Args, NativeFn};
use crate::registry::Module;
use crate::vm::{R, Vm};

const CLASS: &str = r#"<?php
class XMLReader {
    const int NONE = 0;
    const int ELEMENT = 1;
    const int ATTRIBUTE = 2;
    const int TEXT = 3;
    const int CDATA = 4;
    const int ENTITY_REF = 5;
    const int ENTITY = 6;
    const int PI = 7;
    const int COMMENT = 8;
    const int DOC = 9;
    const int DOC_TYPE = 10;
    const int DOC_FRAGMENT = 11;
    const int NOTATION = 12;
    const int WHITESPACE = 13;
    const int SIGNIFICANT_WHITESPACE = 14;
    const int END_ELEMENT = 15;
    const int END_ENTITY = 16;
    const int XML_DECLARATION = 17;
    const int LOADDTD = 1;
    const int DEFAULTATTRS = 2;
    const int VALIDATE = 3;
    const int SUBST_ENTITIES = 4;
    public int $attributeCount { get {} }
    public string $baseURI { get {} }
    public int $depth { get {} }
    public bool $hasAttributes { get {} }
    public bool $hasValue { get {} }
    public bool $isDefault { get {} }
    public bool $isEmptyElement { get {} }
    public string $localName { get {} }
    public string $name { get {} }
    public string $namespaceURI { get {} }
    public int $nodeType { get {} }
    public string $prefix { get {} }
    public string $value { get {} }
    public string $xmlLang { get {} }
    public function close(): true {}
    public function getAttribute(string $name): ?string {}
    public function getAttributeNo(int $index): ?string {}
    public function getAttributeNs(string $name, string $namespace): ?string {}
    public function getParserProperty(int $property): bool {}
    public function isValid(): bool {}
    public function lookupNamespace(string $prefix): ?string {}
    public function moveToAttribute(string $name): bool {}
    public function moveToAttributeNo(int $index): bool {}
    public function moveToAttributeNs(string $name, string $namespace): bool {}
    public function moveToElement(): bool {}
    public function moveToFirstAttribute(): bool {}
    public function moveToNextAttribute(): bool {}
    public function read(): bool {}
    public function next(?string $name = null): bool {}
    public static function open(string $uri, ?string $encoding = null, int $flags = 0) {}
    public static function fromUri(string $uri, ?string $encoding = null, int $flags = 0): static {}
    public static function fromStream($stream, ?string $encoding = null, int $flags = 0, ?string $documentUri = null): static {}
    public function readInnerXml(): string {}
    public function readOuterXml(): string {}
    public function readString(): string {}
    public function setSchema(?string $filename): bool {}
    public function setParserProperty(int $property, bool $value): bool {}
    public function setRelaxNGSchema(?string $filename): bool {}
    public function setRelaxNGSchemaSource(?string $source): bool {}
    public static function XML(string $source, ?string $encoding = null, int $flags = 0) {}
    public static function fromString(string $source, ?string $encoding = null, int $flags = 0): static {}
    public function expand(?DOMNode $baseNode = null): DOMNode|false {}
}"#;

static METHODS: &[(&str, NativeFn)] = &[
    ("xmlreader::$", prop),
    ("xmlreader::close", close),
    ("xmlreader::getattribute", get_attribute),
    ("xmlreader::getattributeno", get_attribute_no),
    ("xmlreader::getattributens", get_attribute_ns),
    ("xmlreader::getparserproperty", get_parser_property),
    ("xmlreader::isvalid", is_valid),
    ("xmlreader::lookupnamespace", lookup_namespace),
    ("xmlreader::movetoattribute", move_to_attribute),
    ("xmlreader::movetoattributeno", move_to_attribute_no),
    ("xmlreader::movetoattributens", move_to_attribute_ns),
    ("xmlreader::movetoelement", move_to_element),
    ("xmlreader::movetofirstattribute", move_to_first_attribute),
    ("xmlreader::movetonextattribute", move_to_next_attribute),
    ("xmlreader::read", read),
    ("xmlreader::next", next),
    ("xmlreader::open", open),
    ("xmlreader::fromuri", from_uri),
    ("xmlreader::fromstream", from_stream),
    ("xmlreader::readinnerxml", read_inner_xml),
    ("xmlreader::readouterxml", read_outer_xml),
    ("xmlreader::readstring", read_string),
    ("xmlreader::setschema", set_schema),
    ("xmlreader::setparserproperty", set_parser_property),
    ("xmlreader::setrelaxngschema", set_relaxng_schema),
    (
        "xmlreader::setrelaxngschemasource",
        set_relaxng_schema_source,
    ),
    ("xmlreader::xml", xml),
    ("xmlreader::fromstring", from_string),
    ("xmlreader::expand", expand),
];

pub static MODULE: Module = Module {
    name: "xmlreader",
    classes: &[("xmlreader", CLASS)],
    methods: METHODS,
    hooks: &[("xmlreader", hooks)],
    ..Module::EMPTY
};

// ------------------------------------------------------------------ state

struct Open {
    r: XmlReader,
    /// The document text (for the generic error display).
    input: Vec<u8>,
    flags: u32,
    /// `read()`/`next()` was called.
    started: bool,
    /// The reader's entity loader (owned by `r`).
    loader: *mut libxml::PhpLoader,
    /// `setSchema()` / `setRelaxNGSchema()`.
    schema: Option<goro_xml::schema::Compiled>,
    /// The document was validated against `schema`.
    schema_done: bool,
    /// No schema validity error so far.
    schema_valid: bool,
}

#[derive(Default)]
struct ReaderObj(Option<Box<Open>>);

impl goro_runtime::NativeData for ReaderObj {}

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags
                .set(cls.flags.get() | crate::class::cflags::NOT_CLONEABLE);
            *o.obj().native.borrow_mut() = Some(Box::new(ReaderObj(None)));
        }),
        // var_dump shows the virtual properties' values; (array), json and
        // get_object_vars() show nothing (observed on PHP 8.5; tests
        // var_dump, virtual_properties).
        debug_props: Some(|vm, o| {
            // Declared properties of subclasses first; isEmptyElement is
            // left out before the first read (PHP throws there; observed
            // on PHP 8.5, test fromString_custom_constructor).
            let mut a = vm.plain_props_array(o, true);
            let fresh = with_open(o, |op| !op.started).unwrap_or(false);
            for n in PROPS {
                if fresh && n == "isEmptyElement" {
                    continue;
                }
                a.insert(
                    goro_runtime::Key::from_bytes(n.as_bytes()),
                    prop_value(vm, o, n),
                );
            }
            Some(a)
        }),
        static_this: true,
        ..Default::default()
    }
}

const PROPS: [&str; 14] = [
    "attributeCount",
    "baseURI",
    "depth",
    "hasAttributes",
    "hasValue",
    "isDefault",
    "isEmptyElement",
    "localName",
    "name",
    "namespaceURI",
    "nodeType",
    "prefix",
    "value",
    "xmlLang",
];

fn with<T>(o: &ObjRef, f: impl FnOnce(&mut ReaderObj) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<ReaderObj>().map(f)
}

/// Runs `f` on the open reader (None when no data is loaded).
fn with_open<T>(o: &ObjRef, f: impl FnOnce(&mut Open) -> T) -> Option<T> {
    with(o, |d| d.0.as_deref_mut().map(f)).flatten()
}

fn this(a: &Args) -> ObjRef {
    a.this_obj().clone()
}

/// Reports the errors the last operation produced.
fn report(vm: &mut Vm, a: &Args, o: &ObjRef) -> R<()> {
    let Some((errs, input, flags)) = with_open(o, |op| {
        let mut errs = op.r.take_errors();
        // Schema validation once the document element is complete (the
        // errors show on the read() that gets there; test 013.phpt).
        if let Some(s) = &op.schema
            && !op.schema_done
            && let Some(root) = op.r.arena.document_element(op.r.doc)
            && op.r.is_node_complete(root)
        {
            op.schema_done = true;
            let v = s.validate_root(&mut op.r.arena, root, false);
            if !v.is_empty() {
                op.schema_valid = false;
            }
            errs.extend(v);
        }
        (errs, std::mem::take(&mut op.input), op.flags)
    }) else {
        return Ok(());
    };
    let r = libxml::report(vm, &a.fname(), &errs, Style::Generic, flags, Some(&input));
    // SAFETY: the loader lives in the reader, which is alive (the object
    // holds it) and not borrowed now.
    let (pending, warnings) = with_open(o, |op| {
        op.input = input;
        let l = unsafe { &mut *op.loader };
        (l.pending.take(), std::mem::take(&mut l.warnings))
    })
    .unwrap_or_default();
    for w in warnings {
        vm.fwarn(a, &w)?;
    }
    if let Some(u) = pending {
        return Err(u);
    }
    r
}

// ------------------------------------------------------------------ properties

fn prop_value(_vm: &mut Vm, o: &ObjRef, name: &str) -> Value {
    let v = with_open(o, |op| {
        let r = &op.r;
        match name {
            "attributeCount" => Value::Int(r.attribute_count()),
            // The current node's base: empty before the first read
            // (observed on PHP 8.5; tests var_dump,
            // fromString_custom_constructor).
            "baseURI" => {
                if r.node_type() == 0 {
                    Value::from(&b""[..])
                } else {
                    Value::from(r.base_uri().as_bytes())
                }
            }
            "depth" => Value::Int(r.depth()),
            "hasAttributes" => Value::Bool(r.attribute_count() > 0),
            "hasValue" => Value::Bool(r.has_value()),
            "isDefault" => Value::Bool(false),
            "isEmptyElement" => Value::Bool(r.is_empty_element()),
            "localName" => Value::from(r.local_name().as_bytes()),
            "name" => Value::from(r.name().as_bytes()),
            "namespaceURI" => Value::from(r.namespace_uri().as_bytes()),
            "nodeType" => Value::Int(r.node_type()),
            "prefix" => Value::from(r.prefix().as_bytes()),
            "value" => Value::from(r.value().as_bytes()),
            "xmlLang" => Value::from(r.xml_lang().as_bytes()),
            _ => Value::Null,
        }
    });
    v.unwrap_or_else(|| match name {
        "attributeCount" | "depth" | "nodeType" => Value::Int(0),
        "hasAttributes" | "hasValue" | "isDefault" | "isEmptyElement" => Value::Bool(false),
        _ => Value::from(&b""[..]),
    })
}

/// The get hook of every virtual property (the hook's name is
/// `XMLReader::$name::get`).
fn prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let fname = a.func.name.to_string_lossy();
    let name = fname
        .split('$')
        .nth(1)
        .and_then(|s| s.split("::").next())
        .unwrap_or("")
        .to_string();
    let o = this(a);
    Ok(prop_value(vm, &o, &name))
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

fn new_static(vm: &mut Vm, a: &Args) -> R<ObjRef> {
    let cls = class_of_static(vm, a);
    let o = vm.instantiate(&cls)?;
    if cls.find_method(b"__construct").is_some() {
        vm.call_method(&o, b"__construct", Vec::new())?;
    }
    Ok(o)
}

fn check_encoding(vm: &mut Vm, a: &Args, i: usize) -> R<Option<String>> {
    match a.opt_str(i) {
        None => Ok(None),
        Some(e) => {
            if e.as_bytes().contains(&0) {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #{} ($encoding) must not contain any null bytes",
                        i + 1
                    ),
                );
            }
            let e = e.to_string_lossy();
            if goro_xml::encoding::lookup(&e).is_none() {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #{} ($encoding) must be a valid character encoding",
                        i + 1
                    ),
                );
            }
            Ok(Some(e))
        }
    }
}

fn load(vm: &mut Vm, o: &ObjRef, input: Vec<u8>, enc: Option<&str>, flags: u32, url: Option<&str>) {
    let mut r = XmlReader::new(input.clone(), enc, flags, url);
    let mut l = Box::new(libxml::PhpLoader::new(vm, "XMLReader::read"));
    l.nonet = flags & goro_xml::xml::opt::NONET != 0;
    let loader: *mut libxml::PhpLoader = &mut *l;
    r.set_loader(l);
    with(o, |d| {
        d.0 = Some(Box::new(Open {
            r,
            input,
            flags,
            started: false,
            loader,
            schema: None,
            schema_done: false,
            schema_valid: true,
        }))
    });
}

/// The base URL of in-memory documents: the working directory (observed
/// on PHP 8.5 in error messages).
fn cwd_url(vm: &Vm) -> String {
    let mut s = vm.cwd.to_string_lossy().into_owned();
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// Reads a URI's document. `None` after "Unable to open source data".
fn read_uri(vm: &mut Vm, a: &Args, uri: &[u8]) -> R<Option<(Vec<u8>, String)>> {
    if uri.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($uri) must not be empty");
    }
    let path = String::from_utf8_lossy(uri).into_owned();
    let url = libxml::resolve(vm, &path, None);
    match libxml::load_document(vm, &a.fname(), &path)? {
        Ok(b) => Ok(Some((b, url))),
        Err(Some(msg)) => {
            vm.fwarn(a, &msg)?;
            Ok(None)
        }
        Err(None) => Ok(None),
    }
}

fn open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let enc = check_encoding(vm, a, 1)?;
    let flags = a.int(2) as u32;
    let uri = a.str(0);
    let Some((bytes, url)) = read_uri(vm, a, uri.as_bytes())? else {
        vm.fwarn(a, "Unable to open source data")?;
        return Ok(Value::Bool(false));
    };
    match &a.this {
        Some(o) => {
            let o = o.clone();
            load(vm, &o, bytes, enc.as_deref(), flags, Some(&url));
            Ok(Value::Bool(true))
        }
        None => {
            let cls = class_of_static(vm, a);
            let o = vm.instantiate_raw(&cls)?;
            load(vm, &o, bytes, enc.as_deref(), flags, Some(&url));
            Ok(Value::Object(o))
        }
    }
}

fn from_uri(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let enc = check_encoding(vm, a, 1)?;
    let flags = a.int(2) as u32;
    let uri = a.str(0);
    let Some((bytes, url)) = read_uri(vm, a, uri.as_bytes())? else {
        return vm.throw("Error", "Unable to open source data");
    };
    let o = new_static(vm, a)?;
    load(vm, &o, bytes, enc.as_deref(), flags, Some(&url));
    Ok(Value::Object(o))
}

fn from_stream(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let Value::Resource(res) = &v else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($stream) must be of type resource, {} given",
                crate::class::TypeName::tname(&v)
            ),
        );
    };
    let enc = check_encoding(vm, a, 1)?;
    let flags = a.int(2) as u32;
    let Some(bytes) = libxml::read_resource(vm, res) else {
        return vm.fthrow(
            a,
            "TypeError",
            "Argument #1 ($stream) must be a valid stream resource",
        );
    };
    let url = match a.opt_str(3) {
        Some(u) => u.to_string_lossy(),
        None => cwd_url(vm),
    };
    let o = new_static(vm, a)?;
    load(vm, &o, bytes, enc.as_deref(), flags, Some(&url));
    Ok(Value::Object(o))
}

fn xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    if src.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($source) must not be empty");
    }
    let enc = check_encoding(vm, a, 1)?;
    let flags = a.int(2) as u32;
    let url = cwd_url(vm);
    match &a.this {
        Some(o) => {
            let o = o.clone();
            load(
                vm,
                &o,
                src.as_bytes().to_vec(),
                enc.as_deref(),
                flags,
                Some(&url),
            );
            Ok(Value::Bool(true))
        }
        None => {
            let cls = class_of_static(vm, a);
            let o = vm.instantiate_raw(&cls)?;
            load(
                vm,
                &o,
                src.as_bytes().to_vec(),
                enc.as_deref(),
                flags,
                Some(&url),
            );
            Ok(Value::Object(o))
        }
    }
}

fn from_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let src = a.str(0);
    if src.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($source) must not be empty");
    }
    let enc = check_encoding(vm, a, 1)?;
    let flags = a.int(2) as u32;
    let url = cwd_url(vm);
    let o = new_static(vm, a)?;
    load(
        vm,
        &o,
        src.as_bytes().to_vec(),
        enc.as_deref(),
        flags,
        Some(&url),
    );
    Ok(Value::Object(o))
}

fn close(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    with(&o, |d| d.0 = None);
    Ok(Value::Bool(true))
}

// ------------------------------------------------------------------ cursor

fn read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    let Some(ok) = with_open(&o, |op| {
        op.started = true;
        op.r.read()
    }) else {
        return vm.throw("Error", "Data must be loaded before reading");
    };
    report(vm, a, &o)?;
    Ok(Value::Bool(ok))
}

fn next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    let name = a.opt_str(0).map(|s| s.to_string_lossy());
    let Some(ok) = with_open(&o, |op| {
        op.started = true;
        op.r.next(name.as_deref())
    }) else {
        return vm.throw("Error", "Data must be loaded before reading");
    };
    report(vm, a, &o)?;
    Ok(Value::Bool(ok))
}

fn opt_string(v: Option<String>) -> Value {
    v.map_or(Value::Null, |s| Value::from(s.as_bytes()))
}

fn nonempty(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<String> {
    let s = a.str(i);
    if s.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} (${name}) must not be empty", i + 1),
        );
    }
    Ok(s.to_string_lossy())
}

fn get_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = nonempty(vm, a, 0, "name")?;
    let o = this(a);
    Ok(opt_string(
        with_open(&o, |op| op.r.get_attribute(&n)).flatten(),
    ))
}

fn get_attribute_no(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = a.int(0);
    let o = this(a);
    if i < 0 {
        return Ok(Value::Null);
    }
    Ok(opt_string(
        with_open(&o, |op| op.r.get_attribute_no(i as usize)).flatten(),
    ))
}

fn get_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = nonempty(vm, a, 0, "name")?;
    let ns = nonempty(vm, a, 1, "namespace")?;
    let o = this(a);
    Ok(opt_string(
        with_open(&o, |op| op.r.get_attribute_ns(&n, &ns)).flatten(),
    ))
}

fn lookup_namespace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = nonempty(vm, a, 0, "prefix")?;
    let o = this(a);
    Ok(opt_string(
        with_open(&o, |op| op.r.lookup_namespace(&p)).flatten(),
    ))
}

fn move_to_attribute(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = nonempty(vm, a, 0, "name")?;
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| op.r.move_to_attribute(&n)).unwrap_or(false),
    ))
}

fn move_to_attribute_no(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i = a.int(0);
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| {
            // A missing index moves back to the element (observed on PHP
            // 8.5; test 003-move-errors).
            if i >= 0 && op.r.move_to_attribute_no(i as usize) {
                true
            } else {
                op.r.move_to_element();
                false
            }
        })
        .unwrap_or(false),
    ))
}

fn move_to_attribute_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = nonempty(vm, a, 0, "name")?;
    let ns = nonempty(vm, a, 1, "namespace")?;
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| op.r.move_to_attribute_ns(&n, &ns)).unwrap_or(false),
    ))
}

fn move_to_element(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| op.r.move_to_element()).unwrap_or(false),
    ))
}

fn move_to_first_attribute(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| op.r.move_to_first_attribute()).unwrap_or(false),
    ))
}

fn move_to_next_attribute(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| op.r.move_to_next_attribute()).unwrap_or(false),
    ))
}

fn read_inner_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    let s = with_open(&o, |op| op.r.read_inner_xml()).unwrap_or_default();
    report(vm, a, &o)?;
    Ok(Value::from(s.as_bytes()))
}

fn read_outer_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    let s = with_open(&o, |op| op.r.read_outer_xml()).unwrap_or_default();
    report(vm, a, &o)?;
    Ok(Value::from(s.as_bytes()))
}

fn read_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    let s = with_open(&o, |op| op.r.read_string()).unwrap_or_default();
    report(vm, a, &o)?;
    Ok(Value::from(s.as_bytes()))
}

// ------------------------------------------------------------------ properties

fn parser_prop(vm: &mut Vm, a: &Args) -> R<i64> {
    let p = a.int(0);
    if !(1..=4).contains(&p) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($property) must be a valid parser property",
        );
    }
    Ok(p)
}

fn get_parser_property(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    if with_open(&o, |_| ()).is_none() {
        return vm.throw(
            "Error",
            "Cannot access parser properties before loading data",
        );
    }
    let p = parser_prop(vm, a)?;
    Ok(Value::Bool(
        with_open(&o, |op| match p {
            1 => op.r.props.load_dtd,
            2 => op.r.props.default_attrs,
            3 => op.r.props.validate,
            _ => op.r.props.subst_entities,
        })
        .unwrap_or(false),
    ))
}

fn set_parser_property(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    if with_open(&o, |_| ()).is_none() {
        return vm.throw(
            "Error",
            "Cannot access parser properties before loading data",
        );
    }
    let p = parser_prop(vm, a)?;
    let v = a.bool(1);
    // Refused once reading started (observed on PHP 8.5).
    if with_open(&o, |op| op.started).unwrap_or(false) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($property) must be a valid parser property",
        );
    }
    with_open(&o, |op| match p {
        1 => op.r.props.load_dtd = v,
        2 => op.r.props.default_attrs = v,
        3 => op.r.props.validate = v,
        _ => op.r.props.subst_entities = v,
    });
    Ok(Value::Bool(true))
}

fn is_valid(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    Ok(Value::Bool(
        with_open(&o, |op| {
            op.started
                && !op.r.failed
                && if op.schema.is_some() {
                    op.schema_valid
                } else {
                    op.r.props.validate
                }
        })
        .unwrap_or(false),
    ))
}

/// `setSchema()`, `setRelaxNGSchema()`, `setRelaxNGSchemaSource()`: only
/// before reading (observed on PHP 8.5: the errors for unloaded or
/// started readers; tests setSchema_error.phpt, 007.phpt, 013.phpt).
fn schema_common(vm: &mut Vm, a: &Args, what: &str, lang: SchemaLang) -> R<Value> {
    let o = this(a);
    if let Some(s) = a.opt_str(0)
        && s.is_empty()
    {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #1 (${what}) must not be empty"),
        );
    }
    let state = with_open(&o, |op| op.started);
    match state {
        None | Some(true) => vm.throw("Error", "Schema must be set prior to reading"),
        Some(false) => {
            let Some(s) = a.opt_str(0) else {
                with_open(&o, |op| op.schema = None);
                return Ok(Value::Bool(true));
            };
            let path = (what == "filename").then(|| s.to_string_lossy());
            match load_schema(vm, &a.fname(), lang, path.as_deref(), s.as_bytes())? {
                Ok(c) => {
                    with_open(&o, |op| op.schema = Some(c));
                    Ok(Value::Bool(true))
                }
                Err(_) => {
                    vm.fwarn(a, "Schema contains errors")?;
                    Ok(Value::Bool(false))
                }
            }
        }
    }
}

fn set_schema(vm: &mut Vm, a: &mut Args) -> R<Value> {
    schema_common(vm, a, "filename", SchemaLang::Xsd)
}

fn set_relaxng_schema(vm: &mut Vm, a: &mut Args) -> R<Value> {
    schema_common(vm, a, "filename", SchemaLang::RelaxNg)
}

fn set_relaxng_schema_source(vm: &mut Vm, a: &mut Args) -> R<Value> {
    schema_common(vm, a, "source", SchemaLang::RelaxNg)
}

// ------------------------------------------------------------------ expand

fn expand(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = this(a);
    if with_open(&o, |_| ()).is_none() {
        return vm.throw("Error", "Data must be loaded before expanding");
    }
    // The document the copy belongs to: the base node's, or a new one.
    let base_doc = match a.obj(0) {
        Some(b) => match super::dom::node_of(&b) {
            Some(n) => Some(super::xmlstore::with(|s| s.arena.doc_of(n))),
            None => {
                let cls = vm.class_of(&b);
                vm.fwarn(
                    a,
                    &format!(
                        "Couldn't fetch {}",
                        crate::class::DisplayName::dname(&cls.name)
                    ),
                )?;
                return Ok(Value::Bool(false));
            }
        },
        None => None,
    };
    let Some(Some(n)) = with_open(&o, |op| op.r.expand()) else {
        report(vm, a, &o)?;
        vm.fwarn(a, "An Error Occurred while expanding")?;
        return Ok(Value::Bool(false));
    };
    report(vm, a, &o)?;
    let copy = with_open(&o, |op| {
        let r = &op.r;
        super::xmlstore::with(|s| {
            let doc = base_doc.unwrap_or_else(|| {
                let d = s.arena.new_document(goro_xml::tree::DocKind::Xml);
                s.arena.doc_mut(d).url = r.url.clone();
                d
            });
            let c = s.arena.import_from(&r.arena, n, doc);
            (c, base_doc.is_none().then_some(doc))
        })
    });
    let Some((c, new_doc)) = copy else {
        return Ok(Value::Bool(false));
    };
    let v = super::dom::wrap_node(vm, c)?;
    if let Some(d) = new_doc {
        // Frees the new document if nothing took a hold on it.
        super::xmlstore::with(|s| s.free_doc_if_unheld(d));
    }
    Ok(v)
}
