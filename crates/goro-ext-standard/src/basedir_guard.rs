//! open_basedir checks for filesystem functions.
//!
//! Each guarded builtin is wrapped: the wrapper performs the
//! php_check_open_basedir() call that the C implementation does before
//! touching the filesystem, then defers to the original implementation.

use goro_core::basedir;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, Vm, VmError};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static ORIG: RefCell<HashMap<&'static str, BuiltinFn>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy)]
enum Kind {
    /// Warn and return false (stat family, mkdir, unlink, chdir, ...).
    Stat(usize),
    /// Warn, then "Failed to open stream: Operation not permitted", false.
    Stream(usize),
    /// Warn, then "Failed to open directory: Operation not permitted", false.
    Dir(usize),
    /// tempnam(): the directory, or the system temp dir when empty.
    TempDir,
    /// rename(): both arguments.
    Pair,
    /// copy(): source and destination are opened as streams.
    Copy,
    /// symlink()/link(): expanded target (relative to the link) and link.
    Link,
    /// linkinfo(): dirname of the argument.
    LinkInfo,
    /// realpath(): check the resolved path.
    Realpath,
    /// glob(): silently drop results outside open_basedir.
    Glob,
    /// error_log(): destination file for message_type 3.
    ErrorLog,
    /// ini_set(): error_log / open_basedir directives.
    IniSet,
}

fn arg_str(args: &[Value], i: usize) -> Option<String> {
    let v = args.get(i)?;
    if matches!(v, Value::Null | Value::Undef) {
        return None;
    }
    if let Value::Resource(_) = v {
        return None;
    }
    let s = v.to_php_string();
    if s.as_bytes().is_empty() || s.as_bytes().contains(&0) {
        return None;
    }
    Some(s.to_string_lossy().to_string())
}

fn active(vm: &Vm) -> bool {
    !basedir::open_basedir_value(vm).is_empty()
}

fn stream_fail(vm: &mut Vm, fname: &str, path: &str, what: &str) -> Result<Value, VmError> {
    vm.emit_warning(&format!(
        "{}({}): Failed to open {}: Operation not permitted",
        fname, path, what
    ));
    Ok(Value::False)
}

fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => ".".to_string(),
    }
}

fn call_orig(vm: &mut Vm, fname: &'static str, args: &[Value]) -> Result<Value, VmError> {
    let f = ORIG.with(|m| m.borrow().get(fname).copied());
    match f {
        Some(f) => f(vm, args),
        None => Ok(Value::False),
    }
}

