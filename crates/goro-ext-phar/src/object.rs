//! php-src ext/phar/phar_object.c: the methods of Phar, PharData and
//! PharFileInfo. The classes are declared in prelude.php, whose methods
//! hand their arguments to `__phar_m()`.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::archive::*;
use crate::pathcheck::path_check;
use crate::{A, CUR_FN, EntryObj, PharObj, R, arr, sv, throw, util, zpp, zpp_ex};

type Obj = Rc<RefCell<PhpObject>>;

const UVE: &[u8] = b"UnexpectedValueException";
const BMCE: &[u8] = b"BadMethodCallException";
const PE: &[u8] = b"PharException";
const RTE: &[u8] = b"RuntimeException";
const IAE: &[u8] = b"InvalidArgumentException";

const SPL_FILE_DIR_SKIPDOTS: i64 = 0x1000;
const SPL_FILE_DIR_UNIXPATHS: i64 = 0x2000;

static PHAR_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(phar_clone),
    ..NativeObjectHandlers::EMPTY
};

static ENTRY_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(entry_clone),
    ..NativeObjectHandlers::EMPTY
};

/// phar_spl_foreign_clone()
fn phar_clone(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    let a = phar_data(src).and_then(|d| d.archive.borrow().clone());
    if let Some(a) = &a {
        a.borrow_mut().refcount += 1;
    }
    dst.borrow_mut().native = Some(NativeData {
        data: Rc::new(PharObj {
            archive: RefCell::new(a),
        }),
        handlers: &PHAR_HANDLERS,
    });
}

/// spl_filesystem_object_clone(): the entry pointer is copied.
fn entry_clone(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    let e = entry_data(src).and_then(|d| d.entry.borrow().clone());
    if let Some(e) = &e {
        if !e.borrow().is_temp_dir {
            e.borrow_mut().fileinfo_lock_count += 1;
            if let Some(p) = e.borrow().phar.upgrade() {
                p.borrow_mut().refcount += 1;
            }
        }
    }
    dst.borrow_mut().native = Some(NativeData {
        data: Rc::new(EntryObj {
            entry: RefCell::new(e),
        }),
        handlers: &ENTRY_HANDLERS,
    });
}

fn phar_data(o: &Obj) -> Option<Rc<PharObj>> {
    let d = o.borrow().native.as_ref()?.data.clone();
    d.downcast::<PharObj>().ok()
}

fn entry_data(o: &Obj) -> Option<Rc<EntryObj>> {
    let d = o.borrow().native.as_ref()?.data.clone();
    d.downcast::<EntryObj>().ok()
}

/// PHAR_ARCHIVE_OBJECT()
fn archive_of(vm: &mut Vm, this: &Option<Obj>) -> Option<ArcRef> {
    let a = this
        .as_ref()
        .and_then(phar_data)
        .and_then(|d| d.archive.borrow().clone());
    if a.is_none() {
        let _ = throw(
            vm,
            BMCE,
            "Cannot call method on an uninitialized Phar object",
        );
    }
    a
}

fn set_archive(this: &Obj, a: Option<ArcRef>) {
    if let Some(d) = phar_data(this) {
        *d.archive.borrow_mut() = a;
    }
}

/// PHAR_ENTRY_OBJECT()
fn entry_of(vm: &mut Vm, this: &Option<Obj>) -> Option<EntryRef> {
    let e = this
        .as_ref()
        .and_then(entry_data)
        .and_then(|d| d.entry.borrow().clone());
    if e.is_none() {
        let _ = throw(
            vm,
            BMCE,
            "Cannot call method on an uninitialized PharFileInfo object",
        );
    }
    e
}

fn bv(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// instanceof_function() on class names (user or internal classes).
fn instance_of(vm: &Vm, class: &[u8], target: &[u8]) -> bool {
    is_a(
        vm,
        &class.to_ascii_lowercase(),
        &target.to_ascii_lowercase(),
        0,
    )
}

fn is_a(vm: &Vm, class_lower: &[u8], target: &[u8], depth: usize) -> bool {
    if class_lower == target {
        return true;
    }
    if depth > 64 {
        return false;
    }
    match vm.classes.get(class_lower) {
        Some(ce) => {
            ce.parent
                .as_ref()
                .is_some_and(|p| is_a(vm, &p.to_ascii_lowercase(), target, depth + 1))
                || ce
                    .interfaces
                    .iter()
                    .any(|i| is_a(vm, &i.to_ascii_lowercase(), target, depth + 1))
        }
        None => goro_core::stubs::stub_instanceof(class_lower, target).unwrap_or(false),
    }
}

fn call_builtin(vm: &mut Vm, name: &[u8], args: &[Value]) -> Option<Value> {
    let f = vm.functions.get(name).copied()?;
    f(vm, args).ok()
}

/// `__phar_m(name, $this, $args)`: run a method.
pub(crate) fn dispatch(vm: &mut Vm, args: &[Value]) -> R {
    let name = args
        .first()
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_default();
    let this = match args.get(1).map(|v| v.deref()) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    };
    let list = match crate::margs(vm, &name, args.get(2).unwrap_or(&Value::Null)) {
        Ok(l) => l,
        Err(()) => return Ok(Value::Null),
    };
    // The method runs as an internal function: no frame of its own for
    // the dispatcher.
    let hide = vm.current_builtin_name().as_deref() == Some("__phar_m");
    if hide {
        vm.pop_builtin_frame();
    }
    CUR_FN.with(|c| c.borrow_mut().push(name.clone()));
    let r = method(vm, &name, this, list);
    CUR_FN.with(|c| c.borrow_mut().pop());
    if hide {
        vm.push_builtin_frame(b"__phar_m", &[], 0);
    }
    r
}

fn method(vm: &mut Vm, name: &str, this: Option<Obj>, list: Vec<Option<Value>>) -> R {
    let (class, m) = name.split_once("::").unwrap_or(("", name));
    let is_entry = class == "PharFileInfo";
    let l = &list;
    macro_rules! p {
        ($spec:expr) => {
            match zpp(vm, name, l, $spec) {
                Some(a) => a,
                None => return Ok(Value::Null),
            }
        };
    }
    if is_entry {
        return entry_method(vm, name, m, this, list);
    }
    match m {
        "running" => {
            let a = p!("|b");
            let retphar = a[0].b().unwrap_or(true);
            let Some(fname) = executed_filename(vm) else {
                return Ok(sv(b""));
            };
            if starts_with_ci(&fname, b"phar://") {
                if let Ok((arch, _)) = split_fname(vm, &fname, 2, 0) {
                    if retphar {
                        return Ok(sv(&fname[..(arch.len() + 7).min(fname.len())]));
                    }
                    return Ok(sv(&arch));
                }
            }
            Ok(sv(b""))
        }
        "mount" => {
            let a = p!("pp");
            mount(vm, &a[0].s().unwrap(), &a[1].s().unwrap())
        }
        "webPhar" => {
            let a = p!("|s!s!s!af!");
            crate::webphar::web_phar(vm, a)
        }
        "mungServer" => {
            let a = p!("a");
            let Some(Value::Array(vals)) = a[0].z() else {
                return Ok(Value::Null);
            };
            let vals = vals.borrow();
            if vals.is_empty() {
                return throw(
                    vm,
                    PE,
                    "No values passed to Phar::mungServer(), expecting an array of any of these strings: PHP_SELF, REQUEST_URI, SCRIPT_FILENAME, SCRIPT_NAME",
                );
            }
            if vals.len() > 4 {
                return throw(
                    vm,
                    PE,
                    "Too many values passed to Phar::mungServer(), expecting an array of any of these strings: PHP_SELF, REQUEST_URI, SCRIPT_FILENAME, SCRIPT_NAME",
                );
            }
            request_initialize();
            for v in vals.values() {
                let Value::String(s) = v.deref() else {
                    return throw(
                        vm,
                        PE,
                        "Non-string value passed to Phar::mungServer(), expecting an array of any of these strings: PHP_SELF, REQUEST_URI, SCRIPT_FILENAME, SCRIPT_NAME",
                    );
                };
                let bit = match s.as_bytes() {
                    b"PHP_SELF" => 1,
                    b"REQUEST_URI" => 2,
                    b"SCRIPT_NAME" => 4,
                    b"SCRIPT_FILENAME" => 8,
                    _ => 0,
                };
                g(|g| g.mung_list |= bit);
            }
            Ok(Value::Null)
        }
        "interceptFileFuncs" => {
            p!("");
            g(|g| g.intercepted = true);
            Ok(Value::Null)
        }
        "createDefaultStub" => {
            let a = p!("|p!p!");
            match create_default_stub(a[0].s().as_deref(), a[1].s().as_deref()) {
                Ok(s) => Ok(sv(&s)),
                Err(e) => throw(vm, PE, &e.unwrap_or_default()),
            }
        }
        "mapPhar" => {
            let a = p!("|s!l");
            request_initialize();
            match open_executed_filename(vm, a[0].s().as_deref()) {
                Ok(()) => Ok(Value::True),
                Err(e) => {
                    if let Some(e) = e {
                        let _ = throw(vm, PE, &e);
                    }
                    Ok(Value::False)
                }
            }
        }
        "loadPhar" => {
            let a = p!("p|s!");
            request_initialize();
            match open_from_filename(vm, &a[0].s().unwrap(), a[1].s().as_deref(), REPORT_ERRORS) {
                Ok(_) => Ok(Value::True),
                Err(e) => {
                    if let Some(e) = e {
                        let _ = throw(vm, PE, &e);
                    }
                    Ok(Value::False)
                }
            }
        }
        "apiVersion" => {
            p!("");
            Ok(sv(PHP_PHAR_API_VERSION.as_bytes()))
        }
        "canCompress" => {
            p!("|l");
            request_initialize();
            Ok(Value::True)
        }
        "canWrite" => {
            p!("");
            Ok(bv(!readonly()))
        }
        "isValidPharFilename" => {
            let a = p!("p|b");
            let executable = a[1].b().unwrap_or(true);
            let f = a[0].s().unwrap();
            let (mut es, mut el) = (None, 0);
            Ok(bv(detect_phar_fname_ext(
                vm,
                &f,
                &mut es,
                &mut el,
                executable as i32,
                2,
                true,
            )))
        }
        "__construct" => construct(vm, name, this, list),
        "__construct_done" => {
            if let Some(o) = &this {
                o.borrow_mut()
                    .set_property(b"__spl_info_class".to_vec(), sv(b"PharFileInfo"));
            }
            Ok(Value::Null)
        }
        "getSupportedSignatures" => {
            p!("");
            let mut a = PhpArray::new();
            for s in ["MD5", "SHA-1", "SHA-256", "SHA-512"] {
                a.push(sv(s.as_bytes()));
            }
            if vm.loaded_extensions.contains(b"openssl".as_slice()) {
                for s in ["OpenSSL", "OpenSSL_SHA256", "OpenSSL_SHA512"] {
                    a.push(sv(s.as_bytes()));
                }
            }
            Ok(arr(a))
        }
        "getSupportedCompression" => {
            p!("");
            request_initialize();
            let mut a = PhpArray::new();
            a.push(sv(b"GZ"));
            a.push(sv(b"BZIP2"));
            Ok(arr(a))
        }
        "unlinkArchive" => {
            let a = p!("p");
            unlink_archive(vm, &a[0].s().unwrap())
        }
        "__destruct" => {
            p!("");
            Ok(Value::Null)
        }
        _ => archive_method(vm, name, m, this, list),
    }
}

/// Phar::mount()
fn mount(vm: &mut Vm, path: &[u8], actual: &[u8]) -> R {
    let fname = executed_filename(vm).unwrap_or_default();
    let carry_on = |vm: &mut Vm, pphar: &ArcRef, path: &[u8], arch: Option<&[u8]>| -> R {
        if util::mount_entry(vm, pphar, actual, path).is_err() {
            let _ = throw(
                vm,
                PE,
                &format!(
                    "Mounting of {} to {} within phar {} failed",
                    l(path),
                    l(actual),
                    arch.map(l).unwrap_or_else(|| "(null)".to_string())
                ),
            );
        }
        Ok(Value::Null)
    };
    let carry_on2 = |vm: &mut Vm, arch: &[u8], path: &[u8]| -> R {
        let Some(pphar) = find_fname(arch) else {
            return throw(
                vm,
                PE,
                &format!("{} is not a phar archive, cannot mount", l(arch)),
            );
        };
        carry_on(vm, &pphar, path, Some(arch))
    };
    if fname.len() > 7 && fname.starts_with(b"phar://") {
        if let Ok((arch, _)) = split_fname(vm, &fname, 2, 0) {
            if path.len() > 7 && path.starts_with(b"phar://") {
                return throw(
                    vm,
                    PE,
                    &format!(
                        "Can only mount internal paths within a phar archive, use a relative path instead of \"{}\"",
                        l(path)
                    ),
                );
            }
            return carry_on2(vm, &arch, path);
        }
    }
    if let Some(pphar) = find_fname(&fname) {
        return carry_on(vm, &pphar, path, None);
    }
    if let Ok((arch, entry)) = split_fname(vm, path, 2, 0) {
        return carry_on2(vm, &arch, &entry);
    }
    throw(
        vm,
        PE,
        &format!("Mounting of {} to {} failed", l(path), l(actual)),
    )
}

