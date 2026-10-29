//! ext/fileinfo (PHP manual: Fileinfo): the finfo class, finfo_open(),
//! finfo_file(), finfo_buffer(), finfo_set_flags(), finfo_close() and
//! mime_content_type(), over goro-ext-fileinfo's magic engine. Messages
//! follow tests ext/fileinfo/tests/*.phpt and PHP 8.5's observed output.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_ext_fileinfo as fi;
use goro_runtime::{ObjRef, PhpStr, Value};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "fileinfo",
    functions: &[
        (
            "function finfo_open(int $flags = FILEINFO_NONE, ?string $magic_database = null): finfo|false",
            finfo_open,
        ),
        ("function finfo_close($finfo): true", finfo_close),
        (
            "function finfo_set_flags(finfo $finfo, int $flags): true",
            finfo_set_flags,
        ),
        (
            "function finfo_file(finfo $finfo, string $filename, int $flags = FILEINFO_NONE, $context = null): string|false",
            finfo_file,
        ),
        (
            "function finfo_buffer(finfo $finfo, string $string, int $flags = FILEINFO_NONE, $context = null): string|false",
            finfo_buffer,
        ),
        (
            "function mime_content_type($filename): string|false",
            mime_content_type,
        ),
    ],
    classes: &[(
        "finfo",
        "<?php #[NotSerializable] #[NotCloneable] class finfo {
            public function __construct(int $flags = FILEINFO_NONE, ?string $magic_database = null) {}
            public function file(string $filename, int $flags = FILEINFO_NONE, $context = null): string|false {}
            public function buffer(string $string, int $flags = FILEINFO_NONE, $context = null): string|false {}
            public function set_flags(int $flags): true {}
        }",
    )],
    methods: &[
        ("finfo::__construct", m_construct),
        ("finfo::file", m_file),
        ("finfo::buffer", m_buffer),
        ("finfo::set_flags", m_set_flags),
    ],
    hooks: &[("finfo", NativeHooks::default)],
    constants: &[
        ("FILEINFO_NONE", ConstVal::Int(fi::NONE)),
        ("FILEINFO_SYMLINK", ConstVal::Int(fi::SYMLINK)),
        ("FILEINFO_MIME", ConstVal::Int(fi::MIME)),
        ("FILEINFO_MIME_TYPE", ConstVal::Int(fi::MIME_TYPE)),
        ("FILEINFO_MIME_ENCODING", ConstVal::Int(fi::MIME_ENCODING)),
        ("FILEINFO_DEVICES", ConstVal::Int(fi::DEVICES)),
        ("FILEINFO_CONTINUE", ConstVal::Int(fi::CONTINUE)),
        ("FILEINFO_PRESERVE_ATIME", ConstVal::Int(fi::PRESERVE_ATIME)),
        ("FILEINFO_RAW", ConstVal::Int(fi::RAW)),
        ("FILEINFO_APPLE", ConstVal::Int(fi::APPLE)),
        ("FILEINFO_EXTENSION", ConstVal::Int(fi::EXTENSION)),
    ],
    listed: true,
    ..Module::EMPTY
};

/// How much of a file is examined.
const BYTES_MAX: usize = 7 * 1024 * 1024;
/// PHP_MAXPATHLEN on Linux.
const MAXPATHLEN: usize = 4096;

struct Finfo {
    db: Rc<fi::Db>,
    flags: i64,
}

impl goro_runtime::NativeData for Finfo {}

fn finfo_of(o: &ObjRef) -> Option<(Rc<fi::Db>, i64)> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<Finfo>().map(|f| (f.db.clone(), f.flags))
}

fn set_flags_of(o: &ObjRef, flags: i64) -> bool {
    let mut n = o.obj().native.borrow_mut();
    let Some(b) = n.as_mut() else { return false };
    let any: &mut dyn std::any::Any = &mut **b;
    match any.downcast_mut::<Finfo>() {
        Some(f) => {
            f.flags = flags;
            true
        }
        None => false,
    }
}

/// A loaded database with the warnings its load printed (magic files are
/// reparsed only when they change).
type Cached = (Option<Rc<fi::Db>>, Vec<String>);

