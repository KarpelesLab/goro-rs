//! ext/xml (PHP manual: XML Parser): `xml_parser_create[_ns]()`,
//! `xml_parse()`, `xml_parse_into_struct()`, the handler setters, options
//! and error functions, on goro-xml's event parser ([`goro_xml::sax`],
//! which carries the behaviour of PHP's libxml-based implementation).
//! Handler resolution rules (strings as method names of the object set
//! with `xml_set_object()`, the 8.4 deprecations, the object swap checks)
//! are from the ext/xml tests' expected output (set_handler_errors,
//! xml_set_object_multiple_times*) and observations on PHP 8.5.

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};
use goro_xml::sax::{self, Handlers, Position, SaxHandler, SaxParser, TargetEncoding};

use crate::class::{DisplayName, NativeHooks, cflags};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Unwind, Vm};

pub static MODULE: Module = Module {
    name: "xml",
    functions: &[
        (
            "function xml_parser_create(?string $encoding = null): XMLParser",
            xml_parser_create,
        ),
        (
            "function xml_parser_create_ns(?string $encoding = null, string $separator = \":\"): XMLParser",
            xml_parser_create_ns,
        ),
        (
            "function xml_set_object(XMLParser $parser, object $object): true",
            xml_set_object,
        ),
        (
            "function xml_set_element_handler(XMLParser $parser, $start_handler, $end_handler): true",
            xml_set_element_handler,
        ),
        (
            "function xml_set_character_data_handler(XMLParser $parser, $handler): true",
            xml_set_character_data_handler,
        ),
        (
            "function xml_set_processing_instruction_handler(XMLParser $parser, $handler): true",
            xml_set_processing_instruction_handler,
        ),
        (
            "function xml_set_default_handler(XMLParser $parser, $handler): true",
            xml_set_default_handler,
        ),
        (
            "function xml_set_unparsed_entity_decl_handler(XMLParser $parser, $handler): true",
            xml_set_unparsed_entity_decl_handler,
        ),
        (
            "function xml_set_notation_decl_handler(XMLParser $parser, $handler): true",
            xml_set_notation_decl_handler,
        ),
        (
            "function xml_set_external_entity_ref_handler(XMLParser $parser, $handler): true",
            xml_set_external_entity_ref_handler,
        ),
        (
            "function xml_set_start_namespace_decl_handler(XMLParser $parser, $handler): true",
            xml_set_start_namespace_decl_handler,
        ),
        (
            "function xml_set_end_namespace_decl_handler(XMLParser $parser, $handler): true",
            xml_set_end_namespace_decl_handler,
        ),
        (
            "function xml_parse(XMLParser $parser, string $data, bool $is_final = false): int",
            xml_parse,
        ),
        (
            "function xml_parse_into_struct(XMLParser $parser, string $data, &$values, &$index = null): int|false",
            xml_parse_into_struct,
        ),
        (
            "function xml_get_error_code(XMLParser $parser): int",
            xml_get_error_code,
        ),
        (
            "function xml_error_string(int $error_code): ?string",
            xml_error_string,
        ),
        (
            "function xml_get_current_line_number(XMLParser $parser): int",
            xml_get_current_line_number,
        ),
        (
            "function xml_get_current_column_number(XMLParser $parser): int",
            xml_get_current_column_number,
        ),
        (
            "function xml_get_current_byte_index(XMLParser $parser): int",
            xml_get_current_byte_index,
        ),
        (
            "function xml_parser_free(XMLParser $parser): bool",
            xml_parser_free,
        ),
        (
            "function xml_parser_set_option(XMLParser $parser, int $option, $value): bool",
            xml_parser_set_option,
        ),
        (
            "function xml_parser_get_option(XMLParser $parser, int $option): string|int|bool",
            xml_parser_get_option,
        ),
    ],
    classes: &[(
        "xmlparser",
        "<?php #[NotSerializable] final class XMLParser {}",
    )],
    hooks: &[("xmlparser", parser_hooks)],
    constants: &[
        ("XML_ERROR_NONE", ConstVal::Int(0)),
        ("XML_ERROR_NO_MEMORY", ConstVal::Int(1)),
        ("XML_ERROR_SYNTAX", ConstVal::Int(2)),
        ("XML_ERROR_NO_ELEMENTS", ConstVal::Int(3)),
        ("XML_ERROR_INVALID_TOKEN", ConstVal::Int(4)),
        ("XML_ERROR_UNCLOSED_TOKEN", ConstVal::Int(5)),
        ("XML_ERROR_PARTIAL_CHAR", ConstVal::Int(6)),
        ("XML_ERROR_TAG_MISMATCH", ConstVal::Int(7)),
        ("XML_ERROR_DUPLICATE_ATTRIBUTE", ConstVal::Int(8)),
        ("XML_ERROR_JUNK_AFTER_DOC_ELEMENT", ConstVal::Int(9)),
        ("XML_ERROR_PARAM_ENTITY_REF", ConstVal::Int(10)),
        ("XML_ERROR_UNDEFINED_ENTITY", ConstVal::Int(11)),
        ("XML_ERROR_RECURSIVE_ENTITY_REF", ConstVal::Int(12)),
        ("XML_ERROR_ASYNC_ENTITY", ConstVal::Int(13)),
        ("XML_ERROR_BAD_CHAR_REF", ConstVal::Int(14)),
        ("XML_ERROR_BINARY_ENTITY_REF", ConstVal::Int(15)),
        ("XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF", ConstVal::Int(16)),
        ("XML_ERROR_MISPLACED_XML_PI", ConstVal::Int(17)),
        ("XML_ERROR_UNKNOWN_ENCODING", ConstVal::Int(18)),
        ("XML_ERROR_INCORRECT_ENCODING", ConstVal::Int(19)),
        ("XML_ERROR_UNCLOSED_CDATA_SECTION", ConstVal::Int(20)),
        ("XML_ERROR_EXTERNAL_ENTITY_HANDLING", ConstVal::Int(21)),
        ("XML_OPTION_CASE_FOLDING", ConstVal::Int(1)),
        ("XML_OPTION_TARGET_ENCODING", ConstVal::Int(2)),
        ("XML_OPTION_SKIP_TAGSTART", ConstVal::Int(3)),
        ("XML_OPTION_SKIP_WHITE", ConstVal::Int(4)),
        ("XML_OPTION_PARSE_HUGE", ConstVal::Int(5)),
        ("XML_SAX_IMPL", ConstVal::Str("libxml")),
    ],
    ..Module::EMPTY
};

