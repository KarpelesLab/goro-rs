//! Compiler and virtual machine executing PHP.
//!
//! - [`compiler`]: AST ([`goro_syntax`]) to bytecode ([`bytecode`]).
//! - [`vm`]: the virtual machine: frames, calls, objects, exceptions,
//!   generators, output, errors.
//! - [`registry`]: the API extensions use to add functions, classes and
//!   constants; [`builtins`] are the engine's own modules.
//!
//! [`run`] runs a script as the `php` CLI does.

// Native hooks are plain fn pointers with spelled-out signatures; the VM
// hands raw slot pointers through crate-internal APIs on purpose (the
// dispatch loop owns their lifetime); float limits are written with the
// digits PHP documents.
#![allow(
    clippy::type_complexity,
    clippy::not_unsafe_ptr_arg_deref,
    clippy::excessive_precision
)]

pub mod builtins;
pub mod bytecode;
pub mod class;
pub mod cli_server;
pub mod compiler;
pub mod func;
pub mod registry;
pub mod request_vars;
pub mod vm;

pub use func::{Args, Func, NativeFn};
pub use registry::{ConstVal, Module};
pub use vm::{R, Unwind, Vm};

/// What to run.
pub enum Script {
    File(Vec<u8>),
    Code(Vec<u8>),
    Stdin,
}

/// Options of a CLI run.
pub struct RunOptions {
    pub script: Script,
    /// `-d name=value` settings, in order.
    pub ini: Vec<(Vec<u8>, Vec<u8>)>,
    /// Script arguments (`$argv[1..]`).
    pub args: Vec<Vec<u8>>,
    /// Extra modules (extension crates).
    pub modules: Vec<&'static Module>,
    /// Line mode (PHP manual: Command line options -B, -R, -F, -E): code
    /// run before the input lines, for each line (`$argn`, `$argi`), and
    /// after.
    pub lines: Option<LineMode>,
}

pub struct LineMode {
    pub begin: Option<Vec<u8>>,
    /// `-R` code or `-F` file, for each input line.
    pub each: Option<Script>,
    pub end: Option<Vec<u8>>,
}

impl RunOptions {
    pub fn new(script: Script) -> RunOptions {
        RunOptions {
            script,
            ini: Vec::new(),
            args: Vec::new(),
            modules: Vec::new(),
            lines: None,
        }
    }
}

/// Runs the -B/-R/-F/-E line mode.
fn run_lines(vm: &mut Vm, lm: &LineMode) -> i32 {
    vm.arm_memory_limit();
    use goro_runtime::{Key, Value};
    let code = |c: &[u8]| {
        let mut s = b"<?php ".to_vec();
        s.extend_from_slice(c);
        s
    };
    if let Some(b) = &lm.begin {
        let r = vm.run_main_code(&code(b), b"Command line begin code");
        let stop = r.is_err();
        vm.handle_unwind(r);
        if stop {
            vm.shutdown();
            return vm.exit_status;
        }
    }
    if let Some(each) = &lm.each {
        let (src, path) = match each {
            Script::File(f) => {
                use std::os::unix::ffi::OsStrExt;
                let p = std::path::Path::new(std::ffi::OsStr::from_bytes(f));
                match std::fs::read(p) {
                    Ok(s) => (strip_shebang(s), f.clone()),
                    Err(_) => {
                        eprintln!("Could not open input file: {}", String::from_utf8_lossy(f));
                        return 1;
                    }
                }
            }
            Script::Code(c) => (code(c), b"Command line run code".to_vec()),
            Script::Stdin => (Vec::new(), Vec::new()),
        };
        use std::io::BufRead;
        let mut n = 0i64;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            // Through the same stdin the script's STDIN reads, without
            // holding it while the code runs (test sapi/cli/tests/010).
            let r = {
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                lock.read_until(b'\n', &mut buf)
            };
            match r {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            n += 1;
            let mut line = buf.clone();
            if line.last() == Some(&b'\n') {
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
            }
            let g = &mut vm.globals.dyn_vars;
            g.insert(
                Key::from("argn"),
                Value::Str(goro_runtime::PhpStr::from(line)),
            );
            g.insert(Key::from("argi"), Value::Int(n));
            let r = vm.run_main_code(&src, &path);
            let stop = r.is_err();
            vm.handle_unwind(r);
            if stop {
                vm.shutdown();
                return vm.exit_status;
            }
        }
    }
    if let Some(e) = &lm.end {
        let r = vm.run_main_code(&code(e), b"Command line end code");
        vm.handle_unwind(r);
    }
    vm.shutdown();
    vm.exit_status
}

