//! The ZipArchive methods (php_zip.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_standard::ext_io;

use crate::zipint::*;
use crate::zpp::{self, Arg, Func, Obj};
use crate::{ZipData, basedir_refused, data_or_attach, expand_filepath, stat_array, vbool, vstr};

type M = fn(&mut Vm, &Func, &Obj, &[Value]) -> Result<Value, ()>;

const METHODS: &[(&str, M)] = &[
    ("open", m_open),
    ("setPassword", m_set_password),
    ("close", m_close),
    ("count", m_count),
    ("getStatusString", m_get_status_string),
    ("clearError", m_clear_error),
    ("addEmptyDir", m_add_empty_dir),
    ("addFromString", m_add_from_string),
    ("addFile", m_add_file),
    ("replaceFile", m_replace_file),
    ("addGlob", m_add_glob),
    ("addPattern", m_add_pattern),
    ("renameIndex", m_rename_index),
    ("renameName", m_rename_name),
    ("setArchiveComment", m_set_archive_comment),
    ("getArchiveComment", m_get_archive_comment),
    ("setArchiveFlag", m_set_archive_flag),
    ("getArchiveFlag", m_get_archive_flag),
    ("setCommentIndex", m_set_comment_index),
    ("setCommentName", m_set_comment_name),
    ("setMtimeIndex", m_set_mtime_index),
    ("setMtimeName", m_set_mtime_name),
    ("getCommentIndex", m_get_comment_index),
    ("getCommentName", m_get_comment_name),
    ("deleteIndex", m_delete_index),
    ("deleteName", m_delete_name),
    ("statName", m_stat_name),
    ("statIndex", m_stat_index),
    ("locateName", m_locate_name),
    ("getNameIndex", m_get_name_index),
    ("unchangeArchive", m_unchange_archive),
    ("unchangeAll", m_unchange_all),
    ("unchangeIndex", m_unchange_index),
    ("unchangeName", m_unchange_name),
    ("extractTo", m_extract_to),
    ("getFromName", m_get_from_name),
    ("getFromIndex", m_get_from_index),
    ("getStreamIndex", m_get_stream_index),
    ("getStreamName", m_get_stream_name),
    ("getStream", m_get_stream),
    ("setExternalAttributesName", m_set_ext_attr_name),
    ("setExternalAttributesIndex", m_set_ext_attr_index),
    ("getExternalAttributesName", m_get_ext_attr_name),
    ("getExternalAttributesIndex", m_get_ext_attr_index),
    ("setCompressionName", m_set_compression_name),
    ("setCompressionIndex", m_set_compression_index),
    ("setEncryptionName", m_set_encryption_name),
    ("setEncryptionIndex", m_set_encryption_index),
    ("registerProgressCallback", m_register_progress),
    ("registerCancelCallback", m_register_cancel),
    ("isCompressionMethodSupported", m_is_compression_supported),
    ("isEncryptionMethodSupported", m_is_encryption_supported),
];

const STATIC_METHODS: &[&str] = &[
    "isCompressionMethodSupported",
    "isEncryptionMethodSupported",
];

fn find(method: &[u8]) -> Option<&'static (&'static str, M)> {
    METHODS
        .iter()
        .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(method))
}

pub fn has_method(_class: &[u8], method: &[u8], is_static: bool) -> bool {
    match find(method) {
        Some(m) => !is_static || STATIC_METHODS.contains(&m.0),
        None => false,
    }
}

pub fn param_names(_class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, m) = goro_core::stubs::find_method(b"ziparchive", method)?;
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<Vec<u8>, &'static [&'static str]>> =
            RefCell::new(std::collections::HashMap::new());
    }
    let key = method.to_ascii_lowercase();
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return Some(v);
    }
    let names: Vec<&'static str> = m.args.iter().map(|a| a.name).collect();
    let leaked: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    CACHE.with(|c| c.borrow_mut().insert(key, leaked));
    Some(leaked)
}

fn this_obj(this: Option<&Value>) -> Option<Obj> {
    match this {
        Some(Value::Object(o)) => Some(o.clone()),
        Some(Value::Reference(r)) => match &*r.borrow() {
            Value::Object(o) => Some(o.clone()),
            _ => None,
        },
        _ => None,
    }
}

pub fn call_method(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some(&(name, imp)) = find(method) else {
        return Value::Null;
    };
    crate::flush_pending(vm);
    let f = Func::method("ZipArchive", name);
    let is_static = STATIC_METHODS.contains(&name);
    let o = match this_obj(this) {
        Some(o) => o,
        None => {
            if is_static {
                return m_static(vm, &f, name, args).unwrap_or(Value::Null);
            }
            zpp::throw(
                vm,
                b"Error",
                &format!("Non-static method {}() cannot be called statically", f.name),
            );
            return Value::Null;
        }
    };
    if is_static {
        return m_static(vm, &f, name, args).unwrap_or(Value::Null);
    }
    // When invoked from the engine there may be no call frame yet.
    let frame = format!("ZipArchive->{}", name);
    let has = vm
        .call_stack
        .last()
        .map(|fr| fr.0.eq_ignore_ascii_case(&frame))
        .unwrap_or(false);
    if !has {
        let line = vm.current_line;
        let file = vm.current_file.clone();
        vm.call_stack.push((frame, file, line, args.to_vec(), true));
    }
    let r = imp(vm, &f, &o, args);
    if !has {
        vm.call_stack.pop();
    }
    r.unwrap_or(Value::Null)
}

fn m_static(vm: &mut Vm, f: &Func, name: &str, args: &[Value]) -> Result<Value, ()> {
    let a = zpp::parse(vm, f, "l|b", args)?;
    let method = a[0].long_or(0);
    let enc = a[1].bool_or(true);
    Ok(vbool(if name == "isCompressionMethodSupported" {
        method as i32 == CM_STORE as i32 || crate::codec::compression_supported(method as i32, enc)
    } else {
        crate::codec::encryption_supported(method as u16, enc)
    }))
}

type Mr = Result<Value, ()>;

fn data(o: &Obj) -> Rc<ZipData> {
    data_or_attach(o)
}

/// ZIP_FROM_OBJECT(): Err after the ValueError
fn require_za(vm: &mut Vm, d: &ZipData) -> Result<(), ()> {
    if d.obj.borrow().za.is_none() {
        zpp::throw(vm, b"ValueError", "Invalid or uninitialized Zip object");
        return Err(());
    }
    Ok(())
}

/// Run `f` on the archive (ZIP_FROM_OBJECT first).
fn with_za<T>(vm: &mut Vm, d: &ZipData, f: impl FnOnce(&mut Archive) -> T) -> Result<T, ()> {
    require_za(vm, d)?;
    let mut o = d.obj.borrow_mut();
    Ok(f(o.za.as_mut().unwrap()))
}

fn s(b: &[u8]) -> Value {
    vstr(b)
}

// -- open / close -------------------------------------------------------------