const H_START: usize = 0;
const H_END: usize = 1;
const H_CDATA: usize = 2;
const H_PI: usize = 3;
const H_DEFAULT: usize = 4;
const H_UNPARSED: usize = 5;
const H_NOTATION: usize = 6;
const H_EXTERNAL: usize = 7;
const H_NS_START: usize = 8;
const H_NS_END: usize = 9;
const NH: usize = 10;

/// A handler as set: a callable, or a method name looked up on the
/// object of `xml_set_object()` when called.
#[derive(Clone)]
enum Handler {
    None,
    Callable(Value),
    Method(PhpStr),
}

/// The native data of an `XMLParser`.
struct ParserData {
    /// Taken out while a parse runs (re-entrant calls see `None`).
    sax: Option<Box<SaxParser>>,
    handlers: [Handler; NH],
    object: Option<Value>,
    /// Position while a parse runs (for `xml_get_current_*` in handlers).
    pos: Position,
    parsing: bool,
    /// Error code while parsing (the parser is out).
    error_code: u32,
    /// Options while parsing (read by `xml_parser_get_option`).
    opts: sax::SaxOptions,
    is_ns: bool,
}

impl goro_runtime::NativeData for ParserData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        for h in &self.handlers {
            if let Handler::Callable(v) = h {
                visit(v);
            }
        }
        if let Some(o) = &self.object {
            visit(o);
        }
    }
    fn clear(&mut self) {
        for h in self.handlers.iter_mut() {
            *h = Handler::None;
        }
        self.object = None;
    }
}

fn parser_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags.set(
                cls.flags.get()
                    | cflags::NO_DYNAMIC
                    | cflags::NOT_SERIALIZABLE
                    | cflags::NOT_CLONEABLE,
            );
        }),
        ..Default::default()
    }
}

fn with<T>(o: &ObjRef, f: impl FnOnce(&mut ParserData) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<ParserData>().map(f)
}

