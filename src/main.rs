use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

mod cli_server;

/// Script path and its arguments, as given on the command line.
static SCRIPT_ARGV: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

fn main() {
    // HARD memory limit: 2GB virtual address space.
    // This MUST be set before anything else to prevent OOM kills.
    // Without this, runaway PHP scripts can allocate 100GB+ and crash the machine.
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" {
            fn setrlimit(resource: i32, rlim: *const [u64; 2]) -> i32;
        }
        let limit: u64 = 2 * 1024 * 1024 * 1024; // 2GB
        let rlim: [u64; 2] = [limit, limit]; // [soft, hard]
        setrlimit(9, &rlim); // 9 = RLIMIT_AS on Linux
    }

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        process::exit(1);
    }

    // Built-in server mode: scan all args for -S (since php -S can appear
    // after -t doc_root -n etc.). Activated when -S is present.
    if args.iter().any(|a| a == "-S") {
        run_builtin_server(&args[1..]);
    }

    // Terminal subcommands that own the whole arg list.
    match args[1].as_str() {
        "-v" | "--version" => {
            println!("goro-rs 0.1.0 (PHP 8.5.4 compatible)");
            println!("Copyright (c) goro-rs contributors");
            println!("Built with Rust");
            return;
        }
        "-h" | "--help" => {
            print_usage(&args[0]);
            return;
        }
        "--test" => {
            // Run PHPT tests
            if args.len() < 3 {
                eprintln!("Error: --test requires a directory argument");
                process::exit(1);
            }
            let dir = Path::new(&args[2]);
            let (pass, fail, skip, error) = goro_phpt::run_test_dir(dir);
            println!("\n=== Test Results ===");
            println!("Pass:  {}", pass);
            println!("Fail:  {}", fail);
            println!("Skip:  {}", skip);
            println!("Error: {}", error);
            println!("Total: {}", pass + fail + skip + error);
            if fail > 0 || error > 0 {
                process::exit(1);
            }
            return;
        }
        _ => {}
    }

    // PHP-CLI-compatible flag scan: skip leading options (-n, -d <dir>, -C, -q,
    // -f <file>, -r <code>) to find the script path. This matters because the
    // PHPT harness shells out to this binary as `goro -n script.php` and many
    // tests run `$php -d ini=val ...`. Without this, the first flag would be
    // mistaken for a filename ("file not found: -n").
    let mut i = 1;
    let mut script: Option<String> = None;
    let mut inline_code: Option<String> = None;
    let mut ini_overrides: Vec<(String, String)> = Vec::new();
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-r" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -r requires a code argument");
                    process::exit(1);
                }
                inline_code = Some(args[i + 1].clone());
                // $argv: "Standard input code" then the remaining arguments
                // (a leading "--" separator is dropped).
                let mut rest = &args[i + 2..];
                if rest.first().map(|s| s.as_str()) == Some("--") {
                    rest = &rest[1..];
                }
                let mut av = vec!["Standard input code".to_string()];
                av.extend(rest.iter().cloned());
                let _ = SCRIPT_ARGV.set(av);
                break;
            }
            "-f" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -f requires a file argument");
                    process::exit(1);
                }
                script = Some(args[i + 1].clone());
                break;
            }
            // INI overrides: -d key=value
            "-d" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -d requires a directive argument");
                    process::exit(1);
                }
                let directive = &args[i + 1];
                if let Some(eq) = directive.find('=') {
                    let key = directive[..eq].trim().to_string();
                    let val = directive[eq + 1..].trim().to_string();
                    ini_overrides.push((key, val));
                } else {
                    ini_overrides.push((directive.trim().to_string(), "1".to_string()));
                }
                i += 2;
                continue;
            }
            // Switches with no argument; ignore.
            "-n" | "-C" | "-q" | "-z" => {
                i += 1;
                continue;
            }
            "--" => break,
            _ => {
                if a.starts_with('-') && a.len() > 1 {
                    // Unknown option: ignore rather than treat as a file, to
                    // mirror PHP's lenient CLI for flags we don't model.
                    i += 1;
                    continue;
                }
                // First bare argument is the script path.
                script = Some(args[i].clone());
                let _ = SCRIPT_ARGV.set(args[i..].to_vec());
                break;
            }
        }
    }

    let cli_error_reporting = apply_ini_overrides(&ini_overrides);

    if let Some(code) = inline_code {
        let code = format!("<?php {}", code);
        run_code(code.as_bytes(), "Command line code", cli_error_reporting);
        return;
    }

    let file_path = match script {
        Some(s) => s,
        None => {
            print_usage(&args[0]);
            process::exit(1);
        }
    };

    let path = Path::new(&file_path);
    if !path.exists() {
        eprintln!("Error: file not found: {}", file_path);
        process::exit(1);
    }
    // PHP exposes an absolute path through __FILE__/__DIR__ even when the script
    // is invoked with a relative path (e.g. `php tests/foo.php`). Mirror that so
    // includes via `__DIR__ . '/companion.inc'` resolve against the real dir.
    let abs_path = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| file_path.clone());
    match std::fs::read(path) {
        Ok(source) => run_code(&source, &abs_path, cli_error_reporting),
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            process::exit(1);
        }
    }
}

