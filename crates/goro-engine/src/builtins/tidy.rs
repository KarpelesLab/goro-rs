//! ext/tidy (PHP manual: Tidy): the `tidy` and `tidyNode` classes, the
//! `tidy_*` functions, the output handler and the ini settings, on the
//! goro-ext-tidy cleaner. Messages and argument handling come from the
//! ext/tidy tests.

use std::cell::RefCell;
use std::os::unix::ffi::OsStrExt;
use std::rc::Rc;

use goro_ext_tidy::{self as t, Config, Document, Encoding, Kind, NodeId, NodeType, OPTIONS, Val};
use goro_runtime::{Key, KeyRef, NativeData, ObjRef, PhpArray, PhpStr, Value};

use crate::bytecode::CastTo;
use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::output::{ObBuffer, PHP_OUTPUT_HANDLER_START, PHP_OUTPUT_HANDLER_STDFLAGS};
use crate::vm::{R, Vm};

/// The date of this implementation, for `tidy_get_release()`.
const RELEASE: &str = "2026/11/02";
/// PHP manual: PHP_OUTPUT_HANDLER_FINAL.
const OUTPUT_FINAL: i64 = 8;

const CLASSES: &str = r#"<?php
class tidy {
    public ?string $errorBuffer = null;
    public ?string $value = null;
    public function __construct(?string $filename = null, array|string|null $config = null, ?string $encoding = null, bool $useIncludePath = false) {}
    public function getOpt(string $option): string|int|bool {}
    public function cleanRepair(): bool {}
    public function parseFile(string $filename, array|string|null $config = null, ?string $encoding = null, bool $useIncludePath = false): bool {}
    public function parseString(string $string, array|string|null $config = null, ?string $encoding = null): bool {}
    public static function repairString(string $string, array|string|null $config = null, ?string $encoding = null): string|false {}
    public static function repairFile(string $filename, array|string|null $config = null, ?string $encoding = null, bool $useIncludePath = false): string|false {}
    public function diagnose(): bool {}
    public function getRelease(): string {}
    public function getOptDoc(string $option): string|false {}
    public function getConfig(): array {}
    public function getStatus(): int {}
    public function getHtmlVer(): int {}
    public function isXhtml(): bool {}
    public function isXml(): bool {}
    public function root(): ?tidyNode {}
    public function head(): ?tidyNode {}
    public function html(): ?tidyNode {}
    public function body(): ?tidyNode {}
}
final class tidyNode {
    public readonly string $value;
    public readonly string $name;
    public readonly int $type;
    public readonly int $line;
    public readonly int $column;
    public readonly bool $proprietary;
    public readonly ?int $id;
    public readonly ?array $attribute;
    public readonly ?array $child;
    private function __construct() {}
    public function hasChildren(): bool {}
    public function hasSiblings(): bool {}
    public function isComment(): bool {}
    public function isHtml(): bool {}
    public function isXhtml(): bool {}
    public function isXml(): bool {}
    public function isText(): bool {}
    public function isJste(): bool {}
    public function isAsp(): bool {}
    public function isPhp(): bool {}
    public function getParent(): ?tidyNode {}
    public function getPreviousSibling(): ?tidyNode {}
    public function getNextSibling(): ?tidyNode {}
}"#;

const CONST_COUNT: usize = 15 + t::TAG_COUNT;

