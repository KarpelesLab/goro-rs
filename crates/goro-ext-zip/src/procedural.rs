//! The deprecated procedural API (zip_open(), zip_read(), zip_entry_*()).

use std::cell::RefCell;
use std::collections::HashMap;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use crate::zipint::*;
use crate::zpp::{self, Func};

const LE_DIR: &[u8] = b"Zip Directory";
const LE_ENTRY: &[u8] = b"Zip Entry";

/// zip_rsrc
struct DirRes {
    za: Option<Archive>,
    index_current: u64,
    num_files: i64,
}

/// zip_read_rsrc
struct EntryRes {
    zf: Option<ZipFile>,
    sb: Stat,
    dir_handle: i64,
}

thread_local! {
    static DIRS: RefCell<HashMap<i64, DirRes>> = RefCell::new(HashMap::new());
    static ENTRIES: RefCell<HashMap<i64, EntryRes>> = RefCell::new(HashMap::new());
}

type R = Result<Value, VmError>;

fn err(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn, &str)] = &[
    ("zip_open", zip_open, ", use ZipArchive::open() instead"),
    ("zip_close", zip_close, ", use ZipArchive::close() instead"),
    (
        "zip_read",
        zip_read,
        ", use ZipArchive::statIndex() instead",
    ),
    ("zip_entry_open", zip_entry_open, ""),
    ("zip_entry_close", zip_entry_close, ""),
    (
        "zip_entry_read",
        zip_entry_read,
        ", use ZipArchive::getFromIndex() instead",
    ),
    (
        "zip_entry_name",
        zip_entry_name,
        ", use ZipArchive::statIndex() instead",
    ),
    (
        "zip_entry_compressedsize",
        zip_entry_compressedsize,
        ", use ZipArchive::statIndex() instead",
    ),
    (
        "zip_entry_filesize",
        zip_entry_filesize,
        ", use ZipArchive::statIndex() instead",
    ),
    (
        "zip_entry_compressionmethod",
        zip_entry_compressionmethod,
        ", use ZipArchive::statIndex() instead",
    ),
];

pub fn register(vm: &mut Vm) {
    for &(name, f, _) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
    }
    goro_core::value::register_resource_type_dtor(LE_DIR, free_dir);
    goro_core::value::register_resource_type_dtor(LE_ENTRY, free_entry);
}

/// php_zip_free_dir(): the last reference to a zip_open() resource went
/// away (it leaves the resource list).
fn free_dir(vm: &mut Vm, id: i64) {
    let r = DIRS.with(|d| d.borrow_mut().remove(&id));
    if let Some(mut za) = r.and_then(|mut r| r.za.take()) {
        if za.close(None).is_err() {
            vm.emit_warning("Cannot destroy the zip context");
        }
    }
    goro_core::value::remove_resource(id);
}

/// php_zip_free_entry()
fn free_entry(_vm: &mut Vm, id: i64) {
    ENTRIES.with(|e| e.borrow_mut().remove(&id));
    goro_core::value::remove_resource(id);
}

/// The #[\Deprecated] attribute of the function: Err when the handler threw.
fn deprecated(vm: &mut Vm, name: &str) -> Result<(), VmError> {
    let suffix = FUNCTIONS.iter().find(|f| f.0 == name).map_or("", |f| f.2);
    vm.emit_function_deprecation(&format!(
        "Function {}() is deprecated since 8.0{}",
        name, suffix
    ))
}

fn args_or_throw(
    vm: &mut Vm,
    f: &Func,
    spec: &str,
    args: &[Value],
) -> Result<Vec<zpp::Arg>, VmError> {
    zpp::parse(vm, f, spec, args).map_err(|_| err(vm))
}