fn create(vm: &mut Vm, a: &Args, sep: Option<&str>) -> R<Value> {
    let target = match a.opt_str(0) {
        None => None,
        Some(e) => {
            let name = e.to_string_lossy();
            match TargetEncoding::from_name(&name) {
                Some(t) => Some(t),
                None => {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        "Argument #1 ($encoding) is not a supported source encoding",
                    );
                }
            }
        }
    };
    let cls = vm
        .find_class(b"XMLParser", false)?
        .expect("XMLParser is declared");
    let o = vm.instantiate_raw(&cls)?;
    let sax = SaxParser::new(sep, target);
    let opts = sax.options.clone();
    *o.obj().native.borrow_mut() = Some(Box::new(ParserData {
        sax: Some(Box::new(sax)),
        handlers: std::array::from_fn(|_| Handler::None),
        object: None,
        pos: Position {
            byte_index: 0,
            line: 1,
            column: 1,
        },
        parsing: false,
        error_code: 0,
        opts,
        is_ns: sep.is_some(),
    }));
    Ok(Value::Object(o))
}

fn xml_parser_create(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create(vm, a, None)
}

fn xml_parser_create_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let sep = if a.has(1) {
        a.str(1).to_string_lossy()
    } else {
        ":".to_string()
    };
    create(vm, a, Some(&sep))
}

fn parser_of(a: &Args) -> ObjRef {
    a.obj(0).expect("XMLParser argument")
}

fn xml_set_object(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated("Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions")?;
    let p = parser_of(a);
    let obj = a.val(1);
    let Value::Object(no) = &obj else {
        return Ok(Value::Bool(true));
    };
    // Every method-name handler must exist on the new object.
    let names = [
        "xml_set_element_handler",
        "xml_set_element_handler",
        "xml_set_character_data_handler",
        "xml_set_processing_instruction_handler",
        "xml_set_default_handler",
        "xml_set_unparsed_entity_decl_handler",
        "xml_set_notation_decl_handler",
        "xml_set_external_entity_ref_handler",
        "xml_set_start_namespace_decl_handler",
        "xml_set_end_namespace_decl_handler",
    ];
    let hs =
        with(&p, |d| d.handlers.clone()).unwrap_or_else(|| std::array::from_fn(|_| Handler::None));
    let cls = vm.class_of(no);
    for (i, h) in hs.iter().enumerate() {
        if let Handler::Method(m) = h
            && cls
                .find_method(&m.as_bytes().to_ascii_lowercase())
                .is_none()
            && cls.magic.borrow().call.is_none()
        {
            return vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #2 ($object) cannot safely swap to object of class {} as method \"{}\" does not exist, which was set via {}()",
                    cls.name.dname(),
                    m.to_string_lossy(),
                    names[i]
                ),
            );
        }
    }
    with(&p, |d| d.object = Some(obj.clone()));
    Ok(Value::Bool(true))
}

/// Validates handler argument `i` (1-based position `argn` in messages).
fn handler_arg(
    vm: &mut Vm,
    a: &Args,
    i: usize,
    argn: usize,
    pname: &str,
    deprecated: &mut bool,
) -> R<Handler> {
    let v = a.val(i);
    let bad = |vm: &mut Vm| -> R<Handler> {
        vm.fthrow(
            a,
            "TypeError",
            &format!("Argument #{argn} (${pname}) must be of type callable|string|null"),
        )
    };
    match &v {
        Value::Null => Ok(Handler::None),
        Value::Str(s) => {
            if !s.is_empty() && vm.is_callable_value(&v, false) {
                return Ok(Handler::Callable(v.clone()));
            }
            if !*deprecated {
                *deprecated = true;
                vm.fdeprecated(a, "Passing non-callable strings is deprecated since 8.4")?;
            }
            if s.is_empty() {
                return Ok(Handler::None);
            }
            let p = parser_of(a);
            let obj = with(&p, |d| d.object.clone()).flatten();
            match obj {
                Some(Value::Object(o)) => {
                    let cls = vm.class_of(&o);
                    if cls.find_method(&s.as_bytes().to_ascii_lowercase()).is_none()
                        && cls.magic.borrow().call.is_none()
                    {
                        return vm.fthrow(
                            a,
                            "ValueError",
                            &format!(
                                "Argument #{argn} (${pname}) method {}::{}() does not exist",
                                cls.name.dname(),
                                s.to_string_lossy()
                            ),
                        );
                    }
                    Ok(Handler::Method(s.clone()))
                }
                _ => vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #{argn} (${pname}) an object must be set via xml_set_object() to be able to lookup method"
                    ),
                ),
            }
        }
        Value::Array(_) | Value::Object(_) => {
            if vm.is_callable_value(&v, false) {
                Ok(Handler::Callable(v.clone()))
            } else {
                bad(vm)
            }
        }
        _ => bad(vm),
    }
}