const fn constants() -> [(&'static str, ConstVal); CONST_COUNT] {
    const NODE_TYPES: [(&str, i64); 14] = [
        ("TIDY_NODETYPE_ROOT", NodeType::Root as i64),
        ("TIDY_NODETYPE_DOCTYPE", NodeType::Doctype as i64),
        ("TIDY_NODETYPE_COMMENT", NodeType::Comment as i64),
        ("TIDY_NODETYPE_PROCINS", NodeType::ProcIns as i64),
        ("TIDY_NODETYPE_TEXT", NodeType::Text as i64),
        ("TIDY_NODETYPE_START", NodeType::Start as i64),
        ("TIDY_NODETYPE_END", NodeType::End as i64),
        ("TIDY_NODETYPE_STARTEND", NodeType::StartEnd as i64),
        ("TIDY_NODETYPE_CDATA", NodeType::Cdata as i64),
        ("TIDY_NODETYPE_SECTION", NodeType::Section as i64),
        ("TIDY_NODETYPE_ASP", NodeType::Asp as i64),
        ("TIDY_NODETYPE_JSTE", NodeType::Jste as i64),
        ("TIDY_NODETYPE_PHP", NodeType::Php as i64),
        ("TIDY_NODETYPE_XMLDECL", NodeType::XmlDecl as i64),
    ];
    let mut out = [("TIDY_TAG_UNKNOWN", ConstVal::Int(0)); CONST_COUNT];
    let mut i = 0;
    while i < NODE_TYPES.len() {
        out[i + 1] = (NODE_TYPES[i].0, ConstVal::Int(NODE_TYPES[i].1));
        i += 1;
    }
    let mut j = 0;
    while j < t::TAG_COUNT {
        out[15 + j] = (t::TAGS[j].constant, ConstVal::Int(t::TAGS[j].id));
        j += 1;
    }
    out
}

static CONSTANTS: [(&str, ConstVal); CONST_COUNT] = constants();

pub static MODULE: Module = Module {
    name: "tidy",
    functions: &[
        (
            "function tidy_parse_string(string $string, array|string|null $config = null, ?string $encoding = null): tidy|false",
            tidy_parse_string,
        ),
        (
            "function tidy_parse_file(string $filename, array|string|null $config = null, ?string $encoding = null, bool $useIncludePath = false): tidy|false",
            tidy_parse_file,
        ),
        (
            "function tidy_repair_string(string $string, array|string|null $config = null, ?string $encoding = null): string|false",
            repair_string,
        ),
        (
            "function tidy_repair_file(string $filename, array|string|null $config = null, ?string $encoding = null, bool $useIncludePath = false): string|false",
            repair_file,
        ),
        (
            "function tidy_get_error_buffer(tidy $tidy): string|false",
            get_error_buffer,
        ),
        ("function tidy_get_output(tidy $tidy): string", get_output),
        ("function tidy_clean_repair(tidy $tidy): bool", clean_repair),
        ("function tidy_diagnose(tidy $tidy): bool", diagnose),
        ("function tidy_get_release(): string", get_release),
        (
            "function tidy_get_opt_doc(tidy $tidy, string $option): string|false",
            get_opt_doc,
        ),
        ("function tidy_get_config(tidy $tidy): array", get_config),
        ("function tidy_get_status(tidy $tidy): int", get_status),
        ("function tidy_get_html_ver(tidy $tidy): int", get_html_ver),
        ("function tidy_is_xhtml(tidy $tidy): bool", is_xhtml),
        ("function tidy_is_xml(tidy $tidy): bool", is_xml),
        ("function tidy_error_count(tidy $tidy): int", error_count),
        ("function tidy_warning_count(tidy $tidy): int", warning_count),
        ("function tidy_access_count(tidy $tidy): int", access_count),
        ("function tidy_config_count(tidy $tidy): int", config_count),
        (
            "function tidy_getopt(tidy $tidy, string $option): string|int|bool",
            get_opt,
        ),
        ("function tidy_get_root(tidy $tidy): ?tidyNode", get_root),
        ("function tidy_get_html(tidy $tidy): ?tidyNode", get_html),
        ("function tidy_get_head(tidy $tidy): ?tidyNode", get_head),
        ("function tidy_get_body(tidy $tidy): ?tidyNode", get_body),
        (
            "function ob_tidyhandler(string $input, int $mode = PHP_OUTPUT_HANDLER_START): string",
            ob_tidyhandler,
        ),
    ],
    classes: &[("tidy", CLASSES), ("tidynode", CLASSES)],
    methods: &[
        ("tidy::__construct", construct),
        ("tidy::getopt", get_opt),
        ("tidy::cleanrepair", clean_repair),
        ("tidy::parsefile", parse_file_method),
        ("tidy::parsestring", parse_string_method),
        ("tidy::repairstring", repair_string),
        ("tidy::repairfile", repair_file),
        ("tidy::diagnose", diagnose),
        ("tidy::getrelease", get_release),
        ("tidy::getoptdoc", get_opt_doc),
        ("tidy::getconfig", get_config),
        ("tidy::getstatus", get_status),
        ("tidy::gethtmlver", get_html_ver),
        ("tidy::isxhtml", is_xhtml),
        ("tidy::isxml", is_xml),
        ("tidy::root", get_root),
        ("tidy::head", get_head),
        ("tidy::html", get_html),
        ("tidy::body", get_body),
        ("tidynode::haschildren", node_has_children),
        ("tidynode::hassiblings", node_has_siblings),
        ("tidynode::iscomment", node_is_comment),
        ("tidynode::ishtml", node_is_html),
        ("tidynode::isxhtml", node_is_xhtml),
        ("tidynode::isxml", node_is_xml),
        ("tidynode::istext", node_is_text),
        ("tidynode::isjste", node_is_jste),
        ("tidynode::isasp", node_is_asp),
        ("tidynode::isphp", node_is_php),
        ("tidynode::getparent", node_get_parent),
        ("tidynode::getprevioussibling", node_prev),
        ("tidynode::getnextsibling", node_next),
    ],
    hooks: &[("tidy", tidy_hooks), ("tidynode", node_hooks)],
    constants: &CONSTANTS,
    ini: &[("tidy.default_config", ""), ("tidy.clean_output", "0")],
    info: Some(|_| format!("Tidy support => enabled\nlibTidy Release => {RELEASE}\n")),
    ..Module::EMPTY
};

/// A `tidy` object: its options, and the document once one was parsed.
struct TidyObj {
    config: Config,
    doc: Option<Rc<RefCell<Document>>>,
}

impl NativeData for TidyObj {}

/// A `tidyNode` object.
struct NodeObj {
    doc: Rc<RefCell<Document>>,
    id: NodeId,
}

impl NativeData for NodeObj {}

fn with_native<D: NativeData, T>(o: &ObjRef, f: impl FnOnce(&mut D) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<D>().map(f)
}

/// The options PHP starts a document with: no generator meta element, and
/// output even when there were errors (test 007.phpt shows `tidy-mark` off,
/// test gh20374.phpt output after an error).
fn php_config() -> Config {
    let mut c = Config::new();
    c.set_bool("tidy-mark", false);
    c.set_bool("force-output", true);
    c
}

/// How problems with the arguments are reported: `new tidy` throws where the
/// other entry points warn (test open_basedir_failure_config.phpt).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Report {
    Warn,
    Throw,
}