thread_local! {
    static CACHE: RefCell<HashMap<(Vec<u8>, u64, i64), Cached>> = RefCell::new(HashMap::new());
}

enum Diag {
    Warning(String),
    Notice(String),
}

fn abs_path(vm: &Vm, p: &[u8]) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    if p.first() == Some(&b'/') {
        return p.to_vec();
    }
    let mut v = vm.cwd.as_os_str().as_bytes().to_vec();
    if !v.ends_with(b"/") {
        v.push(b'/');
    }
    v.extend_from_slice(p);
    v
}

/// Loads the database for finfo_open()/new finfo(); diagnostics are
/// returned (`fname` prefixes them) for the caller to print or throw.
fn load_db(
    vm: &mut Vm,
    fname: &str,
    path: Option<&[u8]>,
    flags: i64,
    diags: &mut Vec<Diag>,
) -> Option<Rc<fi::Db>> {
    use std::os::unix::ffi::OsStrExt;
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return Some(fi::builtin());
    };
    let shown = String::from_utf8_lossy(path).into_owned();
    if path.len() >= MAXPATHLEN {
        if vm
            .get_ini(b"open_basedir")
            .is_some_and(|v| !v.as_bytes().is_empty())
        {
            diags.push(Diag::Warning(format!(
                "{}(): File name is longer than the maximum allowed path length on this platform ({}): {}",
                fname, MAXPATHLEN, shown
            )));
        }
        return None;
    }
    let full = abs_path(vm, path);
    let fulls = String::from_utf8_lossy(&full).into_owned();
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(&full));
    if flags & fi::CHECK != 0 {
        diags.push(Diag::Warning(format!(
            "{}(): using regular magic file `{}'",
            fname, fulls
        )));
    }
    let meta = std::fs::metadata(p);
    let Ok(meta) = meta else {
        for _ in 0..2 {
            diags.push(Diag::Warning(format!(
                "{}({}): Failed to open stream: No such file or directory",
                fname, fulls
            )));
        }
        diags.push(Diag::Warning(format!(
            "{}(): Failed to load magic database at \"{}\"",
            fname, fulls
        )));
        return None;
    };
    let mtime = {
        use std::os::unix::fs::MetadataExt;
        meta.mtime() * 1_000_000_000 + meta.mtime_nsec()
    };
    let mut files: Vec<Vec<u8>> = Vec::new();
    if meta.is_dir() {
        diags.push(Diag::Notice(format!(
            "{}(): Read of 8192 bytes failed with errno=21 Is a directory",
            fname
        )));
        let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(p)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        names.sort();
        for n in names {
            if n.is_file()
                && let Ok(d) = std::fs::read(&n)
            {
                files.push(d);
            }
        }
    }
    let key = (full.clone(), meta.len(), mtime);
    let cached = if meta.is_dir() {
        None
    } else {
        CACHE.with(|c| c.borrow().get(&key).cloned())
    };
    let (db, warns) = match cached {
        Some(c) => c,
        None => {
            if !meta.is_dir() {
                match std::fs::read(p) {
                    Ok(d) => files.push(d),
                    Err(_) => {
                        diags.push(Diag::Warning(format!(
                            "{}(): Failed to load magic database at \"{}\"",
                            fname, fulls
                        )));
                        return None;
                    }
                }
            }
            let srcs: Vec<&[u8]> = files.iter().map(|v| v.as_slice()).collect();
            let (db, w) = fi::load(&srcs);
            let r: Cached = (db.map(Rc::new), w);
            if !meta.is_dir() {
                CACHE.with(|c| {
                    let mut c = c.borrow_mut();
                    if c.len() > 16 {
                        c.clear();
                    }
                    c.insert(key, r.clone());
                });
            }
            r
        }
    };
    for w in warns {
        diags.push(Diag::Warning(format!("{}(): {}", fname, w)));
    }
    if db.is_none() {
        diags.push(Diag::Warning(format!(
            "{}(): Failed to load magic database at \"{}\"",
            fname, fulls
        )));
    }
    db
}