fn guarded(vm: &mut Vm, fname: &'static str, kind: Kind, args: &[Value]) -> Result<Value, VmError> {
    if !active(vm) {
        return call_orig(vm, fname, args);
    }
    match kind {
        Kind::Stat(i) => {
            if let Some(p) = arg_str(args, i) {
                if !vm.check_open_basedir(fname, &p) {
                    return Ok(Value::False);
                }
            }
        }
        Kind::Stream(i) => {
            if let Some(p) = arg_str(args, i) {
                if !vm.check_open_basedir(fname, &p) {
                    return stream_fail(vm, fname, &p, "stream");
                }
            }
        }
        Kind::Dir(i) => {
            if let Some(p) = arg_str(args, i) {
                if !vm.check_open_basedir(fname, &p) {
                    let r = stream_fail(vm, fname, &p, "directory");
                    if fname == "scandir" {
                        vm.emit_warning("scandir(): (errno 1): Operation not permitted");
                    }
                    return r;
                }
            }
        }
        Kind::TempDir => {
            let p = arg_str(args, 0).unwrap_or_else(|| {
                std::env::temp_dir()
                    .to_string_lossy()
                    .trim_end_matches('/')
                    .to_string()
            });
            if !vm.check_open_basedir(fname, &p) {
                return Ok(Value::False);
            }
        }
        Kind::Pair => {
            for i in 0..2 {
                if let Some(p) = arg_str(args, i) {
                    if !vm.check_open_basedir(fname, &p) {
                        return Ok(Value::False);
                    }
                }
            }
        }
        Kind::Copy => {
            if let Some(p) = arg_str(args, 0) {
                if !vm.check_open_basedir(fname, &p) {
                    return Ok(Value::False);
                }
            }
            if let Some(p) = arg_str(args, 1) {
                if !vm.check_open_basedir(fname, &p) {
                    return stream_fail(vm, fname, &p, "stream");
                }
            }
        }
        Kind::Link => {
            if let (Some(target), Some(link)) = (arg_str(args, 0), arg_str(args, 1)) {
                if let Some(source_p) = basedir::expand_filepath(&link) {
                    // virtual_file_ex(CWD_FILEPATH) resolves an existing path's symlinks.
                    let source_p = std::fs::canonicalize(&source_p)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or(source_p);
                    let dir = dirname(&source_p);
                    let dest_p = if target.starts_with('/') {
                        basedir::expand_filepath(&target)
                    } else {
                        basedir::expand_filepath(&format!("{}/{}", dir, target))
                    };
                    if let Some(dest_p) = dest_p {
                        if !vm.check_open_basedir(fname, &dest_p) {
                            return Ok(Value::False);
                        }
                    }
                    if !vm.check_open_basedir(fname, &source_p) {
                        return Ok(Value::False);
                    }
                }
            }
        }
        Kind::LinkInfo => {
            if let Some(p) = arg_str(args, 0) {
                let d = dirname(&p);
                if !vm.check_open_basedir(fname, &d) {
                    return Ok(Value::False);
                }
            }
        }
        Kind::Realpath => {
            let r = call_orig(vm, fname, args)?;
            if let Value::String(s) = &r {
                let p = s.to_string_lossy().to_string();
                if !vm.check_open_basedir(fname, &p) {
                    return Ok(Value::False);
                }
            }
            return Ok(r);
        }
        Kind::Glob => {
            let r = call_orig(vm, fname, args)?;
            if let Value::Array(arr) = &r {
                let vals: Vec<Value> = arr.borrow().values().cloned().collect();
                let mut out = goro_core::array::PhpArray::new();
                let mut limited = false;
                for v in vals {
                    let p = v.to_php_string().to_string_lossy().to_string();
                    if vm.check_open_basedir_silent(&p) {
                        out.push(v);
                    } else {
                        limited = true;
                    }
                }
                if limited && out.is_empty() {
                    return Ok(Value::False);
                }
                return Ok(Value::Array(std::rc::Rc::new(RefCell::new(out))));
            }
            return Ok(r);
        }
        Kind::ErrorLog => {
            if args.get(1).map(|v| v.to_long()) == Some(3) {
                if let Some(p) = arg_str(args, 2) {
                    if !vm.check_open_basedir(fname, &p) {
                        return stream_fail(vm, fname, &p, "stream");
                    }
                }
            }
        }
        Kind::IniSet => {
            let name = args
                .first()
                .map(|v| v.to_php_string().to_string_lossy().to_string())
                .unwrap_or_default();
            if name == "error_log" || name == "mail.log" {
                if let Some(p) = arg_str(args, 1).filter(|p| name != "error_log" || p != "syslog") {
                    if !vm.check_open_basedir(fname, &p) {
                        return Ok(Value::False);
                    }
                }
            } else if name == "open_basedir" {
                let new_val = args
                    .get(1)
                    .map(|v| v.to_php_string().to_string_lossy().to_string())
                    .unwrap_or_default();
                // ini_set() returns (and ini_get() then gives) the value as
                // set; PG(open_basedir) gets its entries resolved.
                let old = vm
                    .constants
                    .get(b"open_basedir".as_slice())
                    .map(|v| v.to_php_string())
                    .unwrap_or_else(|| PhpString::from_bytes(b""));
                return match basedir::update_open_basedir(vm, &new_val) {
                    Some(v) => {
                        vm.constants.insert(
                            b"open_basedir".to_vec(),
                            Value::String(PhpString::from_string(new_val)),
                        );
                        vm.open_basedir_resolved = Some(v);
                        Ok(Value::String(old))
                    }
                    None => Ok(Value::False),
                };
            }
        }
    }
    call_orig(vm, fname, args)
}