fn problem(vm: &mut Vm, fname: &str, how: Report, msg: &str) -> R<()> {
    let m = format!("{fname}(): {msg}");
    match how {
        Report::Warn => vm.warn(&m),
        Report::Throw => vm.throw("Exception", &m),
    }
}

/// Reads a configuration file into `cfg`. `Ok(false)` when open_basedir
/// refuses it, which ends the operation (test
/// open_basedir_failure_config.phpt); a file that can't be read only warns
/// (test 019.phpt).
fn load_config_file(
    vm: &mut Vm,
    fname: &str,
    path: &[u8],
    cfg: &mut Config,
    how: Report,
) -> R<bool> {
    let shown = String::from_utf8_lossy(path).into_owned();
    if !vm.basedir_allows(path) {
        let bd = vm
            .get_ini(b"open_basedir")
            .map(|v| v.to_string_lossy())
            .unwrap_or_default();
        problem(
            vm,
            fname,
            how,
            &format!(
                "open_basedir restriction in effect. File({shown}) is not within the allowed path(s): ({bd})"
            ),
        )?;
        return Ok(false);
    }
    let Ok(data) = std::fs::read(vm.fs_path(path)) else {
        problem(
            vm,
            fname,
            how,
            &format!("Could not load the Tidy configuration file \"{shown}\""),
        )?;
        return Ok(true);
    };
    let before = cfg.errors;
    cfg.load_text(&String::from_utf8_lossy(&data));
    if cfg.errors > before {
        // Test gh10636.phpt.
        vm.notice_msg(&format!(
            "{fname}(): There were errors while parsing the Tidy configuration file \"{shown}\""
        ))?;
    }
    Ok(true)
}

/// `tidy.default_config` for a new document.
fn load_default_config(vm: &mut Vm, cfg: &mut Config) -> R<()> {
    let Some(path) = vm.get_ini(b"tidy.default_config") else {
        return Ok(());
    };
    if path.is_empty() {
        return Ok(());
    }
    let fname = vm.active_function_name();
    load_config_file(vm, &fname, &path, cfg, Report::Warn)?;
    Ok(())
}

/// A float converted as `(int)` does: not representable gives 0 and PHP 8.5's
/// warning, without a function name (test gh20374.phpt).
fn float_option(vm: &mut Vm, f: f64) -> R<i64> {
    if f.is_nan() || f.is_infinite() || f.abs() >= 9.223372036854775807e18 {
        let shown = if f.is_nan() {
            "NAN".to_string()
        } else if f.is_infinite() {
            if f > 0.0 { "INF" } else { "-INF" }.to_string()
        } else {
            goro_runtime::float_fmt::to_php_string(f, 17)
        };
        vm.warn(&format!(
            "The float {shown} is not representable as an int, cast occurred"
        ))?;
        return Ok(0);
    }
    Ok(f as i64)
}

/// A numeric string as an integer option's value. A number outside the
/// integer range gives 0, without a warning (test gh20374.phpt).
fn numeric_string(s: &[u8]) -> Option<i64> {
    if !goro_runtime::numeric::is_numeric_str(s) {
        return None;
    }
    let text = String::from_utf8_lossy(s);
    let text = text.trim();
    if let Ok(i) = text.parse::<i64>() {
        return Some(i);
    }
    let f: f64 = text.parse().ok()?;
    Some(if f.is_finite() && f.abs() < 9.223372036854775807e18 {
        f as i64
    } else {
        0
    })
}