#[allow(clippy::field_reassign_with_default)] // fields filled in step by step
fn run_code(source: &[u8], filename: &str, initial_error_reporting: i64) {
    use goro_core::compiler::Compiler;
    use goro_core::vm::Vm;
    use goro_parser::{Lexer, Parser};

    // Lex
    let mut lexer = Lexer::new(source);
    lexer.skip_shebang();
    let tokens = lexer.tokenize();

    // Parse
    let mut parser = Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            // PHP routes most parser-rejected language errors to stdout as
            // "Fatal error". Plain syntax errors are reported as parse
            // errors and PHP prints them to stdout as well, but with a
            // different leading word.
            let is_fatal = is_fatal_parse_error(&e.message);
            let kind = if is_fatal {
                "Fatal error"
            } else {
                "Parse error"
            };
            let line = e.span.line;
            let out = format!(
                "\n{}: {} in {} on line {}\n",
                kind, e.message, filename, line
            );
            use std::io::Write as _;
            let stdout = std::io::stdout();
            let mut h = stdout.lock();
            let _ = h.write_all(out.as_bytes());
            let _ = h.flush();
            process::exit(255);
        }
    };

    // Drain any deprecations recorded by the lexer (e.g. `${var}` syntax).
    let lexer_deprecations = goro_parser::take_lexer_deprecations();

    // Compile
    let mut compiler = Compiler::new();
    compiler.source_file = filename.as_bytes().to_vec();
    let (op_array, compiled_classes, compile_warnings) = match compiler.compile(&program) {
        Ok(result) => {
            // Drain any pending compile-time warnings so they do not leak into a later session
            let _ = goro_core::compiler::take_pending_compile_warnings();
            result
        }
        Err(e) => {
            // Emit any compile-time diagnostics collected before the fatal, in order
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            for (msg, line) in &lexer_deprecations {
                let _ = handle.write_all(
                    format!("\nDeprecated: {} in {} on line {}\n", msg, filename, line).as_bytes(),
                );
            }
            for (kind, msg, line) in goro_core::compiler::take_pending_compile_warnings() {
                let _ = handle.write_all(
                    format!("\n{}: {} in {} on line {}\n", kind, msg, filename, line).as_bytes(),
                );
            }
            let _ = handle.write_all(
                format!(
                    "\nFatal error: {} in {} on line {}\n",
                    e.message, filename, e.line
                )
                .as_bytes(),
            );
            let _ = handle.flush();
            process::exit(255);
        }
    };

    // Execute
    let mut vm = Vm::new();
    vm.cli_direct_output = true;
    vm.current_file = filename.to_string();
    vm.error_reporting = initial_error_reporting;
    // Emit lexer deprecations first (e.g. `${var}` syntax), then compile warnings
    for (msg, wline) in &lexer_deprecations {
        vm.emit_deprecated_at(msg, *wline);
    }
    for (msg, wline) in &compile_warnings {
        vm.emit_warning_at(msg, *wline);
    }
    goro_ext_standard::register_standard_functions(&mut vm);
    goro_ext_date::register(&mut vm);
    goro_ext_json::register(&mut vm);
    goro_ext_ctype::register(&mut vm);
    goro_ext_hash::register(&mut vm);
    goro_ext_openssl::register(&mut vm);
    goro_ext_zlib::register(&mut vm);
    goro_ext_gmp::register(&mut vm);
    goro_ext_bz2::register(&mut vm);
    goro_ext_curl::register(&mut vm);
    goro_ext_xml::register(&mut vm);
    goro_ext_session::register(&mut vm);
    goro_ext_mysqli::register(&mut vm);
    goro_ext_sockets::register(&mut vm);
    goro_ext_mbstring::register(&mut vm);
    goro_ext_spl::register(&mut vm);
    goro_ext_reflection::register(&mut vm);
    // Register compiled classes
    for class in compiled_classes {
        vm.register_class(class);
    }

    // Request startup (CLI): superglobals and $argv/$argc.
    {
        let mut info = goro_ext_standard::request::RequestInfo::default();
        info.script_filename = filename.as_bytes().to_vec();
        info.argv = match SCRIPT_ARGV.get() {
            Some(a) => a.iter().map(|s| s.as_bytes().to_vec()).collect(),
            None => vec![b"Standard input code".to_vec()],
        };
        info.env = std::env::vars_os()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().as_bytes().to_vec(),
                    v.to_string_lossy().as_bytes().to_vec(),
                )
            })
            .collect();
        let ini = |_k: &str| -> Option<String> { None };
        goro_ext_standard::request::startup(&mut vm, &info, &ini);
    }
    for hook in vm.request_startup_hooks.clone() {
        hook(&mut vm);
    }

    match vm.execute(&op_array) {
        Ok(_) => {
            let output = vm.take_output();
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&output).ok();
            handle.flush().ok();
            if vm.exit_requested && vm.exit_code != 0 {
                process::exit(vm.exit_code as i32);
            }
        }
        Err(e) => {
            // The VM displays uncaught exceptions / fatal errors itself
            // (through the output layer, before shutdown functions).
            if vm.current_exception.is_some() || !vm.fatal_emitted {
                vm.report_uncaught(&e);
            }
            vm.run_shutdown_destructors();
            let output = vm.take_output();
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&output).ok();
            handle.flush().ok();
            process::exit(255);
        }
    }
}

