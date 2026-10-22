//! Engine startup shared by the CLI and the built-in web server SAPIs:
//! module startup (extension registration, INI settings), compilation of
//! scripts and request startup.

use goro_core::compiler::Compiler;
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_parser::Parser;

/// INI settings in the order they apply (later entries override earlier ones).
pub type IniSettings = Vec<(String, String)>;

pub fn register_extensions(vm: &mut Vm) {
    goro_ext_standard::register_standard_functions(vm);
    goro_ext_date::register(vm);
    goro_ext_json::register(vm);
    goro_ext_ctype::register(vm);
    goro_ext_hash::register(vm);
    goro_ext_openssl::register(vm);
    goro_ext_zlib::register(vm);
    goro_ext_gmp::register(vm);
    goro_ext_bz2::register(vm);
    goro_ext_curl::register(vm);
    goro_ext_xml::register(vm);
    goro_ext_session::register(vm);
    goro_ext_mysqli::register(vm);
    goro_ext_pdo::register(vm);
    goro_ext_sockets::register(vm);
    goro_ext_mbstring::register(vm);
    goro_ext_spl::register(vm);
    goro_ext_reflection::register(vm);
    goro_ext_bcmath::register(vm);
    goro_ext_filter::register(vm);
    goro_ext_iconv::register(vm);
    goro_ext_calendar::register(vm);
    vm.register_extension(b"simplexml");
    vm.register_extension(b"tokenizer");
    vm.register_extension(b"pcre");
}

fn ini_lookup<'a>(ini: &'a IniSettings, key: &str) -> Option<&'a str> {
    ini.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn ini_truthy(v: &str) -> bool {
    let l = v.trim().to_ascii_lowercase();
    matches!(l.as_str(), "1" | "on" | "yes" | "true") || l.parse::<i64>().is_ok_and(|n| n != 0)
}

/// Parse INI text (php.ini or the -d definitions) the way php_init_config()
/// does: entries outside of sections and in ordinary sections apply;
/// [PATH=...] and [HOST=...] sections are not used by the CLI.
pub fn parse_ini_text(data: &[u8], out: &mut IniSettings) {
    // A scratch engine resolves constants (E_ALL ...) in the INI values; its
    // diagnostics are not shown.
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    let parsed =
        goro_ext_standard::ini_parser::parse_ini(&mut vm, data, true, 0 /* NORMAL */, "");
    let _ = vm.take_output();
    let Value::Array(arr) = parsed else {
        return;
    };
    for (k, v) in arr.borrow().iter() {
        let key = k.to_string();
        match v {
            Value::Array(section) => {
                let upper = key.to_ascii_uppercase();
                if upper.starts_with("PATH") || upper.starts_with("HOST") {
                    continue;
                }
                for (sk, sv) in section.borrow().iter() {
                    if !matches!(sv, Value::Array(_)) {
                        out.push((sk.to_string(), sv.to_php_string().to_string_lossy()));
                    }
                }
            }
            _ => out.push((key, v.to_php_string().to_string_lossy())),
        }
    }
}

/// Module startup: a fresh engine with every extension registered and the
/// INI settings applied. `sapi` is PHP_SAPI ("cli" or "cli-server").
pub fn new_vm(ini: &IniSettings, sapi: &str) -> Vm {
    use goro_core::value::{
        memory_reset, set_memory_limit, set_php_precision, set_php_serialize_precision,
    };
    memory_reset();
    set_memory_limit(128 * 1024 * 1024);
    set_php_precision(14);
    set_php_serialize_precision(-1);
    goro_parser::set_short_open_tag(ini_lookup(ini, "short_open_tag").is_some_and(ini_truthy));
    goro_ext_mbstring::zend_multibyte::install(&|k: &str| {
        ini_lookup(ini, k).map(|v| v.to_string())
    });

    let mut vm = Vm::new();
    register_extensions(&mut vm);
    // Extensions only loaded on request (`-d extension=zend_test`).
    if goro_ext_zend_test::requested(ini.iter().map(|(k, v)| (k.as_str(), v.as_str()))) {
        goro_ext_zend_test::register(&mut vm);
    }
    vm.constants.insert(
        b"PHP_SAPI".to_vec(),
        Value::String(PhpString::from_string(sapi.to_string())),
    );
    let startup_messages = goro_ext_standard::misc::apply_startup_ini(&mut vm, ini);
    if !vm.startup_hooks.is_empty() {
        let prev = vm.take_output();
        for hook in vm.startup_hooks.clone() {
            hook(&mut vm);
        }
        vm.write_output(&prev);
    }
    if !startup_messages.is_empty() && vm.error_reporting & 8192 != 0 {
        let buf = vm.active_output_buffer();
        buf.splice(0..0, startup_messages);
    }
    vm
}

