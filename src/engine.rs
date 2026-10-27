//! Engine startup shared by the CLI and the built-in web server SAPIs:
//! module startup (extension registration, INI settings), compilation of
//! scripts and request startup.

use goro_core::compiler::Compiler;
use goro_core::opcode::OpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
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
    goro_ext_pgsql::register(vm);
    goro_ext_sockets::register(vm);
    goro_ext_mbstring::register(vm);
    goro_ext_spl::register(vm);
    goro_ext_reflection::register(vm);
    goro_ext_bcmath::register(vm);
    goro_ext_filter::register(vm);
    goro_ext_soap::register(vm);
    goro_ext_iconv::register(vm);
    goro_ext_gettext::register(vm);
    goro_ext_sysvipc::register(vm);
    goro_ext_ftp::register(vm);
    goro_ext_pcntl::register(vm);
    goro_ext_posix::register(vm);
    goro_ext_calendar::register(vm);
    goro_ext_phar::register(vm);
    goro_ext_exif::register(vm);
    goro_ext_zip::register(vm);
    goro_ext_fileinfo::register(vm);
    goro_ext_sodium::register(vm);
    goro_ext_intl::register(vm);
    goro_ext_sqlite3::register(vm);
    goro_ext_ldap::register(vm);
    goro_ext_uuid::register(vm);
    goro_ext_mailparse::register(vm);
    goro_ext_imagick::register(vm);
    goro_ext_xsl::register(vm);
    goro_ext_gd::register(vm);
    goro_ext_dba::register(vm);
    goro_ext_readline::register(vm);
    goro_ext_tidy::register(vm);
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
    parse_ini_text_file(data, out, "Unknown");
}