/// Phar::unlinkArchive()
fn unlink_archive(vm: &mut Vm, fname: &[u8]) -> R {
    if fname.is_empty() {
        return throw(vm, PE, "Unknown phar archive \"\"");
    }
    let phar = match open_from_filename(vm, fname, None, REPORT_ERRORS) {
        Ok(p) => p,
        Err(e) => {
            return throw(
                vm,
                PE,
                &match e {
                    Some(e) => format!("Unknown phar archive \"{}\": {}", l(fname), e),
                    None => format!("Unknown phar archive \"{}\"", l(fname)),
                },
            );
        }
    };
    if let Some(zf) = executed_filename(vm) {
        if starts_with_ci(&zf, b"phar://") {
            if let Ok((arch, _)) = split_fname(vm, &zf, 2, 0) {
                if arch == fname {
                    return throw(
                        vm,
                        PE,
                        &format!(
                            "phar archive \"{}\" cannot be unlinked from within itself",
                            l(fname)
                        ),
                    );
                }
            }
        }
    }
    if phar.borrow().refcount != 0 {
        return throw(
            vm,
            PE,
            &format!(
                "phar archive \"{}\" has open file handles or objects.  fclose() all file handles, and unset() all objects prior to calling unlinkArchive()",
                l(fname)
            ),
        );
    }
    let f = phar.borrow().fname.clone();
    invalidate_last();
    archive_delref(&phar);
    let _ = std::fs::remove_file(l(&f));
    Ok(Value::True)
}

/// Phar::__construct() / PharData::__construct(): the arguments of the
/// RecursiveDirectoryIterator constructor, or null.
fn construct(vm: &mut Vm, name: &str, this: Option<Obj>, list: Vec<Option<Value>>) -> R {
    let Some(this) = this else {
        return Ok(Value::Null);
    };
    let class = this.borrow().class_name.clone();
    let is_data = instance_of(vm, &class, b"PharData");
    let a = if is_data {
        match zpp(vm, name, &list, "p|ls!l") {
            Some(a) => a,
            None => return Ok(Value::Null),
        }
    } else {
        match zpp(vm, name, &list, "p|ls!") {
            Some(a) => a,
            None => return Ok(Value::Null),
        }
    };
    let fname_in = a[0].s().unwrap();
    let flags = a[1]
        .l()
        .unwrap_or(SPL_FILE_DIR_SKIPDOTS | SPL_FILE_DIR_UNIXPATHS);
    let alias = a[2].s();
    let format = a.get(3).and_then(|x| x.l()).unwrap_or(0);
    if phar_data(&this).is_some_and(|d| d.archive.borrow().is_some()) {
        return throw(vm, BMCE, "Cannot call constructor twice");
    }
    let (fname, entry) = match split_fname(vm, &fname_in, !is_data as i32, 2) {
        Ok((arch, entry)) => (arch, Some(entry)),
        Err(_) => (fname_in.clone(), None),
    };
    let phar_data_ =
        match open_or_create_filename(vm, &fname, alias.as_deref(), is_data, REPORT_ERRORS) {
            Ok(p) => p,
            Err(e) => {
                return throw(
                    vm,
                    UVE,
                    &e.unwrap_or_else(|| "Phar creation or opening failed".to_string()),
                );
            }
        };
    {
        let mut p = phar_data_.borrow_mut();
        if is_data && p.is_tar && p.is_brandnew && format == PHAR_FORMAT_ZIP {
            p.is_zip = true;
            p.is_tar = false;
        }
    }
    let pd = phar_data_.borrow().is_data;
    if is_data != pd {
        return throw(
            vm,
            UVE,
            if is_data {
                "PharData class can only be used for non-executable tar and zip archives"
            } else {
                "Phar class can only be used for executable tar and zip archives"
            },
        );
    }
    phar_data_.borrow_mut().refcount += 1;
    this.borrow_mut().native = Some(NativeData {
        data: Rc::new(PharObj {
            archive: RefCell::new(Some(phar_data_.clone())),
        }),
        handlers: &PHAR_HANDLERS,
    });
    let mut url = b"phar://".to_vec();
    url.extend_from_slice(&phar_data_.borrow().fname);
    if let Some(e) = entry {
        url.extend_from_slice(&e);
    }
    phar_data_.borrow_mut().is_data = pd;
    let mut r = PhpArray::new();
    r.push(sv(&url));
    r.push(Value::Long(flags));
    Ok(arr(r))
}

// ---------------------------------------------------------------------------
// building

struct Build {
    phar: Obj,
    archive: ArcRef,
    class: Vec<u8>,
    base: Vec<u8>,
    ret: PhpArray,
    fp: Buf,
}

thread_local! {
    static BUILDS: RefCell<Vec<Build>> = const { RefCell::new(Vec::new()) };
}

/// `__phar_build_begin($phar, $class, $base)`
pub(crate) fn build_begin(vm: &mut Vm, args: &[Value]) -> R {
    let Some(Value::Object(o)) = args.first().map(|v| v.deref()) else {
        return Ok(Value::Null);
    };
    let Some(archive) = phar_data(&o).and_then(|d| d.archive.borrow().clone()) else {
        return Ok(Value::Null);
    };
    let _ = vm;
    BUILDS.with(|b| {
        b.borrow_mut().push(Build {
            phar: o.clone(),
            archive,
            class: args
                .get(1)
                .map(|v| v.to_php_string().as_bytes().to_vec())
                .unwrap_or_default(),
            base: args
                .get(2)
                .map(|v| v.to_php_string().as_bytes().to_vec())
                .unwrap_or_default(),
            ret: PhpArray::new(),
            fp: new_buf(),
        })
    });
    Ok(Value::Null)
}

/// `__phar_build_end($ok)`: the array of added files.
pub(crate) fn build_end(vm: &mut Vm, args: &[Value]) -> R {
    let ok = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let Some(b) = BUILDS.with(|b| b.borrow_mut().pop()) else {
        return Ok(Value::Null);
    };
    if !ok {
        return Ok(Value::Null);
    }
    let _ = &b.phar;
    b.archive.borrow_mut().ufp = Some(b.fp.clone());
    if let Some(e) = flush(vm, &b.archive) {
        let _ = throw(vm, PE, &e);
    }
    Ok(arr(b.ret))
}

/// spl_filesystem_object_get_path() + d_name / file_name of an
/// SplFileInfo object.
fn spl_file_name(vm: &mut Vm, o: &Obj) -> Option<Vec<u8>> {
    let v = Value::Object(o.clone());
    let r = vm.call_callback(
        &{
            let mut cb = PhpArray::new();
            cb.push(v);
            cb.push(sv(b"getPathname"));
            arr(cb)
        },
        &[],
    );
    match r {
        Ok(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    }
}

/// phar_build() for one element: `__phar_build_item($value, $iterator)`.
pub(crate) fn build_item(vm: &mut Vm, args: &[Value]) -> R {
    let value = args.first().map(|v| v.deref()).unwrap_or(Value::Null);
    let iter = args.get(1).cloned().unwrap_or(Value::Null);
    let Some((archive, class, base)) = BUILDS.with(|b| {
        b.borrow()
            .last()
            .map(|b| (b.archive.clone(), b.class.clone(), b.base.clone()))
    }) else {
        return Ok(Value::False);
    };
    let ce = l(&class);
    let get_key = |vm: &mut Vm| -> Result<Vec<u8>, ()> {
        let mut cb = PhpArray::new();
        cb.push(iter.clone());
        cb.push(sv(b"key"));
        let k = vm.call_callback(&arr(cb), &[]).map_err(|_| ())?;
        if vm.current_exception.is_some() {
            return Err(());
        }
        match k {
            Value::String(s) => Ok(s.as_bytes().to_vec()),
            _ => {
                let _ = throw(
                    vm,
                    UVE,
                    &format!(
                        "Iterator {} returned an invalid key (must return a string)",
                        ce
                    ),
                );
                Err(())
            }
        }
    };
    let mut fname: Vec<u8>;
    let str_key: Vec<u8>;
    let opened: Vec<u8>;
    let mut stat_mode: Option<i64> = None;
    match &value {
        Value::String(s) => fname = s.as_bytes().to_vec(),
        Value::Resource(id) => {
            let Ok(k) = get_key(vm) else {
                return Ok(Value::False);
            };
            str_key = k;
            let data = call_builtin(vm, b"stream_get_contents", &[Value::resource(id)]);
            let Some(Value::String(d)) = data else {
                return throw(
                    vm,
                    BMCE,
                    &format!("Iterator {} returned an invalid stream handle", ce),
                );
            };
            let contents = d.as_bytes().to_vec();
            opened = b"[stream]".to_vec();
            if let Some(Value::Array(st)) = call_builtin(vm, b"fstat", &[Value::resource(id)]) {
                stat_mode = st
                    .borrow()
                    .get(&ArrayKey::String(PhpString::from_bytes(b"mode")))
                    .map(|v| v.to_long());
            }
            return add_built(vm, &archive, &str_key, contents, opened, stat_mode);
        }
        Value::Object(o) if instance_of(vm, &o.borrow().class_name.clone(), b"SplFileInfo") => {
            if base.is_empty() {
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "Iterator {} returns an SplFileInfo object, so base directory must be specified",
                        ce
                    ),
                );
            }
            let Some(f) = spl_file_name(vm, o) else {
                return Ok(Value::False);
            };
            let is_dir_obj = goro_core::spl_dir::DIR_CLASSES
                .iter()
                .any(|c| instance_of(vm, &o.borrow().class_name.clone(), c));
            if is_dir_obj && stat_is_dir(vm, &f) == Some(true) {
                return Ok(Value::True);
            }
            let Some(t) = expand_filepath(&f) else {
                return throw(vm, UVE, "Could not resolve file path");
            };
            fname = t;
        }
        _ => {
            return throw(
                vm,
                UVE,
                &format!(
                    "Iterator {} returned an invalid value (must return a string, a stream, or an SplFileInfo object)",
                    ce
                ),
            );
        }
    }
    if !base.is_empty() {
        let Some(b) = expand_filepath(&base) else {
            return throw(vm, UVE, "Could not resolve file path");
        };
        let base_len = b.len();
        let ch_idx = base_len - usize::from(b.last() == Some(&b'/'));
        let ch = fname.get(ch_idx).copied().unwrap_or(0);
        if fname.len() >= base_len && fname.starts_with(&b) && (ch == 0 || ch == b'/') {
            let mut k = fname[base_len..].to_vec();
            if k.is_empty() {
                return Ok(Value::True);
            }
            if k[0] == b'/' || k[0] == b'\\' {
                k.remove(0);
            }
            str_key = k;
        } else {
            return throw(
                vm,
                UVE,
                &format!(
                    "Iterator {} returned a path \"{}\" that is not in the base directory \"{}\"",
                    ce,
                    l(&fname),
                    l(&b)
                ),
            );
        }
    } else {
        let Ok(k) = get_key(vm) else {
            return Ok(Value::False);
        };
        str_key = k;
    }
    // open the source file
    let data = if has_scheme(&fname) {
        goro_ext_standard::native_streams::read_whole_url(vm, &fname, false)
            .ok()
            .flatten()
    } else {
        let p = l(&fname);
        if std::path::Path::new(&p).is_dir() {
            None
        } else {
            std::fs::read(&p).ok()
        }
    };
    let Some(data) = data else {
        return throw(
            vm,
            UVE,
            &format!(
                "Iterator {} returned a file that could not be opened \"{}\"",
                ce,
                l(&fname)
            ),
        );
    };
    if !has_scheme(&fname) {
        use std::os::unix::fs::MetadataExt;
        stat_mode = std::fs::metadata(l(&fname)).ok().map(|m| m.mode() as i64);
        opened = expand_filepath(&fname).unwrap_or_else(|| fname.clone());
        fname.clear();
    } else {
        opened = fname.clone();
    }
    add_built(vm, &archive, &str_key, data, opened, stat_mode)
}

fn umask() -> u32 {
    unsafe {
        let m = libc::umask(0);
        libc::umask(m);
        m as u32
    }
}

