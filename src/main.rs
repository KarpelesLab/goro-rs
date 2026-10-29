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
    "Usage: goro [options] [-f] <file> [--] [args...]
   goro [options] -r <code> [--] [args...]
   goro [options] [-B <begin_code>] -R <code> [-E <end_code>] [--] [args...]
   goro [options] [-B <begin_code>] -F <file> [-E <end_code>] [--] [args...]
   goro [options] -- [args...]
   goro [options] -a

  -a               Run as interactive shell (requires readline extension)
  -c <path>|<file> Look for php.ini file in this directory
  -n               No configuration (ini) files will be used
  -d foo[=bar]     Define INI entry foo with value 'bar'
  -e               Generate extended information for debugger/profiler
  -f <file>        Parse and execute <file>.
  -h               This help
  -i               PHP information
  -l               Syntax check only (lint)
  -m               Show compiled in modules
  -r <code>        Run PHP <code> without using script tags <?..?>
  -B <begin_code>  Run PHP <begin_code> before processing input lines
  -R <code>        Run PHP <code> for every input line
  -F <file>        Parse and execute <file> for every input line
  -E <end_code>    Run PHP <end_code> after processing all input lines
  -H               Hide any passed arguments from external tools.
  -s               Output HTML syntax highlighted source.
  -v               Version number
  -w               Output source with stripped comments and whitespace.
  -z <file>        Load Zend extension <file>.

  args...          Arguments passed to script. Use -- args when first argument
                   starts with - or script is read from stdin

  --ini            Show configuration file names

  --rf <name>      Show information about function <name>.
  --rc <name>      Show information about class <name>.
  --re <name>      Show information about extension <name>.
  --rz <name>      Show information about Zend extension <name>.
  --ri <name>      Show configuration for extension <name>.

  --cgi            Run as the CGI SAPI (as when invoked as php-cgi); see
                   `goro --cgi -h`.

"
}

fn cgi_usage() -> &'static str {
    "Usage: goro --cgi [-q] [-h] [-s] [-v] [-i] [-f <file>]
       goro --cgi <file> [args...]
  -C               Do not chdir to the script's directory
  -c <path>|<file> Look for php.ini file in this directory
  -n               No php.ini file will be used
  -d foo[=bar]     Define INI entry foo with value 'bar'
  -f <file>        Parse and execute <file>.
  -h               This help
  -i               PHP information
  -l               Syntax check only (lint)
  -m               Show compiled in modules
  -q               Quiet-mode.  Suppress HTTP header output.
  -s               Display colour syntax highlighted source.
  -v               Version number
  -w               Display source with stripped comments and whitespace.

The request comes from the CGI environment (RFC 3875: REQUEST_METHOD,
QUERY_STRING, CONTENT_TYPE, CONTENT_LENGTH, HTTP_COOKIE, HTTP_*,
SCRIPT_FILENAME or PATH_TRANSLATED when no file is given) and the body
from stdin; the response is CGI header lines, an empty line and the body.
"
}