/// Sets one option from a PHP value (tests tidy_error1.phpt, gh20374.phpt,
/// 030.phpt).
fn set_option(vm: &mut Vm, a: &Args, cfg: &mut Config, name: &[u8], value: &Value) -> R<()> {
    let fname = a.fname();
    let shown = String::from_utf8_lossy(name).into_owned();
    let idx = std::str::from_utf8(name).ok().and_then(t::find_option);
    let Some(idx) = idx else {
        return vm.throw(
            "ValueError",
            &format!(
                "{fname}(): Argument #2 ($config) Unknown Tidy configuration option \"{shown}\""
            ),
        );
    };
    let opt = &OPTIONS[idx];
    if opt.readonly {
        return vm.throw(
            "ValueError",
            &format!(
                "{fname}(): Argument #2 ($config) Attempting to set read-only option \"{shown}\""
            ),
        );
    }
    let value = value.deref();
    let integer_kind = matches!(
        opt.kind,
        Kind::Int | Kind::AutoBool | Kind::Pick(_) | Kind::Encoding
    );
    if integer_kind {
        match &value {
            Value::Null => {
                cfg.set_raw(idx, Val::Int(0));
                return Ok(());
            }
            Value::Bool(b) => {
                cfg.set_raw(idx, Val::Int(*b as i64));
                return Ok(());
            }
            Value::Int(i) => {
                cfg.set_raw(idx, Val::Int(*i));
                return Ok(());
            }
            Value::Float(f) => {
                let i = float_option(vm, *f)?;
                cfg.set_raw(idx, Val::Int(i));
                return Ok(());
            }
            _ => {}
        }
    } else if opt.kind == Kind::Bool {
        let b = match &value {
            Value::Null => Some(false),
            Value::Bool(b) => Some(*b),
            Value::Int(i) => Some(*i != 0),
            Value::Float(f) => Some(*f != 0.0),
            _ => None,
        };
        if let Some(b) = b {
            cfg.set_raw(idx, Val::Bool(b));
            return Ok(());
        }
    }
    let text = vm.to_php_str(&value)?;
    if integer_kind && let Some(i) = numeric_string(&text) {
        cfg.set_raw(idx, Val::Int(i));
        return Ok(());
    }
    let s = text.to_string_lossy();
    if !cfg.set_parsed(idx, &s) {
        return vm.throw(
            "TypeError",
            &format!(
                "{fname}(): Argument #2 ($config) option \"{shown}\" does not accept \"{s}\" as a value"
            ),
        );
    }
    Ok(())
}

/// Applies the `$config` argument. `Ok(false)` ends the operation.
fn apply_config(vm: &mut Vm, a: &Args, idx: usize, cfg: &mut Config, how: Report) -> R<bool> {
    let fname = a.fname();
    match a.get(idx).map(|v| v.deref()) {
        Some(Value::Array(arr)) => {
            for (k, v) in arr.iter() {
                let KeyRef::Str(name) = k else {
                    return vm.throw(
                        "TypeError",
                        &format!(
                            "{fname}(): Argument #2 ($config) must be of type array with keys as string"
                        ),
                    );
                };
                set_option(vm, a, cfg, name, v)?;
            }
            Ok(true)
        }
        Some(Value::Str(path)) => load_config_file(vm, &fname, &path, cfg, how),
        _ => Ok(true),
    }
}

/// Applies the `$encoding` argument; an empty one means none (test 019.phpt).
fn apply_encoding(vm: &mut Vm, a: &Args, idx: usize, cfg: &mut Config, how: Report) -> R<()> {
    let Some(enc) = a.opt_str(idx) else {
        return Ok(());
    };
    if enc.is_empty() {
        return Ok(());
    }
    let s = enc.to_string_lossy();
    match Encoding::from_name(&s) {
        Some(e) => {
            if let Some(i) = t::find_option("char-encoding") {
                cfg.set_raw(i, Val::Int(e as i64));
            }
            Ok(())
        }
        None => problem(
            vm,
            &a.fname(),
            how,
            &format!("Could not set encoding \"{s}\""),
        ),
    }
}

/// Reads a file to parse: `Ok(None)` if it can't be read. Documents are
/// limited to 4 GiB (test parsing_file_too_large.phpt), which is checked on
/// the size before anything is read.
fn read_input(vm: &mut Vm, a: &Args, file: &[u8], use_include_path: bool) -> R<FileRead> {
    if file.is_empty() {
        return vm.throw("ValueError", "Path must not be empty");
    }
    let resolved = if use_include_path {
        vm.resolve_include(file)
            .map(|p| p.as_os_str().as_bytes().to_vec())
    } else {
        None
    };
    let path = resolved.as_deref().unwrap_or(file);
    if let Ok(m) = std::fs::metadata(vm.fs_path(path))
        && m.is_file()
        && m.len() > u32::MAX as u64
    {
        return Ok(FileRead::TooLarge);
    }
    let Some(mut s) = super::file::open_stream(vm, a, path, b"rb", true)? else {
        return Ok(FileRead::Missing);
    };
    let data = super::file::read_rest(vm, &mut s);
    if data.len() as u64 > u32::MAX as u64 {
        return Ok(FileRead::TooLarge);
    }
    Ok(FileRead::Data(data))
}

enum FileRead {
    Data(Vec<u8>),
    Missing,
    TooLarge,
}

/// Parses into the object and refreshes its properties.
fn parse_into(vm: &mut Vm, o: &ObjRef, cfg: Config, data: &[u8]) {
    let mut doc = Document::new(cfg.clone());
    doc.parse(data);
    let doc = Rc::new(RefCell::new(doc));
    with_native::<TidyObj, _>(o, |x| {
        x.config = cfg;
        x.doc = Some(doc.clone());
    });
    update_props(vm, o, &doc);
}