fn new_finfo(vm: &mut Vm, db: Rc<fi::Db>, flags: i64) -> R<Value> {
    let Some(cls) = vm.find_class(b"finfo", false)? else {
        return Err(vm.fatal("Class \"finfo\" not found"));
    };
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(Finfo { db, flags }));
    Ok(Value::Object(o))
}

fn finfo_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(0, 0);
    let path = a.opt_str(1);
    if let Some(p) = &path
        && p.contains(&0)
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($magic_database) must not contain any null bytes",
        );
    }
    let mut diags = Vec::new();
    let db = load_db(
        vm,
        "finfo_open",
        path.as_ref().map(|p| p.as_bytes()),
        flags,
        &mut diags,
    );
    for d in diags {
        match d {
            Diag::Warning(m) => vm.warn(&m)?,
            Diag::Notice(m) => vm.notice_msg(&m)?,
        }
    }
    match db {
        Some(db) => new_finfo(vm, db, flags),
        None => Ok(Value::Bool(false)),
    }
}

fn m_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let flags = a.int_or(0, 0);
    let path = a.opt_str(1);
    if let Some(p) = &path
        && p.contains(&0)
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($magic_database) must not contain any null bytes",
        );
    }
    let mut diags = Vec::new();
    let db = load_db(
        vm,
        "finfo::__construct",
        path.as_ref().map(|p| p.as_bytes()),
        flags,
        &mut diags,
    );
    // The constructor turns its first warning into an exception.
    for d in diags {
        match d {
            Diag::Warning(m) => return vm.throw("Exception", &m),
            Diag::Notice(m) => vm.notice_msg(&m)?,
        }
    }
    let Some(db) = db else {
        return vm.throw("Exception", "Constructor failed");
    };
    let o = a.this_obj().clone();
    *o.obj().native.borrow_mut() = Some(Box::new(Finfo { db, flags }));
    Ok(Value::Null)
}

fn invalid(vm: &mut Vm) -> R<Value> {
    vm.throw("Error", "Invalid finfo object")
}

fn finfo_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function finfo_close() is deprecated since 8.5, as finfo objects are freed automatically",
    )?;
    let v = a.val(0);
    match &v {
        Value::Object(o) if o.class().name().as_bytes().eq_ignore_ascii_case(b"finfo") => {
            Ok(Value::Bool(true))
        }
        _ => vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($finfo) must be of type finfo, {} given",
                Vm::type_name_given(&v)
            ),
        ),
    }
}

fn finfo_set_flags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(o) = a.obj(0) else {
        return invalid(vm);
    };
    if !set_flags_of(&o, a.int(1)) {
        return invalid(vm);
    }
    Ok(Value::Bool(true))
}

fn m_set_flags(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    if !set_flags_of(&o, a.int(0)) {
        return invalid(vm);
    }
    Ok(Value::Bool(true))
}

/// Whether `path` names a directory (local or through a wrapper's
/// url_stat).
fn is_directory(vm: &mut Vm, path: &[u8]) -> R<bool> {
    let p = match super::streams::rewrite_path(vm, path) {
        Some(p) => p,
        None => path.to_vec(),
    };
    let mode = |st: &goro_runtime::PhpArray| match st.get_str(&PhpStr::from("mode")) {
        Some(Value::Int(m)) => *m as u32,
        _ => 0,
    };
    if let Some(cls) = super::userstream::wrapper_for(vm, &p) {
        let st = super::userstream::url_stat(vm, &cls, &p, 2)?;
        return Ok(st.is_some_and(|s| mode(&s) & 0o170000 == 0o040000));
    }
    if let Some(op) = super::streams::wrapper_for(vm, &p).and_then(|w| w.url_stat) {
        let st = op(vm, &p, 2)?;
        return Ok(st.is_some_and(|s| mode(&s) & 0o170000 == 0o040000));
    }
    if p.windows(3).any(|w| w == b"://") && !p.starts_with(b"file://") {
        return Ok(false);
    }
    let lp = p.strip_prefix(b"file://").unwrap_or(&p);
    Ok(std::fs::metadata(vm.fs_path(lp)).is_ok_and(|m| m.is_dir()))
}