/// Z_PARAM_RESOURCE then zend_fetch_resource()
fn fetch_res(vm: &mut Vm, f: &Func, args: &[Value], i: usize, le: &[u8]) -> Result<i64, VmError> {
    let v = args.get(i).map(|v| v.deref()).unwrap_or(Value::Null);
    let Value::Resource(id) = v else {
        f.type_error(vm, i + 1, "resource", &v);
        return Err(err(vm));
    };
    let ok = goro_core::value::is_open_resource(id.id())
        && goro_core::value::resource_type_of(id.id()).as_deref() == Some(le);
    if !ok {
        let m = format!(
            "{}(): supplied resource is not a valid {} resource",
            f.name,
            String::from_utf8_lossy(le)
        );
        zpp::throw(vm, b"TypeError", &m);
        return Err(err(vm));
    }
    Ok(id.id())
}

fn zip_open(vm: &mut Vm, args: &[Value]) -> R {
    deprecated(vm, "zip_open")?;
    let f = Func::function("zip_open");
    let a = args_or_throw(vm, &f, "P", args)?;
    let filename = a[0].str_or(b"").to_vec();
    if filename.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(err(vm));
    }
    if crate::basedir_refused(vm, &f.name, &filename) {
        return Ok(Value::False);
    }
    let Some(resolved) = crate::expand_filepath(&filename) else {
        f.warn(vm, "No such file or directory");
        return Ok(Value::False);
    };
    match Archive::open(&resolved, 0) {
        Ok(za) => {
            let num_files = za.num_entries(0);
            let id = goro_core::value::alloc_resource_id();
            goro_core::value::register_resource(id, LE_DIR);
            DIRS.with(|d| {
                d.borrow_mut().insert(
                    id,
                    DirRes {
                        za: Some(za),
                        index_current: 0,
                        num_files,
                    },
                )
            });
            Ok(Value::resource(id))
        }
        Err(e) => Ok(Value::Long(e.zip_err as i64)),
    }
}

fn zip_close(vm: &mut Vm, args: &[Value]) -> R {
    deprecated(vm, "zip_close")?;
    let f = Func::function("zip_close");
    let id = fetch_res(vm, &f, args, 0, LE_DIR)?;
    // zend_list_close(): php_zip_free_dir()
    let r = DIRS.with(|d| d.borrow_mut().remove(&id));
    if let Some(mut r) = r {
        if let Some(mut za) = r.za.take() {
            if za.close(None).is_err() {
                f.warn(vm, "Cannot destroy the zip context");
            }
        }
    }
    goro_core::value::close_resource(id);
    Ok(Value::Null)
}

fn zip_read(vm: &mut Vm, args: &[Value]) -> R {
    deprecated(vm, "zip_read")?;
    let f = Func::function("zip_read");
    let id = fetch_res(vm, &f, args, 0, LE_DIR)?;
    let r = DIRS.with(|d| {
        let mut d = d.borrow_mut();
        let r = d.get_mut(&id)?;
        let za = r.za.as_mut()?;
        if r.index_current as i64 >= r.num_files {
            return None;
        }
        let sb = za.stat_index(r.index_current, 0)?;
        let pw = za.default_password.clone();
        let zf = za.fopen_index(r.index_current, 0, pw.as_deref())?;
        r.index_current += 1;
        Some((sb, zf))
    });
    match r {
        Some((sb, zf)) => {
            let eid = goro_core::value::alloc_resource_id();
            goro_core::value::register_resource(eid, LE_ENTRY);
            ENTRIES.with(|e| {
                e.borrow_mut().insert(
                    eid,
                    EntryRes {
                        zf: Some(zf),
                        sb,
                        dir_handle: id,
                    },
                )
            });
            Ok(Value::resource(eid))
        }
        None => Ok(Value::False),
    }
}

fn zip_entry_open(vm: &mut Vm, args: &[Value]) -> R {
    deprecated(vm, "zip_entry_open")?;
    let f = Func::function("zip_entry_open");
    if args.len() < 2 || args.len() > 3 {
        let msg = if args.len() < 2 {
            format!(
                "zip_entry_open() expects at least 2 arguments, {} given",
                args.len()
            )
        } else {
            format!(
                "zip_entry_open() expects at most 3 arguments, {} given",
                args.len()
            )
        };
        zpp::throw(vm, b"ArgumentCountError", &msg);
        return Err(err(vm));
    }
    let eid = fetch_res(vm, &f, args, 1, LE_ENTRY)?;
    fetch_res(vm, &f, args, 0, LE_DIR)?;
    let has = ENTRIES.with(|e| e.borrow().get(&eid).is_some_and(|r| r.zf.is_some()));
    Ok(if has { Value::True } else { Value::False })
}