/// parse_ini_text() of the INI file `filename` (named in syntax errors).
pub fn parse_ini_text_file(data: &[u8], out: &mut IniSettings, filename: &str) {
    // A scratch engine resolves constants (E_ALL ...) in the INI values; its
    // diagnostics are not shown.
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    let parsed =
        goro_ext_standard::ini_parser::parse_ini(&mut vm, data, true, 0 /* NORMAL */, "");
    let _ = vm.take_output();
    let Value::Array(arr) = parsed else {
        // A syntax error: the entries before it still apply, and
        // (CG(ini_parser_unbuffered_errors)) the error goes to stderr.
        let (events, err) =
            goro_ext_standard::ini_parser::parse_ini_startup(&mut vm, data, filename);
        let _ = vm.take_output();
        let mut skip = false;
        for ev in events {
            match ev {
                goro_ext_standard::ini_parser::IniEvent::Section(name) => {
                    let upper = name.to_ascii_uppercase();
                    skip = upper.starts_with(b"PATH") || upper.starts_with(b"HOST");
                }
                goro_ext_standard::ini_parser::IniEvent::Entry(k, v) if !skip => out.push((
                    String::from_utf8_lossy(&k).into_owned(),
                    String::from_utf8_lossy(&v).into_owned(),
                )),
                _ => {}
            }
        }
        if let Some(msg) = err {
            eprint!("PHP:  {}", msg);
        }
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
    goro_parser::set_short_open_tag(ini_lookup(ini, "short_open_tag").is_none_or(ini_truthy));
    goro_ext_mbstring::zend_multibyte::install(&|k: &str| {
        ini_lookup(ini, k).map(|v| v.to_string())
    });

    let mut vm = Vm::new();
    register_extensions(&mut vm);
    // Extensions only loaded on request (`-d extension=zend_test`, dl()).
    goro_ext_zend_test::register_loadables(&mut vm);
    if goro_ext_zend_test::requested(ini.iter().map(|(k, v)| (k.as_str(), v.as_str()))) {
        goro_ext_zend_test::register(&mut vm);
    }
    // Registered last: its startup (preloading) runs after the other
    // extensions' (as accel_post_startup() does).
    goro_ext_opcache::register(&mut vm);
    goro_core::compiler::set_sapi_name(sapi.as_bytes());
    vm.constants.insert(
        b"PHP_SAPI".to_vec(),
        Value::String(PhpString::from_string(sapi.to_string())),
    );
    if sapi == "cli" {
        goro_ext_standard::misc::set_sapi_header_functions(&mut vm, false);
    }
    // (registered by sapi/cli's module startup)
    if sapi != "cli" && sapi != "cli-server" {
        vm.constants
            .shift_remove(b"PHP_CLI_PROCESS_TITLE".as_slice());
    }
    // The request-header functions belong to the web SAPIs (cli-server,
    // cgi, fpm-fcgi register them); php-cli has none of them and php-fpm
    // no apache_response_headers().
    let sapi_lacks: &[&[u8]] = match sapi {
        "cli" => &[
            b"getallheaders",
            b"apache_request_headers",
            b"apache_response_headers",
        ],
        "fpm-fcgi" => &[b"apache_response_headers"],
        _ => &[],
    };
    for f in sapi_lacks {
        vm.functions.remove(*f);
    }
    let startup_messages = goro_ext_standard::misc::apply_startup_ini(&mut vm, ini);
    // The compiler checks the (emulated) C stack too: sized from the start.
    {
        let get = |k: &[u8]| vm.constants.get(k).map(|v| v.to_php_string());
        let (max, reserved) = (
            get(b"zend.max_allowed_stack_size"),
            get(b"zend.reserved_stack_size"),
        );
        goro_core::stack::configure_from_ini(
            max.as_ref().map(|s| s.as_bytes()),
            reserved.as_ref().map(|s| s.as_bytes()),
        );
    }
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
    /// The diagnostics compiling raised before the error (kind, message, line).
    pub prior: Vec<(&'static str, String, u32)>,
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
    // phar_compile_file(): tar/zip-based and compressed phars
    let redirected = goro_ext_phar::main_script_source(filename, source, &[]);
    let source: &[u8] = redirected.as_deref().unwrap_or(source);
    let script = goro_parser::multibyte::lex_script(source, skip_shebang);
    let tokens = script.tokens;
    let source = script.source.as_slice();
    let lexer_warnings = goro_parser::take_lexer_warnings();
    goro_core::stack::install_parser_stack_grower();
    let mut parser = Parser::new(tokens).with_source(source);
    let parsed = parser.parse();
    // Lexer and parser diagnostics, in source order (taken after the parse,
    // which marks the code that is never compiled).
    let mut lexer_deprecations = goro_parser::take_lexer_deprecations();
    lexer_deprecations.sort_by_key(|(_, line)| *line);
    let program = match parsed {
        Ok(p) => p,
        Err(e) => {
            let msg = e.message.clone();
            let (kind, message) = if goro_parser::is_php_parse_error(&msg) {
                ("Parse error", msg)
            } else if is_fatal_parse_error(&msg) {
                ("Fatal error", msg)
            } else if msg.starts_with("The (real) cast")
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
            // (the scanner's deprecations for what it read before the
            // error: non-canonical casts...)
            let prior = lexer_deprecations
                .into_iter()
                .filter(|(_, l)| *l <= e.span.line)
                .map(|(m, l)| ("Deprecated", m, l))
                .collect();
            return Err(CompileError {
                kind,
                message,
                line: e.span.line,
                prior,
            });
        }
    };
    let mut compiler = Compiler::new();
    compiler.source_file = goro_core::fspath::msg_bytes(filename).into_owned();
    compiler.takes_rtd_keys = true;
    match compiler.compile(&program) {
        Ok((op_array, classes, compile_warnings)) => {
            let _ = goro_core::compiler::take_pending_compile_warnings();
            if std::env::var_os("GORO_DUMP_OPS").is_some() {
                eprint!("{}", op_array.dump());
                for c in &classes {
                    for m in c.methods.values() {
                        eprint!("{}", m.op_array.dump());
                    }
                }
            }
            Ok(Compiled {
                op_array,
                classes,
                lexer_deprecations,
                lexer_warnings,
                compile_warnings,
            })
        }
        Err(e) => Err(CompileError {
            kind: "Fatal error",
            message: e.message,
            line: e.line,
            prior: goro_core::compiler::take_pending_compile_warnings(),
        }),
    }
}

/// Load a compiled script into the engine: compile-time diagnostics and
/// class declarations.
pub fn load(vm: &mut Vm, compiled: &mut Compiled) {
    vm.replay_cached_diags(&goro_core::script_cache::merged_compile_diags(
        &goro_parser::order_lexer_diagnostics(
            &compiled.lexer_deprecations,
            &compiled.lexer_warnings,
        ),
        &compiled.compile_warnings,
    ));
    for class in std::mem::take(&mut compiled.classes) {
        vm.register_class(class);
    }
}

/// Compile and run `src`, the main script of a request of a long-running
/// SAPI (goro-httpd, php-fpm), through the opcache script cache: an
/// earlier request's compilation of `name` (its resolved path) is reused
/// while the file is unchanged, as for included files. A compile error is
/// reported (E_COMPILE_ERROR / E_PARSE, the request's last error).
pub fn run_main_script(vm: &mut Vm, src: &[u8], name: &str) -> Result<Value, VmError> {
    if let Some(c) = goro_core::script_cache::lookup(name) {
        vm.replay_cached_diags(&c.diags);
        for class in c.classes.iter().cloned() {
            vm.register_class(class);
        }
        return vm.execute_chunk(&c.op_array);
    }
    let compiled = {
        let _ct = goro_core::compiler::CtVmGuard::new(vm);
        compile(src, name, true)
    };
    match compiled {
        Ok(mut c) => {
            if goro_core::script_cache::enabled()
                && goro_core::script_cache::store_compiled(
                    name,
                    std::rc::Rc::new(goro_core::script_cache::Compiled {
                        op_array: c.op_array.clone(),
                        classes: c.classes.clone().into(),
                        diags: Vec::new(),
                    }),
                )
            {
                vm.run_optimizer_passes();
            }
            load(vm, &mut c);
            vm.execute_chunk(&c.op_array)
        }
        Err(e) => {
            report_compile_error(vm, &e, name);
            vm.fatal_emitted = true;
            vm.last_error = Some((4, e.message.clone(), name.to_string(), e.line));
            Err(VmError {
                message: e.message,
                line: e.line,
            })
        }
    }
}

/// php_execute_script(): `run` (the primary script) between the
/// auto_prepend_file and the auto_append_file. exit() or a fatal error
/// ends the sequence.
pub fn with_auto_files(
    vm: &mut Vm,
    run: impl FnOnce(&mut Vm) -> Result<Value, VmError>,
) -> Result<Value, VmError> {
    if let Some(r) = run_auto_file(vm, b"auto_prepend_file") {
        if r.is_err() || vm.exit_requested || vm.fatal_emitted {
            return r;
        }
    }
    let r = run(vm);
    if r.is_ok() && !vm.exit_requested && !vm.fatal_emitted {
        if let Some(Err(e)) = run_auto_file(vm, b"auto_append_file") {
            return Err(e);
        }
    }
    r
}

/// php_execute_script(): the auto_prepend_file / auto_append_file `key`
/// names, run as a required file of the main script (in the global scope,
/// from include_path). None when the setting is empty.
pub fn run_auto_file(vm: &mut Vm, key: &[u8]) -> Option<Result<Value, VmError>> {
    let file = vm
        .constants
        .get(key)
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    if file.is_empty() {
        return None;
    }
    // (the primary script is the first of get_included_files())
    if vm.included_files_list.is_empty() && !vm.current_file.is_empty() {
        let main = std::fs::canonicalize(&*vm.current_file)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| vm.current_file.to_string());
        vm.mark_included_file(&main);
        vm.included_files_list.push(main);
    }
    let resolved = vm.resolve_include_path(&file);
    let src = match std::fs::read(&resolved) {
        Ok(s) => s,
        Err(e) => {
            let reason = e.to_string();
            let reason = reason
                .find(" (os error")
                .map_or(reason.as_str(), |i| &reason[..i]);
            vm.display_error(
                "Warning",
                &format!("Unknown: Failed to open stream: {}", reason),
                "Unknown",
                0,
            );
            let include_path = vm
                .constants
                .get(b"include_path".as_slice())
                .map(|v| v.to_php_string().to_string_lossy())
                .unwrap_or_else(|| ".".to_string());
            let msg = format!(
                "Failed opening required '{}' (include_path='{}')",
                file, include_path
            );
            let saved = std::mem::replace(&mut vm.current_file, "Unknown".into());
            let err = vm.emit_fatal_typed_at(64, &msg, 0);
            vm.current_file = saved;
            return Some(Err(err));
        }
    };
    let name = std::fs::canonicalize(&resolved)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(resolved);
    // (open_file_for_scanning(): an included file for the _once variants)
    vm.mark_included_file(&name);
    vm.included_files_list.push(name.clone());
    let saved = std::mem::replace(&mut vm.current_file, name.as_str().into());
    let compiled = {
        let _ct = goro_core::compiler::CtVmGuard::new(vm);
        compile(&src, &name, true)
    };
    let r = match compiled {
        Ok(mut c) => {
            load(vm, &mut c);
            vm.execute_chunk(&c.op_array)
        }
        Err(e) => {
            report_compile_error(vm, &e, &name);
            vm.fatal_emitted = true;
            Err(VmError {
                message: e.message,
                line: e.line,
            })
        }
    };
    vm.current_file = saved;
    Some(r)
}

/// Report a compile error through the engine's error display (display_errors,
/// html_errors, log_errors), like php_error_cb.
pub fn report_compile_error(vm: &mut Vm, e: &CompileError, filename: &str) {
    for (kind, msg, line) in &e.prior {
        vm.display_error(kind, msg, filename, *line);
    }
    if e.kind == "Fatal error" {
        let saved = std::mem::replace(&mut vm.current_file, filename.into());
        let bound_fatal = vm.run_compile_time_binds(true);
        vm.current_file = saved;
        if bound_fatal {
            return;
        }
    }
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

#[cfg(test)]
mod tests {
    /// Every built-in function name is registered once: Vm::register_function()
    /// debug-asserts on a second registration (which would silently replace
    /// the first implementation); wrappers go through override_function().
    #[test]
    fn no_duplicate_function_registrations() {
        let ini = vec![("extension".to_string(), "zend_test".to_string())];
        // The engine's STDIN/STDOUT/STDERR streams close their descriptors
        // when the thread's state goes away: keep the test harness' output.
        let saved: Vec<i32> = (0..3).map(|fd| unsafe { libc::dup(fd) }).collect();
        let res = std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || {
                let vm = super::new_vm(&ini, "cli");
                assert!(vm.functions.contains_key(b"strlen".as_slice()));
                assert!(
                    vm.functions
                        .contains_key(b"zend_object_init_with_constructor".as_slice())
                );
            })
            .unwrap()
            .join();
        for (fd, &copy) in saved.iter().enumerate() {
            unsafe {
                libc::dup2(copy, fd as i32);
                libc::close(copy);
            }
        }
        res.unwrap();
    }
}