/// The CGI SAPI's command line (php-cgi's options).
fn cgi(args: Vec<OsString>) -> i32 {
    let args: Vec<Vec<u8>> = args.into_iter().map(|a| a.into_vec()).collect();
    let mut opts = goro_engine::cgi::CgiOptions::default();
    let mut ini_file: Option<Vec<u8>> = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == b"--" {
            i += 1;
            break;
        }
        if !a.starts_with(b"-") || a.len() < 2 {
            break;
        }
        let mut k = 1;
        while k < a.len() {
            let c = a[k];
            if b"cdfz".contains(&c) {
                let v = if k + 1 < a.len() {
                    a[k + 1..].to_vec()
                } else {
                    i += 1;
                    match args.get(i) {
                        Some(v) => v.clone(),
                        None => {
                            print!("{}", cgi_usage());
                            return 1;
                        }
                    }
                };
                match c {
                    b'c' => ini_file = Some(v),
                    b'd' => opts.ini.push(split_ini(&v)),
                    b'f' => opts.script = Some(v),
                    _ => {}
                }
                break;
            }
            match c {
                b'n' | b'e' | b'H' | b'T' => {}
                b'q' => opts.no_headers = true,
                b'C' => opts.no_chdir = true,
                b'h' | b'?' => {
                    print!("{}", cgi_usage());
                    return 0;
                }
                b'v' => {
                    println!(
                        "PHP 8.5.4 (cgi-fcgi) (built: {}) (NTS)\nCopyright (c) The PHP Group\nZend Engine v4.5.4, Copyright (c) Zend Technologies\n    with goro-rs {}, a clean-room implementation",
                        BUILT,
                        env!("CARGO_PKG_VERSION")
                    );
                    return 0;
                }
                b'i' | b'l' | b'm' | b's' | b'w' => {
                    // As the CLI does them (without the CGI headers).
                    let mut cli_args: Vec<OsString> = Vec::new();
                    if let Some(f) = &ini_file {
                        cli_args.push("-c".into());
                        cli_args.push(OsString::from_vec(f.clone()));
                    }
                    for (k, v) in &opts.ini {
                        let mut d = k.clone();
                        d.push(b'=');
                        d.extend_from_slice(v);
                        cli_args.push("-d".into());
                        cli_args.push(OsString::from_vec(d));
                    }
                    cli_args.push(OsString::from_vec(vec![b'-', c]));
                    if let Some(f) = &opts.script {
                        cli_args.push(OsString::from_vec(f.clone()));
                    }
                    cli_args.extend(args[i + 1..].iter().map(|a| OsString::from_vec(a.clone())));
                    return cli(cli_args);
                }
                _ => {
                    print!("{}", cgi_usage());
                    return 1;
                }
            }
            k += 1;
        }
        i += 1;
    }
    let mut rest: Vec<Vec<u8>> = args[i..].to_vec();
    if opts.script.is_none() && !rest.is_empty() {
        opts.script = Some(rest.remove(0));
    }
    opts.args = rest;
    if let Some(f) = &ini_file {
        opts.ini = with_ini_file(f, std::mem::take(&mut opts.ini));
    }
    goro_engine::cgi::run(opts)
}

/// The build date, for `-v` (from SOURCE_DATE_EPOCH-free compile time is not
/// available without a build script: a fixed text).
const BUILT: &str = "Jan  1 2026 00:00:00";

/// Counts heap use for memory_limit (PHP manual: memory_limit).
#[global_allocator]
static ALLOC: goro_runtime::mem::CountingAlloc = goro_runtime::mem::CountingAlloc;

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
    // The CGI SAPI: `goro --cgi ...`, or goro run as php-cgi.
    let as_cgi = std::env::args_os().next().is_some_and(|a0| {
        std::path::Path::new(&a0)
            .file_name()
            .is_some_and(|n| n.as_bytes().starts_with(b"php-cgi"))
    });
    let (cgi_mode, args) = if args.first().is_some_and(|a| a == "--cgi") {
        (true, args[1..].to_vec())
    } else {
        (as_cgi, args)
    };
    let code = std::thread::Builder::new()
        .name("php".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let pid = std::process::id();
            let code = if cgi_mode { cgi(args) } else { cli(args) };
            // A child of pcntl_fork() has no main thread to return to.
            if std::process::id() != pid {
                std::process::exit(code);
            }
            code
        })
        .expect("cannot start engine thread");
    // Process signals go to the engine thread (ext/pcntl handlers).
    goro_engine::builtins::pcntl::block_signals_in_this_thread();
    let code = code.join().unwrap_or(255);
    std::process::exit(code);
}

enum Mode {
    Run,
    Lint,
    Version,
    Modules,
    Info,
    Help,
    Strip,
    Highlight,
    /// --rf, --rc, --re, --rz, --ri: (option letter, name)
    Reflect(char, Vec<u8>),
}

/// Options taking a value (PHP manual: Command line options).
const SHORT_WITH_ARG: &[u8] = b"cdfrBRFEztS";
const SHORT_FLAGS: &[u8] = b"aCehHilmnqsvw?";

/// Long option names and the short option they stand for (`'1'`..: the
/// reflection options).
const LONG: &[(&str, char)] = &[
    ("php-ini", 'c'),
    ("no-php-ini", 'n'),
    ("define", 'd'),
    ("info", 'i'),
    ("syntax-check", 'l'),
    ("modules", 'm'),
    ("run", 'r'),
    ("process-begin", 'B'),
    ("process-code", 'R'),
    ("process-file", 'F'),
    ("process-end", 'E'),
    ("hide-args", 'H'),
    ("version", 'v'),
    ("help", 'h'),
    ("usage", 'h'),
    ("strip", 'w'),
    ("syntax-highlight", 's'),
    ("syntax-highlighting", 's'),
    ("zend-extension", 'z'),
    ("interactive", 'a'),
    ("no-chdir", 'C'),
    ("profile-info", 'e'),
    ("no-header", 'q'),
    ("rf", '1'),
    ("rfunction", '1'),
    ("rc", '2'),
    ("rclass", '2'),
    ("re", '3'),
    ("rextension", '3'),
    ("rz", '4'),
    ("rzendextension", '4'),
    ("ri", '5'),
    ("rextinfo", '5'),
    ("ini", '6'),
];