/// `$errorBuffer` and `$value` follow the document (tests 008.phpt,
/// 023.phpt).
fn update_props(vm: &mut Vm, o: &ObjRef, doc: &Rc<RefCell<Document>>) {
    let d = doc.borrow();
    let buf = d.error_buffer();
    let e = if buf.is_empty() {
        Value::Null
    } else {
        Value::Str(PhpStr::from(buf))
    };
    vm.set_prop_raw(o, b"errorBuffer", e);
    vm.set_prop_raw(o, b"value", Value::Str(PhpStr::from(d.output())));
}

fn tidy_class(vm: &mut Vm) -> R<Rc<crate::class::Class>> {
    match vm.find_class(b"tidy", false)? {
        Some(c) => Ok(c),
        None => vm.throw("Error", "Class \"tidy\" not found"),
    }
}

/// The string length limit shared with files (`tidy_parse_string()`).
fn check_length(vm: &mut Vm, a: &Args, len: usize) -> R<()> {
    if len as u64 > u32::MAX as u64 {
        return vm.fthrow(a, "ValueError", "Argument #1 ($string) is too long");
    }
    Ok(())
}

fn tidy_parse_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let input = a.str(0);
    check_length(vm, a, input.len())?;
    let cls = tidy_class(vm)?;
    let o = vm.instantiate_raw(&cls)?;
    let mut cfg = object_config(&o);
    if !apply_config(vm, a, 1, &mut cfg, Report::Warn)? {
        return Ok(Value::Bool(false));
    }
    apply_encoding(vm, a, 2, &mut cfg, Report::Warn)?;
    parse_into(vm, &o, cfg, &input);
    Ok(Value::Object(o))
}

fn object_config(o: &ObjRef) -> Config {
    with_native::<TidyObj, _>(o, |x| x.config.clone()).unwrap_or_else(php_config)
}

fn tidy_parse_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let file = a.str(0);
    let data = match read_input(vm, a, &file, a.bool_or(3, false))? {
        FileRead::Data(d) => d,
        FileRead::Missing => {
            vm.fwarn(
                a,
                &format!(
                    "Cannot load \"{}\" into memory",
                    String::from_utf8_lossy(&file)
                ),
            )?;
            return Ok(Value::Bool(false));
        }
        FileRead::TooLarge => return vm.throw("ValueError", "File content is too long"),
    };
    let cls = tidy_class(vm)?;
    let o = vm.instantiate_raw(&cls)?;
    let mut cfg = object_config(&o);
    if !apply_config(vm, a, 1, &mut cfg, Report::Warn)? {
        return Ok(Value::Bool(false));
    }
    apply_encoding(vm, a, 2, &mut cfg, Report::Warn)?;
    parse_into(vm, &o, cfg, &data);
    Ok(Value::Object(o))
}

fn parse_string_method(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let input = a.str(0);
    check_length(vm, a, input.len())?;
    let mut cfg = object_config(&o);
    if !apply_config(vm, a, 1, &mut cfg, Report::Warn)? {
        return Ok(Value::Bool(false));
    }
    apply_encoding(vm, a, 2, &mut cfg, Report::Warn)?;
    parse_into(vm, &o, cfg, &input);
    Ok(Value::Bool(true))
}

fn parse_file_method(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let file = a.str(0);
    let data = match read_input(vm, a, &file, a.bool_or(3, false))? {
        FileRead::Data(d) => d,
        FileRead::Missing => {
            vm.fwarn(
                a,
                &format!(
                    "Cannot load \"{}\" into memory",
                    String::from_utf8_lossy(&file)
                ),
            )?;
            return Ok(Value::Bool(false));
        }
        FileRead::TooLarge => return vm.throw("ValueError", "File content is too long"),
    };
    let mut cfg = object_config(&o);
    if !apply_config(vm, a, 1, &mut cfg, Report::Warn)? {
        return Ok(Value::Bool(false));
    }
    apply_encoding(vm, a, 2, &mut cfg, Report::Warn)?;
    parse_into(vm, &o, cfg, &data);
    Ok(Value::Bool(true))
}

fn construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    // Without a file nothing is applied (PHP manual: tidy::__construct).
    let Some(file) = a.opt_str(0) else {
        return Ok(Value::Null);
    };
    let data = match read_input(vm, a, &file, a.bool_or(3, false))? {
        FileRead::Data(d) => d,
        FileRead::Missing => {
            // Test parsing_inexistent_file.phpt: no function name here.
            return vm.throw(
                "Exception",
                &format!(
                    "Cannot load \"{}\" into memory",
                    String::from_utf8_lossy(&file)
                ),
            );
        }
        FileRead::TooLarge => return vm.throw("ValueError", "File content is too long"),
    };
    let mut cfg = object_config(&o);
    if !apply_config(vm, a, 1, &mut cfg, Report::Throw)? {
        return Ok(Value::Null);
    }
    apply_encoding(vm, a, 2, &mut cfg, Report::Throw)?;
    parse_into(vm, &o, cfg, &data);
    Ok(Value::Null)
}

