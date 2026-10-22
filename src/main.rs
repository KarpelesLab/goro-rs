//! The command line SAPI (sapi/cli/php_cli.c).

use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process;

use goro_core::vm::Vm;

mod cli_server;
mod engine;
mod getopt;
#[allow(dead_code)] // the ported parser also handles responses
mod http_parser;
mod mime_types;

use engine::IniSettings;
use getopt::{GETOPT_INVALID_ARG, GetOpt};

const PHP_VERSION: &str = "8.5.4";
const ZEND_VERSION: &str = "4.5.4";

const HARDCODED_INI: &str = "html_errors=0\n\
implicit_flush=1\n\
output_buffering=0\n\
max_execution_time=0\n\
max_input_time=-1\n";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Mode {
    Standard,
    Highlight,
    Lint,
    Strip,
    CliDirect,
    ProcessStdin,
    ReflectionFunction,
    ReflectionClass,
    ReflectionExtension,
    ReflectionZendExtension,
    ReflectionExtInfo,
    ShowIniConfig,
    ShowIniDiff,
}

fn main() {
    // HARD memory limit: 2GB virtual address space.
    // This MUST be set before anything else to prevent OOM kills.
    // Without this, runaway PHP scripts can allocate 100GB+ and crash the machine.
    // Only the soft limit is lowered (the hard limit is kept) so that helper
    // processes such as the test runner's embedded MySQL server can be
    // started without it.
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" {
            fn getrlimit(resource: i32, rlim: *mut [u64; 2]) -> i32;
            fn setrlimit(resource: i32, rlim: *const [u64; 2]) -> i32;
        }
        let limit: u64 = 2 * 1024 * 1024 * 1024; // 2GB
        let mut cur: [u64; 2] = [u64::MAX, u64::MAX];
        getrlimit(9, &mut cur); // 9 = RLIMIT_AS on Linux
        let rlim: [u64; 2] = [limit.min(cur[1]), cur[1]]; // [soft, hard]
        setrlimit(9, &rlim);
        // Ignore SIGPIPE so that writes to closed sockets/pipes fail
        // instead of killing the process (php_cli.c main()).
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let args: Vec<Vec<u8>> = std::env::args_os().map(|a| a.as_bytes().to_vec()).collect();

    // goro's own test runner mode.
    if args.len() >= 2 && args[1] == b"--test" {
        run_tests(&args);
    }

    process::exit(cli_main(args));
}

fn run_tests(args: &[Vec<u8>]) -> ! {
    if args.len() < 3 {
        eprintln!("Error: --test requires a directory argument");
        process::exit(1);
    }
    let dir = Path::new(std::ffi::OsStr::from_bytes(&args[2]));
    let c = goro_phpt::run_test_dir(dir);
    println!("\n=== Test Results ===");
    println!("Pass:  {}", c.pass);
    println!("Fail:  {}", c.fail);
    println!("Skip:  {}", c.skip);
    println!("Error: {}", c.error);
    // Expected failures (--XFAIL--) are not failures, like in
    // run-tests.php; XPass: XFAIL tests that passed.
    println!("XFail: {}", c.xfail);
    println!("XPass: {}", c.xpass);
    println!("Total: {}", c.total());
    if c.fail > 0 || c.error > 0 {
        process::exit(1);
    }
    process::exit(0);
}