fn add_built(
    vm: &mut Vm,
    archive: &ArcRef,
    str_key: &[u8],
    data: Vec<u8>,
    opened: Vec<u8>,
    stat_mode: Option<i64>,
) -> R {
    if str_key.len() >= 5 && str_key.starts_with(b".phar") {
        return Ok(Value::True);
    }
    let fname = archive.borrow().fname.clone();
    let idata = match util::get_or_create_entry_data(vm, &fname, str_key, b"w+b", 0, true) {
        Ok(d) => d,
        Err(e) => {
            return throw(
                vm,
                BMCE,
                &format!(
                    "Entry {} cannot be created: {}",
                    l(str_key),
                    e.unwrap_or_default()
                ),
            );
        }
    };
    let ufp = BUILDS
        .with(|b| b.borrow().last().map(|b| b.fp.clone()))
        .unwrap_or_else(new_buf);
    {
        let mut e = idata.internal_file.borrow_mut();
        e.fp = None;
        e.fp_type = FpType::Ufp;
        let off = ufp.borrow().len() as i64;
        e.offset = off;
        e.offset_abs = off;
        ufp.borrow_mut().extend_from_slice(&data);
        e.uncompressed_filesize = data.len() as u32;
        e.compressed_filesize = data.len() as u32;
        match stat_mode {
            Some(m) => e.flags = m as u32 & PHAR_ENT_PERM_MASK,
            None => e.flags &= !umask(),
        }
    }
    // the build archive's ufp is the temp file
    archive.borrow_mut().ufp = Some(ufp);
    BUILDS.with(|b| {
        if let Some(b) = b.borrow_mut().last_mut() {
            b.ret.set(
                ArrayKey::String(PhpString::from_bytes(str_key)),
                sv(&opened),
            );
        }
    });
    entry_delref(idata);
    Ok(Value::True)
}

// ---------------------------------------------------------------------------
// archive methods

fn readonly_check(vm: &mut Vm, phar: &ArcRef, class: &[u8], msg: &str) -> bool {
    if readonly() && !phar.borrow().is_data {
        let _ = throw(vm, class, msg);
        return false;
    }
    true
}