/// `[$obj, 'name']` reaching `__call` (no such method).
fn is_trampoline(vm: &mut Vm, v: &Value) -> bool {
    let Value::Array(arr) = v else { return false };
    let (Some(Value::Object(o)), Some(Value::Str(m))) = (arr.get_int(0), arr.get_int(1)) else {
        return false;
    };
    let cls = vm.class_of(o);
    cls.find_method(&m.as_bytes().to_ascii_lowercase())
        .is_none()
        && cls.magic.borrow().call.is_some()
}

fn set_one(vm: &mut Vm, a: &Args, slot: usize) -> R<Value> {
    let h = handler_arg(vm, a, 1, 2, "handler", &mut false)?;
    let p = parser_of(a);
    with(&p, |d| d.handlers[slot] = h);
    Ok(Value::Bool(true))
}

fn xml_set_element_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut dep = false;
    let s = handler_arg(vm, a, 1, 2, "start_handler", &mut dep)?;
    // After a trampoline start handler (a method reached through
    // __call), a bad third argument reports #2 (observed on PHP 8.5; test
    // set_element_handler_trampoline_errors).
    let tramp = is_trampoline(vm, &a.val(1));
    let e = if tramp {
        handler_arg(vm, a, 2, 2, "start_handler", &mut dep)?
    } else {
        handler_arg(vm, a, 2, 3, "end_handler", &mut dep)?
    };
    let p = parser_of(a);
    with(&p, |d| {
        d.handlers[H_START] = s;
        d.handlers[H_END] = e;
    });
    Ok(Value::Bool(true))
}

fn xml_set_character_data_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_CDATA)
}
fn xml_set_processing_instruction_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_PI)
}
fn xml_set_default_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_DEFAULT)
}
fn xml_set_unparsed_entity_decl_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_UNPARSED)
}
fn xml_set_notation_decl_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_NOTATION)
}
fn xml_set_external_entity_ref_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_EXTERNAL)
}
fn xml_set_start_namespace_decl_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_NS_START)
}
fn xml_set_end_namespace_decl_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    set_one(vm, a, H_NS_END)
}

// ------------------------------------------------------------------ parsing

/// Routes goro-xml's events to the PHP handlers (and to the
/// `xml_parse_into_struct()` collector).
struct Bridge {
    vm: *mut Vm,
    parser: ObjRef,
    pending: Option<Unwind>,
    collect: Option<Collect>,
}

fn bytes(b: &[u8]) -> Value {
    Value::from(b)
}

impl Bridge {
    fn vm(&mut self) -> &mut Vm {
        // SAFETY: the VM outlives the parse and is not otherwise used
        // while the parser runs (handlers are called through it here).
        unsafe { &mut *self.vm }
    }

    fn set_pos(&mut self, pos: &Position) {
        let p = *pos;
        with(&self.parser, |d| d.pos = p);
    }

    /// Calls handler `slot` with `args` (after the parser). Returns its
    /// result, or None when not set or after an exception.
    fn call(&mut self, slot: usize, args: Vec<Value>) -> Option<Value> {
        if self.pending.is_some() {
            return None;
        }
        let (h, obj) = with(&self.parser, |d| {
            (d.handlers[slot].clone(), d.object.clone())
        })?;
        let callable = match h {
            Handler::None => return None,
            Handler::Callable(v) => v,
            Handler::Method(m) => match obj {
                Some(o) => {
                    let mut arr = PhpArray::new();
                    let _ = arr.push(o);
                    let _ = arr.push(Value::Str(m));
                    Value::Array(arr)
                }
                None => return None,
            },
        };
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(Value::Object(self.parser.clone()));
        full.extend(args);
        let r = self.vm().call_value(&callable, full);
        match r {
            Ok(v) => Some(v),
            Err(u) => {
                self.pending = Some(u);
                None
            }
        }
    }
}