/// A started engine: the modules, then the startup settings (`-d`, in
/// order) and what derives from them.
pub fn startup(ini: &[(Vec<u8>, Vec<u8>)], extra: &[&'static Module]) -> Vm {
    let mut modules = builtins::modules();
    modules.extend(extra.iter().copied());
    let mut vm = Vm::new(&modules);
    for (k, v) in ini {
        vm.set_ini(k, v);
    }
    builtins::misc::startup_ini_deprecations(&mut vm, ini);
    for m in vm.modules.clone() {
        if let Some(f) = m.startup_ini {
            f(&mut vm, ini);
        }
    }
    // disable_functions: since PHP 8.0 disabled functions are not defined
    // at all (PHP manual: php.ini directives; test
    // Zend/tests/errmsg/errmsg_020.phpt).
    if let Some(list) = vm.get_ini(b"disable_functions") {
        for name in list.split(|&c| c == b',') {
            let n = name.trim_ascii().to_ascii_lowercase();
            if !n.is_empty() {
                let k = goro_runtime::PhpStr::from(n);
                if let Some(f) = vm.functions.remove(&k) {
                    vm.disabled_functions.insert(k, f);
                }
            }
        }
    }
    vm
}

/// Runs a script; returns the process exit status.
pub fn run(opts: RunOptions) -> i32 {
    let mut vm = startup(&opts.ini, &opts.modules);
    if let Some(lm) = &opts.lines {
        let mut argv = vec![b"Standard input code".to_vec()];
        argv.extend(opts.args.iter().cloned());
        vm.set_script(b"Standard input code", &argv);
        return run_lines(&mut vm, lm);
    }
    let (src, path, display): (Vec<u8>, Vec<u8>, Vec<u8>) = match &opts.script {
        Script::File(p) => {
            use std::os::unix::ffi::OsStrExt;
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(p));
            match std::fs::read(path) {
                Ok(s) => {
                    // __FILE__ is the absolute path.
                    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
                    (s, abs.as_os_str().as_bytes().to_vec(), p.clone())
                }
                Err(_) => {
                    eprintln!("Could not open input file: {}", String::from_utf8_lossy(p));
                    return 1;
                }
            }
        }
        Script::Code(c) => {
            let mut s = b"<?php ".to_vec();
            s.extend_from_slice(c);
            (
                s,
                b"Command line code".to_vec(),
                b"Standard input code".to_vec(),
            )
        }
        Script::Stdin => {
            use std::io::Read;
            let mut s = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut s);
            (s, b"Standard input code".to_vec(), b"-".to_vec())
        }
    };
    let mut argv = vec![match &opts.script {
        Script::File(_) => display.clone(),
        Script::Code(_) => b"Standard input code".to_vec(),
        Script::Stdin => b"Standard input code".to_vec(),
    }];
    if let Script::Code(_) = &opts.script {
        argv[0] = b"Standard input code".to_vec();
    }
    argv.extend(opts.args.iter().cloned());
    vm.set_script(&path, &argv);
    if let Script::File(_) = &opts.script {
        vm.included.push(goro_runtime::PhpStr::new(&path));
        vm.included_set.insert(goro_runtime::PhpStr::new(&path));
    }
    let src = if let Script::Code(_) = &opts.script {
        src
    } else {
        strip_shebang(src)
    };
    builtins::output::start_ini_handler(&mut vm);
    vm.run_script(&src, &path)
}

/// A `#!` first line is not output (PHP CLI behaviour). It still counts
/// as line 1: it becomes an empty PHP block whose closing tag eats the
/// newline.
fn strip_shebang(src: Vec<u8>) -> Vec<u8> {
    if !src.starts_with(b"#!") {
        return src;
    }
    match src.iter().position(|&c| c == b'\n') {
        Some(p) => {
            let mut v = b"<?php ?>".to_vec();
            v.extend_from_slice(&src[p..]);
            v
        }
        None => Vec::new(),
    }
}
