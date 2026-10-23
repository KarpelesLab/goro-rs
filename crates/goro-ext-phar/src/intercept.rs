//! php-src ext/phar/func_interceptors.c: relative paths of the file
//! functions resolve inside the executing phar after
//! Phar::interceptFileFuncs().

use std::cell::RefCell;
use std::collections::HashMap;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, Vm, VmError};

use crate::archive::*;
use crate::util;

thread_local! {
    static ORIG: RefCell<HashMap<&'static str, BuiltinFn>> = RefCell::new(HashMap::new());
}

fn orig(vm: &mut Vm, name: &'static str, args: &[Value]) -> Result<Value, VmError> {
    match ORIG.with(|o| o.borrow().get(name).copied()) {
        Some(f) => f(vm, args),
        None => Ok(Value::Null),
    }
}

fn active() -> bool {
    g(|g| g.intercepted && !g.fname_map.is_empty())
}

fn str_arg(args: &[Value], i: usize) -> Option<Vec<u8>> {
    match args.get(i).map(|v| v.deref()) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

fn is_relative(p: &[u8]) -> bool {
    !(p.first() == Some(&b'/')) && memmem(p, b"://").is_none()
}

/// phar_get_name_for_relative_paths()
fn name_for_relative(vm: &mut Vm, filename: &[u8], using_include_path: bool) -> Option<Vec<u8>> {
    let fname = executed_filename(vm)?;
    if !starts_with_ci(&fname, b"phar://") {
        return None;
    }
    let (arch, _) = split_fname(vm, &fname, 2, 0).ok()?;
    let phar = util::get_archive(&arch, None).ok()?;
    if using_include_path {
        return util::find_in_include_path(vm, filename).map(|(n, _)| n);
    }
    let entry = fix_filepath(filename.to_vec(), true);
    let exists = if entry.first() == Some(&b'/') {
        phar.borrow().manifest.contains_key(&entry[1..])
    } else {
        phar.borrow().manifest.contains_key(&entry)
    };
    if !exists {
        return None;
    }
    let mut name = b"phar://".to_vec();
    name.extend_from_slice(&arch);
    if entry.first() != Some(&b'/') {
        name.push(b'/');
    }
    name.extend_from_slice(&entry);
    Some(name)
}

fn with_arg(args: &[Value], i: usize, v: Vec<u8>) -> Vec<Value> {
    let mut a = args.to_vec();
    a[i] = Value::String(PhpString::from_vec(v));
    a
}

fn phar_fopen(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if active() {
        if let Some(f) = str_arg(args, 0) {
            let use_inc = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
            if use_inc || is_relative(&f) {
                if let Some(name) = name_for_relative(vm, &f, use_inc) {
                    let mut a = with_arg(args, 0, name);
                    if a.len() > 2 {
                        a[2] = Value::False;
                    }
                    return orig(vm, "fopen", &a);
                }
            }
        }
    }
    orig(vm, "fopen", args)
}

fn phar_file_get_contents(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if active() {
        if let Some(f) = str_arg(args, 0) {
            let use_inc = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            if use_inc || is_relative(&f) {
                if let Some(name) = name_for_relative(vm, &f, use_inc) {
                    let mut a = with_arg(args, 0, name);
                    if a.len() > 1 {
                        a[1] = Value::False;
                    }
                    return orig(vm, "file_get_contents", &a);
                }
            }
        }
    }
    orig(vm, "file_get_contents", args)
}

fn phar_readfile(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if active() {
        if let Some(f) = str_arg(args, 0) {
            let use_inc = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
            if use_inc || is_relative(&f) {
                if let Some(name) = name_for_relative(vm, &f, use_inc) {
                    let mut a = with_arg(args, 0, name);
                    if a.len() > 1 {
                        a[1] = Value::False;
                    }
                    return orig(vm, "readfile", &a);
                }
            }
        }
    }
    orig(vm, "readfile", args)
}

fn phar_opendir(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if active() {
        if let Some(f) = str_arg(args, 0) {
            if is_relative(&f) {
                if let Some(fname) = executed_filename(vm) {
                    if starts_with_ci(&fname, b"phar://") {
                        if let Ok((arch, _)) = split_fname(vm, &fname, 2, 0) {
                            let entry = fix_filepath(f.clone(), true);
                            let mut name = b"phar://".to_vec();
                            name.extend_from_slice(&arch);
                            if entry.first() != Some(&b'/') {
                                name.push(b'/');
                            }
                            name.extend_from_slice(&entry);
                            return orig(vm, "opendir", &with_arg(args, 0, name));
                        }
                    }
                }
            }
        }
    }
    orig(vm, "opendir", args)
}

const FS_PERMS: i32 = 0;
const FS_INODE: i32 = 1;
const FS_SIZE: i32 = 2;
const FS_OWNER: i32 = 3;
const FS_GROUP: i32 = 4;
const FS_ATIME: i32 = 5;
const FS_MTIME: i32 = 6;
const FS_CTIME: i32 = 7;
const FS_TYPE: i32 = 8;
const FS_IS_W: i32 = 9;
const FS_IS_R: i32 = 10;
const FS_IS_X: i32 = 11;
const FS_IS_FILE: i32 = 12;
const FS_IS_DIR: i32 = 13;
const FS_IS_LINK: i32 = 14;
const FS_EXISTS: i32 = 15;
const FS_LSTAT: i32 = 16;
const FS_STAT: i32 = 17;

fn is_exists_check(t: i32) -> bool {
    matches!(
        t,
        FS_EXISTS | FS_IS_W | FS_IS_R | FS_IS_X | FS_IS_FILE | FS_IS_DIR | FS_IS_LINK
    )
}

fn is_link_op(t: i32) -> bool {
    matches!(t, FS_TYPE | FS_IS_LINK | FS_LSTAT)
}

#[derive(Default)]
struct Sb {
    dev: i64,
    ino: i64,
    mode: i64,
    nlink: i64,
    uid: i64,
    gid: i64,
    rdev: i64,
    size: i64,
    atime: i64,
    mtime: i64,
    ctime: i64,
    blksize: i64,
    blocks: i64,
}

/// phar_fancy_stat()
fn fancy_stat(vm: &mut Vm, sb: &Sb, t: i32) -> Value {
    let (mut rmask, mut wmask, mut xmask) = (0o4, 0o2, 0o1);
    if (FS_IS_W..=FS_IS_X).contains(&t) {
        let (uid, gid) = unsafe { (libc::getuid() as i64, libc::getgid() as i64) };
        if sb.uid == uid {
            rmask = 0o400;
            wmask = 0o200;
            xmask = 0o100;
        } else if sb.gid == gid {
            rmask = 0o40;
            wmask = 0o20;
            xmask = 0o10;
        }
    }
    let b = |c: bool| if c { Value::True } else { Value::False };
    let fmt = sb.mode & 0o170000;
    match t {
        FS_PERMS => Value::Long(sb.mode),
        FS_INODE => Value::Long(sb.ino),
        FS_SIZE => Value::Long(sb.size),
        FS_OWNER => Value::Long(sb.uid),
        FS_GROUP => Value::Long(sb.gid),
        FS_ATIME => Value::Long(sb.atime),
        FS_MTIME => Value::Long(sb.mtime),
        FS_CTIME => Value::Long(sb.ctime),
        FS_TYPE => {
            if fmt == 0o120000 {
                return Value::String(PhpString::from_bytes(b"link"));
            }
            match fmt {
                0o040000 => Value::String(PhpString::from_bytes(b"dir")),
                0o100000 => Value::String(PhpString::from_bytes(b"file")),
                _ => {
                    let f = vm.current_builtin_name().unwrap_or_default();
                    vm.emit_notice_at(
                        &format!("{}(): Unknown file type ({})", f, fmt),
                        vm.current_line,
                    );
                    Value::String(PhpString::from_bytes(b"unknown"))
                }
            }
        }
        FS_IS_W => b(sb.mode & wmask != 0),
        FS_IS_R => b(sb.mode & rmask != 0),
        FS_IS_X => b(sb.mode & xmask != 0 && fmt != 0o040000),
        FS_IS_FILE => b(fmt == 0o100000),
        FS_IS_DIR => b(fmt == 0o040000),
        FS_IS_LINK => b(fmt == 0o120000),
        FS_EXISTS => Value::True,
        _ => {
            let vals = [
                sb.dev, sb.ino, sb.mode, sb.nlink, sb.uid, sb.gid, sb.rdev, sb.size, sb.atime,
                sb.mtime, sb.ctime, sb.blksize, sb.blocks,
            ];
            let names = [
                "dev", "ino", "mode", "nlink", "uid", "gid", "rdev", "size", "atime", "mtime",
                "ctime", "blksize", "blocks",
            ];
            let mut a = PhpArray::new();
            for v in vals {
                a.push(Value::Long(v));
            }
            for (n, v) in names.iter().zip(vals) {
                a.set(
                    ArrayKey::String(PhpString::from_bytes(n.as_bytes())),
                    Value::Long(v),
                );
            }
            crate::arr(a)
        }
    }
}

/// phar_file_stat(): Some(result) when served from the phar.
fn file_stat(vm: &mut Vm, filename: &[u8], t: i32) -> Option<Value> {
    if filename.is_empty() {
        return Some(Value::False);
    }
    if !is_relative(filename) {
        return None;
    }
    let fname = executed_filename(vm)?;
    if !starts_with_ci(&fname, b"phar://") {
        return None;
    }
    let (last, last_name) = g(|g| (g.last_phar.clone(), g.last_phar_name.clone()));
    let phar = match last {
        Some(lp) if fname.len() - 7 >= last_name.len() && fname[7..].starts_with(&last_name) => lp,
        _ => {
            let (arch, _) = split_fname(vm, &fname, 2, 0).ok()?;
            util::get_archive(&arch, None).ok()?
        }
    };
    let entry = fix_filepath(filename.to_vec(), true);
    let mut data: Option<EntryRef> = None;
    let mut sb = Sb::default();
    let mut statme = false;
    if entry.first() == Some(&b'/') {
        data = phar.borrow().find(&entry[1..]);
    } else {
        data = phar.borrow().find(&entry).or(data);
        if data.is_none() && phar.borrow().virtual_dirs.contains(&entry) {
            if is_exists_check(t) {
                return Some(Value::True);
            }
            sb.mode = 0o777 | 0o040000;
            let ts = phar.borrow().max_timestamp as i64;
            sb.mtime = ts;
            sb.atime = ts;
            sb.ctime = ts;
            statme = true;
        }
    }
    if data.is_none() && !statme {
        // notfound: the path without the current directory
        let save = g(|g| g.cwd.replace(Vec::new()));
        let entry = fix_filepath(filename.to_vec(), true);
        g(|g| g.cwd = save);
        let e2 = entry.get(1..).unwrap_or_default();
        if let Some(d) = phar.borrow().find(e2) {
            if is_exists_check(t) {
                return Some(Value::True);
            }
            data = Some(d);
        } else if phar.borrow().virtual_dirs.contains(e2) {
            if is_exists_check(t) {
                return Some(Value::True);
            }
            sb.mode = 0o777 | 0o040000;
            let ts = phar.borrow().max_timestamp as i64;
            sb.mtime = ts;
            sb.atime = ts;
            sb.ctime = ts;
        } else {
            if !is_exists_check(t) {
                let f = vm.current_builtin_name().unwrap_or_default();
                vm.emit_warning(&format!(
                    "{}(): {}stat failed for {}",
                    f,
                    if is_link_op(t) { "L" } else { "" },
                    l(filename)
                ));
            }
            return Some(Value::False);
        }
    }
    if let Some(d) = &data {
        let d = d.borrow();
        if !d.is_dir {
            sb.size = d.uncompressed_filesize as i64;
            sb.mode = (d.flags & PHAR_ENT_PERM_MASK) as i64;
            sb.mode |= if d.link.is_some() {
                0o100000 | 0o120000
            } else {
                0o100000
            };
        } else {
            sb.mode = (d.flags & PHAR_ENT_PERM_MASK) as i64 | 0o040000;
            if d.link.is_some() {
                sb.mode |= 0o120000;
            }
        }
        sb.mtime = d.timestamp as i64;
        sb.atime = d.timestamp as i64;
        sb.ctime = d.timestamp as i64;
        sb.ino = d.inode as i64;
    }
    if !phar.borrow().is_writeable {
        sb.mode = (sb.mode & 0o555) | (sb.mode & !0o777);
    }
    sb.nlink = 1;
    sb.rdev = -1;
    sb.dev = 0xc;
    sb.blksize = -1;
    sb.blocks = -1;
    Some(fancy_stat(vm, &sb, t))
}

macro_rules! stat_fns {
    ($( $f:ident => ($name:literal, $t:expr) ),* $(,)?) => {
        $(
            fn $f(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
                if g(|g| g.intercepted) {
                    if let Some(p) = str_arg(args, 0) {
                        if !p.contains(&0) && args.len() == 1 {
                            if let Some(v) = file_stat(vm, &p, $t) {
                                return Ok(v);
                            }
                        }
                    }
                }
                orig(vm, $name, args)
            }
        )*
        const STAT_FNS: &[(&str, BuiltinFn)] = &[ $( ($name, $f) ),* ];
    };
}

stat_fns! {
    phar_fileperms => ("fileperms", FS_PERMS),
    phar_fileinode => ("fileinode", FS_INODE),
    phar_filesize => ("filesize", FS_SIZE),
    phar_fileowner => ("fileowner", FS_OWNER),
    phar_filegroup => ("filegroup", FS_GROUP),
    phar_fileatime => ("fileatime", FS_ATIME),
    phar_filemtime => ("filemtime", FS_MTIME),
    phar_filectime => ("filectime", FS_CTIME),
    phar_filetype => ("filetype", FS_TYPE),
    phar_is_writable => ("is_writable", FS_IS_W),
    phar_is_readable => ("is_readable", FS_IS_R),
    phar_is_executable => ("is_executable", FS_IS_X),
    phar_file_exists => ("file_exists", FS_EXISTS),
    phar_is_dir => ("is_dir", FS_IS_DIR),
    phar_lstat => ("lstat", FS_LSTAT),
    phar_stat => ("stat", FS_STAT),
}

/// phar_is_file / phar_is_link
fn file_or_link(vm: &mut Vm, args: &[Value], link: bool) -> Option<Value> {
    if !active() {
        return None;
    }
    let p = str_arg(args, 0)?;
    if !is_relative(&p) {
        return None;
    }
    let fname = executed_filename(vm)?;
    if !starts_with_ci(&fname, b"phar://") {
        return None;
    }
    let (arch, _) = split_fname(vm, &fname, 2, 0).ok()?;
    if let Ok(phar) = util::get_archive(&arch, None) {
        let entry = fix_filepath(p.clone(), true);
        let e = if entry.first() == Some(&b'/') {
            phar.borrow().find(&entry[1..])
        } else {
            phar.borrow().find(&entry)
        };
        if let Some(e) = e {
            let e = e.borrow();
            return Some(if link {
                if e.link.is_some() {
                    Value::True
                } else {
                    Value::False
                }
            } else if e.is_dir {
                Value::False
            } else {
                Value::True
            });
        }
    }
    Some(Value::False)
}

fn phar_is_file(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match file_or_link(vm, args, false) {
        Some(v) => Ok(v),
        None => orig(vm, "is_file", args),
    }
}

fn phar_is_link(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    match file_or_link(vm, args, true) {
        Some(v) => Ok(v),
        None => orig(vm, "is_link", args),
    }
}

/// phar_intercept_functions_init()
pub fn register(vm: &mut Vm) {
    let mut list: Vec<(&'static str, BuiltinFn)> = vec![
        ("fopen", phar_fopen),
        ("file_get_contents", phar_file_get_contents),
        ("is_file", phar_is_file),
        ("is_link", phar_is_link),
        ("opendir", phar_opendir),
        ("readfile", phar_readfile),
    ];
    list.extend_from_slice(STAT_FNS);
    for (name, f) in list {
        if vm.functions.contains_key(name.as_bytes()) {
            if let Some(o) = vm.override_function(name.as_bytes(), f) {
                ORIG.with(|m| m.borrow_mut().insert(name, o));
            }
        }
    }
}