fn archive_method(
    vm: &mut Vm,
    name: &str,
    m: &str,
    this: Option<Obj>,
    list: Vec<Option<Value>>,
) -> R {
    let l_ = &list;
    macro_rules! p {
        ($spec:expr) => {
            match zpp(vm, name, l_, $spec) {
                Some(a) => a,
                None => return Ok(Value::Null),
            }
        };
    }
    macro_rules! phar {
        () => {
            match archive_of(vm, &this) {
                Some(p) => p,
                None => return Ok(Value::Null),
            }
        };
    }
    macro_rules! flush_or_throw {
        ($vm:expr, $phar:expr, $class:expr) => {
            if let Some(e) = flush($vm, $phar) {
                return throw($vm, $class, &e);
            }
        };
    }
    match m {
        "buildFromDirectory" => {
            let a = p!("P|S");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot write to archive - write operations restricted by INI setting",
            ) {
                return Ok(Value::Null);
            }
            let mut r = PhpArray::new();
            r.push(sv(&a[0].s().unwrap()));
            r.push(sv(&a[1].s().unwrap_or_default()));
            Ok(arr(r))
        }
        "buildFromIterator" => {
            let ok = match l_.first().and_then(|v| v.clone()).map(|v| v.deref()) {
                Some(Value::Object(o)) => {
                    instance_of(vm, &o.borrow().class_name.clone(), b"Traversable")
                }
                Some(Value::Generator(_)) => true,
                _ => false,
            };
            if !ok {
                if l_.is_empty() {
                    p!("O|S!");
                    return Ok(Value::Null);
                }
                let v = l_[0].clone().unwrap_or(Value::Null).deref();
                return throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #1 ($iterator) must be of type Traversable, {} given",
                        name,
                        Vm::value_type_name(&v)
                    ),
                );
            }
            let mut rest: Vec<Option<Value>> = l_.clone();
            rest[0] = Some(Value::Null);
            let a = match zpp(vm, name, &rest, "z|S!") {
                Some(a) => a,
                None => return Ok(Value::Null),
            };
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot write out phar archive, phar is read-only",
            ) {
                return Ok(Value::Null);
            }
            let mut r = PhpArray::new();
            r.push(l_[0].clone().unwrap_or(Value::Null));
            r.push(sv(&a[1].s().unwrap_or_default()));
            Ok(arr(r))
        }
        "count" => {
            p!("|l");
            let phar = phar!();
            let n = phar.borrow().manifest.len() as i64;
            Ok(Value::Long(n))
        }
        "isFileFormat" => {
            let a = p!("l");
            let phar = phar!();
            let p = phar.borrow();
            match a[0].l().unwrap() {
                PHAR_FORMAT_TAR => Ok(bv(p.is_tar)),
                PHAR_FORMAT_ZIP => Ok(bv(p.is_zip)),
                PHAR_FORMAT_PHAR => Ok(bv(!p.is_tar && !p.is_zip)),
                _ => {
                    drop(p);
                    throw(vm, PE, "Unknown file format specified")
                }
            }
        }
        "convertToExecutable" | "convertToData" => {
            let a = p!("|l!l!s!");
            let phar = phar!();
            convert(vm, &phar, m == "convertToData", &a)
        }
        "isCompressed" => {
            p!("");
            let phar = phar!();
            let f = phar.borrow().flags;
            if f & PHAR_FILE_COMPRESSED_GZ != 0 {
                return Ok(Value::Long(PHAR_ENT_COMPRESSED_GZ as i64));
            }
            if f & PHAR_FILE_COMPRESSED_BZ2 != 0 {
                return Ok(Value::Long(PHAR_ENT_COMPRESSED_BZ2 as i64));
            }
            Ok(Value::False)
        }
        "isWritable" => {
            p!("");
            let phar = phar!();
            let (w, fname, brandnew) = {
                let p = phar.borrow();
                (p.is_writeable, p.fname.clone(), p.is_brandnew)
            };
            if !w {
                return Ok(Value::False);
            }
            use std::os::unix::fs::MetadataExt;
            match std::fs::metadata(l(&fname)) {
                Ok(md) => Ok(bv(md.mode() & 0o222 != 0)),
                Err(_) => Ok(bv(brandnew)),
            }
        }
        "delete" => {
            let a = p!("P");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot write out phar archive, phar is read-only",
            ) {
                return Ok(Value::Null);
            }
            let f = a[0].s().unwrap();
            let entry = phar.borrow().find(&f);
            match entry {
                Some(e) => {
                    if e.borrow().is_deleted {
                        return Ok(Value::True);
                    }
                    let mut eb = e.borrow_mut();
                    eb.is_deleted = true;
                    eb.is_modified = true;
                    drop(eb);
                    phar.borrow_mut().is_modified = true;
                }
                None => {
                    return throw(
                        vm,
                        BMCE,
                        &format!("Entry {} does not exist and cannot be deleted", l(&f)),
                    );
                }
            }
            flush_or_throw!(vm, &phar, PE);
            Ok(Value::True)
        }
        "getAlias" => {
            p!("");
            let phar = phar!();
            let a = phar.borrow().alias.clone();
            Ok(match a {
                Some(a) => sv(&a),
                None => Value::Null,
            })
        }
        "getPath" => {
            p!("");
            let phar = phar!();
            let f = phar.borrow().fname.clone();
            Ok(sv(&f))
        }
        "setAlias" => {
            let a = p!("S");
            let phar = phar!();
            set_alias(vm, &phar, &a[0].s().unwrap())
        }
        "getVersion" => {
            p!("");
            let phar = phar!();
            let v = phar.borrow().version.clone();
            Ok(sv(v.as_bytes()))
        }
        "startBuffering" => {
            p!("");
            let phar = phar!();
            phar.borrow_mut().donotflush = true;
            Ok(Value::Null)
        }
        "isBuffering" => {
            p!("");
            let phar = phar!();
            let b = phar.borrow().donotflush;
            Ok(bv(b))
        }
        "stopBuffering" => {
            p!("");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot write out phar archive, phar is read-only",
            ) {
                return Ok(Value::Null);
            }
            phar.borrow_mut().donotflush = false;
            if let Some(e) = flush(vm, &phar) {
                let _ = throw(vm, PE, &e);
            }
            Ok(Value::Null)
        }
        "setStub" => set_stub(vm, name, &this, l_),
        "setDefaultStub" => {
            let a = p!("|s!s!");
            let phar = phar!();
            let (is_data, is_tar, is_zip) = {
                let p = phar.borrow();
                (p.is_data, p.is_tar, p.is_zip)
            };
            if is_data {
                return throw(
                    vm,
                    UVE,
                    if is_tar {
                        "A Phar stub cannot be set in a plain tar archive"
                    } else {
                        "A Phar stub cannot be set in a plain zip archive"
                    },
                );
            }
            let index = a[0].s();
            let webindex = a[1].s();
            if (index.is_some() || webindex.is_some()) && (is_tar || is_zip) {
                let n = if index.is_some() { 1 } else { 2 };
                let pname = if n == 1 { "index" } else { "webIndex" };
                return throw(
                    vm,
                    b"ValueError",
                    &format!(
                        "{}(): Argument #{} (${}) must be null for a tar- or zip-based phar stub, string given",
                        name, n, pname
                    ),
                );
            }
            if readonly() {
                return throw(vm, UVE, "Cannot change stub: phar.readonly=1");
            }
            let mut stub = None;
            if !is_tar && !is_zip {
                match create_default_stub(index.as_deref(), webindex.as_deref()) {
                    Ok(s) => stub = Some(s),
                    Err(e) => return throw(vm, UVE, &e.unwrap_or_default()),
                }
            }
            if let Some(e) = flush_ex(vm, &phar, stub.as_deref(), true) {
                return throw(vm, PE, &e);
            }
            Ok(Value::True)
        }
        "setSignatureAlgorithm" => {
            let a = p!("l|s!");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot set signature algorithm, phar is read-only",
            ) {
                return Ok(Value::Null);
            }
            let algo = a[0].l().unwrap();
            match algo as u32 {
                PHAR_SIG_SHA256
                | PHAR_SIG_SHA512
                | PHAR_SIG_MD5
                | PHAR_SIG_SHA1
                | PHAR_SIG_OPENSSL
                | PHAR_SIG_OPENSSL_SHA256
                | PHAR_SIG_OPENSSL_SHA512
                    if (0..=u32::MAX as i64).contains(&algo) =>
                {
                    {
                        let mut p = phar.borrow_mut();
                        p.sig_flags = algo as u32;
                        p.is_modified = true;
                    }
                    let key = a[1].s();
                    g(|g| g.openssl_privatekey = key);
                    if let Some(e) = flush(vm, &phar) {
                        let _ = throw(vm, PE, &e);
                    }
                    Ok(Value::Null)
                }
                _ => throw(vm, UVE, "Unknown signature algorithm specified"),
            }
        }
        "getSignature" => {
            p!("");
            let phar = phar!();
            let (sig, flags) = {
                let p = phar.borrow();
                (p.signature.clone(), p.sig_flags)
            };
            let Some(sig) = sig else {
                return Ok(Value::False);
            };
            let mut a = PhpArray::new();
            a.set(ArrayKey::String(PhpString::from_bytes(b"hash")), sv(&sig));
            let t = match flags {
                PHAR_SIG_MD5 => "MD5".to_string(),
                PHAR_SIG_SHA1 => "SHA-1".to_string(),
                PHAR_SIG_SHA256 => "SHA-256".to_string(),
                PHAR_SIG_SHA512 => "SHA-512".to_string(),
                PHAR_SIG_OPENSSL => "OpenSSL".to_string(),
                PHAR_SIG_OPENSSL_SHA256 => "OpenSSL_SHA256".to_string(),
                PHAR_SIG_OPENSSL_SHA512 => "OpenSSL_SHA512".to_string(),
                n => format!("Unknown ({})", n),
            };
            a.set(
                ArrayKey::String(PhpString::from_bytes(b"hash_type")),
                sv(t.as_bytes()),
            );
            Ok(arr(a))
        }
        "getModified" => {
            p!("");
            let phar = phar!();
            let b = phar.borrow().is_modified;
            Ok(bv(b))
        }
        "compress" => {
            let a = p!("l|s!");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot compress phar archive, phar is read-only",
            ) {
                return Ok(Value::Null);
            }
            if phar.borrow().is_zip {
                return throw(
                    vm,
                    UVE,
                    "Cannot compress zip-based archives with whole-archive compression",
                );
            }
            let flags = match a[0].l().unwrap() {
                0 => PHAR_FILE_COMPRESSED_NONE,
                n if n == PHAR_ENT_COMPRESSED_GZ as i64 => PHAR_FILE_COMPRESSED_GZ,
                n if n == PHAR_ENT_COMPRESSED_BZ2 as i64 => PHAR_FILE_COMPRESSED_BZ2,
                _ => {
                    return throw(
                        vm,
                        BMCE,
                        "Unknown compression specified, please pass one of Phar::GZ or Phar::BZ2",
                    );
                }
            };
            let fmt = if phar.borrow().is_tar {
                PHAR_FORMAT_TAR
            } else {
                PHAR_FORMAT_PHAR
            };
            convert_to_other(vm, &phar, fmt, a[1].s(), flags)
        }
        "decompress" => {
            let a = p!("|s!");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                UVE,
                "Cannot decompress phar archive, phar is read-only",
            ) {
                return Ok(Value::Null);
            }
            if phar.borrow().is_zip {
                return throw(
                    vm,
                    UVE,
                    "Cannot decompress zip-based archives with whole-archive compression",
                );
            }
            let fmt = if phar.borrow().is_tar {
                PHAR_FORMAT_TAR
            } else {
                PHAR_FORMAT_PHAR
            };
            convert_to_other(vm, &phar, fmt, a[0].s(), PHAR_FILE_COMPRESSED_NONE)
        }
        "compressFiles" => {
            let a = p!("l");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                BMCE,
                "Phar is readonly, cannot change compression",
            ) {
                return Ok(Value::Null);
            }
            let flags = match a[0].l().unwrap() {
                n if n == PHAR_ENT_COMPRESSED_GZ as i64 => PHAR_ENT_COMPRESSED_GZ,
                n if n == PHAR_ENT_COMPRESSED_BZ2 as i64 => PHAR_ENT_COMPRESSED_BZ2,
                _ => {
                    return throw(
                        vm,
                        BMCE,
                        "Unknown compression specified, please pass one of Phar::GZ or Phar::BZ2",
                    );
                }
            };
            if phar.borrow().is_tar {
                return throw(
                    vm,
                    BMCE,
                    "Cannot compress with Gzip compression, tar archives cannot compress individual files, use compress() to compress the whole archive",
                );
            }
            set_compression(&phar, flags);
            phar.borrow_mut().is_modified = true;
            if let Some(e) = flush(vm, &phar) {
                let _ = throw(vm, BMCE, &e);
            }
            Ok(Value::Null)
        }
        "decompressFiles" => {
            p!("");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                BMCE,
                "Phar is readonly, cannot change compression",
            ) {
                return Ok(Value::Null);
            }
            if phar.borrow().is_tar {
                return Ok(Value::True);
            }
            set_compression(&phar, PHAR_ENT_COMPRESSED_NONE);
            phar.borrow_mut().is_modified = true;
            if let Some(e) = flush(vm, &phar) {
                let _ = throw(vm, BMCE, &e);
            }
            Ok(Value::True)
        }
        "copy" => {
            let a = p!("PP");
            let phar = phar!();
            copy(vm, &phar, &a[0].s().unwrap(), &a[1].s().unwrap())
        }
        "offsetExists" => {
            let a = p!("P");
            let phar = phar!();
            let f = a[0].s().unwrap();
            let entry = phar.borrow().find(&f);
            if let Some(e) = entry {
                if e.borrow().is_deleted {
                    return Ok(Value::False);
                }
                if f.starts_with(b".phar") {
                    return Ok(Value::False);
                }
                return Ok(Value::True);
            }
            let info = this
                .as_ref()
                .map(|o| o.borrow().get_property(b"__spl_info_class"))
                .map(|v| v.to_php_string().as_bytes().to_vec())
                .unwrap_or_default();
            if !instance_of(vm, &info, b"PharFileInfo") {
                return Ok(Value::False);
            }
            let r = phar.borrow().virtual_dirs.contains(&f);
            Ok(bv(r))
        }
        "offsetGet" => {
            let a = p!("P");
            let phar = phar!();
            let f = a[0].s().unwrap();
            match util::get_entry_info_dir(vm, &phar, &f, 1, false) {
                Err(e) => {
                    let msg = match e {
                        Some(e) => format!("Entry {} does not exist, {}", l(&f), e),
                        None => format!("Entry {} does not exist", l(&f)),
                    };
                    throw(vm, BMCE, &msg)
                }
                Ok(_) => {
                    let pf = l(&phar.borrow().fname);
                    if f == b".phar/stub.php" {
                        return throw(
                            vm,
                            BMCE,
                            &format!(
                                "Cannot get stub \".phar/stub.php\" directly in phar \"{}\", use getStub",
                                pf
                            ),
                        );
                    }
                    if f == b".phar/alias.txt" {
                        return throw(
                            vm,
                            BMCE,
                            &format!(
                                "Cannot get alias \".phar/alias.txt\" directly in phar \"{}\", use getAlias",
                                pf
                            ),
                        );
                    }
                    if f.starts_with(b".phar") {
                        return throw(
                            vm,
                            BMCE,
                            "Cannot directly get any files or directories in magic \".phar\" directory",
                        );
                    }
                    let mut url = b"phar://".to_vec();
                    url.extend_from_slice(&phar.borrow().fname);
                    url.push(b'/');
                    url.extend_from_slice(&f);
                    let info = this
                        .as_ref()
                        .map(|o| o.borrow().get_property(b"__spl_info_class"))
                        .map(|v| v.to_php_string().as_bytes().to_vec())
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| b"SplFileInfo".to_vec());
                    let mut r = PhpArray::new();
                    r.push(sv(&info));
                    r.push(sv(&url));
                    Ok(arr(r))
                }
            }
        }
        "offsetSet" => {
            let (fname, content, res) = match zpp_ex(vm, name, l_, "Pr", true) {
                Some(a) => (a[0].s().unwrap(), None, a[1].z()),
                None => {
                    let a = p!("PS");
                    (a[0].s().unwrap(), a[1].s(), None)
                }
            };
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                BMCE,
                "Write operations disabled by the php.ini setting phar.readonly",
            ) {
                return Ok(Value::Null);
            }
            let pf = l(&phar.borrow().fname);
            if fname == b".phar/stub.php" {
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "Cannot set stub \".phar/stub.php\" directly in phar \"{}\", use setStub",
                        pf
                    ),
                );
            }
            if fname == b".phar/alias.txt" {
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "Cannot set alias \".phar/alias.txt\" directly in phar \"{}\", use setAlias",
                        pf
                    ),
                );
            }
            if fname.starts_with(b".phar") {
                return throw(
                    vm,
                    BMCE,
                    "Cannot set any files or directories in magic \".phar\" directory",
                );
            }
            add_file(vm, &this, &phar, &fname, content.as_deref(), res);
            Ok(Value::Null)
        }
        "offsetUnset" => {
            let a = p!("P");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                BMCE,
                "Write operations disabled by the php.ini setting phar.readonly",
            ) {
                return Ok(Value::Null);
            }
            let f = a[0].s().unwrap();
            let entry = phar.borrow().find(&f);
            if let Some(e) = entry {
                if e.borrow().is_deleted {
                    return Ok(Value::Null);
                }
                {
                    let mut eb = e.borrow_mut();
                    eb.is_modified = false;
                    eb.is_deleted = true;
                }
                if let Some(e) = flush(vm, &phar) {
                    let _ = throw(vm, PE, &e);
                }
            }
            Ok(Value::Null)
        }
        "addEmptyDir" => {
            let a = p!("P");
            let phar = phar!();
            let d = a[0].s().unwrap();
            if d.starts_with(b".phar") {
                return throw(
                    vm,
                    BMCE,
                    "Cannot create a directory in magic \".phar\" directory",
                );
            }
            let fname = phar.borrow().fname.clone();
            match util::get_or_create_entry_data(vm, &fname, &d, b"w+b", 2, true) {
                Err(e) => {
                    let msg = match e {
                        Some(e) => format!(
                            "Directory {} does not exist and cannot be created: {}",
                            l(&d),
                            e
                        ),
                        None => format!("Directory {} does not exist and cannot be created", l(&d)),
                    };
                    throw(vm, BMCE, &msg)
                }
                Ok(data) => {
                    let p2 = data.phar.clone();
                    entry_delref(data);
                    if let Some(e) = flush(vm, &p2) {
                        let _ = throw(vm, PE, &e);
                    }
                    Ok(Value::Null)
                }
            }
        }
        "addFile" => {
            let a = p!("P|S!");
            let phar = phar!();
            let file = a[0].s().unwrap();
            // a phar:// source stays open while the entry is added
            if starts_with_ci(&file, b"phar://")
                && goro_ext_standard::native_streams::native_wrapper_active(b"phar")
            {
                let src = match split_fname(vm, &file, 2, 0) {
                    Ok((arch, entry)) => {
                        if open_from_filename(vm, &arch, None, 0).is_err() {
                            None
                        } else {
                            match util::get_entry_data(vm, &arch, &entry[1..], b"r", 0, false) {
                                Ok(Some(d)) => Some(d),
                                _ => None,
                            }
                        }
                    }
                    Err(_) => None,
                };
                let Some(idata) = src else {
                    return throw(
                        vm,
                        RTE,
                        &format!(
                            "phar error: unable to open file \"{}\" to add to phar archive",
                            l(&file)
                        ),
                    );
                };
                let data =
                    util::read_entry_contents(vm, &idata.internal_file, true).unwrap_or_default();
                let mode = {
                    let st = crate::stream::dostat(
                        &idata.phar.borrow(),
                        Some(&idata.internal_file.borrow()),
                        false,
                    );
                    st.mode
                };
                let local = a[1].s().unwrap_or(file.clone());
                add_file_data(vm, &this, &phar, &local, data, Some(mode));
                entry_delref(idata);
                return Ok(Value::Null);
            }
            let data = if has_scheme(&file) {
                goro_ext_standard::native_streams::read_whole_url(vm, &file, false)
                    .ok()
                    .flatten()
            } else {
                let p = l(&file);
                if std::path::Path::new(&p).is_dir() {
                    None
                } else {
                    std::fs::read(&p).ok()
                }
            };
            let Some(data) = data else {
                return throw(
                    vm,
                    RTE,
                    &format!(
                        "phar error: unable to open file \"{}\" to add to phar archive",
                        l(&file)
                    ),
                );
            };
            let local = a[1].s().unwrap_or(file.clone());
            let mode = if has_scheme(&file) {
                None
            } else {
                use std::os::unix::fs::MetadataExt;
                std::fs::metadata(l(&file)).ok().map(|m| m.mode() as i64)
            };
            add_file_data(vm, &this, &phar, &local, data, mode);
            Ok(Value::Null)
        }
        "addFromString" => {
            let a = p!("PS");
            let phar = phar!();
            add_file(
                vm,
                &this,
                &phar,
                &a[0].s().unwrap(),
                Some(&a[1].s().unwrap()),
                None,
            );
            Ok(Value::Null)
        }
        "getStub" => {
            p!("");
            let phar = phar!();
            get_stub(vm, &phar)
        }
        "hasMetadata" => {
            p!("");
            let phar = phar!();
            let b = phar.borrow().metadata.has_data();
            Ok(bv(b))
        }
        "getMetadata" => {
            let a = p!("|a");
            let phar = phar!();
            let meta = phar.borrow().metadata.clone();
            get_metadata(vm, &meta, a[0].z(), "Phar::getMetadata")
        }
        "setMetadata" => {
            let a = p!("z");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                BMCE,
                "Write operations disabled by the php.ini setting phar.readonly",
            ) {
                return Ok(Value::Null);
            }
            let v = a[0].z().unwrap();
            let Some(s) = goro_ext_standard::serialize::php_serialize(vm, &v) else {
                return Ok(Value::Null);
            };
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            {
                let mut p = phar.borrow_mut();
                p.metadata.free();
                p.metadata.val = Some(v);
                p.metadata.str = Some(s);
                p.is_modified = true;
            }
            if let Some(e) = flush(vm, &phar) {
                let _ = throw(vm, PE, &e);
            }
            Ok(Value::Null)
        }
        "delMetadata" => {
            p!("");
            let phar = phar!();
            if !readonly_check(
                vm,
                &phar,
                BMCE,
                "Write operations disabled by the php.ini setting phar.readonly",
            ) {
                return Ok(Value::Null);
            }
            if !phar.borrow().metadata.has_data() {
                return Ok(Value::True);
            }
            {
                let mut p = phar.borrow_mut();
                p.metadata.free();
                p.is_modified = true;
            }
            if let Some(e) = flush(vm, &phar) {
                return throw(vm, PE, &e);
            }
            Ok(Value::True)
        }
        "extractTo" => extract_to(vm, name, &this, l_),
        _ => Ok(Value::Null),
    }
}