impl SaxHandler for Bridge {
    fn start_element(&mut self, name: &[u8], attrs: &[(Vec<u8>, Vec<u8>)], pos: &Position) {
        // The user's handler runs before the struct collector (observed on
        // PHP 8.5; test gh17187_1).
        self.set_pos(pos);
        let mut arr = PhpArray::new();
        for (k, v) in attrs {
            arr.insert(Key::from_bytes(k), bytes(v));
        }
        self.call(H_START, vec![bytes(name), Value::Array(arr)]);
        if let Some(c) = &mut self.collect {
            c.start(name, attrs);
        }
    }
    fn end_element(&mut self, name: &[u8], pos: &Position) {
        self.set_pos(pos);
        self.call(H_END, vec![bytes(name)]);
        if let Some(c) = &mut self.collect {
            c.end(name);
        }
    }
    fn character_data(&mut self, data: &[u8], pos: &Position) {
        self.set_pos(pos);
        self.call(H_CDATA, vec![bytes(data)]);
        if let Some(c) = &mut self.collect {
            c.cdata(data);
        }
    }
    fn processing_instruction(&mut self, target: &[u8], data: &[u8], pos: &Position) {
        self.set_pos(pos);
        self.call(H_PI, vec![bytes(target), bytes(data)]);
    }
    fn default(&mut self, data: &[u8], pos: &Position) {
        self.set_pos(pos);
        self.call(H_DEFAULT, vec![bytes(data)]);
    }
    fn unparsed_entity_decl(
        &mut self,
        name: &[u8],
        system: &[u8],
        public: Option<&[u8]>,
        notation: &[u8],
    ) {
        self.call(
            H_UNPARSED,
            vec![
                bytes(name),
                Value::Bool(false),
                bytes(system),
                public.map_or(Value::Bool(false), bytes),
                bytes(notation),
            ],
        );
    }
    fn notation_decl(&mut self, name: &[u8], system: Option<&[u8]>, public: Option<&[u8]>) {
        self.call(
            H_NOTATION,
            vec![
                bytes(name),
                Value::Bool(false),
                system.map_or(Value::Bool(false), bytes),
                public.map_or(Value::Bool(false), bytes),
            ],
        );
    }
    fn external_entity_ref(
        &mut self,
        names: &[u8],
        system: &[u8],
        public: Option<&[u8]>,
        pos: &Position,
    ) -> bool {
        self.set_pos(pos);
        match self.call(
            H_EXTERNAL,
            vec![
                bytes(names),
                Value::from(&b""[..]),
                bytes(system),
                public.map_or(Value::Bool(false), bytes),
            ],
        ) {
            Some(v) => goro_runtime::convert::to_bool_quiet(&v),
            None => true,
        }
    }
    fn start_namespace_decl(&mut self, prefix: Option<&[u8]>, uri: &[u8]) {
        self.call(
            H_NS_START,
            vec![prefix.map_or(Value::Bool(false), bytes), bytes(uri)],
        );
    }
    fn end_namespace_decl(&mut self, prefix: Option<&[u8]>) {
        self.call(H_NS_END, vec![prefix.map_or(Value::Bool(false), bytes)]);
    }
}

fn handlers_set(p: &ObjRef) -> Handlers {
    with(p, |d| {
        let s = |i: usize| !matches!(d.handlers[i], Handler::None);
        Handlers {
            element: s(H_START) || s(H_END),
            cdata: s(H_CDATA),
            pi: s(H_PI),
            default: s(H_DEFAULT),
            external_entity: s(H_EXTERNAL),
            unparsed_entity: s(H_UNPARSED),
            notation: s(H_NOTATION),
            namespace: s(H_NS_START) || s(H_NS_END),
        }
    })
    .unwrap_or_default()
}