/// Parses, cleans and prints without keeping a document.
fn quick_repair(vm: &mut Vm, a: &Args, data: &[u8]) -> R<Value> {
    let mut cfg = php_config();
    load_default_config(vm, &mut cfg)?;
    if !apply_config(vm, a, 1, &mut cfg, Report::Warn)? {
        return Ok(Value::Bool(false));
    }
    apply_encoding(vm, a, 2, &mut cfg, Report::Warn)?;
    let mut doc = Document::new(cfg);
    doc.parse(data);
    doc.clean_repair();
    Ok(Value::Str(PhpStr::from(doc.output())))
}

fn repair_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let input = a.str(0);
    check_length(vm, a, input.len())?;
    quick_repair(vm, a, &input)
}

fn repair_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let file = a.str(0);
    // Test 019.phpt: a file that can't be read is silent here.
    let data = match read_input(vm, a, &file, a.bool_or(3, false))? {
        FileRead::Data(d) => d,
        FileRead::Missing => return Ok(Value::Bool(false)),
        FileRead::TooLarge => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #1 ($filename) File content is too long",
            );
        }
    };
    quick_repair(vm, a, &data)
}

/// The `tidy` object of a method or of a function's first argument.
fn tidy_arg(a: &Args) -> Option<ObjRef> {
    a.this.clone().or_else(|| a.obj(0))
}

/// The argument position of `$option`, which a method's `$this` shifts.
fn option_arg(a: &Args) -> usize {
    if a.this.is_some() { 0 } else { 1 }
}

fn doc_of(o: &ObjRef) -> Option<Rc<RefCell<Document>>> {
    with_native::<TidyObj, _>(o, |x| x.doc.clone()).flatten()
}

fn this_doc(a: &Args) -> Option<Rc<RefCell<Document>>> {
    tidy_arg(a).and_then(|o| doc_of(&o))
}

/// The document, or the error of test uninitialized.phpt.
fn initialized(vm: &mut Vm, a: &Args) -> R<Rc<RefCell<Document>>> {
    match this_doc(a) {
        Some(d) => Ok(d),
        None => vm.throw("Error", "tidy object is not initialized"),
    }
}

fn get_error_buffer(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match this_doc(a) {
        Some(d) if !d.borrow().error_buffer().is_empty() => {
            Value::Str(PhpStr::from(d.borrow().error_buffer()))
        }
        _ => Value::Bool(false),
    })
}

fn get_output(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(match this_doc(a) {
        Some(d) => Value::Str(PhpStr::from(d.borrow().output())),
        None => Value::Str(PhpStr::from("")),
    })
}

fn clean_repair(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (Some(o), Some(d)) = (tidy_arg(a), this_doc(a)) else {
        return Ok(Value::Bool(false));
    };
    d.borrow_mut().clean_repair();
    update_props(vm, &o, &d);
    Ok(Value::Bool(true))
}

fn diagnose(vm: &mut Vm, a: &mut Args) -> R<Value> {
    // Test bug54682.phpt: nothing happens without a document.
    let (Some(o), Some(d)) = (tidy_arg(a), this_doc(a)) else {
        return Ok(Value::Bool(false));
    };
    d.borrow_mut().run_diagnostics();
    update_props(vm, &o, &d);
    Ok(Value::Bool(true))
}

fn get_release(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Str(PhpStr::from(RELEASE)))
}

/// The option an `$option` argument names (tests 007.phpt, 021.phpt).
fn option_index(vm: &mut Vm, a: &Args) -> R<usize> {
    let i = option_arg(a);
    let name = a.str(i);
    let found = std::str::from_utf8(&name).ok().and_then(t::find_option);
    match found {
        Some(idx) => Ok(idx),
        None => vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($option) is an invalid configuration option, \"{}\" given",
                i + 1,
                String::from_utf8_lossy(&name)
            ),
        ),
    }
}

fn get_opt_doc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let idx = option_index(vm, a)?;
    Ok(Value::Str(PhpStr::from(OPTIONS[idx].doc)))
}

fn option_value(c: &Config, i: usize) -> Value {
    match c.value(i) {
        Val::Bool(b) => Value::Bool(*b),
        Val::Int(n) => Value::Int(*n),
        Val::Str(s) => Value::Str(PhpStr::from(s.as_str())),
    }
}

fn config_of(a: &Args) -> Config {
    tidy_arg(a).map_or_else(php_config, |o| object_config(&o))
}

fn get_opt(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let idx = option_index(vm, a)?;
    Ok(option_value(&config_of(a), idx))
}