fn m_open(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "P|l", args)?;
    let filename = a[0].str_or(b"").to_vec();
    let mut flags = a[1].long_or(0);
    let d = data(o);
    if filename.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    if basedir_refused(vm, &f.name, &filename) {
        return Ok(Value::False);
    }
    let Some(resolved) = expand_filepath(&filename) else {
        f.warn(vm, "No such file or directory");
        return Ok(Value::False);
    };
    let old = d.obj.borrow_mut().za.take();
    if let Some(mut za) = old {
        if za.close(None).is_err() {
            f.warn(vm, "Empty string as source");
            return Ok(Value::False);
        }
    }
    d.obj.borrow_mut().filename = None;
    if flags & (ZIP_TRUNCATE | ZIP_RDONLY) as i64 == 0 {
        if let Ok(md) = std::fs::metadata(&resolved) {
            if md.len() == 0 {
                let line = vm.current_line;
                vm.emit_deprecated_at(
                    &format!("{}(): Using empty file as ZipArchive is deprecated", f.name),
                    line,
                );
                if vm.current_exception.is_some() {
                    return Err(());
                }
                flags |= ZIP_TRUNCATE as i64;
            }
        }
    }
    if flags < 0 {
        return Ok(Value::Long(ER_INVAL as i64));
    }
    match Archive::open(&resolved, flags as u32) {
        Ok(za) => {
            let mut zo = d.obj.borrow_mut();
            zo.filename = Some(resolved);
            zo.za = Some(za);
            Ok(Value::True)
        }
        Err(e) => Ok(Value::Long(e.zip_err as i64)),
    }
}

fn m_set_password(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "s", args)?;
    let pw = a[0].str_or(b"").to_vec();
    let d = data(o);
    require_za(vm, &d)?;
    if pw.is_empty() {
        return Ok(Value::False);
    }
    with_za(vm, &d, |za| za.set_default_password(Some(&pw)))?;
    Ok(Value::True)
}

/// The callbacks of the object, called during zip_close().
struct Hooks<'a> {
    vm: &'a mut Vm,
    progress: Option<(f64, Value)>,
    cancel: Option<Value>,
}

impl ProgressHooks for Hooks<'_> {
    fn progress(&mut self, state: f64) {
        // zend_call_function() does nothing while an exception is pending
        if self.vm.current_exception.is_some() {
            return;
        }
        if let Some((_, cb)) = &self.progress {
            let cb = cb.clone();
            let _ = self.vm.call_callback(&cb, &[Value::Double(state)]);
        }
    }

    /// php_zip_cancel_callback()
    fn cancel(&mut self) -> i64 {
        let Some(cb) = self.cancel.clone() else {
            return 0;
        };
        if self.vm.current_exception.is_some() {
            return -1;
        }
        let r = self.vm.call_callback(&cb, &[]);
        if self.vm.current_exception.is_some() {
            return -1;
        }
        let v = match r {
            Ok(v) => v,
            Err(_) => return -1,
        };
        let v = v.deref();
        let n = match &v {
            Value::Long(n) => Some(*n),
            Value::Double(d) if d.is_finite() && d.fract() == 0.0 => Some(*d as i64),
            Value::True => Some(1),
            Value::False | Value::Null => Some(0),
            Value::String(s) => {
                let t = s.to_string_lossy();
                t.trim().parse::<i64>().ok()
            }
            _ => None,
        };
        match n {
            Some(n) => n as i32 as i64,
            None => {
                let m = format!(
                    "Return value of callback provided to ZipArchive::registerCancelCallback() must be of type int, {} returned",
                    Vm::value_type_name(&v)
                );
                zpp::throw(self.vm, b"TypeError", &m);
                -1
            }
        }
    }

    fn has_progress(&self) -> bool {
        self.progress.is_some()
    }

    fn has_cancel(&self) -> bool {
        self.cancel.is_some()
    }

    fn precision(&self) -> f64 {
        self.progress.as_ref().map_or(0.0, |p| p.0)
    }
}

/// zip_close() of the object's archive with its callbacks: Ok or the
/// error of the failed close (the archive is discarded either way).
pub fn close_archive(vm: &mut Vm, d: &ZipData) -> Result<(), ZipError> {
    let (za, progress, cancel) = {
        let mut zo = d.obj.borrow_mut();
        (zo.za.take(), zo.progress.clone(), zo.cancel.clone())
    };
    let Some(mut za) = za else { return Ok(()) };
    let r = if progress.is_some() || cancel.is_some() {
        let mut h = Hooks {
            vm,
            progress,
            cancel,
        };
        za.close(Some(&mut h))
    } else {
        za.close(None)
    };
    // the callbacks are released with the archive
    let mut zo = d.obj.borrow_mut();
    zo.progress = None;
    zo.cancel = None;
    match r {
        Ok(()) => Ok(()),
        Err(()) => Err(za.error),
    }
}

fn m_close(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    zpp::parse(vm, f, "", args)?;
    let d = data(o);
    require_za(vm, &d)?;
    let r = close_archive(vm, &d);
    let err = match r {
        Ok(()) => {
            let mut zo = d.obj.borrow_mut();
            zo.err_zip = 0;
            zo.err_sys = 0;
            false
        }
        Err(e) => {
            let msg = e.strerror();
            f.warn(vm, &msg);
            let mut zo = d.obj.borrow_mut();
            zo.err_zip = e.zip_err;
            zo.err_sys = e.sys_err;
            true
        }
    };
    let fname = d.obj.borrow_mut().filename.take();
    if let Some(fname) = fname {
        let _ = ext_io::call_as(
            vm,
            &f.name,
            b"clearstatcache",
            &[Value::True, Value::String(PhpString::from_string(fname))],
        );
    }
    Ok(vbool(!err))
}

fn m_count(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    zpp::parse(vm, f, "", args)?;
    let d = data(o);
    let n = with_za(vm, &d, |za| za.num_entries(0))?;
    Ok(Value::Long(n))
}

fn m_clear_error(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    zpp::parse(vm, f, "", args)?;
    let d = data(o);
    let mut zo = d.obj.borrow_mut();
    match zo.za.as_mut() {
        Some(za) => za.error.clear(),
        None => {
            zo.err_zip = 0;
            zo.err_sys = 0;
        }
    }
    Ok(Value::Null)
}

fn m_get_status_string(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    zpp::parse(vm, f, "", args)?;
    let d = data(o);
    let zo = d.obj.borrow();
    let e = match &zo.za {
        Some(za) => za.error,
        None => ZipError::new(zo.err_zip, zo.err_sys),
    };
    Ok(s(e.strerror().as_bytes()))
}

// -- adding ---------------------------------------------------------------------

fn m_add_empty_dir(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "s|l", args)?;
    let dirname = a[0].str_or(b"").to_vec();
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    if dirname.is_empty() {
        return Ok(Value::False);
    }
    let mut zo = d.obj.borrow_mut();
    let za = zo.za.as_mut().unwrap();
    let id = za.dir_add(&dirname, flags);
    let ok = id != -1;
    if ok {
        za.error.clear();
    }
    zo.last_id = id;
    Ok(vbool(ok))
}