/// pharobj_set_compression()
fn set_compression(phar: &ArcRef, compress: u32) {
    for e in phar.borrow().manifest.values() {
        let mut e = e.borrow_mut();
        if e.is_deleted {
            continue;
        }
        e.old_flags = e.flags;
        e.flags &= !PHAR_ENT_COMPRESSION_MASK;
        e.flags |= compress;
        e.is_modified = true;
    }
}

/// Phar::setAlias()
fn set_alias(vm: &mut Vm, phar: &ArcRef, new_alias: &[u8]) -> R {
    if !readonly_check(
        vm,
        phar,
        UVE,
        "Cannot write out phar archive, phar is read-only",
    ) {
        return Ok(Value::Null);
    }
    invalidate_last();
    let (is_data, is_tar, cur_alias, fname) = {
        let p = phar.borrow();
        (p.is_data, p.is_tar, p.alias.clone(), p.fname.clone())
    };
    if is_data {
        return throw(
            vm,
            UVE,
            if is_tar {
                "A Phar alias cannot be set in a plain tar archive"
            } else {
                "A Phar alias cannot be set in a plain zip archive"
            },
        );
    }
    if cur_alias.as_deref() == Some(new_alias) {
        return Ok(Value::True);
    }
    let mut valid = false;
    if let Some(fd) = find_alias(new_alias) {
        let msg = format!(
            "alias \"{}\" is already used for archive \"{}\" and cannot be used for other archives",
            l(new_alias),
            l(&fd.borrow().fname)
        );
        if util::free_alias(&fd).is_ok() {
            valid = true;
        } else {
            return throw(vm, PE, &msg);
        }
    }
    if !valid && !validate_alias(new_alias) {
        return throw(
            vm,
            UVE,
            &format!(
                "Invalid alias \"{}\" specified for phar \"{}\"",
                l(new_alias),
                l(&fname)
            ),
        );
    }
    let mut readd = false;
    if let Some(ca) = cur_alias.as_ref().filter(|a| !a.is_empty()) {
        if find_alias(ca).is_some() {
            g(|g| g.alias_map.shift_remove(ca.as_slice()));
            readd = true;
        }
    }
    let old_temp = phar.borrow().is_temporary_alias;
    {
        let mut p = phar.borrow_mut();
        p.alias = if new_alias.is_empty() {
            None
        } else {
            Some(new_alias.to_vec())
        };
        p.is_temporary_alias = false;
    }
    if let Some(e) = flush(vm, phar) {
        {
            let mut p = phar.borrow_mut();
            p.alias = cur_alias.clone();
            p.is_temporary_alias = old_temp;
        }
        if readd {
            if let Some(ca) = cur_alias {
                g(|g| {
                    g.alias_map.entry(ca).or_insert_with(|| phar.clone());
                });
            }
        }
        return throw(vm, PE, &e);
    }
    g(|g| {
        g.alias_map
            .entry(new_alias.to_vec())
            .or_insert_with(|| phar.clone());
    });
    Ok(Value::True)
}

/// Phar::setStub()
fn set_stub(vm: &mut Vm, name: &str, this: &Option<Obj>, list: &[Option<Value>]) -> R {
    let Some(phar) = archive_of(vm, this) else {
        return Ok(Value::Null);
    };
    if !readonly_check(vm, &phar, UVE, "Cannot change stub, phar is read-only") {
        return Ok(Value::Null);
    }
    let (is_data, is_tar) = {
        let p = phar.borrow();
        (p.is_data, p.is_tar)
    };
    if is_data {
        return throw(
            vm,
            UVE,
            if is_tar {
                "A Phar stub cannot be set in a plain tar archive"
            } else {
                "A Phar stub cannot be set in a plain zip archive"
            },
        );
    }
    if let Some(a) = zpp_ex(vm, name, list, "r|l", true) {
        let msg = format!(
            "Calling {}(resource $stub, int $length) is deprecated",
            name
        );
        vm.emit_deprecated_at(&msg, vm.current_line);
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        let res = a[0].z().unwrap();
        let len = a[1].l().unwrap_or(-1);
        let content = if len > 0 {
            call_builtin(vm, b"stream_get_contents", &[res, Value::Long(len)])
        } else {
            call_builtin(vm, b"stream_get_contents", &[res])
        };
        let Some(Value::String(c)) = content else {
            let f = l(&phar.borrow().fname);
            return throw(
                vm,
                PE,
                &format!("unable to read resource to copy stub to new phar \"{}\"", f),
            );
        };
        if let Some(e) = flush_ex(vm, &phar, Some(c.as_bytes()), false) {
            let _ = throw(vm, PE, &e);
        }
        return Ok(Value::True);
    }
    let Some(a) = zpp(vm, name, list, "S") else {
        return Ok(Value::Null);
    };
    let stub = a[0].s().unwrap();
    if let Some(e) = flush_ex(vm, &phar, Some(&stub), false) {
        return throw(vm, PE, &e);
    }
    Ok(Value::True)
}

/// Phar::getStub()
fn get_stub(vm: &mut Vm, phar: &ArcRef) -> R {
    let (is_tar, is_zip) = {
        let p = phar.borrow();
        (p.is_tar, p.is_zip)
    };
    if is_tar || is_zip {
        let stub = phar.borrow().find(b".phar/stub.php");
        let Some(stub) = stub else {
            return Ok(sv(b""));
        };
        let (flags, off, len, csize) = {
            let s = stub.borrow();
            (
                s.flags,
                s.offset_abs.max(0) as usize,
                s.uncompressed_filesize as usize,
                s.compressed_filesize as usize,
            )
        };
        let (fp, brandnew, fname) = {
            let p = phar.borrow();
            (p.fp.clone(), p.is_brandnew, p.fname.clone())
        };
        let data: Vec<u8> =
            if let (Some(fp), false, 0) = (fp, brandnew, flags & PHAR_ENT_COMPRESSION_MASK) {
                fp.borrow().clone()
            } else {
                let Some(d) = read_file(&fname) else {
                    return throw(
                        vm,
                        UVE,
                        &format!("phar error: unable to open phar \"{}\"", l(&fname)),
                    );
                };
                if flags & PHAR_ENT_COMPRESSION_MASK != 0 {
                    let raw = d
                        .get(off..(off + csize).min(d.len()))
                        .unwrap_or_default()
                        .to_vec();
                    match util::decompress(vm, flags, &raw) {
                        Some(dd) => {
                            if dd.len() < len {
                                return throw(vm, RTE, "Unable to read stub");
                            }
                            return Ok(sv(&dd[..len]));
                        }
                        None => return throw(vm, RTE, "Unable to read stub"),
                    }
                }
                d
            };
        if off + len > data.len() {
            return throw(vm, RTE, "Unable to read stub");
        }
        return Ok(sv(&data[off..off + len]));
    }
    let (len, fp, brandnew, fname) = {
        let p = phar.borrow();
        (p.halt_offset, p.fp.clone(), p.is_brandnew, p.fname.clone())
    };
    let data = match fp {
        Some(fp) if !brandnew => fp.borrow().clone(),
        _ => match read_file(&fname) {
            Some(d) => d,
            None => return throw(vm, RTE, "Unable to read stub"),
        },
    };
    if len > data.len() {
        return throw(vm, RTE, "Unable to read stub");
    }
    Ok(sv(&data[..len]))
}

/// phar_metadata_tracker_unserialize_or_copy()
fn get_metadata(vm: &mut Vm, meta: &Meta, options: Option<Value>, method: &str) -> R {
    if !meta.has_data() {
        return Ok(Value::Null);
    }
    let has_opts = matches!(&options, Some(Value::Array(a)) if !a.borrow().is_empty());
    if meta.val.is_none() || has_opts {
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
        let s = meta.str.clone().unwrap_or_default();
        return match goro_ext_standard::serialize::php_unserialize_with_options(
            vm,
            &s,
            options.as_ref(),
            method,
        ) {
            Ok(v) => Ok(v),
            Err(()) => Ok(Value::Null),
        };
    }
    Ok(meta.val.clone().unwrap())
}

/// phar_add_file() with the contents as a string or a stream resource.
fn add_file(
    vm: &mut Vm,
    this: &Option<Obj>,
    phar: &ArcRef,
    file_name: &[u8],
    content: Option<&[u8]>,
    res: Option<Value>,
) {
    if let Some(r) = res {
        let c = call_builtin(vm, b"stream_get_contents", std::slice::from_ref(&r));
        let Some(Value::String(c)) = c else {
            let _ = throw(
                vm,
                BMCE,
                &format!("Entry {} could not be written to", l(file_name)),
            );
            return;
        };
        let mode = match call_builtin(vm, b"fstat", &[r]) {
            Some(Value::Array(a)) => a
                .borrow()
                .get(&ArrayKey::String(PhpString::from_bytes(b"mode")))
                .map(|v| v.to_long()),
            _ => None,
        };
        add_file_data(vm, this, phar, file_name, c.as_bytes().to_vec(), mode);
        return;
    }
    add_file_entry(
        vm,
        this,
        phar,
        file_name,
        content.unwrap_or_default().to_vec(),
        None,
        true,
    );
}

fn add_file_data(
    vm: &mut Vm,
    this: &Option<Obj>,
    phar: &ArcRef,
    file_name: &[u8],
    data: Vec<u8>,
    mode: Option<i64>,
) {
    add_file_entry(vm, this, phar, file_name, data, mode, false);
}

fn add_file_entry(
    vm: &mut Vm,
    this: &Option<Obj>,
    phar: &ArcRef,
    file_name: &[u8],
    data: Vec<u8>,
    mode: Option<i64>,
    from_string: bool,
) {
    if file_name.starts_with(b".phar") || file_name.starts_with(b"/.phar") {
        let prefix_len = usize::from(file_name.first() == Some(&b'/')) + 5;
        let next = file_name.get(prefix_len).copied().unwrap_or(0);
        if next == b'/' || next == b'\\' || next == 0 {
            let _ = throw(
                vm,
                BMCE,
                "Cannot create any files in magic \".phar\" directory",
            );
            return;
        }
    }
    let fname = phar.borrow().fname.clone();
    let idata = match util::get_or_create_entry_data(vm, &fname, file_name, b"w+b", 0, true) {
        Ok(d) => d,
        Err(e) => {
            let msg = match e {
                Some(e) => format!(
                    "Entry {} does not exist and cannot be created: {}",
                    l(file_name),
                    e
                ),
                None => format!(
                    "Entry {} does not exist and cannot be created",
                    l(file_name)
                ),
            };
            let _ = throw(vm, BMCE, &msg);
            return;
        }
    };
    if !idata.internal_file.borrow().is_dir {
        if let Some(fp) = &idata.fp {
            let mut f = fp.borrow_mut();
            let start = (idata.position + idata.zero).max(0) as usize;
            f.truncate(start);
            f.extend_from_slice(&data);
        }
        let mut e = idata.internal_file.borrow_mut();
        e.compressed_filesize = data.len() as u32;
        e.uncompressed_filesize = data.len() as u32;
    }
    {
        let mut e = idata.internal_file.borrow_mut();
        match mode {
            Some(m) if !from_string => e.flags = m as u32 & PHAR_ENT_PERM_MASK,
            _ => e.flags &= !umask(),
        }
    }
    let p2 = idata.phar.clone();
    if !Rc::ptr_eq(&p2, phar) {
        if let Some(o) = this {
            set_archive(o, Some(p2.clone()));
        }
    }
    entry_delref(idata);
    if let Some(e) = flush(vm, &p2) {
        let _ = throw(vm, PE, &e);
    }
}