fn get_config(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = config_of(a);
    let mut out = PhpArray::new();
    for (i, o) in OPTIONS.iter().enumerate() {
        out.insert(Key::from_bytes(o.name.as_bytes()), option_value(&c, i));
    }
    Ok(Value::Array(out))
}

fn get_status(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Int(this_doc(a).map_or(0, |d| d.borrow().status())))
}

fn get_html_ver(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = initialized(vm, a)?;
    let v = d.borrow().html_version();
    Ok(Value::Int(v))
}

fn is_xhtml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = initialized(vm, a)?;
    let v = d.borrow().is_xhtml();
    Ok(Value::Bool(v))
}

fn is_xml(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let d = initialized(vm, a)?;
    let v = d.borrow().is_xml();
    Ok(Value::Bool(v))
}

fn count_of(a: &Args, f: impl FnOnce(&Document) -> u32) -> Value {
    Value::Int(this_doc(a).map_or(0, |d| f(&d.borrow()) as i64))
}

fn error_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(count_of(a, Document::error_count))
}

fn warning_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(count_of(a, Document::warning_count))
}

fn access_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(count_of(a, Document::access_count))
}

fn config_count(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(count_of(a, Document::config_count))
}

// ----- tidyNode ------------------------------------------------------------

/// A `tidyNode` for a node and, in order, for everything under it: the
/// object numbers in test 010.phpt show the subtree built at once, parents
/// first.
fn node_object(vm: &mut Vm, doc: &Rc<RefCell<Document>>, id: NodeId) -> R<Value> {
    let Some(cls) = vm.find_class(b"tidynode", false)? else {
        return Ok(Value::Null);
    };
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(NodeObj {
        doc: doc.clone(),
        id,
    }));
    let (value, name, kind, line, col, proprietary, tag_id, attrs, children) = {
        let d = doc.borrow();
        let n = &d.nodes[id];
        let attrs = if n.attrs.is_empty() {
            Value::Null
        } else {
            let mut arr = PhpArray::new();
            for at in &n.attrs {
                // Test gh12980.phpt: no value reads as an empty one.
                arr.insert(
                    Key::from_bytes(at.name.as_bytes()),
                    Value::Str(PhpStr::from(at.value.as_deref().unwrap_or(""))),
                );
            }
            Value::Array(arr)
        };
        let tag_id = if n.name.is_empty() {
            Value::Null
        } else {
            Value::Int(n.tag.map_or(0, |g| g.id))
        };
        (
            d.node_text(id),
            n.name.clone(),
            n.kind as i64,
            n.line as i64,
            n.col as i64,
            d.is_proprietary(id),
            tag_id,
            attrs,
            n.children.clone(),
        )
    };
    vm.set_prop_raw(&o, b"value", Value::Str(PhpStr::from(value)));
    vm.set_prop_raw(&o, b"name", Value::Str(PhpStr::from(name)));
    vm.set_prop_raw(&o, b"type", Value::Int(kind));
    vm.set_prop_raw(&o, b"line", Value::Int(line));
    vm.set_prop_raw(&o, b"column", Value::Int(col));
    vm.set_prop_raw(&o, b"proprietary", Value::Bool(proprietary));
    vm.set_prop_raw(&o, b"id", tag_id);
    vm.set_prop_raw(&o, b"attribute", attrs);
    let child = if children.is_empty() {
        Value::Null
    } else {
        let mut arr = PhpArray::new();
        for c in children {
            let v = node_object(vm, doc, c)?;
            let _ = arr.push(v);
        }
        Value::Array(arr)
    };
    vm.set_prop_raw(&o, b"child", child);
    Ok(Value::Object(o))
}

fn node_or_null(
    vm: &mut Vm,
    a: &Args,
    pick: impl FnOnce(&Document) -> Option<NodeId>,
) -> R<Value> {
    let Some(doc) = this_doc(a) else {
        return Ok(Value::Null);
    };
    let id = pick(&doc.borrow());
    match id {
        Some(i) => node_object(vm, &doc, i),
        None => Ok(Value::Null),
    }
}

fn get_root(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_or_null(vm, a, |d| Some(d.root))
}

fn get_html(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_or_null(vm, a, Document::html)
}

fn get_head(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_or_null(vm, a, Document::head)
}

fn get_body(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_or_null(vm, a, Document::body)
}

fn this_node<T>(a: &Args, f: impl FnOnce(&Document, NodeId) -> T) -> Option<T> {
    let o = a.this.as_ref()?;
    with_native::<NodeObj, _>(o, |n| f(&n.doc.borrow(), n.id))
}

fn node_bool(a: &Args, f: impl FnOnce(&Document, NodeId) -> bool) -> R<Value> {
    Ok(Value::Bool(this_node(a, f).unwrap_or(false)))
}

fn node_has_children(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_bool(a, |d, i| !d.nodes[i].children.is_empty())
}

fn node_has_siblings(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_bool(a, |d, i| {
        d.prev_sibling(i).is_some() || d.next_sibling(i).is_some()
    })
}

