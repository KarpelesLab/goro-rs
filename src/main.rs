use std::io::Write;
use std::path::Path;
use std::process;

fn main() {
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
            run_code(code.as_bytes());
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
                Ok(source) => run_code(&source),
                Err(e) => {
                    eprintln!("Error reading file: {}", e);
                    process::exit(1);
                }
            }
        }
    }
}

fn run_code(source: &[u8]) {
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
    let compiler = Compiler::new();
    let op_array = match compiler.compile(&program) {
        Ok(ops) => ops,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(255);
        }
    };

    // Execute
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);

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
            if !output.is_empty() {
                let stdout = std::io::stdout();
                let mut handle = stdout.lock();
                handle.write_all(&output).ok();
                handle.flush().ok();
            }
            eprintln!("\n{}", e);
            process::exit(255);
        }
    }
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