fn m_add_from_string(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "sS|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let buffer = a[1].str_or(b"").to_vec();
    let flags = a[2].long_or(FL_OVERWRITE as i64) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    let mut zo = d.obj.borrow_mut();
    let za = zo.za.as_mut().unwrap();
    let id = za.file_replace(None, Some(&name), Source::buffer(buffer), flags);
    let ok = id != -1;
    if ok {
        za.error.clear();
    }
    zo.last_id = id;
    Ok(vbool(ok))
}

/// php_zip_add_file(): 1 or -1
#[allow(clippy::too_many_arguments)]
pub fn add_file(
    vm: &mut Vm,
    f: &Func,
    d: &ZipData,
    filename: &[u8],
    entry_name: &[u8],
    start: u64,
    len: i64,
    replace: i64,
    flags: u32,
) -> i32 {
    if basedir_refused(vm, &f.name, filename) {
        return -1;
    }
    let Some(resolved) = expand_filepath(filename) else {
        f.warn(vm, "No such file or directory");
        return -1;
    };
    if std::fs::metadata(&resolved).is_err() {
        f.warn(vm, "No such file or directory");
        return -1;
    }
    let mut flags = flags;
    let mut zo = d.obj.borrow_mut();
    let Some(za) = zo.za.as_mut() else {
        return -1;
    };
    let src = if flags & crate::FL_OPEN_FILE_NOW != 0 {
        if std::fs::File::open(&resolved).is_err() {
            return -1;
        }
        flags ^= crate::FL_OPEN_FILE_NOW;
        match Source::file(&resolved, start, len) {
            Ok(s) => match s.read_all() {
                Ok(data) => {
                    let mtime = s.stat().mtime;
                    let _ = mtime;
                    s.preloaded(data)
                }
                Err(e) => {
                    za.error = e;
                    return -1;
                }
            },
            Err(e) => {
                za.error = e;
                return -1;
            }
        }
    } else {
        match Source::file(&resolved, start, len) {
            Ok(s) => s,
            Err(e) => {
                za.error = e;
                return -1;
            }
        }
    };
    if replace >= 0 {
        if za.entries.len() as u64 <= replace as u64 {
            za.error.set(ER_INVAL, 0);
            return -1;
        }
        if za.file_replace(Some(replace as u64), None, src, flags) < 0 {
            return -1;
        }
        za.error.clear();
        return 1;
    }
    let id = za.file_replace(None, Some(entry_name), src, flags);
    zo.last_id = id;
    if id < 0 {
        return -1;
    }
    zo.za.as_mut().unwrap().error.clear();
    1
}

fn m_add_file(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "P|slll", args)?;
    let filename = a[0].str_or(b"").to_vec();
    let mut entry = a[1].str_or(b"").to_vec();
    let start = a[2].long_or(0);
    let len = a[3].long_or(0);
    let flags = a[4].long_or(FL_OVERWRITE as i64) as u32;
    if filename.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    if entry.is_empty() {
        entry = filename.clone();
    }
    let d = data(o);
    let r = add_file(vm, f, &d, &filename, &entry, start as u64, len, -1, flags);
    Ok(vbool(r >= 0))
}

fn m_replace_file(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "Pl|lll", args)?;
    let filename = a[0].str_or(b"").to_vec();
    let index = a[1].long_or(0);
    let start = a[2].long_or(0);
    let len = a[3].long_or(0);
    let flags = a[4].long_or(0) as u32;
    if filename.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    if index < 0 {
        f.value_error(vm, 2, "must be greater than or equal to 0");
        return Err(());
    }
    let d = data(o);
    let r = add_file(vm, f, &d, &filename, b"", start as u64, len, index, flags);
    Ok(vbool(r >= 0))
}

/// zip_options
#[derive(Default)]
struct Options {
    remove_all_path: bool,
    remove_path: Option<Vec<u8>>,
    add_path: Option<Vec<u8>>,
    flags: u32,
    comp_method: i64,
    comp_flags: u32,
    enc_method: i64,
    enc_password: Option<Vec<u8>>,
}

/// php_zip_parse_options(): Err after an exception
fn parse_options(vm: &mut Vm, f: &Func, opts: &PhpArray) -> Result<Options, ()> {
    let mut o = Options {
        flags: FL_OVERWRITE,
        comp_method: -1,
        enc_method: -1,
        ..Default::default()
    };
    let get = |k: &str| {
        opts.get(&ArrayKey::String(PhpString::from_bytes(k.as_bytes())))
            .map(|v| v.deref())
    };
    let tname = |v: &Value| match v {
        Value::Null => "null".to_string(),
        Value::True => "true".to_string(),
        Value::False => "false".to_string(),
        v => Vm::value_type_name(v),
    };
    if let Some(v) = get("remove_all_path") {
        if !matches!(v, Value::True | Value::False) {
            f.warn(
                vm,
                &format!(
                    "Option \"remove_all_path\" must be of type bool, {} given",
                    tname(&v)
                ),
            );
        }
        o.remove_all_path = v.to_long() != 0;
    }
    if let Some(v) = get("comp_method") {
        if !matches!(v, Value::Long(_)) {
            f.warn(
                vm,
                &format!(
                    "Option \"comp_method\" must be of type int, {} given",
                    tname(&v)
                ),
            );
        }
        o.comp_method = v.to_long();
        if let Some(v) = get("comp_flags") {
            if !matches!(v, Value::Long(_)) {
                f.warn(
                    vm,
                    &format!(
                        "Option \"comp_flags\" must be of type int, {} given",
                        tname(&v)
                    ),
                );
            }
            o.comp_flags = v.to_long() as u32;
        }
    }
    if let Some(v) = get("enc_method") {
        if !matches!(v, Value::Long(_)) {
            f.warn(
                vm,
                &format!(
                    "Option \"enc_method\" must be of type int, {} given",
                    tname(&v)
                ),
            );
        }
        o.enc_method = v.to_long();
        if let Some(v) = get("enc_password") {
            match v {
                Value::String(s) => o.enc_password = Some(s.as_bytes().to_vec()),
                v => {
                    let m = format!(
                        "Option \"enc_password\" must be of type string, {} given",
                        tname(&v)
                    );
                    zpp::throw(vm, b"TypeError", &m);
                    return Err(());
                }
            }
        }
    }
    for (key, slot) in [("remove_path", 0), ("add_path", 1)] {
        if let Some(v) = get(key) {
            let Value::String(sv) = v else {
                let m = format!(
                    "Option \"{}\" must be of type string, {} given",
                    key,
                    tname(&v)
                );
                zpp::throw(vm, b"TypeError", &m);
                return Err(());
            };
            if sv.is_empty() {
                let m = format!("Option \"{}\" must not be empty", key);
                zpp::throw(vm, b"ValueError", &m);
                return Err(());
            }
            if sv.len() >= 4096 {
                let m = format!("Option \"{}\" must be less than {} bytes", key, 4096 - 1);
                zpp::throw(vm, b"ValueError", &m);
                return Err(());
            }
            if slot == 0 {
                o.remove_path = Some(sv.as_bytes().to_vec());
            } else {
                o.add_path = Some(sv.as_bytes().to_vec());
            }
        }
    }
    if let Some(v) = get("flags") {
        let Value::Long(n) = v else {
            let m = format!("Option \"flags\" must be of type int, {} given", tname(&v));
            zpp::throw(vm, b"TypeError", &m);
            return Err(());
        };
        o.flags = n as u32;
    }
    Ok(o)
}