/// Runs a parse with the parser taken out of the object.
fn run(
    vm: &mut Vm,
    p: &ObjRef,
    data: &[u8],
    is_final: bool,
    collect: Option<Collect>,
) -> R<Option<(bool, Option<Collect>)>> {
    let sax = with(p, |d| {
        let s = d.sax.take();
        if let Some(s) = &s {
            d.parsing = true;
            d.opts = s.options.clone();
            d.error_code = s.error_code;
            d.pos = s.position;
        }
        s
    })
    .flatten();
    let Some(mut sax) = sax else {
        return vm.throw("Error", "Parser must not be called recursively");
    };
    let mut handlers = handlers_set(p);
    if collect.is_some() {
        handlers.element = true;
        handlers.cdata = true;
    }
    let mut b = Bridge {
        vm: vm as *mut Vm,
        parser: p.clone(),
        pending: None,
        collect,
    };
    let ok = sax.parse(data, is_final, handlers, &mut b);
    let pending = b.pending.take();
    let collected = b.collect.take();
    drop(b);
    with(p, |d| {
        d.parsing = false;
        d.sax = Some(sax);
    });
    if let Some(u) = pending {
        return Err(u);
    }
    Ok(Some((ok, collected)))
}

fn xml_parse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = parser_of(a);
    let data = a.str(1);
    let fin = a.bool(2);
    match run(vm, &p, data.as_bytes(), fin, None)? {
        Some((ok, _)) => Ok(Value::Int(ok as i64)),
        None => Ok(Value::Int(0)),
    }
}

fn xml_parse_into_struct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = parser_of(a);
    let data = a.str(1);
    let skip_white = with(&p, |d| d.sax.as_ref().map(|s| s.options.skip_white))
        .flatten()
        .unwrap_or(false);
    if with(&p, |d| d.sax.is_none()).unwrap_or(false) {
        vm.fwarn(a, "Parser must not be called recursively")?;
        return Ok(Value::Bool(false));
    }
    // The arrays are reset, then filled as the parse goes (handlers see
    // and may replace them; observed on PHP 8.5, tests gh17187_1/2).
    write_ref(vm, a, 2, Value::Array(PhpArray::new()))?;
    if a.has(3) {
        write_ref(vm, a, 3, Value::Array(PhpArray::new()))?;
    }
    let Some(values) = a.ref_at(2) else {
        return Ok(Value::Bool(false));
    };
    let c = Collect {
        values,
        index: a.ref_at(3),
        level: 0,
        last_open: false,
        last: None,
        open: Vec::new(),
        skip_white,
        truncated: false,
    };
    let r = run(vm, &p, data.as_bytes(), true, Some(c))?;
    let Some((ok, Some(c))) = r else {
        return Ok(Value::Int(0));
    };
    if c.truncated {
        vm.fwarn(a, "Maximum depth exceeded - Results truncated")?;
    }
    Ok(Value::Int(ok as i64))
}

/// `xml_parse_into_struct()`'s collector, writing into the referenced
/// arrays (key order: `tag, type, level, attributes, value`; `tag, value,
/// type, level` for cdata entries).
struct Collect {
    values: goro_runtime::PhpRef,
    index: Option<goro_runtime::PhpRef>,
    level: u32,
    /// The last entry is an open tag without content yet.
    last_open: bool,
    /// Key of the last entry pushed.
    last: Option<i64>,
    /// Tags of the open elements.
    open: Vec<Vec<u8>>,
    skip_white: bool,
    truncated: bool,
}

impl Collect {
    fn push(&mut self, tag: &[u8], entry: PhpArray) {
        // The index entry is written first (observed on PHP 8.5: with both
        // arguments bound to one array, the tag key precedes the entry;
        // test gh17187_1).
        let k = match &*self.values.borrow() {
            Value::Array(arr) => arr.data().next_index(),
            _ => None,
        };
        self.last = k;
        let Some(k) = k else { return };
        if let Some(ix) = &self.index {
            let mut v = ix.borrow_mut();
            if let Value::Array(arr) = &mut *v {
                let e = arr.entry(Key::from_bytes(tag));
                if e.is_null() {
                    *e = Value::Array(PhpArray::new());
                }
                if let Value::Array(l) = e {
                    let _ = l.push(Value::Int(k));
                }
            }
        }
        let mut v = self.values.borrow_mut();
        if let Value::Array(arr) = &mut *v {
            arr.insert(Key::Int(k), Value::Array(entry));
        }
    }

    /// Runs `f` on the last entry.
    fn with_last(&mut self, f: impl FnOnce(&mut PhpArray)) {
        let Some(k) = self.last else { return };
        let mut v = self.values.borrow_mut();
        if let Value::Array(arr) = &mut *v
            && let Some(Value::Array(e)) = arr.get_mut(goro_runtime::KeyRef::Int(k))
        {
            f(e);
        }
    }