/// Phar::copy()
fn copy(vm: &mut Vm, phar: &ArcRef, old_file: &[u8], new_file: &[u8]) -> R {
    let pf = l(&phar.borrow().fname);
    if readonly() && !phar.borrow().is_data {
        return throw(
            vm,
            UVE,
            &format!(
                "Cannot copy \"{}\" to \"{}\", phar is read-only",
                l(old_file),
                l(new_file)
            ),
        );
    }
    if old_file.starts_with(b".phar") {
        return throw(
            vm,
            UVE,
            &format!(
                "file \"{}\" cannot be copied to file \"{}\", cannot copy Phar meta-file in {}",
                l(old_file),
                l(new_file),
                pf
            ),
        );
    }
    if new_file.starts_with(b".phar") {
        return throw(
            vm,
            UVE,
            &format!(
                "file \"{}\" cannot be copied to file \"{}\", cannot copy to Phar meta-file in {}",
                l(old_file),
                l(new_file),
                pf
            ),
        );
    }
    let old = phar.borrow().find(old_file);
    let Some(oldentry) = old.filter(|e| !e.borrow().is_deleted) else {
        return throw(
            vm,
            UVE,
            &format!(
                "file \"{}\" cannot be copied to file \"{}\", file does not exist in {}",
                l(old_file),
                l(new_file),
                pf
            ),
        );
    };
    if let Some(t) = phar.borrow().find(new_file) {
        if !t.borrow().is_deleted {
            return throw(
                vm,
                UVE,
                &format!(
                    "file \"{}\" cannot be copied to file \"{}\", file must not already exist in phar {}",
                    l(old_file),
                    l(new_file),
                    pf
                ),
            );
        }
    }
    let chk = path_check(new_file);
    if chk.result.is_err() {
        return throw(
            vm,
            UVE,
            &format!(
                "file \"{}\" contains invalid characters {}, cannot be copied from \"{}\" in phar {}",
                l(new_file),
                chk.error,
                l(old_file),
                pf
            ),
        );
    }
    let mut newentry = oldentry.borrow().clone();
    newentry.filename = new_file.to_vec();
    newentry.fp_refcount = 0;
    if oldentry.borrow().fp_type != FpType::Fp {
        if let Err(e) = util::copy_entry_fp(vm, &oldentry, &mut newentry) {
            return throw(vm, PE, &e.unwrap_or_default());
        }
    }
    let target = oldentry.borrow().phar();
    target
        .borrow_mut()
        .manifest
        .entry(new_file.to_vec())
        .or_insert_with(|| newentry.into_ref());
    phar.borrow_mut().is_modified = true;
    if let Some(e) = flush(vm, phar) {
        let _ = throw(vm, PE, &e);
    }
    Ok(Value::True)
}

/// Phar::convertToExecutable() / convertToData()
fn convert(vm: &mut Vm, phar: &ArcRef, to_data: bool, a: &[A]) -> R {
    if !to_data && readonly() {
        return throw(
            vm,
            UVE,
            "Cannot write out executable phar archive, phar is read-only",
        );
    }
    let mut format = a[0].l().unwrap_or(PHAR_FORMAT_SAME);
    let (is_tar, is_zip, aflags) = {
        let p = phar.borrow();
        (p.is_tar, p.is_zip, p.flags)
    };
    match format {
        9021976 | PHAR_FORMAT_SAME => {
            if is_tar {
                format = PHAR_FORMAT_TAR;
            } else if is_zip {
                format = PHAR_FORMAT_ZIP;
            } else if to_data {
                return throw(
                    vm,
                    UVE,
                    "Cannot write out data phar archive, use Phar::TAR or Phar::ZIP",
                );
            } else {
                format = PHAR_FORMAT_PHAR;
            }
        }
        PHAR_FORMAT_PHAR if to_data => {
            return throw(
                vm,
                UVE,
                "Cannot write out data phar archive, use Phar::TAR or Phar::ZIP",
            );
        }
        PHAR_FORMAT_PHAR | PHAR_FORMAT_TAR | PHAR_FORMAT_ZIP => {}
        _ => {
            return throw(
                vm,
                BMCE,
                if to_data {
                    "Unknown file format specified, please pass one of Phar::TAR or Phar::ZIP"
                } else {
                    "Unknown file format specified, please pass one of Phar::PHAR, Phar::TAR or Phar::ZIP"
                },
            );
        }
    }
    let flags = match a[1].l() {
        None => aflags & PHAR_FILE_COMPRESSION_MASK,
        Some(9021976) => aflags & PHAR_FILE_COMPRESSION_MASK,
        Some(0) => PHAR_FILE_COMPRESSED_NONE,
        Some(n) if n == PHAR_ENT_COMPRESSED_GZ as i64 => {
            if format == PHAR_FORMAT_ZIP {
                return throw(
                    vm,
                    BMCE,
                    "Cannot compress entire archive with gzip, zip archives do not support whole-archive compression",
                );
            }
            PHAR_FILE_COMPRESSED_GZ
        }
        Some(n) if n == PHAR_ENT_COMPRESSED_BZ2 as i64 => {
            if format == PHAR_FORMAT_ZIP {
                return throw(
                    vm,
                    BMCE,
                    "Cannot compress entire archive with bz2, zip archives do not support whole-archive compression",
                );
            }
            PHAR_FILE_COMPRESSED_BZ2
        }
        _ => {
            return throw(
                vm,
                BMCE,
                "Unknown compression specified, please pass one of Phar::GZ or Phar::BZ2",
            );
        }
    };
    let is_data = phar.borrow().is_data;
    phar.borrow_mut().is_data = to_data;
    let r = convert_to_other(vm, phar, format, a[2].s(), flags);
    phar.borrow_mut().is_data = is_data;
    r
}

/// phar_convert_to_other(): [class, fname] of the new object, or null.
fn convert_to_other(
    vm: &mut Vm,
    source: &ArcRef,
    convert: i64,
    ext: Option<Vec<u8>>,
    flags: u32,
) -> R {
    invalidate_last();
    let newphar = Rc::new(RefCell::new(Archive::default()));
    {
        let s = source.borrow();
        let mut p = newphar.borrow_mut();
        p.flags = flags;
        p.is_data = s.is_data;
        match convert {
            PHAR_FORMAT_TAR => p.is_tar = true,
            PHAR_FORMAT_ZIP => p.is_zip = true,
            _ => p.is_data = false,
        }
        p.fp = Some(new_buf());
        p.fname = s.fname.clone();
        p.is_temporary_alias = s.is_temporary_alias;
        p.alias = s.alias.clone();
        p.metadata = s.metadata.clone();
    }
    let entries: Vec<EntryRef> = source.borrow().manifest.values().cloned().collect();
    for er in entries {
        let mut newentry = er.borrow().clone();
        if newentry.link.is_none() && newentry.tmp.is_none() {
            // phar_copy_file_contents()
            if let Err(e) = util::open_entry_fp(vm, &er, true) {
                let s = er.borrow();
                let pf = l(&s.phar().borrow().fname);
                let msg = match e {
                    Some(e) => format!(
                        "Cannot convert phar archive \"{}\", unable to open entry \"{}\" contents: {}",
                        pf,
                        l(&s.filename),
                        e
                    ),
                    None => format!(
                        "Cannot convert phar archive \"{}\", unable to open entry \"{}\" contents",
                        pf,
                        l(&s.filename)
                    ),
                };
                drop(s);
                return throw(vm, UVE, &msg);
            }
            // (the copy reads the source entry's now-open fp)
            newentry.fp_type = er.borrow().fp_type;
            newentry.offset = er.borrow().offset;
            let data = util::read_entry_contents(vm, &er, false);
            let Some(data) = data else {
                let s = er.borrow();
                let msg = format!(
                    "Cannot convert phar archive \"{}\", unable to copy entry \"{}\" contents",
                    l(&s.phar().borrow().fname),
                    l(&s.filename)
                );
                drop(s);
                return throw(vm, UVE, &msg);
            };
            let fp = newphar.borrow().fp.clone().unwrap();
            let offset = fp.borrow().len() as i64;
            fp.borrow_mut().extend_from_slice(&data);
            newentry.fp_type = FpType::Fp;
            newentry.offset = offset;
        }
        newentry.fp = None;
        newentry.cfp = None;
        let (is_zip, is_tar) = {
            let p = newphar.borrow();
            (p.is_zip, p.is_tar)
        };
        newentry.is_zip = is_zip;
        newentry.is_tar = is_tar;
        if newentry.is_tar {
            newentry.tar_type = if er.borrow().is_dir {
                TAR_DIR
            } else {
                TAR_FILE
            };
        }
        newentry.header_offset = 0;
        newentry.is_modified = true;
        newentry.phar = Rc::downgrade(&newphar);
        newentry.old_flags = newentry.flags & !PHAR_ENT_COMPRESSION_MASK;
        newentry.fp_refcount = 0;
        newentry.fileinfo_lock_count = 0;
        let fname = newphar.borrow().fname.clone();
        set_inode(&mut newentry, &fname);
        let key = newentry.filename.clone();
        util::add_virtual_dirs(&mut newphar.borrow_mut(), &key);
        newphar
            .borrow_mut()
            .manifest
            .entry(key)
            .or_insert_with(|| newentry.into_ref());
    }
    rename_archive(vm, newphar, ext)
}

/// phar_rename_archive()
fn rename_archive(vm: &mut Vm, phar: ArcRef, ext: Option<Vec<u8>>) -> R {
    const EXT_LIST: &[&[u8]] = &[
        b".phar.tar.bz2",
        b".phar.tar.gz",
        b".phar.php",
        b".phar.bz2",
        b".phar.zip",
        b".phar.tar",
        b".phar.gz",
        b".tar.bz2",
        b".tar.gz",
        b".phar",
        b".tar",
        b".zip",
    ];
    let (is_zip, is_tar, is_data, flags, oldpath) = {
        let p = phar.borrow();
        (p.is_zip, p.is_tar, p.is_data, p.flags, p.fname.clone())
    };
    let ext: Vec<u8> = match ext {
        None => {
            let e: &[u8] = if is_zip {
                if is_data { b"zip" } else { b"phar.zip" }
            } else if is_tar {
                match flags {
                    PHAR_FILE_COMPRESSED_GZ => {
                        if is_data {
                            b"tar.gz"
                        } else {
                            b"phar.tar.gz"
                        }
                    }
                    PHAR_FILE_COMPRESSED_BZ2 => {
                        if is_data {
                            b"tar.bz2"
                        } else {
                            b"phar.tar.bz2"
                        }
                    }
                    _ => {
                        if is_data {
                            b"tar"
                        } else {
                            b"phar.tar"
                        }
                    }
                }
            } else {
                match flags {
                    PHAR_FILE_COMPRESSED_GZ => b"phar.gz",
                    PHAR_FILE_COMPRESSED_BZ2 => b"phar.bz2",
                    _ => b"phar",
                }
            };
            e.to_vec()
        }
        Some(e) => {
            let chk = path_check(&e);
            if chk.result.is_err() {
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "{}phar converted from \"{}\" has invalid extension {}",
                        if is_data { "data " } else { "" },
                        l(&oldpath),
                        l(&e)
                    ),
                );
            }
            // (phar_path_check() strips a leading '/')
            chk.path(&e).to_vec()
        }
    };
    let oldname_start = oldpath
        .iter()
        .rposition(|&c| c == b'/')
        .map(|i| i + 1)
        .unwrap_or(0);
    let oldname = &oldpath[oldname_start..];
    let mut basename = oldname.to_vec();
    let mut found = false;
    for e in EXT_LIST {
        if oldname.len() > e.len() && oldname.ends_with(e) {
            basename.truncate(oldname.len() - e.len());
            found = true;
            break;
        }
    }
    if !found {
        if let Some(d) = basename.iter().rposition(|&c| c == b'.') {
            basename.truncate(d);
        }
    }
    let ext = if ext.first() == Some(&b'.') {
        ext[1..].to_vec()
    } else {
        ext
    };
    let mut newname = basename;
    newname.push(b'.');
    newname.extend_from_slice(&ext);
    let mut newpath = oldpath[..oldname_start].to_vec();
    newpath.extend_from_slice(&newname);
    {
        let mut p = phar.borrow_mut();
        p.fname = newpath.clone();
        p.ext = Some(newpath.len() - ext.len() - 1);
    }
    let mut phar = phar;
    let mut reused: Option<ArcRef> = None;
    if let Some(pphar) = find_fname(&newpath) {
        if phar.borrow().manifest.is_empty() {
            {
                let src = phar.borrow();
                let mut pp = pphar.borrow_mut();
                pp.is_tar = src.is_tar;
                pp.is_zip = src.is_zip;
                pp.is_data = src.is_data;
                pp.flags = src.flags;
                pp.fp = src.fp.clone();
            }
            phar = pphar.clone();
            phar.borrow_mut().refcount += 1;
            reused = Some(pphar);
        } else {
            let f = l(&newpath);
            return throw(
                vm,
                BMCE,
                &format!(
                    "Unable to add newly converted phar \"{}\" to the list of phars, a phar with that name already exists",
                    f
                ),
            );
        }
    }
    let err_reused = |phar: &ArcRef, reused: &Option<ArcRef>| {
        if reused.as_ref().is_some_and(|r| Rc::ptr_eq(r, phar)) {
            phar.borrow_mut().refcount -= 1;
        }
    };
    let check_path = if reused.is_some() {
        oldpath.clone()
    } else {
        newpath.clone()
    };
    if std::fs::metadata(l(&check_path)).is_ok() {
        err_reused(&phar, &reused);
        return throw(
            vm,
            BMCE,
            &format!(
                "phar \"{}\" exists and must be unlinked prior to conversion",
                l(&check_path)
            ),
        );
    }
    let is_data = phar.borrow().is_data;
    let flen = newpath.len();
    let (mut es, mut el) = (None, 0i64);
    if !is_data {
        if !detect_phar_fname_ext(vm, &newpath, &mut es, &mut el, 1, 1, true) {
            err_reused(&phar, &reused);
            return throw(
                vm,
                BMCE,
                &format!(
                    "phar \"{}\" has invalid extension {}",
                    l(&phar.borrow().fname),
                    l(&ext)
                ),
            );
        }
        {
            let mut p = phar.borrow_mut();
            p.ext = es;
            p.ext_len = el.max(0) as usize;
        }
        let is_reused = reused.as_ref().is_some_and(|r| Rc::ptr_eq(r, &phar));
        if phar.borrow().alias.is_some() && !is_reused {
            let temp = phar.borrow().is_temporary_alias;
            if temp {
                phar.borrow_mut().alias = None;
            } else {
                {
                    let mut p = phar.borrow_mut();
                    p.alias = Some(newpath.clone());
                    p.is_temporary_alias = true;
                }
                g(|g| {
                    g.alias_map.insert(newpath.clone(), phar.clone());
                });
            }
        }
    } else {
        if !detect_phar_fname_ext(vm, &newpath, &mut es, &mut el, 0, 1, true) {
            err_reused(&phar, &reused);
            return throw(
                vm,
                BMCE,
                &format!(
                    "data phar \"{}\" has invalid extension {}",
                    l(&phar.borrow().fname),
                    l(&ext)
                ),
            );
        }
        {
            let mut p = phar.borrow_mut();
            p.ext = es;
            p.ext_len = el.max(0) as usize;
        }
        let is_reused = reused.as_ref().is_some_and(|r| Rc::ptr_eq(r, &phar));
        if !is_reused {
            phar.borrow_mut().alias = None;
        }
    }
    let _ = flen;
    if reused.is_none() {
        g(|g| {
            g.fname_map.insert(newpath.clone(), phar.clone());
        });
    }
    if let Some(e) = flush_ex(vm, &phar, None, true) {
        fname_map_del(&newpath);
        return throw(vm, BMCE, &e);
    }
    let class: &[u8] = if phar.borrow().is_data {
        b"PharData"
    } else {
        b"Phar"
    };
    let fname = phar.borrow().fname.clone();
    let mut r = PhpArray::new();
    r.push(sv(class));
    r.push(sv(&fname));
    Ok(arr(r))
}