fn takes_arg(c: char) -> bool {
    SHORT_WITH_ARG.contains(&(c as u8)) || matches!(c, '1'..='5')
}

/// A PHP string literal for `s`.
fn php_quote(s: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &c in s {
        if c == b'\'' || c == b'\\' {
            out.push(b'\\');
        }
        out.push(c);
    }
    out.push(b'\'');
    out
}

fn opt_error(i: usize, what: &str) -> i32 {
    // getopt-style message, then the usage (observed on PHP 8.5; test
    // sapi/cli/tests/bug78323).
    println!("Error in argument {}, char 2: {}", i, what);
    print!("{}", usage());
    1
}

fn cli(args: Vec<OsString>) -> i32 {
    let args: Vec<Vec<u8>> = args.into_iter().map(|a| a.into_vec()).collect();
    let mut ini: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut ini_file: Option<Vec<u8>> = None;
    let mut script: Option<Script> = None;
    let mut rest: Vec<Vec<u8>> = Vec::new();
    let mut mode = Mode::Run;
    let mut begin: Option<Vec<u8>> = None;
    let mut each: Option<Script> = None;
    let mut end: Option<Vec<u8>> = None;
    let mut interactive = false;
    let mut server: Option<Vec<u8>> = None;
    let mut docroot: Option<Vec<u8>> = None;
    let mut positional = false;
    let mut i = 0;
    'args: while i < args.len() {
        let a = args[i].clone();
        // After the script file (or -f/-r and a non-option), the rest goes
        // to the script.
        if positional
            || (script.is_some() && !a.starts_with(b"-"))
            || matches!(mode, Mode::Reflect(..))
        {
            // Everything after the script goes to the script.
            if a == b"--" && rest.is_empty() {
                i += 1;
                continue;
            }
            rest.extend(args[i..].iter().cloned());
            break;
        }
        if a == b"--" {
            rest.extend(args[i + 1..].iter().cloned());
            break;
        }
        if a == b"-" {
            script = Some(Script::Stdin);
            i += 1;
            continue;
        }
        if !a.starts_with(b"-") {
            script = Some(Script::File(a));
            positional = true;
            i += 1;
            continue;
        }
        // The options of this argument: (letter, attached value).
        let mut opts: Vec<(char, Option<Vec<u8>>)> = Vec::new();
        if let Some(long) = a.strip_prefix(b"--") {
            let (name, val) = match long.iter().position(|&c| c == b'=') {
                Some(p) => (&long[..p], Some(long[p + 1..].to_vec())),
                None => (long, None),
            };
            let name = String::from_utf8_lossy(name).into_owned();
            // goro's own: memory_limit=-1 without the 4 GB backstop.
            if name == "unlimited-mem" && val.is_none() {
                goro_engine::vm::setup::set_unlimited_memory(true);
                i += 1;
                continue;
            }
            match LONG.iter().find(|(n, _)| *n == name) {
                Some((_, c)) => opts.push((*c, val)),
                None => {
                    return opt_error(
                        i + 1,
                        &if val.is_some() {
                            format!("no argument for option {}", name)
                        } else {
                            format!("option not found {}", name)
                        },
                    );
                }
            }
        } else {
            let mut k = 1;
            while k < a.len() {
                let c = a[k] as char;
                if c == ':' {
                    return opt_error(i + 1, ": in flags");
                }
                if takes_arg(c) {
                    let attached = (k + 1 < a.len()).then(|| a[k + 1..].to_vec());
                    opts.push((c, attached));
                    break;
                }
                if !SHORT_FLAGS.contains(&a[k]) {
                    return opt_error(i + 1, &format!("option not found {}", c));
                }
                opts.push((c, None));
                k += 1;
            }
        }
        for (c, attached) in opts {
            let val = if takes_arg(c) {
                match attached {
                    Some(v) => Some(v),
                    None => {
                        i += 1;
                        match args.get(i) {
                            Some(v) => Some(v.clone()),
                            None => {
                                return opt_error(i, &format!("no argument for option {}", c));
                            }
                        }
                    }
                }
            } else {
                None
            };
            match c {
                'n' | 'q' | 'H' | 'C' | 'e' | 'z' => {}
                'c' => ini_file = val,
                'd' => ini.push(split_ini(&val.unwrap())),
                'f' => {
                    if matches!(script, Some(Script::File(_))) {
                        println!("You can use -f only once.");
                        return 1;
                    }
                    script = Some(Script::File(val.unwrap()));
                }
                'r' => {
                    if matches!(script, Some(Script::Code(_))) {
                        println!("You can use -r only once.");
                        return 1;
                    }
                    script = Some(Script::Code(val.unwrap()));
                }
                'B' | 'E' | 'R' | 'F' => {
                    let v = val.unwrap();
                    match c {
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
                'l' => mode = Mode::Lint,
                'v' => mode = Mode::Version,
                'm' => mode = Mode::Modules,
                'i' => mode = Mode::Info,
                'h' | '?' => mode = Mode::Help,
                'w' => mode = Mode::Strip,
                's' => mode = Mode::Highlight,
                'a' => interactive = true,
                'S' => server = val,
                't' => docroot = val,
                '6' => {
                    // --ini: no configuration file is read by default.
                    println!(
                        "Configuration File (php.ini) Path: \nLoaded Configuration File:         (none)\nScan for additional .ini files in: (none)\nAdditional .ini files parsed:      (none)"
                    );
                    return 0;
                }
                '1'..='5' => {
                    mode = Mode::Reflect(c, val.unwrap());
                    i += 1;
                    continue 'args;
                }
                _ => {}
            }
        }
        i += 1;
    }
    // php.ini given with -c: its settings come before -d ones.
    if let Some(f) = &ini_file {
        ini = with_ini_file(f, ini);
    }
    if let Some(addr) = server
        && matches!(mode, Mode::Run)
    {
        // The built-in web server (PHP manual: Built-in web server).
        let router = match script {
            Some(Script::File(f)) => Some(f),
            _ => None,
        };
        return goro_engine::cli_server::serve(goro_engine::cli_server::ServerOptions {
            addr,
            docroot,
            router,
            ini,
        });
    }
    if interactive && matches!(mode, Mode::Run) {
        println!("Interactive shell (-a) requires the readline extension.");
        return 1;
    }
    let run_code = |code: Vec<u8>, ini: Vec<(Vec<u8>, Vec<u8>)>, args: Vec<Vec<u8>>| {
        goro_engine::run(RunOptions {
            script: Script::Code(code),
            ini,
            args,
            modules: Vec::new(),
            lines: None,
        })
    };
    match mode {
        Mode::Help => {
            print!("{}", usage());
            0
        }
        Mode::Version => {
            println!(
                "PHP 8.5.4 (cli) (built: {}) (NTS)\nCopyright (c) The PHP Group\nZend Engine v4.5.4, Copyright (c) Zend Technologies\n    with goro-rs {}, a clean-room implementation",
                BUILT,
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
        Mode::Info => run_code(b"phpinfo();".to_vec(), ini, rest),
        Mode::Reflect(c, name) => {
            // (PHP manual: Command line options, --rf/--rc/--re/--rz/--ri)
            let q = php_quote(&name);
            let mut code = Vec::new();
            match c {
                '1' | '2' | '3' => {
                    let cls = match c {
                        '1' => "ReflectionFunction",
                        '2' => "ReflectionClass",
                        _ => "ReflectionExtension",
                    };
                    code.extend_from_slice(format!("try {{ echo new {}(", cls).as_bytes());
                    code.extend_from_slice(&q);
                    code.extend_from_slice(b"), \"\\n\"; } catch (ReflectionException $e) { echo 'Exception: ', $e->getMessage(), \"\\n\"; }");
                }
                '4' => {
                    code.extend_from_slice(b"echo 'Exception: Zend Extension ', ");
                    code.extend_from_slice(&q);
                    code.extend_from_slice(b", \" does not exist\\n\";");
                }
                _ => {
                    code.extend_from_slice(b"if (!extension_loaded(");
                    code.extend_from_slice(&q);
                    code.extend_from_slice(b")) { echo \"Extension '\", ");
                    code.extend_from_slice(&q);
                    code.extend_from_slice(
                        b", \"' not present.\\n\"; } else { (new ReflectionExtension(",
                    );
                    code.extend_from_slice(&q);
                    code.extend_from_slice(b"))->info(); }");
                }
            }
            run_code(code, ini, rest)
        }
        Mode::Strip | Mode::Highlight => {
            let f = match script {
                Some(Script::File(f)) => f,
                Some(Script::Stdin) | None => b"php://stdin".to_vec(),
                Some(Script::Code(_)) => {
                    eprintln!("Either execute direct code, process stdin or lint php files.");
                    return 1;
                }
            };
            if f != b"php://stdin"
                && !std::path::Path::new(std::ffi::OsStr::from_bytes(&f)).exists()
            {
                eprintln!("Could not open input file: {}", String::from_utf8_lossy(&f));
                return 1;
            }
            let mut code = if matches!(mode, Mode::Strip) {
                b"echo php_strip_whitespace(".to_vec()
            } else {
                b"highlight_file(".to_vec()
            };
            code.extend_from_slice(&php_quote(&f));
            code.extend_from_slice(b");");
            run_code(code, ini, rest)
        }
        Mode::Lint => {
            // Every file argument is checked.
            match script {
                Some(Script::File(f)) => {
                    let mut lint_files = vec![f];
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

/// The settings of a php.ini given with -c (a file, or a directory holding
/// php.ini), followed by `ini` (sections are for other SAPIs; test
/// sapi/cli/tests/023).
fn with_ini_file(f: &[u8], ini: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(f));
    let p = if p.is_dir() {
        p.join("php.ini")
    } else {
        p.to_path_buf()
    };
    let Ok(text) = std::fs::read(&p) else {
        return ini;
    };
    let mut from_file = Vec::new();
    for line in text.split(|&c| c == b'\n') {
        let line = line.trim_ascii();
        if line.starts_with(b"[") {
            break;
        }
        if line.is_empty() || line.starts_with(b";") || line.starts_with(b"#") {
            continue;
        }
        if let Some(p) = line.iter().position(|&c| c == b'=') {
            let k = line[..p].trim_ascii().to_vec();
            let mut v = line[p + 1..].trim_ascii().to_vec();
            if v.len() >= 2 && (v[0] == b'"' || v[0] == b'\'') && v[v.len() - 1] == v[0] {
                v = v[1..v.len() - 1].to_vec();
            } else if let Some(c) = v.iter().position(|&c| c == b';') {
                v.truncate(c);
                while v.last().is_some_and(|c| c.is_ascii_whitespace()) {
                    v.pop();
                }
            }
            from_file.push((k, v));
        }
    }
    from_file.extend(ini);
    from_file
}

fn split_ini(v: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match v.iter().position(|&c| c == b'=') {
        Some(p) => {
            let mut val = v[p + 1..].to_vec();
            // Quoted values lose their quotes; elsewhere `;` starts a
            // comment, as in php.ini (observed on PHP 8.5:
            // `-d 'precision=5 ; x'` gives "5").
            if val.len() >= 2 && val[0] == b'"' && val[val.len() - 1] == b'"' {
                // `\"`, `\\` and `\$` are escapes, other backslashes stay
                // (observed on PHP 8.5; test mail_variation4.phpt sets
                // sendmail_path="kill \$\$")
                let inner = &val[1..val.len() - 1];
                let mut out = Vec::with_capacity(inner.len());
                let mut i = 0;
                while i < inner.len() {
                    if inner[i] == b'\\' && matches!(inner.get(i + 1), Some(b'"' | b'\\' | b'$')) {
                        i += 1;
                    }
                    out.push(inner[i]);
                    i += 1;
                }
                val = out;
            } else if val.first() != Some(&b'"') {
                if let Some(c) = val.iter().position(|&c| c == b';') {
                    val.truncate(c);
                }
                while val.last().is_some_and(|c| c.is_ascii_whitespace()) {
                    val.pop();
                }
                // bare boolean words are "1" or "" (observed on PHP 8.5:
                // `-d display_errors=on` reads back as "1"; test
                // auto_detect_line_endings_1.phpt)
                match val.to_ascii_lowercase().as_slice() {
                    b"on" | b"yes" | b"true" => val = b"1".to_vec(),
                    b"off" | b"no" | b"false" | b"none" | b"null" => val.clear(),
                    _ => {}
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