fn zip_entry_close(vm: &mut Vm, args: &[Value]) -> R {
    deprecated(vm, "zip_entry_close")?;
    let f = Func::function("zip_entry_close");
    let eid = fetch_res(vm, &f, args, 0, LE_ENTRY)?;
    ENTRIES.with(|e| e.borrow_mut().remove(&eid));
    goro_core::value::close_resource(eid);
    Ok(Value::True)
}

fn zip_entry_read(vm: &mut Vm, args: &[Value]) -> R {
    deprecated(vm, "zip_entry_read")?;
    let f = Func::function("zip_entry_read");
    if args.is_empty() || args.len() > 2 {
        let msg = if args.is_empty() {
            "zip_entry_read() expects at least 1 argument, 0 given".to_string()
        } else {
            format!(
                "zip_entry_read() expects at most 2 arguments, {} given",
                args.len()
            )
        };
        zpp::throw(vm, b"ArgumentCountError", &msg);
        return Err(err(vm));
    }
    let eid = fetch_res(vm, &f, args, 0, LE_ENTRY)?;
    let mut len = 0i64;
    if args.len() > 1 {
        let a = args_or_throw(vm, &f, "zl", args)?;
        len = a[1].long_or(0);
    }
    if len <= 0 {
        len = 1024;
    }
    let r = ENTRIES.with(|e| {
        let mut e = e.borrow_mut();
        let r = e.get_mut(&eid)?;
        let zf = r.zf.as_mut()?;
        Some(zf.read(len as usize))
    });
    Ok(match r {
        None => Value::False,
        Some(Ok(b)) if !b.is_empty() => Value::String(PhpString::from_vec(b)),
        Some(_) => Value::String(PhpString::empty()),
    })
}

/// php_zip_entry_get_info()
fn entry_info(vm: &mut Vm, name: &str, args: &[Value], opt: u8) -> R {
    deprecated(vm, name)?;
    let f = Func::function(name);
    if args.len() != 1 {
        let msg = format!(
            "{}() expects exactly 1 argument, {} given",
            name,
            args.len()
        );
        zpp::throw(vm, b"ArgumentCountError", &msg);
        return Err(err(vm));
    }
    let eid = fetch_res(vm, &f, args, 0, LE_ENTRY)?;
    let info = ENTRIES.with(|e| {
        let e = e.borrow();
        let r = e.get(&eid)?;
        r.zf.as_ref()?;
        if !goro_core::value::is_registered_resource(r.dir_handle) {
            return None;
        }
        Some(r.sb.clone())
    });
    let Some(sb) = info else {
        return Ok(Value::False);
    };
    Ok(match opt {
        0 => Value::String(PhpString::from_bytes(c_str(&sb.name))),
        1 => Value::Long(sb.comp_size as i64),
        2 => Value::Long(sb.size as i64),
        _ => {
            let m: &str = match sb.comp_method {
                0 => "stored",
                1 => "shrunk",
                2..=5 => "reduced",
                6 => "imploded",
                7 => "tokenized",
                8 => "deflated",
                9 => "deflatedX",
                10 => "implodedX",
                _ => return Ok(Value::False),
            };
            Value::String(PhpString::from_bytes(m.as_bytes()))
        }
    })
}

fn zip_entry_name(vm: &mut Vm, args: &[Value]) -> R {
    entry_info(vm, "zip_entry_name", args, 0)
}

fn zip_entry_compressedsize(vm: &mut Vm, args: &[Value]) -> R {
    entry_info(vm, "zip_entry_compressedsize", args, 1)
}

fn zip_entry_filesize(vm: &mut Vm, args: &[Value]) -> R {
    entry_info(vm, "zip_entry_filesize", args, 2)
}

fn zip_entry_compressionmethod(vm: &mut Vm, args: &[Value]) -> R {
    entry_info(vm, "zip_entry_compressionmethod", args, 3)
}