macro_rules! guards {
    ($( $wrapper:ident => ($name:literal, $kind:expr) ),* $(,)?) => {
        $(
            fn $wrapper(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                guarded(vm, $name, $kind, args)
            }
        )*
        pub fn register(vm: &mut Vm) {
            $(
                if vm.functions.contains_key($name.as_bytes()) {
                    if let Some(orig) = vm.override_function($name.as_bytes(), $wrapper) {
                        ORIG.with(|m| m.borrow_mut().insert($name, orig));
                    }
                }
            )*
        }
    };
}

guards! {
    g_file_exists => ("file_exists", Kind::Stat(0)),
    g_is_file => ("is_file", Kind::Stat(0)),
    g_is_dir => ("is_dir", Kind::Stat(0)),
    g_is_link => ("is_link", Kind::Stat(0)),
    g_is_readable => ("is_readable", Kind::Stat(0)),
    g_is_writable => ("is_writable", Kind::Stat(0)),
    g_is_writeable => ("is_writeable", Kind::Stat(0)),
    g_is_executable => ("is_executable", Kind::Stat(0)),
    g_filetype => ("filetype", Kind::Stat(0)),
    g_stat => ("stat", Kind::Stat(0)),
    g_lstat => ("lstat", Kind::Stat(0)),
    g_fileatime => ("fileatime", Kind::Stat(0)),
    g_filectime => ("filectime", Kind::Stat(0)),
    g_filemtime => ("filemtime", Kind::Stat(0)),
    g_fileinode => ("fileinode", Kind::Stat(0)),
    g_fileowner => ("fileowner", Kind::Stat(0)),
    g_filegroup => ("filegroup", Kind::Stat(0)),
    g_fileperms => ("fileperms", Kind::Stat(0)),
    g_filesize => ("filesize", Kind::Stat(0)),
    g_disk_free_space => ("disk_free_space", Kind::Stat(0)),
    g_diskfreespace => ("diskfreespace", Kind::Stat(0)),
    g_disk_total_space => ("disk_total_space", Kind::Stat(0)),
    g_chdir => ("chdir", Kind::Stat(0)),
    g_chmod => ("chmod", Kind::Stat(0)),
    g_chown => ("chown", Kind::Stat(0)),
    g_chgrp => ("chgrp", Kind::Stat(0)),
    g_touch => ("touch", Kind::Stat(0)),
    g_mkdir => ("mkdir", Kind::Stat(0)),
    g_rmdir => ("rmdir", Kind::Stat(0)),
    g_unlink => ("unlink", Kind::Stat(0)),
    g_tempnam => ("tempnam", Kind::TempDir),
    g_readlink => ("readlink", Kind::Stat(0)),
    g_rename => ("rename", Kind::Pair),
    g_copy => ("copy", Kind::Copy),
    g_symlink => ("symlink", Kind::Link),
    g_link => ("link", Kind::Link),
    g_linkinfo => ("linkinfo", Kind::LinkInfo),
    g_realpath => ("realpath", Kind::Realpath),
    g_glob => ("glob", Kind::Glob),
    g_fopen => ("fopen", Kind::Stream(0)),
    g_file => ("file", Kind::Stream(0)),
    g_file_get_contents => ("file_get_contents", Kind::Stream(0)),
    g_file_put_contents => ("file_put_contents", Kind::Stream(0)),
    g_readfile => ("readfile", Kind::Stream(0)),
    g_parse_ini_file => ("parse_ini_file", Kind::Stream(0)),
    g_opendir => ("opendir", Kind::Dir(0)),
    g_scandir => ("scandir", Kind::Dir(0)),
    g_dir => ("dir", Kind::Dir(0)),
    g_error_log => ("error_log", Kind::ErrorLog),
    g_ini_set => ("ini_set", Kind::IniSet),
    g_ini_alter => ("ini_alter", Kind::IniSet),
}