fn run_identify(
    vm: &mut Vm,
    a: &Args,
    db: &fi::Db,
    data: &[u8],
    flags: i64,
    file_mode: bool,
) -> R<Value> {
    match fi::identify(db, data, flags, file_mode) {
        Ok(s) => Ok(Value::Str(PhpStr::from(s))),
        Err(m) => {
            vm.fwarn(a, &format!("Failed identify data 0:{}", m))?;
            Ok(Value::Bool(false))
        }
    }
}

/// finfo_file() and finfo::file(): `argn` is the position of $filename.
fn do_file(vm: &mut Vm, a: &mut Args, db: Rc<fi::Db>, oflags: i64, argn: usize) -> R<Value> {
    let fname = a.str(argn);
    let flags = match a.opt_int(argn + 1) {
        Some(f) if f != 0 => f,
        _ => oflags,
    };
    if let Some(ctx) = a.get(argn + 2).map(|v| v.deref())
        && !matches!(ctx, Value::Null)
    {
        let ok = matches!(&ctx, Value::Resource(r) if r.res().kind.borrow().as_bytes() == b"stream-context");
        if !ok {
            return vm.fthrow(
                a,
                "TypeError",
                "supplied resource is not a valid Stream-Context resource",
            );
        }
    }
    if fname.is_empty() {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #{} ($filename) must not be empty", argn + 1),
        );
    }
    if fname.contains(&0) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #{} ($filename) must not contain any null bytes",
                argn + 1
            ),
        );
    }
    identify_path(vm, a, &db, &fname, flags)
}

fn identify_path(vm: &mut Vm, a: &Args, db: &fi::Db, fname: &[u8], flags: i64) -> R<Value> {
    if is_directory(vm, fname)? {
        return Ok(Value::Str(PhpStr::from("directory")));
    }
    let Some(mut s) = super::file::open_stream(vm, a, fname, b"rb", false)? else {
        return Ok(Value::Bool(false));
    };
    let data = read_up_to(vm, &mut s);
    run_identify(vm, a, db, &data, flags, true)
}

fn read_up_to(vm: &mut Vm, s: &mut super::file::Stream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    while out.len() < BYTES_MAX {
        match s.read_some(vm, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    out.truncate(BYTES_MAX);
    out
}

fn finfo_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((db, flags)) = a.obj(0).and_then(|o| finfo_of(&o)) else {
        return invalid(vm);
    };
    do_file(vm, a, db, flags, 1)
}

fn m_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((db, flags)) = finfo_of(a.this_obj()) else {
        return invalid(vm);
    };
    do_file(vm, a, db, flags, 0)
}

fn do_buffer(vm: &mut Vm, a: &mut Args, db: Rc<fi::Db>, oflags: i64, argn: usize) -> R<Value> {
    if a.len() > argn + 2 {
        vm.fdeprecated(a, "The $context parameter has no effect for finfo_buffer()")?;
    }
    let data = a.str(argn);
    let flags = match a.opt_int(argn + 1) {
        Some(f) if f != 0 => f,
        _ => oflags,
    };
    run_identify(vm, a, &db, data.as_bytes(), flags, false)
}

fn finfo_buffer(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((db, flags)) = a.obj(0).and_then(|o| finfo_of(&o)) else {
        return invalid(vm);
    };
    do_buffer(vm, a, db, flags, 1)
}

fn m_buffer(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((db, flags)) = finfo_of(a.this_obj()) else {
        return invalid(vm);
    };
    do_buffer(vm, a, db, flags, 0)
}

fn mime_content_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let db = fi::builtin();
    match &v {
        Value::Str(s) => {
            if s.is_empty() {
                return vm.fthrow(a, "ValueError", "Argument #1 ($filename) must not be empty");
            }
            if s.contains(&0) {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #1 ($filename) must not contain any null bytes",
                );
            }
            let s = s.clone();
            identify_path(vm, a, &db, &s, fi::MIME_TYPE)
        }
        Value::Resource(_) => {
            let data = super::file::with_stream(vm, a, 0, |vm, s| Ok(read_up_to(vm, s)))?;
            run_identify(vm, a, &db, &data, fi::MIME_TYPE, true)
        }
        _ => vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #1 ($filename) must be of type resource|string, {} given",
                Vm::type_name_given(&v)
            ),
        ),
    }
}
