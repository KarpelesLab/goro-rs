//! Compiler and virtual machine executing PHP.
//!
//! - [`compiler`]: AST ([`goro_syntax`]) to bytecode ([`bytecode`]).
//! - [`vm`]: the virtual machine: frames, calls, objects, exceptions,
//!   generators, output, errors.
//! - [`registry`]: the API extensions use to add functions, classes and
//!   constants; [`builtins`] are the engine's own modules.
//!
//! [`run`] runs a script as the `php` CLI does.

pub mod builtins;
pub mod bytecode;
pub mod class;
pub mod compiler;
pub mod func;
pub mod registry;
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
}

/// Runs a script; returns the process exit status.
pub fn run(opts: RunOptions) -> i32 {
    let mut modules = builtins::modules();
    modules.extend(opts.modules.iter().copied());
    let mut vm = Vm::new(&modules);
    for (k, v) in &opts.ini {
        vm.set_ini(k, v);
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