const GLOB_ONLYDIR: i64 = 1 << 13;

/// php_zip_glob(): Err(-1) / Ok(list)
fn zip_glob(vm: &mut Vm, f: &Func, pattern: &[u8], flags: i64) -> Result<Vec<Vec<u8>>, ()> {
    if pattern.len() >= 4096 {
        f.warn(
            vm,
            &format!(
                "Pattern exceeds the maximum allowed length of {} characters",
                4096
            ),
        );
        return Err(());
    }
    // PHP_GLOB_AVAILABLE_FLAGS
    let available: i64 = 0x0001
        | 0x0002
        | 0x0004
        | 0x0008
        | 0x0010
        | 0x0040
        | 0x0080
        | 0x0400
        | 0x1000
        | GLOB_ONLYDIR;
    let _ = available;
    let r = ext_io::call_as(
        vm,
        &f.name,
        b"glob",
        &[
            Value::String(PhpString::from_bytes(pattern)),
            Value::Long(flags),
        ],
    );
    if vm.current_exception.is_some() {
        return Err(());
    }
    let list = match r {
        Ok(Value::Array(a)) => a
            .borrow()
            .iter()
            .map(|(_, v)| v.to_php_string().as_bytes().to_vec())
            .collect::<Vec<_>>(),
        Ok(Value::False) => return Err(()),
        _ => Vec::new(),
    };
    if list.is_empty() {
        return Ok(list);
    }
    let first = String::from_utf8_lossy(&list[0]).into_owned();
    if !vm.check_open_basedir(&f.name, &first) {
        return Err(());
    }
    let mut out = Vec::new();
    for p in list {
        if flags & GLOB_ONLYDIR != 0 {
            match std::fs::metadata(String::from_utf8_lossy(&p).as_ref()) {
                Ok(m) if m.is_dir() => {}
                _ => continue,
            }
        }
        out.push(p);
    }
    Ok(out)
}

/// php_zip_pcre()
fn zip_pcre(vm: &mut Vm, f: &Func, regexp: &[u8], path: &[u8]) -> Result<Vec<Vec<u8>>, ()> {
    let p = String::from_utf8_lossy(path).into_owned();
    if !vm.check_open_basedir(&f.name, &p) {
        return Err(());
    }
    let mut names: Vec<Vec<u8>> = match std::fs::read_dir(&p) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| {
                use std::os::unix::ffi::OsStrExt;
                e.file_name().as_bytes().to_vec()
            })
            .collect(),
        Err(_) => return Ok(Vec::new()),
    };
    names.push(b".".to_vec());
    names.push(b"..".to_vec());
    names.sort();
    let Some(pce) = goro_ext_standard::regex::get_compiled_regex(vm, regexp, &f.name) else {
        f.warn(vm, "Invalid expression");
        return Err(());
    };
    let mut out = Vec::new();
    for n in names {
        if n == b"." || n == b".." {
            continue;
        }
        if path.len() + n.len() + 1 >= 4096 {
            f.warn(
                vm,
                &format!(
                    "add_path string too long (max: {}, {} given)",
                    4096 - 1,
                    path.len() + n.len() + 1
                ),
            );
            break;
        }
        let rc = goro_ext_standard::regex::pcre_match_subject(vm, &pce, &n);
        if rc < 0 {
            continue;
        }
        let mut full = path.to_vec();
        full.push(b'/');
        full.extend_from_slice(&n);
        let fs = String::from_utf8_lossy(&full).into_owned();
        match std::fs::metadata(&fs) {
            Ok(m) => {
                if m.is_dir() {
                    continue;
                }
            }
            Err(_) => {
                f.warn(vm, &format!("Cannot read <{}>", fs));
                continue;
            }
        }
        out.push(full);
    }
    Ok(out)
}