/// Phar::extractTo()
fn extract_to(vm: &mut Vm, name: &str, this: &Option<Obj>, list: &[Option<Value>]) -> R {
    // Z_PARAM_PATH, Z_PARAM_ARRAY_HT_OR_STR_OR_NULL, Z_PARAM_BOOL
    let mut l2 = list.to_vec();
    let files = if l2.len() > 1 {
        let v = l2[1].clone().map(|v| v.deref());
        l2[1] = Some(Value::Null);
        v
    } else {
        None
    };
    let Some(a) = zpp(vm, name, &l2, "p|z!b") else {
        return Ok(Value::Null);
    };
    let files = match files {
        Some(Value::Array(a)) => Some(Ok(a)),
        Some(Value::Null) | None => None,
        Some(v @ Value::String(_)) => Some(Err(v.to_php_string().as_bytes().to_vec())),
        Some(v) => match &v {
            Value::Long(_) | Value::Double(_) | Value::True | Value::False => {
                Some(Err(v.to_php_string().as_bytes().to_vec()))
            }
            _ => {
                return throw(
                    vm,
                    b"TypeError",
                    &format!(
                        "{}(): Argument #2 ($files) must be of type array|string|null, {} given",
                        name,
                        Vm::value_type_name(&v)
                    ),
                );
            }
        },
    };
    let overwrite = a[2].b().unwrap_or(false);
    let Some(phar) = archive_of(vm, this) else {
        return Ok(Value::Null);
    };
    let pathto = a[0].s().unwrap();
    let pf = phar.borrow().fname.clone();
    if read_file(&pf).is_none() {
        return throw(
            vm,
            IAE,
            &format!("Invalid argument, {} cannot be found", l(&pf)),
        );
    }
    if pathto.is_empty() {
        return throw(
            vm,
            IAE,
            "Invalid argument, extraction path must be non-zero length",
        );
    }
    if pathto.len() >= 4096 {
        let t = &pathto[..50];
        return throw(
            vm,
            IAE,
            &format!(
                "Cannot extract to \"{}...\", destination directory is too long for filesystem",
                l(t)
            ),
        );
    }
    match std::fs::metadata(l(&pathto)) {
        Err(_) => {
            if mkdir_p(&pathto, 0o777).is_err() {
                return throw(
                    vm,
                    RTE,
                    &format!("Unable to create path \"{}\" for extraction", l(&pathto)),
                );
            }
        }
        Ok(m) if !m.is_dir() => {
            return throw(
                vm,
                RTE,
                &format!(
                    "Unable to use path \"{}\" for extraction, it is a file, must be a directory",
                    l(&pathto)
                ),
            );
        }
        _ => {}
    }
    match files {
        Some(Ok(ht)) => {
            if ht.borrow().is_empty() {
                return Ok(Value::False);
            }
            let vals: Vec<Value> = ht.borrow().values().cloned().collect();
            for v in vals {
                let Value::String(f) = v.deref() else {
                    return throw(
                        vm,
                        IAE,
                        "Invalid argument, array of filenames to extract contains non-string value",
                    );
                };
                match extract_helper(vm, &phar, Some(f.as_bytes()), &pathto, overwrite) {
                    Err(e) => {
                        return throw(
                            vm,
                            PE,
                            &format!("Extraction from phar \"{}\" failed: {}", l(&pf), e),
                        );
                    }
                    Ok(0) => {
                        return throw(
                            vm,
                            PE,
                            &format!(
                                "phar error: attempted to extract non-existent file or directory \"{}\" from phar \"{}\"",
                                f.to_string_lossy(),
                                l(&pf)
                            ),
                        );
                    }
                    Ok(_) => {}
                }
            }
            Ok(Value::True)
        }
        other => {
            let filename = other.map(|r| r.unwrap_err());
            match extract_helper(vm, &phar, filename.as_deref(), &pathto, overwrite) {
                Err(e) => throw(
                    vm,
                    PE,
                    &format!("Extraction from phar \"{}\" failed: {}", l(&pf), e),
                ),
                Ok(0) if filename.is_some() => throw(
                    vm,
                    PE,
                    &format!(
                        "phar error: attempted to extract non-existent file or directory \"{}\" from phar \"{}\"",
                        l(filename.as_deref().unwrap()),
                        l(&pf)
                    ),
                ),
                Ok(_) => Ok(Value::True),
            }
        }
    }
}

fn mkdir_p(path: &[u8], mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(l(path))
}

fn extract_helper(
    vm: &mut Vm,
    phar: &ArcRef,
    search: Option<&[u8]>,
    pathto: &[u8],
    overwrite: bool,
) -> Result<i32, String> {
    let entries: Vec<EntryRef> = phar.borrow().manifest.values().cloned().collect();
    match search {
        None => {
            let mut n = 0;
            for e in entries {
                extract_file(vm, overwrite, &e, pathto)?;
                n += 1;
            }
            Ok(n)
        }
        Some(s) if !s.is_empty() && s.last() == Some(&b'/') => {
            let mut n = 0;
            for e in entries {
                if !e.borrow().filename.starts_with(s) {
                    continue;
                }
                extract_file(vm, overwrite, &e, pathto)?;
                n += 1;
            }
            Ok(n)
        }
        Some(s) => {
            let Some(e) = phar.borrow().find(s) else {
                return Ok(0);
            };
            extract_file(vm, overwrite, &e, pathto)?;
            Ok(1)
        }
    }
}

