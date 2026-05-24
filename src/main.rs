use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

mod cli_server;

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
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-r" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -r requires a code argument");
                    process::exit(1);
                }
                inline_code = Some(args[i + 1].clone());
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
            // INI / behavior flags that take a following argument.
            "-d" => { i += 2; continue; }
            // Switches with no argument; ignore.
            "-n" | "-C" | "-q" | "-z" => { i += 1; continue; }
            "--" => { i += 1; break; }
            _ => {
                if a.starts_with('-') && a.len() > 1 {
                    // Unknown option: ignore rather than treat as a file, to
                    // mirror PHP's lenient CLI for flags we don't model.
                    i += 1;
                    continue;
                }
                // First bare argument is the script path.
                script = Some(args[i].clone());
                break;
            }
        }
    }

    if let Some(code) = inline_code {
        let code = format!("<?php {}", code);
        run_code(code.as_bytes(), "Command line code");
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
    match std::fs::read(path) {
        Ok(source) => run_code(&source, &file_path),
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            process::exit(1);
        }
    }
}

fn run_code(source: &[u8], filename: &str) {
    use goro_core::compiler::Compiler;
    use goro_core::vm::Vm;
    use goro_parser::{Lexer, Parser};

    // Lex
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    // Parse
    let mut parser = Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(255);
        }
    };

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
            for (kind, msg, line) in goro_core::compiler::take_pending_compile_warnings() {
                let _ = handle.write_all(
                    format!("\n{}: {} in {} on line {}\n", kind, msg, filename, line).as_bytes()
                );
            }
            let _ = handle.write_all(
                format!("\nFatal error: {} in {} on line {}\n", e.message, filename, e.line).as_bytes()
            );
            let _ = handle.flush();
            process::exit(255);
        }
    };

    // Execute
    let mut vm = Vm::new();
    vm.current_file = filename.to_string();
    // Emit compile warnings before execution
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

    match vm.execute(&op_array) {
        Ok(_) => {
            let output = vm.take_output();
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(&output).ok();
            handle.flush().ok();
        }
        Err(e) => {
            // Print any output generated before the error
            let output = vm.take_output();
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            if !output.is_empty() {
                handle.write_all(&output).ok();
            }
            // Format fatal error like PHP does (to stdout, not stderr)
            if let Some(exc) = vm.current_exception.take() {
                if let goro_core::value::Value::Object(obj) = &exc {
                    let obj = obj.borrow();
                    let class = String::from_utf8_lossy(&obj.class_name);
                    let msg = obj.get_property(b"message");
                    let msg_str = msg.to_php_string().to_string_lossy();
                    let file = obj.get_property(b"file").to_php_string().to_string_lossy();
                    let line_val = obj.get_property(b"line").to_long();
                    let file_str = if file.is_empty() { "Unknown".to_string() } else { file };
                    let line_num = if line_val > 0 { line_val as u32 } else { e.line };
                    // Get trace from exception object
                    let trace_val = obj.get_property(b"trace");
                    let trace_str = format_trace_from_value(&trace_val, &vm.format_stack_trace());
                    let fatal = if msg_str.is_empty() {
                        format!(
                            "\nFatal error: Uncaught {} in {}:{}\nStack trace:\n{}\n  thrown in {} on line {}\n",
                            class, file_str, line_num, trace_str, file_str, line_num
                        )
                    } else {
                        format!(
                            "\nFatal error: Uncaught {}: {} in {}:{}\nStack trace:\n{}\n  thrown in {} on line {}\n",
                            class, msg_str, file_str, line_num, trace_str, file_str, line_num
                        )
                    };
                    handle.write_all(fatal.as_bytes()).ok();
                } else {
                    let fatal = format!(
                        "\nFatal error: {} in {} on line {}\n",
                        e.message, filename, e.line
                    );
                    handle.write_all(fatal.as_bytes()).ok();
                }
            } else {
                let fatal = format!(
                    "\nFatal error: {} in {} on line {}\n",
                    e.message, filename, e.line
                );
                handle.write_all(fatal.as_bytes()).ok();
            }
            handle.flush().ok();
            process::exit(255);
        }
    }
}

fn format_trace_from_value(trace_val: &goro_core::value::Value, fallback: &str) -> String {
    use goro_core::value::Value;
    use goro_core::array::ArrayKey;
    use goro_core::string::PhpString;

    if let Value::Array(arr) = trace_val {
        let arr = arr.borrow();
        if arr.len() == 0 {
            return "#0 {main}".to_string();
        }
        let mut lines = Vec::new();
        let mut idx = 0;
        for (_key, frame_val) in arr.iter() {
            if let Value::Array(frame) = frame_val {
                let frame = frame.borrow();
                let file = frame.get(&ArrayKey::String(PhpString::from_bytes(b"file")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let line = frame.get(&ArrayKey::String(PhpString::from_bytes(b"line")))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                let function = frame.get(&ArrayKey::String(PhpString::from_bytes(b"function")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let class = frame.get(&ArrayKey::String(PhpString::from_bytes(b"class")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let type_str = frame.get(&ArrayKey::String(PhpString::from_bytes(b"type")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let args_str = if let Some(args_val) = frame.get(&ArrayKey::String(PhpString::from_bytes(b"args"))) {
                    if let Value::Array(args_arr) = args_val {
                        let args_arr = args_arr.borrow();
                        let formatted: Vec<String> = args_arr.iter().map(|(_k, v)| {
                            goro_core::vm::Vm::format_trace_arg(v)
                        }).collect();
                        formatted.join(", ")
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let loc = if file.is_empty() || file == "[internal function]" || line == 0 {
                    "[internal function]".to_string()
                } else {
                    format!("{}({})", file, line)
                };
                lines.push(format!("#{} {}: {}{}{}({})", idx, loc, class, type_str, function, args_str));
            }
            idx += 1;
        }
        lines.push(format!("#{} {{main}}", idx));
        return lines.join("\n");
    }
    if !fallback.is_empty() {
        return fallback.to_string();
    }
    "#0 {main}".to_string()
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
    let doc_root = doc_root.unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    });
    let opts = cli_server::CliServerOptions {
        addr,
        doc_root,
        router,
    };
    cli_server::run(opts)
}