    fn last_is_cdata_at(&self, level: u32) -> bool {
        let Some(k) = self.last else { return false };
        let v = self.values.borrow();
        let Value::Array(arr) = &*v else { return false };
        let Some(Value::Array(e)) = arr.get_int(k) else {
            return false;
        };
        matches!(e.get_bytes(b"type"), Some(Value::Str(t)) if t.as_bytes() == b"cdata")
            && matches!(e.get_bytes(b"level"), Some(Value::Int(l)) if *l == level as i64)
    }

    fn entry(tag: &[u8], kind: &[u8], level: u32) -> PhpArray {
        let mut m = PhpArray::new();
        m.insert(Key::from_bytes(b"tag"), bytes(tag));
        m.insert(Key::from_bytes(b"type"), bytes(kind));
        m.insert(Key::from_bytes(b"level"), Value::Int(level as i64));
        m
    }

    fn start(&mut self, name: &[u8], attrs: &[(Vec<u8>, Vec<u8>)]) {
        self.level += 1;
        self.open.push(name.to_vec());
        if self.level > sax::MAX_STRUCT_LEVEL {
            self.truncated = true;
            return;
        }
        let mut m = Self::entry(name, b"open", self.level);
        if !attrs.is_empty() {
            let mut aa = PhpArray::new();
            for (k, v) in attrs {
                aa.insert(Key::from_bytes(k), bytes(v));
            }
            m.insert(Key::from_bytes(b"attributes"), Value::Array(aa));
        }
        self.push(name, m);
        self.last_open = true;
    }

    fn end(&mut self, name: &[u8]) {
        if self.last_open {
            self.with_last(|e| {
                e.insert(Key::from_bytes(b"type"), bytes(b"complete"));
            });
        } else {
            // Levels past the limit still close (observed on PHP 8.5).
            let m = Self::entry(name, b"close", self.level);
            self.push(name, m);
        }
        self.last_open = false;
        self.open.pop();
        self.level = self.level.saturating_sub(1);
    }

    fn cdata(&mut self, data: &[u8]) {
        if self.level == 0 || self.level > sax::MAX_STRUCT_LEVEL {
            return;
        }
        let white = data
            .iter()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'));
        if self.last_open {
            let skip = self.skip_white;
            self.with_last(|e| {
                let k = Key::from_bytes(b"value");
                match e.get_mut(k.as_ref()) {
                    Some(Value::Str(s)) => s.push_bytes(data),
                    _ => {
                        if !(skip && white) {
                            e.insert(k, bytes(data));
                        }
                    }
                }
            });
            return;
        }
        if self.last_is_cdata_at(self.level) {
            self.with_last(|e| {
                if let Some(Value::Str(s)) = e.get_mut(Key::from_bytes(b"value").as_ref()) {
                    s.push_bytes(data);
                }
            });
            return;
        }
        if self.skip_white && white {
            return;
        }
        let tag = self.open.last().cloned().unwrap_or_default();
        let mut m = PhpArray::new();
        m.insert(Key::from_bytes(b"tag"), bytes(&tag));
        m.insert(Key::from_bytes(b"value"), bytes(data));
        m.insert(Key::from_bytes(b"type"), bytes(b"cdata"));
        m.insert(Key::from_bytes(b"level"), Value::Int(self.level as i64));
        self.push(&tag, m);
    }
}

fn write_ref(vm: &mut Vm, a: &Args, i: usize, v: Value) -> R<()> {
    if let Some(r) = a.ref_at(i) {
        let v = vm.typed_ref_value(&r, v)?;
        drop(r.set(v));
    }
    Ok(())
}

fn xml_get_error_code(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = parser_of(a);
    Ok(Value::Int(
        with(&p, |d| match &d.sax {
            Some(s) => s.error_code,
            None => d.error_code,
        })
        .unwrap_or(0) as i64,
    ))
}

fn xml_error_string(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = a.int(0);
    Ok(Value::from(sax::error_string(c).as_bytes()))
}

fn position(p: &ObjRef) -> Position {
    with(p, |d| match &d.sax {
        Some(s) => s.position,
        None => d.pos,
    })
    .unwrap_or(Position {
        byte_index: 0,
        line: 1,
        column: 1,
    })
}

