use std::io::Write;
use std::path::Path;
use std::process;

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

    match args[1].as_str() {
        "-r" => {
            // Run PHP code from command line
            if args.len() < 3 {
                eprintln!("Error: -r requires a code argument");
                process::exit(1);
            }
            let code = format!("<?php {}", args[2]);
            run_code(code.as_bytes(), "Command line code");
        }
        "-v" | "--version" => {
            println!("goro-rs 0.1.0 (PHP 8.5.4 compatible)");
            println!("Copyright (c) goro-rs contributors");
            println!("Built with Rust");
        }
        "-h" | "--help" => {
            print_usage(&args[0]);
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
        }
        file_path => {
            // Run a PHP file
            let path = Path::new(file_path);
            if !path.exists() {
                eprintln!("Error: file not found: {}", file_path);
                process::exit(1);
            }
            match std::fs::read(path) {
                Ok(source) => run_code(&source, file_path),
                Err(e) => {
                    eprintln!("Error reading file: {}", e);
                    process::exit(1);
                }
            }
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
    let (op_array, compiled_classes) = match compiler.compile(&program) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(255);
        }
    };

    // Execute
    let mut vm = Vm::new();
    vm.current_file = filename.to_string();
    goro_ext_standard::register_standard_functions(&mut vm);
    goro_ext_date::register(&mut vm);
    goro_ext_json::register(&mut vm);
    goro_ext_ctype::register(&mut vm);
    goro_ext_hash::register(&mut vm);
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
                    write!(handle, "\nFatal error: {}\n", e).ok();
                }
            } else {
                write!(handle, "\nFatal error: {}\n", e).ok();
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
                let loc = if file.is_empty() {
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
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -r <code>     Run PHP code from the command line");
    eprintln!("  -v, --version Display version information");
    eprintln!("  -h, --help    Display this help message");
    eprintln!("  --test <dir>  Run PHPT tests in directory");
}
