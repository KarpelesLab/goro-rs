//! The `goro` command: a PHP 8.5 CLI (PHP manual: Command line usage,
//! Command line options), plus the `.phpt` test runner.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use goro_engine::{RunOptions, Script};

/// The engine runs on a thread with a large stack: deeply nested code
/// (parser recursion, callbacks of callbacks) needs it, as PHP has its own
/// stack limits only for user-visible recursion.
const STACK_SIZE: usize = 1 << 30;

fn usage() -> &'static str {
    "Usage: goro [options] [-f] <file> [--] [args...]\n   goro [options] -r <code> [--] [args...]\n   goro [options] -- [args...]\n\n  -d key[=value]   Define INI entry key with value 'value'.\n  -f <file>        Parse and execute <file>.\n  -h               This help\n  -i               PHP information\n  -l               Syntax check only (lint)\n  -m               Show compiled in modules\n  -n               No configuration (ini) files will be used\n  -r <code>        Run PHP <code> without using script tags <?..?>\n  -v               Version number\n"
}

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args
        .first()
        .map(|a| a.to_string_lossy().into_owned())
        .as_deref()
    {
        Some("--test") => std::process::exit(goro_phpt::cli::test_main(&args[1..])),
        Some("--test-compare") => std::process::exit(goro_phpt::cli::compare_main(&args[1..])),
        _ => {}
    }
    let code = std::thread::Builder::new()
        .name("php".into())
        .stack_size(STACK_SIZE)
        .spawn(move || cli(args))
        .expect("cannot start engine thread")
        .join()
        .unwrap_or(255);
    std::process::exit(code);
}

enum Mode {
    Run,
    Lint,
    Version,
    Modules,
    Info,
    Help,
}

fn cli(args: Vec<OsString>) -> i32 {
    let args: Vec<Vec<u8>> = args.into_iter().map(|a| a.into_vec()).collect();
    let mut ini: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut script: Option<Script> = None;
    let mut rest: Vec<Vec<u8>> = Vec::new();
    let mut mode = Mode::Run;
    let mut begin: Option<Vec<u8>> = None;
    let mut each: Option<Script> = None;
    let mut end: Option<Vec<u8>> = None;
    let mut lint_files: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if script.is_some() {
            // Everything after the script goes to the script.
            if a == b"--" && rest.is_empty() {
                i += 1;
                continue;
            }
            rest.extend(args[i..].iter().cloned());
            break;
        }
        match a.as_slice() {
            b"--" => {
                rest.extend(args[i + 1..].iter().cloned());
                script = Some(Script::Stdin);
                break;
            }
            b"-n" | b"-q" | b"-H" | b"-C" | b"-e" => {}
            b"-c" | b"-z" => {
                i += 1;
            }
            b"-d" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    ini.push(split_ini(v));
                }
            }
            b"-f" => {
                i += 1;
                match args.get(i) {
                    Some(f) => script = Some(Script::File(f.clone())),
                    None => {
                        eprint!("{}", usage());
                        return 1;
                    }
                }
            }
            b"-r" => {
                i += 1;
                match args.get(i) {
                    Some(c) => script = Some(Script::Code(c.clone())),
                    None => {
                        eprint!("{}", usage());
                        return 1;
                    }
                }
            }
            b"-B" | b"--process-begin" | b"-E" | b"--process-end" | b"-R" | b"--process-code"
            | b"-F" | b"--process-file" => {
                let opt = a.clone();
                i += 1;
                let Some(v) = args.get(i).cloned() else {
                    eprint!("{}", usage());
                    return 1;
                };
                let which = match opt.as_slice() {
                    b"-B" | b"--process-begin" => 'B',
                    b"-E" | b"--process-end" => 'E',
                    b"-R" | b"--process-code" => 'R',
                    _ => 'F',
                };
                match which {
                    'B' if begin.is_some() => {
                        println!("You can use -B only once.");
                        return 1;
                    }
                    'E' if end.is_some() => {
                        println!("You can use -E only once.");
                        return 1;
                    }
                    'R' | 'F' if each.is_some() => {
                        println!("You can use -R or -F only once.");
                        return 1;
                    }
                    'B' => begin = Some(v),
                    'E' => end = Some(v),
                    'R' => each = Some(Script::Code(v)),
                    _ => each = Some(Script::File(v)),
                }
            }
            b"-l" | b"--syntax-check" => mode = Mode::Lint,
            b"-v" | b"--version" => mode = Mode::Version,
            b"-m" | b"--modules" => mode = Mode::Modules,
            b"-i" | b"--info" => mode = Mode::Info,
            b"-h" | b"-?" | b"--help" | b"--usage" => mode = Mode::Help,
            b"-" => script = Some(Script::Stdin),
            _ if a.starts_with(b"-d") => ini.push(split_ini(&a[2..])),
            _ if a.starts_with(b"-") && a.len() > 1 => {
                eprintln!(
                    "Error in argument {}, char 2: option not found {}",
                    i + 1,
                    String::from_utf8_lossy(&a[1..2])
                );
                eprint!("{}", usage());
                return 1;
            }
            _ => script = Some(Script::File(a.clone())),
        }
        i += 1;
    }
    match mode {
        Mode::Help => {
            print!("{}", usage());
            0
        }
        Mode::Version => {
            println!(
                "PHP 8.5.4 (cli) (goro {})\nCopyright (c) The PHP Group\nZend Engine v4.5.4, goro-rs clean-room implementation",
                env!("CARGO_PKG_VERSION")
            );
            0
        }
        Mode::Modules => {
            let mut names: Vec<&str> = Vec::new();
            for m in goro_engine::builtins::modules() {
                if m.listed && !names.contains(&m.name) {
                    names.push(m.name);
                }
            }
            names.sort_by_key(|n| n.to_ascii_lowercase());
            println!("[PHP Modules]");
            for n in names {
                println!("{}", n);
            }
            println!("\n[Zend Modules]\n");
            0
        }
        Mode::Info => {
            let code = b"phpinfo();".to_vec();
            goro_engine::run(RunOptions {
                script: Script::Code(code),
                ini,
                args: rest,
                modules: Vec::new(),
                lines: None,
            })
        }
        Mode::Lint => {
            // Every file argument is checked.
            match script {
                Some(Script::File(f)) => {
                    lint_files.push(f);
                    lint_files.extend(rest);
                    let mut status = 0;
                    for f in lint_files {
                        let s = lint(Some(Script::File(f)));
                        if s != 0 {
                            status = s;
                        }
                    }
                    status
                }
                other => lint(other),
            }
        }
        Mode::Run => {
            let lines = if begin.is_some() || each.is_some() || end.is_some() {
                Some(goro_engine::LineMode { begin, each, end })
            } else {
                None
            };
            let script = script.unwrap_or(Script::Stdin);
            goro_engine::run(RunOptions {
                script,
                ini,
                args: rest,
                modules: Vec::new(),
                lines,
            })
        }
    }
}