fn xml_get_current_line_number(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(position(&parser_of(a)).line as i64))
}
fn xml_get_current_column_number(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(position(&parser_of(a)).column as i64))
}
fn xml_get_current_byte_index(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(position(&parser_of(a)).byte_index as i64))
}

fn xml_parser_free(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function xml_parser_free() is deprecated since 8.5, as it has no effect since PHP 8.0",
    )?;
    let p = parser_of(a);
    if with(&p, |d| d.parsing).unwrap_or(false) {
        return vm.fthrow(a, "Error", "Parser cannot be freed while it is parsing");
    }
    Ok(Value::Bool(true))
}

/// Option value checks shared by the bool and int options: PHP warns
/// about arrays and objects and goes on with the converted value.
fn option_value_warn(vm: &mut Vm, a: &Args, v: &Value) -> R<()> {
    if matches!(v, Value::Array(_) | Value::Object(_)) {
        vm.fwarn(
            a,
            &format!(
                "Argument #3 ($value) must be of type string|int|bool, {} given",
                crate::class::TypeName::tname(v)
            ),
        )?;
    }
    Ok(())
}

fn xml_parser_set_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = parser_of(a);
    let opt = a.int(1);
    let v = a.val(2);
    match opt {
        1 | 4 => {
            option_value_warn(vm, a, &v)?;
            let b = vm.to_bool(&v);
            with(&p, |d| {
                let o = match &mut d.sax {
                    Some(s) => &mut s.options,
                    None => &mut d.opts,
                };
                if opt == 1 {
                    o.case_folding = b;
                } else {
                    o.skip_white = b;
                }
            });
            Ok(Value::Bool(true))
        }
        3 => {
            option_value_warn(vm, a, &v)?;
            let n = match &v {
                Value::Object(_) => {
                    vm.warn(&format!(
                        "Object of class {} could not be converted to int",
                        crate::class::TypeName::tname(&v)
                    ))?;
                    1
                }
                Value::Array(arr) => !arr.is_empty() as i64,
                _ => vm.to_int(&v)?,
            };
            if !(0..=i32::MAX as i64).contains(&n) {
                vm.fwarn(
                    a,
                    "Argument #3 ($value) must be between 0 and 2147483647 for option XML_OPTION_SKIP_TAGSTART",
                )?;
                return Ok(Value::Bool(false));
            }
            with(&p, |d| {
                if let Some(s) = &mut d.sax {
                    s.options.skip_tagstart = n as usize;
                }
                d.opts.skip_tagstart = n as usize;
            });
            Ok(Value::Bool(true))
        }
        2 => {
            option_value_warn(vm, a, &v)?;
            let s = vm.to_php_str(&v)?;
            match TargetEncoding::from_name(&s.to_string_lossy()) {
                Some(t) => {
                    with(&p, |d| {
                        if let Some(s) = &mut d.sax {
                            s.options.target = t;
                        }
                        d.opts.target = t;
                    });
                    Ok(Value::Bool(true))
                }
                None => vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #3 ($value) is not a supported target encoding",
                ),
            }
        }
        5 => {
            if with(&p, |d| d.parsing).unwrap_or(false) {
                return vm.throw(
                    "Error",
                    "Cannot change option XML_OPTION_PARSE_HUGE while parsing",
                );
            }
            option_value_warn(vm, a, &v)?;
            let b = vm.to_bool(&v);
            with(&p, |d| {
                if let Some(s) = &mut d.sax {
                    s.options.parse_huge = b;
                }
            });
            Ok(Value::Bool(true))
        }
        _ => vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($option) must be a XML_OPTION_* constant",
        ),
    }
}

fn xml_parser_get_option(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let p = parser_of(a);
    let opt = a.int(1);
    let o = with(&p, |d| match &d.sax {
        Some(s) => s.options.clone(),
        None => d.opts.clone(),
    })
    .unwrap_or_default();
    let _ = with(&p, |d| d.is_ns);
    Ok(match opt {
        1 => Value::Bool(o.case_folding),
        2 => Value::from(o.target.name().as_bytes()),
        3 => Value::Int(o.skip_tagstart as i64),
        4 => Value::Bool(o.skip_white),
        5 => Value::Bool(o.parse_huge),
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($option) must be a XML_OPTION_* constant",
            );
        }
    })
}