/// php_zip_add_from_pattern()
fn add_from_pattern(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value], glob: bool) -> Mr {
    let a = if glob {
        zpp::parse(vm, f, "P|lh", args)?
    } else {
        zpp::parse(vm, f, "P|sh", args)?
    };
    let pattern = a[0].str_or(b"").to_vec();
    if pattern.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    let opts = match &a[2] {
        Arg::Arr(h) if !h.borrow().is_empty() => {
            let h = h.borrow().clone();
            parse_options(vm, f, &h)?
        }
        _ => Options {
            flags: 0,
            comp_method: 0,
            enc_method: 0,
            ..Default::default()
        },
    };
    let opts_given = matches!(&a[2], Arg::Arr(h) if !h.borrow().is_empty());
    let found = if glob {
        zip_glob(vm, f, &pattern, a[1].long_or(0))
    } else {
        let path = a[1].str_or(b".").to_vec();
        zip_pcre(vm, f, &pattern, &path)
    };
    let files = match found {
        Ok(v) => v,
        Err(()) => {
            if vm.current_exception.is_some() {
                return Err(());
            }
            return Ok(Value::False);
        }
    };
    if files.is_empty() {
        return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    }
    let d = data(o);
    // (with no options, opts is zero-initialized: flags 0)
    let (flags, comp_method, enc_method) = if opts_given {
        (opts.flags, opts.comp_method, opts.enc_method)
    } else {
        (0, -1, -1)
    };
    let opts_flags = if opts_given { flags } else { 0 };
    for file in &files {
        let stripped: Vec<u8> = if opts.remove_all_path {
            ext_io::basename(file)
        } else if let Some(rp) = opts
            .remove_path
            .as_ref()
            .filter(|rp| file.len() > rp.len() && file.starts_with(rp))
        {
            if file[rp.len()] == b'/' {
                file[rp.len() + 1..].to_vec()
            } else {
                file[rp.len()..].to_vec()
            }
        } else {
            file.clone()
        };
        let entry: Vec<u8> = match &opts.add_path {
            Some(ap) => {
                if ap.len() + stripped.len() > 4096 {
                    f.warn(
                        vm,
                        &format!(
                            "Entry name too long (max: {}, {} given)",
                            4096 - 1,
                            ap.len() + stripped.len()
                        ),
                    );
                    return Ok(Value::False);
                }
                let mut e = ap.clone();
                e.extend_from_slice(&stripped);
                e.truncate(4095);
                e
            }
            None => {
                let mut e = stripped;
                e.truncate(4095);
                e
            }
        };
        if add_file(vm, f, &d, file, &entry, 0, 0, -1, opts_flags) < 0 {
            return Ok(Value::False);
        }
        if comp_method >= 0 {
            let r = {
                let mut zo = d.obj.borrow_mut();
                let id = zo.last_id;
                zo.za.as_mut().map_or(-1, |za| {
                    za.set_file_compression(id as u64, comp_method as i32, opts.comp_flags)
                })
            };
            if r != 0 {
                return Ok(Value::False);
            }
        }
        if enc_method >= 0 {
            let r = {
                let mut zo = d.obj.borrow_mut();
                let id = zo.last_id;
                zo.za
                    .as_mut()
                    .map_or(-1, |za| za.file_set_encryption(id as u64, EM_NONE, None))
            };
            if r < 0 {
                f.warn(vm, "password reset failed");
                return Ok(Value::False);
            }
            let r = {
                let mut zo = d.obj.borrow_mut();
                let id = zo.last_id;
                zo.za.as_mut().map_or(-1, |za| {
                    za.file_set_encryption(
                        id as u64,
                        enc_method as u16,
                        opts.enc_password.as_deref(),
                    )
                })
            };
            if r != 0 {
                return Ok(Value::False);
            }
        }
    }
    let mut arr = PhpArray::new();
    for file in files {
        arr.push(s(&file));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn m_add_glob(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    add_from_pattern(vm, f, o, args, true)
}

fn m_add_pattern(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    add_from_pattern(vm, f, o, args, false)
}

// -- names / comments / flags --------------------------------------------------------

fn m_rename_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ls", args)?;
    let index = a[0].long_or(0);
    let new_name = a[1].str_or(b"").to_vec();
    if index < 0 {
        return Ok(Value::False);
    }
    let d = data(o);
    require_za(vm, &d)?;
    if new_name.is_empty() {
        f.must_not_be_empty(vm, 2);
        return Err(());
    }
    let r = with_za(vm, &d, |za| za.file_rename(index as u64, &new_name, 0))?;
    Ok(vbool(r == 0))
}

/// PHP_ZIP_STAT_PATH(): Ok(None) -> RETURN_FALSE
fn stat_path(
    vm: &mut Vm,
    f: &Func,
    d: &ZipData,
    name: &[u8],
    flags: u32,
) -> Result<Option<Stat>, ()> {
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    with_za(vm, d, |za| za.stat_name(name, flags))
}

fn m_rename_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ss", args)?;
    let name = a[0].str_or(b"").to_vec();
    let new_name = a[1].str_or(b"").to_vec();
    let d = data(o);
    require_za(vm, &d)?;
    if new_name.is_empty() {
        f.must_not_be_empty(vm, 2);
        return Err(());
    }
    let Some(sb) = stat_path(vm, f, &d, &name, 0)? else {
        return Ok(Value::False);
    };
    let r = with_za(vm, &d, |za| za.file_rename(sb.index, &new_name, 0))?;
    Ok(vbool(r == 0))
}

fn m_set_archive_comment(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "s", args)?;
    let comment = a[0].str_or(b"").to_vec();
    let d = data(o);
    require_za(vm, &d)?;
    if comment.len() > 0xffff {
        f.value_error(vm, 1, "must be less than 65535 bytes");
        return Err(());
    }
    let r = with_za(vm, &d, |za| za.set_archive_comment(&comment))?;
    Ok(vbool(r == 0))
}

fn m_get_archive_comment(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "|l", args)?;
    let flags = a[0].long_or(0) as u32;
    let d = data(o);
    let c = with_za(vm, &d, |za| za.get_archive_comment(flags))?;
    Ok(s(&c))
}

fn m_set_archive_flag(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ll", args)?;
    let flag = a[0].long_or(0) as u32;
    let value = a[1].long_or(0) as i32;
    let d = data(o);
    let r = with_za(vm, &d, |za| za.set_archive_flag(flag, value != 0))?;
    Ok(vbool(r == 0))
}

fn m_get_archive_flag(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "l|l", args)?;
    let flag = a[0].long_or(0) as u32;
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    let r = with_za(vm, &d, |za| za.get_archive_flag(flag, flags))?;
    Ok(Value::Long(r))
}

/// PHP_ZIP_SET_FILE_COMMENT()
fn set_file_comment(za: &mut Archive, idx: u64, comment: &[u8]) -> Value {
    vbool(za.file_set_comment(idx, comment, 0) >= 0)
}

fn m_set_comment_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ss", args)?;
    let name = a[0].str_or(b"").to_vec();
    let comment = a[1].str_or(b"").to_vec();
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    let d = data(o);
    require_za(vm, &d)?;
    if comment.len() > 0xffff {
        f.value_error(vm, 2, "must be less than 65535 bytes");
        return Err(());
    }
    with_za(vm, &d, |za| {
        let idx = za.name_locate(&name, 0) as i32;
        if idx < 0 {
            return Value::False;
        }
        set_file_comment(za, idx as u64, &comment)
    })
}

fn m_set_comment_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ls", args)?;
    let index = a[0].long_or(0);
    let comment = a[1].str_or(b"").to_vec();
    let d = data(o);
    require_za(vm, &d)?;
    if comment.len() > 0xffff {
        f.value_error(vm, 2, "must be less than 65535 bytes");
        return Err(());
    }
    with_za(vm, &d, |za| {
        if za.stat_index(index as u64, 0).is_none() {
            return Value::False;
        }
        set_file_comment(za, index as u64, &comment)
    })
}

fn m_set_ext_attr_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "sll|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let opsys = a[1].long_or(0);
    let attr = a[2].long_or(0);
    let flags = a[3].long_or(0) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    with_za(vm, &d, |za| {
        let idx = za.name_locate(&name, 0);
        if idx < 0 {
            return Value::False;
        }
        vbool(
            za.file_set_external_attributes(idx as u64, flags, (opsys & 0xff) as u8, attr as u32)
                >= 0,
        )
    })
}

fn m_set_ext_attr_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "lll|l", args)?;
    let index = a[0].long_or(0);
    let opsys = a[1].long_or(0);
    let attr = a[2].long_or(0);
    let flags = a[3].long_or(0) as u32;
    let d = data(o);
    with_za(vm, &d, |za| {
        if za.stat_index(index as u64, 0).is_none() {
            return Value::False;
        }
        vbool(
            za.file_set_external_attributes(index as u64, flags, (opsys & 0xff) as u8, attr as u32)
                >= 0,
        )
    })
}

fn assign_ref(v: &Arg, val: Value) {
    if let Arg::Any(Value::Reference(r)) = v {
        *r.borrow_mut() = val;
    }
}