/// A compile-time failure: (error kind, message, line).
pub struct CompileError {
    pub kind: &'static str,
    pub message: String,
    pub line: u32,
}

/// The output of a successful compilation.
pub struct Compiled {
    pub op_array: OpArray,
    pub classes: Vec<goro_core::object::ClassEntry>,
    pub lexer_deprecations: Vec<(String, u32)>,
    pub lexer_warnings: Vec<(String, u32)>,
    pub compile_warnings: Vec<(String, u32)>,
}

/// Whether the parser's message is reported as "Fatal error" (a compile
/// error) rather than "Parse error".
fn is_fatal_parse_error(err_msg: &str) -> bool {
    err_msg.contains("Match expressions")
        || err_msg.starts_with("Unparenthesized `")
        || err_msg.starts_with("Cannot mix [] and list()")
        || err_msg.ends_with("can only be used from the outermost scope")
        || err_msg.starts_with("Cannot assign to array(), use [] instead")
        || err_msg.starts_with("Assignments can only happen to writable values")
        || err_msg.contains("Cannot use")
        || err_msg.starts_with("Cannot create Closure")
        || err_msg.contains("cannot declare")
        || err_msg.contains("cannot implement")
        || err_msg.contains("cannot extend")
        || err_msg.contains("is reserved")
        || err_msg.contains("must be compatible")
        || err_msg.contains("must be")
        || err_msg.contains("previously implemented")
        || err_msg.contains("already in use")
        || err_msg.contains("is redundant")
        || err_msg.contains("not allowed")
        || err_msg.contains("Can't use")
        || err_msg.contains("Abstract function")
        || err_msg.contains("Non-abstract method")
        || err_msg.contains("cannot be part of an intersection type")
        || err_msg.contains("cannot be a reference")
        || err_msg.contains("Cannot redeclare")
        || err_msg.contains("cannot take arguments")
        || err_msg.contains("must take exactly")
        || err_msg.contains("cannot be static")
        || err_msg.contains("may not be nested")
        || err_msg.contains("cannot accept")
        || err_msg.contains("may be declared abstract")
        || err_msg.contains("must have type")
        || err_msg.contains("Duplicate value in enum")
        || err_msg.contains("Visibility of property")
        || err_msg.contains("asymmetric visibility")
        || err_msg.contains("cannot have type")
        || err_msg.contains("Enum ")
        || err_msg.contains("Property ")
        || err_msg.contains("Readonly property")
        || err_msg.contains("Static property")
        || err_msg.contains("constant must")
        || err_msg.contains("class must")
        || err_msg.starts_with("Unknown hook")
        || err_msg.starts_with("Cannot override final property hook")
        || err_msg.starts_with("Type of parameter")
        || err_msg.contains("define the same hooked property")
        || err_msg.starts_with("Parameter $")
        || err_msg.starts_with("set hook parameter")
        || err_msg.starts_with("Cannot use the static modifier on a property hook")
        || err_msg.starts_with("#[\\NoDiscard] is not supported for property hooks")
        || err_msg.starts_with("Cannot use the public modifier on a property hook")
        || err_msg.starts_with("Cannot use the protected modifier on a property hook")
        || err_msg.starts_with("Cannot use the private modifier on a property hook")
        || err_msg.starts_with("Property hooks cannot have visibility")
        || err_msg.starts_with("Property hook cannot be")
        || err_msg.starts_with("Property hook list must")
        || err_msg.starts_with("Property hook cannot have")
        || err_msg.starts_with("Cannot declare hooks for static property")
        || err_msg.starts_with("get hook of property")
        || err_msg.starts_with("set hook of property")
        || err_msg.starts_with("Abstract property")
        || err_msg.starts_with("Property in interface cannot be explicitly abstract")
        || err_msg.starts_with("Property in interface cannot be final")
        || err_msg.starts_with("Property in interface cannot be protected or private")
        || err_msg.starts_with("Final promoted property")
}