fn split_ini(v: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match v.iter().position(|&c| c == b'=') {
        Some(p) => {
            let mut val = v[p + 1..].to_vec();
            // Quoted values lose their quotes; elsewhere `;` starts a
            // comment, as in php.ini (observed on PHP 8.5:
            // `-d 'precision=5 ; x'` gives "5").
            if val.len() >= 2 && val[0] == b'"' && val[val.len() - 1] == b'"' {
                val = val[1..val.len() - 1].to_vec();
            } else if val.first() != Some(&b'"')
                && let Some(c) = val.iter().position(|&c| c == b';')
            {
                val.truncate(c);
                while val.last().is_some_and(|c| c.is_ascii_whitespace()) {
                    val.pop();
                }
            }
            (v[..p].to_vec(), val)
        }
        None => (v.to_vec(), b"1".to_vec()),
    }
}

fn lint(script: Option<Script>) -> i32 {
    let (src, name) = match script {
        Some(Script::File(f)) => {
            let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&f));
            match std::fs::read(p) {
                Ok(s) => (s, String::from_utf8_lossy(&f).into_owned()),
                Err(_) => {
                    eprintln!("Could not open input file: {}", String::from_utf8_lossy(&f));
                    return 1;
                }
            }
        }
        Some(Script::Code(_)) => {
            eprintln!("Either execute direct code, process stdin or lint php files.");
            return 1;
        }
        _ => {
            use std::io::Read;
            let mut s = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut s);
            (s, "Standard input code".to_string())
        }
    };
    let mut vm = goro_engine::Vm::new(&goro_engine::builtins::modules());
    match vm.compile_source(&src, name.as_bytes(), false) {
        Ok(_) => {
            println!("No syntax errors detected in {}", name);
            0
        }
        Err(e) => {
            let (label, msg) = match e.message.strip_prefix('\u{1}') {
                Some(m) => ("Parse error", m.to_string()),
                None => ("Fatal error", e.message.clone()),
            };
            println!("\n{}: {} in {} on line {}", label, msg, name, e.line);
            println!("Errors parsing {}", name);
            255
        }
    }
}