fn m_get_ext_attr_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "szz|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let flags = a[3].long_or(0) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    let r = with_za(vm, &d, |za| {
        let idx = za.name_locate(&name, 0);
        if idx < 0 {
            return None;
        }
        za.file_get_external_attributes(idx as u64, flags)
    })?;
    match r {
        Some((opsys, attr)) => {
            assign_ref(&a[1], Value::Long(opsys as i64));
            assign_ref(&a[2], Value::Long(attr as i64));
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

fn m_get_ext_attr_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "lzz|l", args)?;
    let index = a[0].long_or(0);
    let flags = a[3].long_or(0) as u32;
    let d = data(o);
    let r = with_za(vm, &d, |za| {
        za.stat_index(index as u64, 0)?;
        za.file_get_external_attributes(index as u64, flags)
    })?;
    match r {
        Some((opsys, attr)) => {
            assign_ref(&a[1], Value::Long(opsys as i64));
            assign_ref(&a[2], Value::Long(attr as i64));
            Ok(Value::True)
        }
        None => Ok(Value::False),
    }
}

fn set_encryption(
    vm: &mut Vm,
    f: &Func,
    d: &ZipData,
    idx: i64,
    method: i64,
    pw: Option<Vec<u8>>,
) -> Mr {
    let r = with_za(vm, d, |za| {
        za.file_set_encryption(idx as u64, EM_NONE, None)
    })?;
    if r < 0 {
        f.warn(vm, "password reset failed");
        return Ok(Value::False);
    }
    let r = with_za(vm, d, |za| {
        za.file_set_encryption(idx as u64, method as u16, pw.as_deref())
    })?;
    Ok(vbool(r == 0))
}

fn m_set_encryption_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "sl|s!", args)?;
    let name = a[0].str_or(b"").to_vec();
    let method = a[1].long_or(0);
    let pw = a[2].str().map(|p| p.to_vec());
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    let idx = with_za(vm, &d, |za| za.name_locate(&name, 0))?;
    if idx < 0 {
        return Ok(Value::False);
    }
    set_encryption(vm, f, &d, idx, method, pw)
}

fn m_set_encryption_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ll|s!", args)?;
    let index = a[0].long_or(0);
    let method = a[1].long_or(0);
    let pw = a[2].str().map(|p| p.to_vec());
    let d = data(o);
    require_za(vm, &d)?;
    set_encryption(vm, f, &d, index, method, pw)
}

fn m_get_comment_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "s|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    with_za(vm, &d, |za| {
        let idx = za.name_locate(&name, 0) as i32;
        if idx < 0 {
            return Value::False;
        }
        s(&za.file_get_comment(idx as u64, flags).unwrap_or_default())
    })
}

fn m_get_comment_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "l|l", args)?;
    let index = a[0].long_or(0);
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    with_za(vm, &d, |za| {
        if za.stat_index(index as u64, 0).is_none() {
            return Value::False;
        }
        s(&za.file_get_comment(index as u64, flags).unwrap_or_default())
    })
}

fn m_set_compression_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "sl|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let method = a[1].long_or(0);
    let flags = a[2].long_or(0);
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    with_za(vm, &d, |za| {
        let idx = za.name_locate(&name, 0);
        if idx < 0 {
            return Value::False;
        }
        vbool(za.set_file_compression(idx as u64, method as i32, flags as u32) == 0)
    })
}

fn m_set_compression_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ll|l", args)?;
    let index = a[0].long_or(0);
    let method = a[1].long_or(0);
    let flags = a[2].long_or(0);
    let d = data(o);
    with_za(vm, &d, |za| {
        vbool(za.set_file_compression(index as u64, method as i32, flags as u32) == 0)
    })
}

fn m_set_mtime_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "sl|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let mtime = a[1].long_or(0);
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        f.must_not_be_empty(vm, 1);
        return Err(());
    }
    with_za(vm, &d, |za| {
        let idx = za.name_locate(&name, 0);
        if idx < 0 {
            return Value::False;
        }
        vbool(za.file_set_mtime(idx as u64, mtime) == 0)
    })
}

fn m_set_mtime_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "ll|l", args)?;
    let index = a[0].long_or(0);
    let mtime = a[1].long_or(0);
    let d = data(o);
    with_za(vm, &d, |za| {
        vbool(za.file_set_mtime(index as u64, mtime) == 0)
    })
}

fn m_delete_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "l", args)?;
    let index = a[0].long_or(0);
    let d = data(o);
    require_za(vm, &d)?;
    if index < 0 {
        return Ok(Value::False);
    }
    with_za(vm, &d, |za| vbool(za.delete(index as u64) >= 0))
}

fn m_delete_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "s", args)?;
    let name = a[0].str_or(b"").to_vec();
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        return Ok(Value::False);
    }
    let Some(sb) = stat_path(vm, f, &d, &name, 0)? else {
        return Ok(Value::False);
    };
    with_za(vm, &d, |za| vbool(za.delete(sb.index) == 0))
}

fn m_stat_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "P|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    match stat_path(vm, f, &d, &name, flags)? {
        Some(sb) => Ok(stat_array(&sb)),
        None => Ok(Value::False),
    }
}

fn m_stat_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "l|l", args)?;
    let index = a[0].long_or(0);
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    let r = with_za(vm, &d, |za| za.stat_index(index as u64, flags))?;
    Ok(r.map_or(Value::False, |sb| stat_array(&sb)))
}

fn m_locate_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "P|l", args)?;
    let name = a[0].str_or(b"").to_vec();
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        return Ok(Value::False);
    }
    let idx = with_za(vm, &d, |za| za.name_locate(&name, flags))?;
    Ok(if idx >= 0 {
        Value::Long(idx)
    } else {
        Value::False
    })
}

fn m_get_name_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "l|l", args)?;
    let index = a[0].long_or(0);
    let flags = a[1].long_or(0) as u32;
    let d = data(o);
    let r = with_za(vm, &d, |za| za.get_name(index as i32 as u64, flags))?;
    Ok(match r {
        Some(n) => s(c_str(&n)),
        None => Value::False,
    })
}

fn m_unchange_archive(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    zpp::parse(vm, f, "", args)?;
    let d = data(o);
    with_za(vm, &d, |za| vbool(za.unchange_archive() == 0))
}

fn m_unchange_all(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    zpp::parse(vm, f, "", args)?;
    let d = data(o);
    with_za(vm, &d, |za| vbool(za.unchange_all() == 0))
}

fn m_unchange_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "l", args)?;
    let index = a[0].long_or(0);
    let d = data(o);
    require_za(vm, &d)?;
    if index < 0 {
        return Ok(Value::False);
    }
    with_za(vm, &d, |za| vbool(za.unchange(index as u64) == 0))
}

fn m_unchange_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "s", args)?;
    let name = a[0].str_or(b"").to_vec();
    let d = data(o);
    require_za(vm, &d)?;
    if name.is_empty() {
        return Ok(Value::False);
    }
    let Some(sb) = stat_path(vm, f, &d, &name, 0)? else {
        return Ok(Value::False);
    };
    with_za(vm, &d, |za| vbool(za.unchange(sb.index) == 0))
}