fn s(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const USAGE: &str = r#"Usage: {p} [options] [-f] <file> [--] [args...]
   {p} [options] -r <code> [--] [args...]
   {p} [options] [-B <begin_code>] -R <code> [-E <end_code>] [--] [args...]
   {p} [options] [-B <begin_code>] -F <file> [-E <end_code>] [--] [args...]
   {p} [options] -S <addr>:<port> [-t docroot] [router]
   {p} [options] -- [args...]
   {p} [options] -a

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
  -S <addr>:<port> Run with built-in web server.
  -t <docroot>     Specify document root <docroot> for built-in web server.
  -s               Output HTML syntax highlighted source.
  -v               Version number
  -w               Output source with stripped comments and whitespace.

  args...          Arguments passed to script. Use -- args when first argument
                   starts with - or script is read from stdin

  --ini            Show configuration file names
  --ini=diff       Show INI entries that differ from the built-in default

  --rf <name>      Show information about function <name>.
  --rc <name>      Show information about class <name>.
  --re <name>      Show information about extension <name>.
  --rz <name>      Show information about Zend extension <name>.
  --ri <name>      Show configuration for extension <name>.

  --repeat <count> Repeat script execution <count> times.
                   For internal purposes only.

"#;

/// php_cli_usage()
fn usage(argv0: &[u8]) {
    let a = s(argv0);
    let prog = match a.rfind('/') {
        Some(p) => &a[p + 1..],
        None => "php",
    };
    let text = USAGE.replace("{p}", prog);
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(text.as_bytes());
    let _ = so.flush();
}

/// php_get_version()
fn version_string(sapi: &str) -> String {
    format!(
        "PHP {} ({}) (built: {}) (NTS)\nCopyright (c) The PHP Group\nZend Engine v{}, Copyright (c) Zend Technologies\n",
        PHP_VERSION,
        sapi,
        build_date(),
        ZEND_VERSION
    )
}

/// php_build_date: "Mon DD YYYY HH:MM:SS" of the binary.
fn build_date() -> String {
    let secs = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {:2} {} {:02}:{:02}:{:02}",
        MONTHS[(m - 1) as usize],
        d,
        y,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// php_ini_builder_define(): one -d definition as an INI line.
fn ini_define_line(arg: &[u8], out: &mut Vec<u8>) {
    match arg.iter().position(|&c| c == b'=') {
        Some(eq) => {
            let val = &arg[eq + 1..];
            let first = val.first().copied().unwrap_or(0);
            if !first.is_ascii_alphanumeric() && first != b'"' && first != b'\'' && first != 0 {
                // php_ini_builder_quoted()
                out.extend_from_slice(&arg[..eq]);
                out.extend_from_slice(b"=\"");
                out.extend_from_slice(val);
                out.extend_from_slice(b"\"\n");
            } else {
                out.extend_from_slice(arg);
                out.push(b'\n');
            }
        }
        None => {
            out.extend_from_slice(arg);
            out.extend_from_slice(b"=1\n");
        }
    }
}

/// php_init_config(): locate and parse php.ini (unless -n was given).
fn load_php_ini(ini_path_override: Option<&[u8]>, ini_ignore: bool, ini: &mut IniSettings) {
    if ini_ignore {
        return;
    }
    let mut search: Vec<std::path::PathBuf> = Vec::new();
    if let Some(p) = ini_path_override {
        search.push(Path::new(std::ffi::OsStr::from_bytes(p)).to_path_buf());
    } else if let Ok(rc) = std::env::var("PHPRC") {
        search.push(rc.into());
    }
    for p in search {
        let candidates: Vec<std::path::PathBuf> = if p.is_dir() {
            vec![p.join("php-cli.ini"), p.join("php.ini")]
        } else {
            vec![p.clone()]
        };
        for c in candidates {
            if c.is_file() {
                if let Ok(data) = std::fs::read(&c) {
                    let full = std::fs::canonicalize(&c).unwrap_or(c.clone());
                    let opened = full.to_string_lossy().into_owned();
                    goro_ext_standard::misc::set_ini_opened_path(Some(opened.clone()));
                    INI_OPENED_PATH.with(|o| *o.borrow_mut() = Some(opened));
                    engine::parse_ini_text(&data, ini);
                    return;
                }
            }
        }
    }
}

thread_local! {
    static INI_OPENED_PATH: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn cli_main(mut argv: Vec<Vec<u8>>) -> i32 {
    let argv0 = argv.first().cloned().unwrap_or_else(|| b"php".to_vec());
    let mut ini_path_override: Option<Vec<u8>> = None;
    let mut ini_ignore = false;
    let mut defines: Vec<u8> = Vec::new();
    let mut server_mode = false;

    {
        let mut g = GetOpt::new(&argv, true);
        let mut optarg = None;
        loop {
            let c = g.next(&mut optarg);
            if c == -1 {
                break;
            }
            match c {
                c if c == 'c' as i32 => ini_path_override = optarg.take(),
                c if c == 'n' as i32 => ini_ignore = true,
                c if c == 'd' as i32 => {
                    if let Some(a) = optarg.take() {
                        ini_define_line(&a, &mut defines);
                    }
                }
                c if c == 'S' as i32 => server_mode = true,
                c if c == 'h' as i32 || c == '?' as i32 => {
                    usage(&argv0);
                    return 0;
                }
                GETOPT_INVALID_ARG => {
                    usage(&argv0);
                    return 1;
                }
                c if c == 'i' as i32 || c == 'v' as i32 || c == 'm' as i32 => {
                    server_mode = false;
                    break;
                }
                _ => {}
            }
        }
    }

    // Configuration: SAPI defaults, php.ini, then the INI entries
    // (hard-coded CLI settings and -d definitions).
    // (html_errors defaults to on in main.c; the CLI's hard-coded INI turns
    // it off.)
    let mut ini: IniSettings = vec![
        ("html_errors".to_string(), "1".to_string()),
        ("display_errors".to_string(), "1".to_string()),
    ];
    load_php_ini(ini_path_override.as_deref(), ini_ignore, &mut ini);
    let mut entries: Vec<u8> = Vec::new();
    if !server_mode {
        entries.extend_from_slice(HARDCODED_INI.as_bytes());
    }
    entries.extend_from_slice(&defines);
    engine::parse_ini_text(&entries, &mut ini);

    if server_mode {
        return cli_server::do_cli_server(&argv, ini);
    }
    do_cli(&mut argv, &ini)
}

const PARAM_MODE_CONFLICT: &str = "Either execute direct code, process stdin or use a file.\n";

fn puts(text: &str) {
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(text.as_bytes());
    let _ = so.flush();
}

/// Write the engine's pending output to stdout.
fn flush_output(vm: &mut Vm) {
    let out = vm.take_output();
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(&out);
    let _ = so.flush();
}

fn do_cli(argv: &mut [Vec<u8>], ini: &IniSettings) -> i32 {
    let argc = argv.len();
    // First pass: -i, -v and -m.
    {
        let mut g = GetOpt::new(argv, false);
        let mut optarg = None;
        loop {
            let c = g.next(&mut optarg);
            if c == -1 {
                break;
            }
            if c == 'i' as i32 {
                let mut vm = engine::new_vm(ini, "cli");
                let info = cli_request_info(b"Standard input code", b"", &[]);
                engine::request_startup(&mut vm, &info, ini);
                run_snippet(&mut vm, "phpinfo(INFO_ALL & ~INFO_CREDITS);");
                flush_output(&mut vm);
                return 0;
            } else if c == 'v' as i32 {
                puts(&version_string("cli"));
                return 0;
            } else if c == 'm' as i32 {
                let mut vm = engine::new_vm(ini, "cli");
                let info = cli_request_info(b"Standard input code", b"", &[]);
                engine::request_startup(&mut vm, &info, ini);
                run_snippet(
                    &mut vm,
                    "$m = get_loaded_extensions(); usort($m, 'strcasecmp'); \
                     echo \"[PHP Modules]\\n\"; foreach ($m as $n) echo $n, \"\\n\"; \
                     echo \"\\n[Zend Modules]\\n\\n\";",
                );
                flush_output(&mut vm);
                return 0;
            }
        }
    }

    let mut mode = Mode::Standard;
    let mut interactive = false;
    let mut script_file: Option<Vec<u8>> = None;
    let mut exec_direct: Option<Vec<u8>> = None;
    let mut exec_run: Option<Vec<u8>> = None;
    let mut exec_begin: Option<Vec<u8>> = None;
    let mut exec_end: Option<Vec<u8>> = None;
    let mut reflection_what: Option<Vec<u8>> = None;
    let mut param_error: Option<&str> = None;
    let mut hide_argv = false;
    let mut num_repeats: i64 = 1;
    let mut exit_status: i32 = 0;

    let mut g = GetOpt::new(argv, false);
    let mut optarg: Option<Vec<u8>> = None;
    loop {
        let c = g.next(&mut optarg);
        if c == -1 {
            break;
        }
        let arg = optarg.take();
        let ch = if (0..128).contains(&c) {
            c as u8 as char
        } else {
            '\0'
        };
        match (c, ch) {
            (_, 'a') => {
                // goro has no readline extension
                param_error = Some("Interactive shell (-a) requires the readline extension.\n");
                let _ = &mut interactive;
            }
            (_, 'C') | (_, 'q') => {}
            (_, 'F') => {
                if mode == Mode::ProcessStdin {
                    if exec_run.is_some() || script_file.is_some() {
                        param_error = Some("You can use -R or -F only once.\n");
                        continue;
                    }
                } else if mode != Mode::Standard {
                    param_error = Some(PARAM_MODE_CONFLICT);
                    continue;
                }
                mode = Mode::ProcessStdin;
                script_file = arg;
            }
            (_, 'f') => {
                if mode == Mode::CliDirect || mode == Mode::ProcessStdin {
                    param_error = Some(PARAM_MODE_CONFLICT);
                    continue;
                } else if script_file.is_some() {
                    param_error = Some("You can use -f only once.\n");
                    continue;
                }
                script_file = arg;
            }
            (_, 'l') => {
                if mode != Mode::Standard {
                    continue;
                }
                mode = Mode::Lint;
                exit_status = 0;
            }
            (_, 'r') => {
                if mode == Mode::CliDirect {
                    if exec_direct.is_some() || script_file.is_some() {
                        param_error = Some("You can use -r only once.\n");
                        continue;
                    }
                } else if mode != Mode::Standard || interactive {
                    param_error = Some(PARAM_MODE_CONFLICT);
                    continue;
                }
                mode = Mode::CliDirect;
                exec_direct = arg;
            }
            (_, 'R') => {
                if mode == Mode::ProcessStdin {
                    if exec_run.is_some() || script_file.is_some() {
                        param_error = Some("You can use -R or -F only once.\n");
                        continue;
                    }
                } else if mode != Mode::Standard {
                    param_error = Some(PARAM_MODE_CONFLICT);
                    continue;
                }
                mode = Mode::ProcessStdin;
                exec_run = arg;
            }
            (_, 'B') => {
                if mode == Mode::ProcessStdin {
                    if exec_begin.is_some() {
                        param_error = Some("You can use -B only once.\n");
                        continue;
                    }
                } else if mode != Mode::Standard || interactive {
                    param_error = Some(PARAM_MODE_CONFLICT);
                    continue;
                }
                mode = Mode::ProcessStdin;
                exec_begin = arg;
            }
            (_, 'E') => {
                if mode == Mode::ProcessStdin {
                    if exec_end.is_some() {
                        param_error = Some("You can use -E only once.\n");
                        continue;
                    }
                } else if mode != Mode::Standard || interactive {
                    param_error = Some(PARAM_MODE_CONFLICT);
                    continue;
                }
                mode = Mode::ProcessStdin;
                exec_end = arg;
            }
            (_, 's') => {
                if mode == Mode::CliDirect || mode == Mode::ProcessStdin {
                    param_error = Some("Source highlighting only works for files.\n");
                    continue;
                }
                mode = Mode::Highlight;
            }
            (_, 'w') => {
                if mode == Mode::CliDirect || mode == Mode::ProcessStdin {
                    param_error = Some("Source stripping only works for files.\n");
                    continue;
                }
                mode = Mode::Strip;
            }
            (_, 'H') => hide_argv = true,
            (10, _) => {
                mode = Mode::ReflectionFunction;
                reflection_what = arg;
            }
            (11, _) => {
                mode = Mode::ReflectionClass;
                reflection_what = arg;
            }
            (12, _) => {
                mode = Mode::ReflectionExtension;
                reflection_what = arg;
            }
            (13, _) => {
                mode = Mode::ReflectionZendExtension;
                reflection_what = arg;
            }
            (14, _) => {
                mode = Mode::ReflectionExtInfo;
                reflection_what = arg;
            }
            (15, _) => match arg {
                Some(a) => {
                    if a == b"diff" {
                        mode = Mode::ShowIniDiff;
                    } else {
                        param_error = Some("Unknown argument for --ini\n");
                    }
                }
                None => mode = Mode::ShowIniConfig,
            },
            (16, _) => {
                num_repeats = s(&arg.unwrap_or_default()).trim().parse().unwrap_or(0);
            }
            _ => {}
        }
    }
    let mut optind = g.optind;
    let _ = hide_argv;

    if let Some(e) = param_error {
        puts(e);
        return 1;
    }

    if num_repeats > 1 {
        puts("Executing for the first time...\n");
    }

    loop {
        // do_repeat:
        if argc > optind
            && script_file.is_none()
            && mode != Mode::CliDirect
            && mode != Mode::ProcessStdin
            && argv[optind - 1] != b"--"
        {
            script_file = Some(argv[optind].clone());
            optind += 1;
        }
        let status = run_request(
            mode,
            ini,
            argv,
            optind,
            script_file.as_deref(),
            exec_direct.as_deref(),
            exec_begin.as_deref(),
            exec_run.as_deref(),
            exec_end.as_deref(),
            reflection_what.as_deref(),
            &mut exit_status,
        );
        if !status {
            exit_status = 1;
        }
        if mode == Mode::Lint && argc > optind && argv[optind] != b"--" {
            script_file = None;
            continue;
        }
        num_repeats -= 1;
        if num_repeats > 0 {
            puts("Finished execution, repeating...\n");
            continue;
        }
        break;
    }
    exit_status
}

/// The CLI request information: argv[0] = php_self, then the script
/// arguments.
#[allow(clippy::field_reassign_with_default)] // fields filled in step by step
fn cli_request_info(
    php_self: &[u8],
    script_filename: &[u8],
    rest: &[Vec<u8>],
) -> goro_ext_standard::request::RequestInfo {
    let mut info = goro_ext_standard::request::RequestInfo::default();
    info.script_filename = script_filename.to_vec();
    info.php_self = Some(php_self.to_vec());
    info.argv = std::iter::once(php_self.to_vec())
        .chain(rest.iter().cloned())
        .collect();
    info.env = std::env::vars_os()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect();
    info
}

/// Compile and run a piece of PHP code (without <?php) in the global scope.
fn run_snippet(vm: &mut Vm, code: &str) {
    let src = format!("<?php {}", code);
    if let Ok(mut c) = engine::compile(src.as_bytes(), "Command line code", false) {
        engine::load(vm, &mut c);
        let r = vm.execute(&c.op_array);
        finish_errors(vm, r);
    }
}

fn finish_errors(vm: &mut Vm, r: Result<goro_core::value::Value, goro_core::vm::VmError>) {
    if let Err(e) = r {
        if vm.current_exception.is_some() || !vm.fatal_emitted {
            vm.report_uncaught(&e);
        }
        vm.run_shutdown_destructors();
    }
}

/// Exit status of a finished request (EG(exit_status)).
fn request_exit_status(vm: &Vm, failed: bool) -> i32 {
    if vm.exit_requested {
        return vm.exit_code as i32;
    }
    if failed || vm.fatal_emitted { 255 } else { 0 }
}

fn read_script(path: &[u8]) -> Option<Vec<u8>> {
    match std::fs::read(std::ffi::OsStr::from_bytes(path)) {
        Ok(d) if !Path::new(std::ffi::OsStr::from_bytes(path)).is_dir() => Some(d),
        _ => {
            let mut se = std::io::stderr().lock();
            let _ = se.write_all(b"Could not open input file: ");
            let _ = se.write_all(path);
            let _ = se.write_all(b"\n");
            None
        }
    }
}

fn read_stdin() -> Vec<u8> {
    let mut v = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut v);
    v
}

/// One request of do_cli(). Returns false on a startup failure (exit 1).
#[allow(clippy::too_many_arguments)]
fn run_request(
    mode: Mode,
    ini: &IniSettings,
    argv: &[Vec<u8>],
    optind: usize,
    script_file: Option<&[u8]>,
    exec_direct: Option<&[u8]>,
    exec_begin: Option<&[u8]>,
    exec_run: Option<&[u8]>,
    exec_end: Option<&[u8]>,
    reflection_what: Option<&[u8]>,
    exit_status: &mut i32,
) -> bool {
    // The script source (file or stdin), read like cli_seek_file_begin().
    let mut source: Option<Vec<u8>> = None;
    let php_self: Vec<u8>;
    let mut translated: Vec<u8> = Vec::new();
    let mut script_filename: Vec<u8> = Vec::new();
    if let Some(sf) = script_file {
        if mode != Mode::ProcessStdin || exec_run.is_none() {
            match read_script(sf) {
                Some(d) => source = Some(d),
                None => return false,
            }
        }
        php_self = sf.to_vec();
        script_filename = sf.to_vec();
        translated = std::fs::canonicalize(std::ffi::OsStr::from_bytes(sf))
            .map(|p| p.as_os_str().as_bytes().to_vec())
            .unwrap_or_else(|_| sf.to_vec());
    } else {
        php_self = b"Standard input code".to_vec();
        if mode < Mode::CliDirect {
            source = Some(read_stdin());
        }
    }
    // argv: the script name, then the arguments after the options.
    let rest: Vec<Vec<u8>> = if optind <= argv.len() {
        argv[optind..].to_vec()
    } else {
        Vec::new()
    };
    let main_name = if script_file.is_some() {
        s(&translated)
    } else {
        "Standard input code".to_string()
    };
    goro_ext_standard::misc::set_translated_path(script_file.map(|_| s(&translated)));

    match mode {
        Mode::Lint => {
            let src = source.unwrap_or_default();
            let mut vm = engine::new_vm(ini, "cli");
            let info = cli_request_info(&php_self, &script_filename, &rest);
            engine::request_startup(&mut vm, &info, ini);
            match engine::compile(&src, &main_name, true) {
                Ok(_) => {
                    vm.write_output(
                        format!("No syntax errors detected in {}\n", s(&php_self)).as_bytes(),
                    );
                }
                Err(e) => {
                    engine::report_compile_error(&mut vm, &e, &main_name);
                    vm.write_output(format!("Errors parsing {}\n", s(&php_self)).as_bytes());
                    *exit_status = 255;
                }
            }
            flush_output(&mut vm);
            return true;
        }
        Mode::Strip => {
            let src = source.unwrap_or_default();
            puts_bytes(&goro_ext_standard::highlight::strip_source(&src));
            return true;
        }
        Mode::Highlight => {
            let src = source.unwrap_or_default();
            let vm = engine::new_vm(ini, "cli");
            puts_bytes(&goro_ext_standard::highlight::highlight_source(&vm, &src));
            return true;
        }
        Mode::ShowIniConfig => {
            let opened = INI_OPENED_PATH.with(|o| o.borrow().clone());
            let mut out = String::new();
            out.push_str("Configuration File (php.ini) Path: \"\"\n");
            match opened {
                Some(p) => out.push_str(&format!("Loaded Configuration File:         \"{}\"\n", p)),
                None => out.push_str("Loaded Configuration File:         (none)\n"),
            }
            out.push_str("Scan for additional .ini files in: (none)\n");
            out.push_str("Additional .ini files parsed:      (none)\n");
            puts(&out);
            return true;
        }
        _ => {}
    }

    let mut vm = engine::new_vm(ini, "cli");
    vm.cli_direct_output = true;
    vm.current_file = main_name.clone();
    let info = cli_request_info(&php_self, &script_filename, &rest);
    engine::request_startup(&mut vm, &info, ini);

    let failed = match mode {
        Mode::Standard => {
            let src = source.unwrap_or_default();
            run_main(&mut vm, &src, &main_name)
        }
        Mode::CliDirect => {
            let code = exec_direct.unwrap_or_default();
            let mut src = b"<?php ".to_vec();
            src.extend_from_slice(code);
            vm.current_file = "Command line code".to_string();
            run_main(&mut vm, &src, "Command line code")
        }
        Mode::ProcessStdin => process_stdin(
            &mut vm,
            exec_begin,
            exec_run,
            exec_end,
            source.as_deref(),
            &main_name,
        ),
        Mode::ReflectionFunction
        | Mode::ReflectionClass
        | Mode::ReflectionExtension
        | Mode::ReflectionZendExtension => {
            let what = s(reflection_what.unwrap_or_default());
            let class = match mode {
                Mode::ReflectionFunction if what.contains("::") => "ReflectionMethod",
                Mode::ReflectionFunction => "ReflectionFunction",
                Mode::ReflectionClass => "ReflectionClass",
                Mode::ReflectionExtension => "ReflectionExtension",
                _ => "ReflectionZendExtension",
            };
            let code = format!(
                "try {{ $__r = new {}({}); echo $__r, \"\\n\"; }} catch (\\Throwable $__e) {{ echo 'Exception: ', $__e->getMessage(), \"\\n\"; exit(1); }}",
                class,
                php_string_literal(&what)
            );
            run_snippet(&mut vm, &code);
            false
        }
        Mode::ReflectionExtInfo
            if reflection_what.is_some_and(|w| w.eq_ignore_ascii_case(b"standard")) =>
        {
            // php_info_print_module() of PHP_MINFO(basic)
            let ini = |vm: &Vm, k: &str, d: &str| -> String {
                let v = vm
                    .constants
                    .get(k.as_bytes())
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_else(|| d.to_string());
                if v.is_empty() {
                    "no value".to_string()
                } else {
                    v
                }
            };
            let mut out = String::from(
                "\nstandard\n\nDynamic Library Support => enabled\nPath to sendmail => /usr/sbin/sendmail -t -i\n\nDirective => Local Value => Master Value\n",
            );
            for (k, d) in [
                ("unserialize_max_depth", "4096"),
                ("user_agent", ""),
                ("from", ""),
                ("default_socket_timeout", "60"),
                ("auto_detect_line_endings", "Off"),
                ("assert.active", "On"),
                ("assert.bail", "Off"),
                ("assert.warning", "On"),
                ("assert.callback", ""),
                ("assert.exception", "On"),
                ("url_rewriter.tags", "form="),
                ("url_rewriter.hosts", ""),
            ] {
                let v = ini(&vm, k, d);
                out.push_str(&format!("{} => {} => {}\n", k, v, v));
            }
            vm.write_output(out.as_bytes());
            false
        }
        Mode::ReflectionExtInfo => {
            let what = s(reflection_what.unwrap_or_default());
            let code = format!(
                "$__n = {0}; if (extension_loaded($__n)) {{ echo \"\\n\"; (new ReflectionExtension($__n))->info(); }} \
                 elseif ($__n === 'main') {{ foreach (ini_get_all(null, false) as $__k => $__v) {{ }} }} \
                 else {{ echo \"Extension '\", $__n, \"' not present.\\n\"; exit(1); }}",
                php_string_literal(&what)
            );
            run_snippet(&mut vm, &code);
            false
        }
        Mode::ShowIniDiff => {
            run_snippet(&mut vm, "echo \"Non-default INI settings:\\n\";");
            false
        }
        _ => false,
    };
    flush_output(&mut vm);
    *exit_status = request_exit_status(&vm, failed);
    true
}

fn puts_bytes(b: &[u8]) {
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(b);
    let _ = so.flush();
}

/// A PHP single-quoted string literal.
fn php_string_literal(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Compile and execute the main script. Returns true when the request
/// failed (parse error, uncaught exception or fatal error).
fn run_main(vm: &mut Vm, src: &[u8], filename: &str) -> bool {
    match engine::compile(src, filename, true) {
        Ok(mut c) => {
            engine::load(vm, &mut c);
            let r = vm.execute(&c.op_array);
            let failed = r.is_err();
            finish_errors(vm, r);
            failed
        }
        Err(e) => {
            engine::report_compile_error(vm, &e, filename);
            vm.fatal_emitted = true;
            true
        }
    }
}

/// PHP_CLI_MODE_PROCESS_STDIN: -B, then -R / -F for every input line, then -E.
fn process_stdin(
    vm: &mut Vm,
    exec_begin: Option<&[u8]>,
    exec_run: Option<&[u8]>,
    exec_end: Option<&[u8]>,
    file_source: Option<&[u8]>,
    file_name: &str,
) -> bool {
    use goro_core::string::PhpString;
    use goro_core::value::Value;

    fn compile_code(vm: &mut Vm, code: &[u8], name: &str) -> Option<engine::Compiled> {
        let mut src = b"<?php ".to_vec();
        src.extend_from_slice(code);
        match engine::compile(&src, name, false) {
            Ok(c) => Some(c),
            Err(e) => {
                engine::report_compile_error(vm, &e, name);
                vm.fatal_emitted = true;
                None
            }
        }
    }

    let run = |vm: &mut Vm, c: &mut engine::Compiled| -> Result<(), goro_core::vm::VmError> {
        engine::load(vm, c);
        vm.execute_chunk(&c.op_array).map(|_| ())
    };

    let mut result: Result<(), goro_core::vm::VmError> = Ok(());
    if let Some(code) = exec_begin {
        match compile_code(vm, code, "Command line begin code") {
            Some(mut c) => result = run(vm, &mut c),
            None => return true,
        }
    }
    let mut run_code = match exec_run {
        Some(code) => match compile_code(vm, code, "Command line run code") {
            Some(c) => Some(c),
            None => return true,
        },
        None => None,
    };
    let mut file_code = match (exec_run, file_source) {
        (None, Some(src)) => match engine::compile(src, file_name, true) {
            Ok(c) => Some(c),
            Err(e) => {
                engine::report_compile_error(vm, &e, file_name);
                vm.fatal_emitted = true;
                return true;
            }
        },
        _ => None,
    };
    if result.is_ok() && !vm.exit_requested {
        // Lines come from the STDIN stream itself (php_stream_gets(s_in)),
        // so the script sees what is left of the input.
        let stdin = vm
            .constants
            .get(b"STDIN".as_slice())
            .cloned()
            .unwrap_or(Value::Resource(1));
        let fgets = Value::String(PhpString::from_bytes(b"fgets"));
        let mut index: i64 = 0;
        loop {
            vm.flush_cli_output();
            let mut line: Vec<u8> = match vm.call_callback(&fgets, std::slice::from_ref(&stdin)) {
                Ok(Value::String(s)) => s.as_bytes().to_vec(),
                _ => break,
            };
            while matches!(line.last(), Some(b'\n') | Some(b'\r')) {
                line.pop();
            }
            index += 1;
            vm.set_global(
                b"argn".to_vec(),
                Value::String(PhpString::from_bytes(&line)),
            );
            vm.set_global(b"argi".to_vec(), Value::Long(index));
            if let Some(c) = run_code.as_mut() {
                result = run(vm, c);
            } else if let Some(c) = file_code.as_mut() {
                vm.current_file = file_name.to_string();
                result = vm.execute_chunk(&c.op_array).map(|_| ());
            }
            if result.is_err() || vm.exit_requested || vm.fatal_emitted {
                break;
            }
        }
    }
    if result.is_ok() && !vm.exit_requested && !vm.fatal_emitted {
        if let Some(code) = exec_end {
            match compile_code(vm, code, "Command line end code") {
                Some(mut c) => result = run(vm, &mut c),
                None => return true,
            }
        }
    }
    let r = vm.finish_request(result.map(|_| Value::Null));
    let failed = r.is_err();
    finish_errors(vm, r);
    failed
}