fn is_fatal_parse_error(err_msg: &str) -> bool {
    // PHP classifies many parser-rejected language errors as "Fatal error"
    // rather than "Parse error". Mirror the predicate used by the PHPT
    // harness (crates/goro-phpt/src/lib.rs) so the CLI binary produces
    // matching output for these cases.
    err_msg.contains("Match expressions")
        || err_msg.starts_with("Cannot create Closure")
        || err_msg.contains("Cannot use")
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

fn apply_ini_overrides(overrides: &[(String, String)]) -> i64 {
    use goro_core::value::{set_memory_limit, set_php_precision, set_php_serialize_precision};
    // Default PHP error_reporting (E_ALL); overridden by -d error_reporting=...
    let mut error_reporting: i64 = 32767;
    for (key, val) in overrides {
        match key.as_str() {
            "precision" => {
                if let Ok(p) = val.parse::<i32>() {
                    set_php_precision(p);
                }
            }
            "serialize_precision" => {
                if let Ok(p) = val.parse::<i32>() {
                    set_php_serialize_precision(p);
                }
            }
            "memory_limit" => {
                // Parse like ini_set memory_limit: support k/m/g suffix
                let s = val.trim();
                let (num_part, mul): (&str, i64) = if let Some(last) = s.chars().last() {
                    match last {
                        'k' | 'K' => (&s[..s.len() - 1], 1024),
                        'm' | 'M' => (&s[..s.len() - 1], 1024 * 1024),
                        'g' | 'G' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
                        _ => (s, 1),
                    }
                } else {
                    (s, 1)
                };
                if let Ok(n) = num_part.parse::<i64>() {
                    set_memory_limit(n * mul);
                }
            }
            "error_reporting" => {
                if let Ok(n) = val.parse::<i64>() {
                    error_reporting = n;
                }
            }
            _ => {
                // Unhandled INI directive — silently ignored for now.
            }
        }
    }
    error_reporting
}

fn print_usage(program: &str) {
    eprintln!("Usage: {} [options] [-f] <file> [--] [args...]", program);
    eprintln!("       {} -r <code>", program);
    eprintln!("       {} -S addr:port [-t docroot] [router]", program);
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -r <code>          Run PHP code from the command line");
    eprintln!("  -S <addr:port>     Run with the built-in web server");
    eprintln!("  -t <docroot>       Document root for built-in web server");
    eprintln!("  -n                 No php.ini file used");
    eprintln!("  -v, --version      Display version information");
    eprintln!("  -h, --help         Display this help message");
    eprintln!("  --test <dir>       Run PHPT tests in directory");
}

fn run_builtin_server(args: &[String]) -> ! {
    let mut addr: Option<String> = None;
    let mut doc_root: Option<PathBuf> = None;
    let mut router: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-S" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: -S requires an address argument");
                    process::exit(1);
                }
                addr = Some(args[i].clone());
            }
            "-t" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: -t requires a docroot argument");
                    process::exit(1);
                }
                doc_root = Some(PathBuf::from(&args[i]));
            }
            "-n" => { /* ignore: no INI */ }
            "-d" => {
                // skip the next arg (ini directive)
                i += 1;
            }
            _ => {
                // Treat as router script (last positional argument)
                router = Some(PathBuf::from(a));
            }
        }
        i += 1;
    }
    let addr = match addr {
        Some(a) => a,
        None => {
            eprintln!("Error: -S requires an address argument");
            process::exit(1);
        }
    };
    let doc_root =
        doc_root.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let opts = cli_server::CliServerOptions {
        addr,
        doc_root,
        router,
    };
    cli_server::run(opts)
}