// -- extraction / reading ------------------------------------------------------------

/// virtual_file_ex(CWD_EXPAND) on an empty state: the path made absolute
/// to "" (relative segments resolved, `..` above the root dropped).
fn clean_path(file: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for seg in file.split(|&c| c == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let mut out = Vec::new();
    for p in parts {
        out.push(b'/');
        out.extend_from_slice(p);
    }
    if file.last() == Some(&b'/') && !out.is_empty() {
        out.push(b'/');
    }
    out
}

/// php_zip_make_relative_path()
fn make_relative_path(path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    if path[0] == b'/' {
        return Some(path[1..].to_vec());
    }
    let mut i = path.len();
    loop {
        while i > 0 && path[i] != b'/' {
            i -= 1;
        }
        if i == 0 {
            return Some(path.to_vec());
        }
        if i >= 2 && path[i - 1] == b'.' {
            return Some(path[i + 1..].to_vec());
        }
        i -= 1;
    }
}

/// zend_dirname()
fn dirname(path: &[u8]) -> Vec<u8> {
    let mut end = path.len();
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return if path.is_empty() {
            b".".to_vec()
        } else {
            b"/".to_vec()
        };
    }
    while end > 0 && path[end - 1] != b'/' {
        end -= 1;
    }
    if end == 0 {
        return b".".to_vec();
    }
    while end > 0 && path[end - 1] == b'/' {
        end -= 1;
    }
    if end == 0 {
        return b"/".to_vec();
    }
    path[..end].to_vec()
}

fn quiet<T>(vm: &mut Vm, f: impl FnOnce(&mut Vm) -> T) -> T {
    let saved = vm.warning_capture.replace(Vec::new());
    let r = f(vm);
    vm.warning_capture = saved;
    r
}

/// php_stream_stat_path_ex(QUIET) success
fn path_exists(vm: &mut Vm, f: &Func, p: &[u8]) -> bool {
    quiet(vm, |vm| {
        matches!(
            ext_io::call_as(
                vm,
                &f.name,
                b"file_exists",
                &[Value::String(PhpString::from_bytes(p))]
            ),
            Ok(Value::True)
        )
    })
}

fn mkdir_recursive(vm: &mut Vm, f: &Func, p: &[u8], report: bool) -> bool {
    // php_plain_files_mkdir(PHP_STREAM_MKDIR_RECURSIVE)
    let plain = !p.windows(3).any(|w| w == b"://") || p.starts_with(b"file://");
    if plain {
        let path = String::from_utf8_lossy(p.strip_prefix(b"file://").unwrap_or(p)).into_owned();
        let r = if std::path::Path::new(&path).exists() {
            Err(std::io::Error::from_raw_os_error(17))
        } else {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o777)
                .create(&path)
        };
        return match r {
            Ok(()) => true,
            Err(e) => {
                if report {
                    let msg = crate::zipint::os_strerror(e.raw_os_error().unwrap_or(0));
                    f.warn(vm, &msg);
                }
                false
            }
        };
    }
    let go = |vm: &mut Vm| {
        matches!(
            ext_io::call_as(
                vm,
                &f.name,
                b"mkdir",
                &[
                    Value::String(PhpString::from_bytes(p)),
                    Value::Long(0o777),
                    Value::True
                ]
            ),
            Ok(Value::True)
        )
    };
    if report { go(vm) } else { quiet(vm, go) }
}

/// php_zip_extract_file(): true on success
fn extract_file(vm: &mut Vm, f: &Func, d: &ZipData, dest: &[u8], file: &[u8], idx: i64) -> bool {
    let mut idx = idx;
    if idx < 0 {
        idx = match d.obj.borrow_mut().za.as_mut() {
            Some(za) => za.name_locate(file, 0),
            None => -1,
        };
        if idx < 0 {
            return false;
        }
    }
    let cleaned = clean_path(c_str(file));
    let Some(path_cleaned) = make_relative_path(&cleaned) else {
        return false;
    };
    let sb = match d.obj.borrow_mut().za.as_mut() {
        Some(za) => za.stat_index(idx as u64, 0),
        None => None,
    };
    if path_cleaned.len() >= 4096 {
        return false;
    }
    let Some(sb) = sb else { return false };
    let is_dir_only = path_cleaned.len() > 1 && path_cleaned.last() == Some(&b'/');
    let mut basename = Vec::new();
    let dir_full: Vec<u8> = if is_dir_only {
        let mut v = dest.to_vec();
        v.push(b'/');
        v.extend_from_slice(&path_cleaned);
        v
    } else {
        let dn = dirname(&path_cleaned);
        let v = if dn.is_empty() || dn == b"." {
            dest.to_vec()
        } else {
            let mut v = dest.to_vec();
            v.push(b'/');
            v.extend_from_slice(&dn);
            v
        };
        basename = ext_io::basename(&path_cleaned);
        if basedir_refused(vm, &f.name, &v) {
            return false;
        }
        v
    };
    if !path_exists(vm, f, &dir_full) && !mkdir_recursive(vm, f, &dir_full, true) {
        return false;
    }
    if is_dir_only {
        return true;
    }
    let mut full = dir_full.clone();
    full.push(b'/');
    full.extend_from_slice(&basename);
    if full.len() > 4096 {
        f.warn(
            vm,
            &format!("Full extraction path exceed MAXPATHLEN ({})", 4096),
        );
        return false;
    }
    if basedir_refused(vm, &f.name, &full) {
        return false;
    }
    let zf = match d.obj.borrow_mut().za.as_mut() {
        Some(za) => {
            let pw = za.default_password.clone();
            za.fopen_index(idx as u64, 0, pw.as_deref())
        }
        None => None,
    };
    let Some(mut zf) = zf else {
        return false;
    };
    let Ok(Some(fid)) = ext_io::open_as(vm, Some(&f.name), &full, "w+b", true) else {
        return false;
    };
    let mut ok = true;
    loop {
        match zf.read(8192) {
            Ok(b) if !b.is_empty() => {
                ext_io::write(vm, fid, &b);
            }
            Ok(_) => break,
            Err(_) => {
                ok = false;
                break;
            }
        }
    }
    let full_v = Value::String(PhpString::from_bytes(&full));
    // stream_metadata(PHP_STREAM_META_TOUCH) of plain files
    if !full.contains(&b':') || full.starts_with(b"file://") {
        let _ = ext_io::call_as(
            vm,
            &f.name,
            b"touch",
            &[full_v, Value::Long(sb.mtime), Value::Long(sb.mtime)],
        );
    }
    ext_io::close(vm, fid);
    ok
}