/// phar_extract_file()
fn extract_file(vm: &mut Vm, overwrite: bool, entry: &EntryRef, dest: &[u8]) -> Result<(), String> {
    let (is_mounted, fname, is_dir, flags) = {
        let e = entry.borrow();
        (e.is_mounted, e.filename.clone(), e.is_dir, e.flags)
    };
    if is_mounted || fname.starts_with(b".phar") {
        return Ok(());
    }
    // virtual_file_ex() relative to "/" fails (EINVAL) for a path that
    // does not fit MAXPATHLEN: the message names the destination itself
    let too_long = if fname.first() == Some(&b'/') {
        fname.len() >= 4096 - 1
    } else {
        1 + fname.len() + 1 >= 4096 - 1
    };
    if too_long && fname.len() > 50 {
        return Err(format!(
            "Cannot extract \"{}...\" to \"{}...\", extracted filename is too long for filesystem",
            l(&fname[..50]),
            l(dest)
        ));
    }
    // virtual_file_ex() relative to "/"
    let mut parts: Vec<&[u8]> = Vec::new();
    for seg in fname.split(|&c| c == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err(format!("Cannot extract \"{}\", internal error", l(&fname)));
    }
    let filename: Vec<u8> = parts.join(&b'/');
    let mut fullpath = dest.to_vec();
    fullpath.push(b'/');
    fullpath.extend_from_slice(&filename);
    if fullpath.len() >= 4096 {
        let fp50 = &fullpath[..50];
        if fname.len() > 50 {
            return Err(format!(
                "Cannot extract \"{}...\" to \"{}...\", extracted filename is too long for filesystem",
                l(&fname[..50]),
                l(fp50)
            ));
        }
        return Err(format!(
            "Cannot extract \"{}\" to \"{}...\", extracted filename is too long for filesystem",
            l(&fname),
            l(fp50)
        ));
    }
    if !overwrite && std::fs::symlink_metadata(l(&fullpath)).is_ok() {
        return Err(format!(
            "Cannot extract \"{}\" to \"{}\", path already exists",
            l(&fname),
            l(&fullpath)
        ));
    }
    let dir: Vec<u8> = match filename.iter().rposition(|&c| c == b'/') {
        Some(s) => fullpath[..dest.len() + s + 1].to_vec(),
        None => fullpath[..dest.len()].to_vec(),
    };
    if std::fs::metadata(l(&dir)).is_err() {
        let mode = if is_dir {
            flags & PHAR_ENT_PERM_MASK
        } else {
            0o777
        };
        if mkdir_p(&dir, mode).is_err() {
            return Err(format!(
                "Cannot extract \"{}\", could not create directory \"{}\"",
                l(&fname),
                l(&dir)
            ));
        }
    }
    if is_dir {
        return Ok(());
    }
    if let Err(e) = util::open_entry_fp(vm, entry, true) {
        return Err(match e {
            Some(e) => format!(
                "Cannot extract \"{}\" to \"{}\", unable to open internal file pointer: {}",
                l(&fname),
                l(&fullpath),
                e
            ),
            None => format!(
                "Cannot extract \"{}\" to \"{}\", unable to open internal file pointer",
                l(&fname),
                l(&fullpath)
            ),
        });
    }
    let Some(data) = util::read_entry_contents(vm, entry, false) else {
        return Err(format!(
            "Cannot extract \"{}\" to \"{}\", unable to seek internal file pointer",
            l(&fname),
            l(&fullpath)
        ));
    };
    if std::fs::write(l(&fullpath), &data).is_err() {
        return Err(format!(
            "Cannot extract \"{}\", could not open for writing \"{}\"",
            l(&fname),
            l(&fullpath)
        ));
    }
    use std::os::unix::fs::PermissionsExt;
    if std::fs::set_permissions(
        l(&fullpath),
        std::fs::Permissions::from_mode(flags & PHAR_ENT_PERM_MASK),
    )
    .is_err()
    {
        return Err(format!(
            "Cannot extract \"{}\" to \"{}\", setting file permissions failed",
            l(&fname),
            l(&fullpath)
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Phar::webPhar()

// ---------------------------------------------------------------------------
// PharFileInfo

fn entry_method(
    vm: &mut Vm,
    name: &str,
    m: &str,
    this: Option<Obj>,
    list: Vec<Option<Value>>,
) -> R {
    let l_ = &list;
    macro_rules! p {
        ($spec:expr) => {
            match zpp(vm, name, l_, $spec) {
                Some(a) => a,
                None => return Ok(Value::Null),
            }
        };
    }
    macro_rules! entry {
        () => {
            match entry_of(vm, &this) {
                Some(e) => e,
                None => return Ok(Value::Null),
            }
        };
    }
    match m {
        "__construct" => {
            let a = p!("p");
            let Some(this) = this else {
                return Ok(Value::Null);
            };
            let fname = a[0].s().unwrap();
            if entry_data(&this).is_some_and(|d| d.entry.borrow().is_some()) {
                return throw(vm, BMCE, "Cannot call constructor twice");
            }
            let split = if fname.len() < 7 || !fname.starts_with(b"phar://") {
                None
            } else {
                split_fname(vm, &fname, 2, 0).ok()
            };
            let Some((arch, entry)) = split else {
                return throw(
                    vm,
                    RTE,
                    &format!(
                        "'{}' is not a valid phar archive URL (must have at least phar://filename.phar)",
                        l(&fname)
                    ),
                );
            };
            let phar = match open_from_filename(vm, &arch, None, REPORT_ERRORS) {
                Ok(p) => p,
                Err(e) => {
                    return throw(
                        vm,
                        RTE,
                        &match e {
                            Some(e) => format!("Cannot open phar file '{}': {}", l(&fname), e),
                            None => format!("Cannot open phar file '{}'", l(&fname)),
                        },
                    );
                }
            };
            let info = match util::get_entry_info_dir(vm, &phar, &entry, 1, true) {
                Ok(i) => i,
                Err(e) => {
                    let (sep, msg) = match &e {
                        Some(m) => (", ", m.clone()),
                        None => ("", String::new()),
                    };
                    return throw(
                        vm,
                        RTE,
                        &format!(
                            "Cannot access phar file entry '{}' in archive '{}'{}{}",
                            l(&entry),
                            l(&arch),
                            sep,
                            msg
                        ),
                    );
                }
            };
            if !info.borrow().is_temp_dir {
                info.borrow_mut().fileinfo_lock_count += 1;
                phar.borrow_mut().refcount += 1;
            }
            this.borrow_mut().native = Some(NativeData {
                data: Rc::new(EntryObj {
                    entry: RefCell::new(Some(info)),
                }),
                handlers: &ENTRY_HANDLERS,
            });
            Ok(sv(&fname))
        }
        "__destruct" => {
            p!("");
            if let Some(d) = this.as_ref().and_then(entry_data) {
                d.release();
            }
            Ok(Value::Null)
        }
        "getCompressedSize" => {
            p!("");
            let e = entry!();
            let n = e.borrow().compressed_filesize as i64;
            Ok(Value::Long(n))
        }
        "isCompressed" => {
            let a = p!("|l!");
            let e = entry!();
            let flags = e.borrow().flags;
            match a[0].l() {
                None | Some(9021976) => Ok(bv(flags & PHAR_ENT_COMPRESSION_MASK != 0)),
                Some(n) if n == PHAR_ENT_COMPRESSED_GZ as i64 => {
                    Ok(bv(flags & PHAR_ENT_COMPRESSED_GZ != 0))
                }
                Some(n) if n == PHAR_ENT_COMPRESSED_BZ2 as i64 => {
                    Ok(bv(flags & PHAR_ENT_COMPRESSED_BZ2 != 0))
                }
                _ => throw(vm, BMCE, "Unknown compression type specified"),
            }
        }
        "getCRC32" => {
            p!("");
            let e = entry!();
            let (is_dir, checked, crc) = {
                let eb = e.borrow();
                (eb.is_dir, eb.is_crc_checked, eb.crc32)
            };
            if is_dir {
                return throw(vm, BMCE, "Phar entry is a directory, does not have a CRC");
            }
            if checked {
                Ok(Value::Long(crc as i64))
            } else {
                throw(vm, BMCE, "Phar entry was not CRC checked")
            }
        }
        "isCRCChecked" => {
            p!("");
            let e = entry!();
            let b = e.borrow().is_crc_checked;
            Ok(bv(b))
        }
        "getPharFlags" => {
            p!("");
            let e = entry!();
            let f = e.borrow().flags & !(PHAR_ENT_PERM_MASK | PHAR_ENT_COMPRESSION_MASK);
            Ok(Value::Long(f as i64))
        }
        "chmod" => {
            let a = p!("l");
            let e = entry!();
            if e.borrow().is_temp_dir {
                let f = l(&e.borrow().filename);
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "Phar entry \"{}\" is a temporary directory (not an actual entry in the archive), cannot chmod",
                        f
                    ),
                );
            }
            let phar = e.borrow().phar();
            if readonly() && !phar.borrow().is_data {
                let msg = format!(
                    "Cannot modify permissions for file \"{}\" in phar \"{}\", write operations are prohibited",
                    l(&e.borrow().filename),
                    l(&phar.borrow().fname)
                );
                return throw(vm, PE, &msg);
            }
            {
                let mut eb = e.borrow_mut();
                eb.flags &= !PHAR_ENT_PERM_MASK;
                eb.flags |= (a[0].l().unwrap() & 0o777) as u32;
                eb.old_flags = eb.flags;
                eb.is_modified = true;
            }
            phar.borrow_mut().is_modified = true;
            goro_ext_standard::native_streams::clear_stat_cache_pub();
            if let Some(err) = flush(vm, &phar) {
                let _ = throw(vm, PE, &err);
            }
            Ok(Value::Null)
        }
        "hasMetadata" => {
            p!("");
            let e = entry!();
            let b = e.borrow().metadata.has_data();
            Ok(bv(b))
        }
        "getMetadata" => {
            let a = p!("|a");
            let e = entry!();
            let meta = e.borrow().metadata.clone();
            get_metadata(vm, &meta, a[0].z(), "PharFileInfo::getMetadata")
        }
        "setMetadata" => {
            let a = p!("z");
            let e = entry!();
            let phar = e.borrow().phar();
            if readonly() && !phar.borrow().is_data {
                return throw(
                    vm,
                    BMCE,
                    "Write operations disabled by the php.ini setting phar.readonly",
                );
            }
            if e.borrow().is_temp_dir {
                return throw(
                    vm,
                    BMCE,
                    "Phar entry is a temporary directory (not an actual entry in the archive), cannot set metadata",
                );
            }
            let v = a[0].z().unwrap();
            let Some(s) = goro_ext_standard::serialize::php_serialize(vm, &v) else {
                return Ok(Value::Null);
            };
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            {
                let mut eb = e.borrow_mut();
                eb.metadata.free();
                eb.metadata.val = Some(v);
                eb.metadata.str = Some(s);
                eb.is_modified = true;
            }
            phar.borrow_mut().is_modified = true;
            if let Some(err) = flush(vm, &phar) {
                let _ = throw(vm, PE, &err);
            }
            Ok(Value::Null)
        }
        "delMetadata" => {
            p!("");
            let e = entry!();
            let phar = e.borrow().phar();
            if readonly() && !phar.borrow().is_data {
                return throw(
                    vm,
                    BMCE,
                    "Write operations disabled by the php.ini setting phar.readonly",
                );
            }
            if e.borrow().is_temp_dir {
                return throw(
                    vm,
                    BMCE,
                    "Phar entry is a temporary directory (not an actual entry in the archive), cannot delete metadata",
                );
            }
            if e.borrow().metadata.has_data() {
                {
                    let mut eb = e.borrow_mut();
                    eb.metadata.free();
                    eb.is_modified = true;
                }
                phar.borrow_mut().is_modified = true;
                if let Some(err) = flush(vm, &phar) {
                    return throw(vm, PE, &err);
                }
            }
            Ok(Value::True)
        }
        "getContent" => {
            p!("");
            let e = entry!();
            let phar = e.borrow().phar();
            let (is_dir, fname) = {
                let eb = e.borrow();
                (eb.is_dir, eb.filename.clone())
            };
            let pf = l(&phar.borrow().fname);
            if is_dir {
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "phar error: Cannot retrieve contents, \"{}\" in phar \"{}\" is a directory",
                        l(&fname),
                        pf
                    ),
                );
            }
            let link = util::get_link_source(&e).unwrap_or_else(|| e.clone());
            if let Err(err) = util::open_entry_fp(vm, &link, false) {
                return throw(
                    vm,
                    BMCE,
                    &format!(
                        "phar error: Cannot retrieve contents, \"{}\" in phar \"{}\": {}",
                        l(&fname),
                        pf,
                        err.unwrap_or_default()
                    ),
                );
            }
            match util::read_entry_contents(vm, &link, false) {
                Some(d) => Ok(sv(&d)),
                None => throw(
                    vm,
                    BMCE,
                    &format!(
                        "phar error: Cannot retrieve contents of \"{}\" in phar \"{}\"",
                        l(&fname),
                        pf
                    ),
                ),
            }
        }
        "compress" => {
            let a = p!("l");
            let e = entry!();
            entry_compress(vm, &e, a[0].l().unwrap())
        }
        "decompress" => {
            p!("");
            let e = entry!();
            entry_decompress(vm, &e)
        }
        _ => Ok(Value::Null),
    }
}

/// PharFileInfo::compress()
fn entry_compress(vm: &mut Vm, e: &EntryRef, method: i64) -> R {
    let (is_tar, is_dir, is_deleted, flags) = {
        let eb = e.borrow();
        (eb.is_tar, eb.is_dir, eb.is_deleted, eb.flags)
    };
    if is_tar {
        return throw(
            vm,
            BMCE,
            "Cannot compress with Gzip compression, not possible with tar-based phar archives",
        );
    }
    if is_dir {
        return throw(
            vm,
            BMCE,
            "Phar entry is a directory, cannot set compression",
        );
    }
    let phar = e.borrow().phar();
    if readonly() && !phar.borrow().is_data {
        return throw(vm, BMCE, "Phar is readonly, cannot change compression");
    }
    if is_deleted {
        return throw(vm, BMCE, "Cannot compress deleted file");
    }
    let (want, other, oname, wname) = match method {
        n if n == PHAR_ENT_COMPRESSED_GZ as i64 => (
            PHAR_ENT_COMPRESSED_GZ,
            PHAR_ENT_COMPRESSED_BZ2,
            "bzip2",
            "gzip",
        ),
        n if n == PHAR_ENT_COMPRESSED_BZ2 as i64 => (
            PHAR_ENT_COMPRESSED_BZ2,
            PHAR_ENT_COMPRESSED_GZ,
            "gzip",
            "bzip2",
        ),
        _ => return throw(vm, BMCE, "Unknown compression type specified"),
    };
    if flags & want != 0 {
        return Ok(Value::True);
    }
    if flags & other != 0 {
        if let Err(err) = util::open_entry_fp(vm, e, true) {
            let (f, pf) = (l(&e.borrow().filename), l(&phar.borrow().fname));
            return throw(
                vm,
                BMCE,
                &format!(
                    "phar error: Cannot decompress {}-compressed file \"{}\" in phar \"{}\" in order to compress with {}: {}",
                    oname,
                    f,
                    pf,
                    wname,
                    err.unwrap_or_default()
                ),
            );
        }
    }
    {
        let mut eb = e.borrow_mut();
        eb.old_flags = eb.flags;
        eb.flags &= !PHAR_ENT_COMPRESSION_MASK;
        eb.flags |= want;
        eb.is_modified = true;
    }
    phar.borrow_mut().is_modified = true;
    if let Some(err) = flush(vm, &phar) {
        return throw(vm, PE, &err);
    }
    Ok(Value::True)
}

/// PharFileInfo::decompress()
fn entry_decompress(vm: &mut Vm, e: &EntryRef) -> R {
    let (is_dir, flags, is_deleted) = {
        let eb = e.borrow();
        (eb.is_dir, eb.flags, eb.is_deleted)
    };
    if is_dir {
        return throw(
            vm,
            BMCE,
            "Phar entry is a directory, cannot set compression",
        );
    }
    if flags & PHAR_ENT_COMPRESSION_MASK == 0 {
        return Ok(Value::True);
    }
    let phar = e.borrow().phar();
    if readonly() && !phar.borrow().is_data {
        return throw(vm, BMCE, "Phar is readonly, cannot decompress");
    }
    if is_deleted {
        return throw(vm, BMCE, "Cannot compress deleted file");
    }
    let ctype = match flags & PHAR_ENT_COMPRESSION_MASK {
        PHAR_ENT_COMPRESSED_GZ => "gzip",
        PHAR_ENT_COMPRESSED_BZ2 => "bz2",
        _ => {
            return throw(
                vm,
                BMCE,
                "Cannot decompress file compressed with unknown compression type",
            );
        }
    };
    if let Err(err) = util::open_entry_fp(vm, e, true) {
        let (f, pf) = (l(&e.borrow().filename), l(&phar.borrow().fname));
        return throw(
            vm,
            BMCE,
            &format!(
                "Phar error: Cannot decompress {}-compressed file \"{}\" in phar \"{}\": {}",
                ctype,
                f,
                pf,
                err.unwrap_or_default()
            ),
        );
    }
    {
        let mut eb = e.borrow_mut();
        eb.old_flags = eb.flags;
        eb.flags &= !PHAR_ENT_COMPRESSION_MASK;
        eb.is_modified = true;
    }
    phar.borrow_mut().is_modified = true;
    if let Some(err) = flush(vm, &phar) {
        return throw(vm, PE, &err);
    }
    Ok(Value::True)
}