/// Lex, parse and compile a script.
pub fn compile(
    source: &[u8],
    filename: &str,
    skip_shebang: bool,
) -> Result<Compiled, CompileError> {
    let script = goro_parser::multibyte::lex_script(source, skip_shebang);
    let tokens = script.tokens;
    let source = script.source.as_slice();
    let mut lexer_deprecations = goro_parser::take_lexer_deprecations();
    // Lexer and parser diagnostics, in source order.
    lexer_deprecations.sort_by_key(|(_, line)| *line);
    let lexer_warnings = goro_parser::take_lexer_warnings();
    let mut parser = Parser::new(tokens).with_source(source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            let msg = e.message.clone();
            let (kind, message) = if is_fatal_parse_error(&msg) {
                ("Fatal error", msg)
            } else if msg.starts_with("syntax error")
                || msg.starts_with("The (real) cast")
                || msg.starts_with("Invalid UTF-8 codepoint")
                || msg.starts_with("Invalid numeric literal")
                || msg.starts_with("Invalid body indentation level")
                || msg.starts_with("Invalid indentation - tabs and spaces")
                || msg.starts_with("Unterminated comment")
            {
                ("Parse error", msg)
            } else {
                ("Parse error", "syntax error".to_string())
            };
            return Err(CompileError {
                kind,
                message,
                line: e.span.line,
            });
        }
    };
    let mut compiler = Compiler::new();
    compiler.source_file = filename.as_bytes().to_vec();
    match compiler.compile(&program) {
        Ok((op_array, classes, compile_warnings)) => {
            let _ = goro_core::compiler::take_pending_compile_warnings();
            Ok(Compiled {
                op_array,
                classes,
                lexer_deprecations,
                lexer_warnings,
                compile_warnings,
            })
        }
        Err(e) => {
            let _ = goro_core::compiler::take_pending_compile_warnings();
            Err(CompileError {
                kind: "Fatal error",
                message: e.message,
                line: e.line,
            })
        }
    }
}

/// Load a compiled script into the engine: compile-time diagnostics and
/// class declarations.
pub fn load(vm: &mut Vm, compiled: &mut Compiled) {
    for (msg, line) in &compiled.lexer_deprecations {
        vm.emit_deprecated_at(msg, *line);
    }
    for (msg, line) in &compiled.lexer_warnings {
        vm.emit_warning_at(msg, *line);
    }
    for (msg, line) in &compiled.compile_warnings {
        vm.emit_compile_diagnostic(msg, *line);
    }
    for class in std::mem::take(&mut compiled.classes) {
        vm.register_class(class);
    }
}

/// Report a compile error through the engine's error display (display_errors,
/// html_errors, log_errors), like php_error_cb.
pub fn report_compile_error(vm: &mut Vm, e: &CompileError, filename: &str) {
    vm.display_error(e.kind, &e.message, filename, e.line);
}

/// Request startup: request headers state, output buffering and the
/// superglobals.
pub fn request_startup(
    vm: &mut Vm,
    info: &goro_ext_standard::request::RequestInfo,
    ini: &IniSettings,
) {
    vm.headers_sent = false;
    vm.output_start = None;
    goro_ext_standard::ob::request_startup(vm);
    let lookup = |k: &str| -> Option<String> { ini_lookup(ini, k).map(|s| s.to_string()) };
    goro_ext_standard::request::startup(vm, info, &lookup);
    for hook in vm.request_startup_hooks.clone() {
        hook(vm);
    }
}