fn m_extract_to(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    if args.is_empty() || args.len() > 2 {
        let given = args.len();
        let msg = if given == 0 {
            format!("{}() expects at least 1 argument, 0 given", f.name)
        } else {
            format!("{}() expects at most 2 arguments, {} given", f.name, given)
        };
        zpp::throw(vm, b"ArgumentCountError", &msg);
        return Err(());
    }
    let a = zpp::parse(vm, f, "p|z", args)?;
    let pathto = a[0].str_or(b"").to_vec();
    enum Files {
        None,
        Str(Vec<u8>),
        Arr(Rc<RefCell<PhpArray>>),
    }
    let files = match &a[1] {
        Arg::Absent => Files::None,
        Arg::Any(v) => match v.deref() {
            Value::Null => Files::None,
            Value::Array(h) => Files::Arr(h),
            Value::String(s) => Files::Str(s.as_bytes().to_vec()),
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                Files::Str(v.deref().to_php_string().as_bytes().to_vec())
            }
            other => {
                f.type_error(vm, 2, "array|string|null", &other);
                return Err(());
            }
        },
        _ => Files::None,
    };
    let d = data(o);
    require_za(vm, &d)?;
    if pathto.is_empty() {
        return Ok(Value::False);
    }
    if !path_exists(vm, f, &pathto) && !mkdir_recursive(vm, f, &pathto, false) {
        return Ok(Value::False);
    }
    match files {
        Files::Str(s) => {
            if !extract_file(vm, f, &d, &pathto, &s, -1) {
                return Ok(Value::False);
            }
        }
        Files::Arr(h) => {
            let n = h.borrow().len();
            if n == 0 {
                return Ok(Value::False);
            }
            for i in 0..n {
                let v = h.borrow().get(&ArrayKey::Int(i as i64)).map(|v| v.deref());
                if let Some(Value::String(s)) = v {
                    if !extract_file(vm, f, &d, &pathto, s.as_bytes(), -1) {
                        return Ok(Value::False);
                    }
                }
            }
        }
        Files::None => {
            let count = d
                .obj
                .borrow()
                .za
                .as_ref()
                .map_or(-1, |za| za.num_entries(0));
            if count == -1 {
                f.warn(vm, "Illegal archive");
                return Ok(Value::False);
            }
            for i in 0..count {
                let name = d
                    .obj
                    .borrow_mut()
                    .za
                    .as_mut()
                    .and_then(|za| za.get_name(i as u64, FL_UNCHANGED));
                let Some(name) = name else {
                    return Ok(Value::False);
                };
                if !extract_file(vm, f, &d, &pathto, c_str(&name), i) {
                    return Ok(Value::False);
                }
            }
        }
    }
    Ok(Value::True)
}

/// php_zip_get_from()
fn get_from(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value], by_name: bool) -> Mr {
    let d = data(o);
    let (sb, len, flags, name, index) = if by_name {
        let a = zpp::parse(vm, f, "P|ll", args)?;
        let name = a[0].str_or(b"").to_vec();
        let len = a[1].long_or(0);
        let flags = a[2].long_or(0) as u32;
        require_za(vm, &d)?;
        let Some(sb) = stat_path(vm, f, &d, &name, flags)? else {
            return Ok(Value::False);
        };
        (sb, len, flags, Some(name), -1i64)
    } else {
        let a = zpp::parse(vm, f, "l|ll", args)?;
        let index = a[0].long_or(0);
        let len = a[1].long_or(0);
        let flags = a[2].long_or(0) as u32;
        let r = with_za(vm, &d, |za| za.stat_index(index as u64, 0))?;
        let Some(sb) = r else {
            return Ok(Value::False);
        };
        (sb, len, flags, None, index)
    };
    if sb.size < 1 {
        return Ok(s(b""));
    }
    let len = if len < 1 {
        sb.size as usize
    } else {
        len as usize
    };
    let zf = with_za(vm, &d, |za| {
        if index >= 0 {
            let pw = za.default_password.clone();
            za.fopen_index(index as u64, flags, pw.as_deref())
        } else {
            za.fopen(name.as_deref().unwrap_or_default(), flags)
        }
    })?;
    let Some(mut zf) = zf else {
        return Ok(Value::False);
    };
    match zf.read(len) {
        Ok(b) if !b.is_empty() => Ok(Value::String(PhpString::from_vec(b))),
        _ => Ok(s(b"")),
    }
}

fn m_get_from_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    get_from(vm, f, o, args, true)
}

fn m_get_from_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    get_from(vm, f, o, args, false)
}

/// php_zip_get_stream()
fn get_stream(
    vm: &mut Vm,
    f: &Func,
    o: &Obj,
    args: &[Value],
    by_name: bool,
    accept_flags: bool,
) -> Mr {
    let d = data(o);
    let (name, index, flags) = if by_name {
        let a = if accept_flags {
            zpp::parse(vm, f, "P|l", args)?
        } else {
            zpp::parse(vm, f, "P", args)?
        };
        let flags = if accept_flags { a[1].long_or(0) } else { 0 };
        (Some(a[0].str_or(b"").to_vec()), 0, flags as u32)
    } else {
        let a = zpp::parse(vm, f, "l|l", args)?;
        (None, a[0].long_or(0), a[1].long_or(0) as u32)
    };
    require_za(vm, &d)?;
    let sb = match &name {
        Some(n) => stat_path(vm, f, &d, n, flags)?,
        None => with_za(vm, &d, |za| za.stat_index(index as u64, flags))?,
    };
    let Some(sb) = sb else {
        return Ok(Value::False);
    };
    let zf = with_za(vm, &d, |za| {
        let pw = za.default_password.clone();
        za.fopen_index(sb.index, flags, pw.as_deref())
    })?;
    let Some(zf) = zf else {
        return Ok(Value::False);
    };
    let id = crate::stream::alloc(zf, None, &sb.name);
    Ok(Value::resource(&id))
}

fn m_get_stream_index(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    get_stream(vm, f, o, args, false, true)
}

fn m_get_stream_name(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    get_stream(vm, f, o, args, true, true)
}

fn m_get_stream(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    get_stream(vm, f, o, args, true, false)
}

fn m_register_progress(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "df", args)?;
    let rate = a[0].double_or(0.0);
    let Arg::Any(cb) = &a[1] else { return Err(()) };
    let d = data(o);
    require_za(vm, &d)?;
    d.obj.borrow_mut().progress = Some((rate, cb.clone()));
    Ok(Value::True)
}

fn m_register_cancel(vm: &mut Vm, f: &Func, o: &Obj, args: &[Value]) -> Mr {
    let a = zpp::parse(vm, f, "f", args)?;
    let Arg::Any(cb) = &a[0] else { return Err(()) };
    let d = data(o);
    require_za(vm, &d)?;
    d.obj.borrow_mut().cancel = Some(cb.clone());
    Ok(Value::True)
}

fn m_is_compression_supported(vm: &mut Vm, f: &Func, _o: &Obj, args: &[Value]) -> Mr {
    m_static(vm, f, "isCompressionMethodSupported", args)
}

fn m_is_encryption_supported(vm: &mut Vm, f: &Func, _o: &Obj, args: &[Value]) -> Mr {
    m_static(vm, f, "isEncryptionMethodSupported", args)
}