fn node_is_kind(a: &Args, kind: NodeType) -> R<Value> {
    node_bool(a, |d, i| d.nodes[i].kind == kind)
}

fn node_is_comment(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_is_kind(a, NodeType::Comment)
}

/// Test bug77040.phpt: true for tags only, not text or CDATA.
fn node_is_html(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_bool(a, |d, i| {
        matches!(
            d.nodes[i].kind,
            NodeType::Start | NodeType::End | NodeType::StartEnd
        )
    })
}

fn node_is_xhtml(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_bool(a, |d, _| d.is_xhtml())
}

fn node_is_xml(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_bool(a, |d, _| d.is_xml())
}

fn node_is_text(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_is_kind(a, NodeType::Text)
}

fn node_is_jste(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_is_kind(a, NodeType::Jste)
}

fn node_is_asp(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_is_kind(a, NodeType::Asp)
}

fn node_is_php(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_is_kind(a, NodeType::Php)
}

fn node_related(
    vm: &mut Vm,
    a: &Args,
    pick: impl FnOnce(&Document, NodeId) -> Option<NodeId>,
) -> R<Value> {
    let Some(o) = a.this.as_ref() else {
        return Ok(Value::Null);
    };
    let Some(doc) = with_native::<NodeObj, _>(o, |n| n.doc.clone()) else {
        return Ok(Value::Null);
    };
    let id = with_native::<NodeObj, _>(o, |n| n.id).unwrap_or(0);
    let related = pick(&doc.borrow(), id);
    match related {
        Some(r) => node_object(vm, &doc, r),
        None => Ok(Value::Null),
    }
}

fn node_get_parent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_related(vm, a, |d, i| d.nodes[i].parent)
}

fn node_prev(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_related(vm, a, Document::prev_sibling)
}

fn node_next(vm: &mut Vm, a: &mut Args) -> R<Value> {
    node_related(vm, a, Document::next_sibling)
}

// ----- output handler ------------------------------------------------------

/// `ob_tidyhandler()` (PHP manual; tests 017.phpt, bug77594.phpt): tidies
/// the output when it arrives in one piece, and leaves chunked output alone,
/// since parts of a document can't be repaired independently.
fn ob_tidyhandler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let input = a.str(0);
    let mode = a.int_or(1, PHP_OUTPUT_HANDLER_START);
    let whole = mode & PHP_OUTPUT_HANDLER_START != 0 && mode & OUTPUT_FINAL != 0;
    if !whole {
        return Ok(Value::Str(input));
    }
    let mut cfg = php_config();
    load_default_config(vm, &mut cfg)?;
    let mut doc = Document::new(cfg);
    doc.parse(&input);
    doc.clean_repair();
    Ok(Value::Str(PhpStr::from(doc.output())))
}

/// `tidy.clean_output`: the handler starts with the request (test 026.phpt).
pub fn start_clean_output(vm: &mut Vm) {
    let on = vm.get_ini(b"tidy.clean_output").is_some_and(|v| {
        let s = v.to_string_lossy().to_ascii_lowercase();
        matches!(s.as_str(), "1" | "on" | "yes" | "true")
    });
    if on {
        vm.out.buffers.push(ObBuffer::new(
            Some(Value::Str(PhpStr::from("ob_tidyhandler"))),
            0,
            PHP_OUTPUT_HANDLER_STDFLAGS,
            PhpStr::from("ob_tidyhandler"),
        ));
    }
}

// ----- hooks ---------------------------------------------------------------

/// The numeric casts of test 025.phpt, and the document as a string.
fn cast_result(text: impl FnOnce() -> Vec<u8>, to: CastTo) -> Option<R<Value>> {
    Some(Ok(match to {
        CastTo::Int => Value::Int(0),
        CastTo::Float => Value::Float(0.0),
        CastTo::Bool => Value::Bool(true),
        CastTo::String => Value::Str(PhpStr::from(text())),
        _ => return None,
    }))
}

fn tidy_hooks() -> NativeHooks {
    NativeHooks {
        // The default configuration file is read as the object is created,
        // so its diagnostics name the caller: test gh10636.phpt shows
        // "main()" for `new tidy`.
        create: Some(|vm, o| {
            let mut config = php_config();
            let r = load_default_config(vm, &mut config);
            *o.obj().native.borrow_mut() = Some(Box::new(TidyObj { config, doc: None }));
            vm.handle_unwind(r);
        }),
        cast: Some(|_, o, to| {
            cast_result(
                || match doc_of(o) {
                    Some(d) => d.borrow().output(),
                    None => Vec::new(),
                },
                to,
            )
        }),
        ..Default::default()
    }
}

fn node_hooks() -> NativeHooks {
    NativeHooks {
        cast: Some(|_, o, to| {
            cast_result(
                || {
                    with_native::<NodeObj, _>(o, |n| n.doc.borrow().node_text(n.id))
                        .unwrap_or_default()
                },
                to,
            )
        }),
        ..Default::default()
    }
}
