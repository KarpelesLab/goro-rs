//! php-src's ext/zip (php_zip.c, zip_stream.c) on top of a port of libzip
//! (`zipint`): the ZipArchive class, the deprecated zip_* functions and
//! the zip:// stream wrapper.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm};

mod codec;
mod methods;
mod procedural;
mod stream;
mod zipint;
mod zpp;

use zipint::*;
use zpp::Obj;

/// The extension name (`extension_loaded('zip')`).
pub const NAME: &[u8] = b"zip";

/// PHP_ZIP_VERSION
pub const PHP_ZIP_VERSION: &str = "1.22.7";
const LIBZIP_VERSION: &str = "1.11.4";

/// ZIP_FL_OPEN_FILE_NOW (not from libzip)
pub const FL_OPEN_FILE_NOW: u32 = 1 << 30;

/// ze_zip_object
pub struct ZipObj {
    pub za: Option<Archive>,
    pub filename: Option<String>,
    pub last_id: i64,
    pub err_zip: i32,
    pub err_sys: i32,
    pub progress: Option<(f64, Value)>,
    pub cancel: Option<Value>,
}

impl Drop for ZipObj {
    /// php_zip_object_free_storage(): the callbacks need the VM, so an
    /// archive with callbacks is closed as soon as the VM is at hand.
    fn drop(&mut self) {
        if let Some(mut za) = self.za.take() {
            if self.progress.is_some() || self.cancel.is_some() {
                let p = self.progress.take();
                let c = self.cancel.take();
                PENDING.with(|q| q.borrow_mut().push((za, p, c)));
            } else {
                let _ = za.close(None);
            }
        }
    }
}

type Pending = (Archive, Option<(f64, Value)>, Option<Value>);

thread_local! {
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
}

/// Close the archives of freed objects that have callbacks.
pub fn flush_pending(vm: &mut Vm) {
    loop {
        let next = PENDING.with(|q| {
            let mut q = q.borrow_mut();
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        });
        let Some((za, progress, cancel)) = next else {
            break;
        };
        let d = ZipData {
            obj: RefCell::new(ZipObj {
                za: Some(za),
                filename: None,
                last_id: -1,
                err_zip: 0,
                err_sys: 0,
                progress,
                cancel,
            }),
        };
        if let Err(e) = methods::close_archive(vm, &d) {
            let line = vm.current_line;
            vm.emit_warning_at(
                &format!("Unknown: Cannot destroy the zip context: {}", e.strerror()),
                line,
            );
        }
    }
}

pub struct ZipData {
    pub obj: RefCell<ZipObj>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    no_property_ptr: false,
    read_property: Some(h_read_property),
    write_property: Some(h_write_property),
    has_property: Some(h_has_property),
    unset_property: None,
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: Some(h_get_properties),
    count_elements: Some(h_count),
    cast: None,
    compare: None,
    clone_obj: Some(h_clone),
};

pub fn data_of(o: &Obj) -> Option<Rc<ZipData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<ZipData>().ok()
}

thread_local! {
    /// The live ZipArchive objects, closed at shutdown (where PHP frees the
    /// remaining objects) with their callbacks.
    static LIVE: RefCell<Vec<std::rc::Weak<ZipData>>> = const { RefCell::new(Vec::new()) };
}

/// php_zip_object_free_storage() of the objects still alive at shutdown
fn shutdown(vm: &mut Vm) {
    flush_pending(vm);
    let live: Vec<Rc<ZipData>> = LIVE.with(|l| {
        let v: Vec<_> = l.borrow().iter().filter_map(|w| w.upgrade()).collect();
        l.borrow_mut().clear();
        v
    });
    for d in live {
        flush_pending(vm);
        if d.obj.borrow().za.is_none() {
            continue;
        }
        if let Err(e) = methods::close_archive(vm, &d) {
            let line = vm.current_line;
            vm.emit_warning_at(
                &format!("Unknown: Cannot destroy the zip context: {}", e.strerror()),
                line,
            );
        }
    }
}

fn attach(o: &Obj) {
    let d = Rc::new(ZipData {
        obj: RefCell::new(ZipObj {
            za: None,
            filename: None,
            last_id: -1,
            err_zip: 0,
            err_sys: 0,
            progress: None,
            cancel: None,
        }),
    });
    LIVE.with(|l| {
        let mut l = l.borrow_mut();
        l.retain(|w| w.strong_count() > 0);
        l.push(Rc::downgrade(&d));
    });
    o.borrow_mut().native = Some(NativeData {
        data: d as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

/// php_zip_object_new()
fn create_object(_vm: &mut Vm, o: &Obj) {
    if data_of(o).is_none() {
        attach(o);
    }
}

pub fn data_or_attach(o: &Obj) -> Rc<ZipData> {
    if let Some(d) = data_of(o) {
        return d;
    }
    attach(o);
    data_of(o).unwrap()
}

// ---------------------------------------------------------------------------
// properties

const PROPS: [&str; 6] = [
    "lastId",
    "status",
    "statusSys",
    "numFiles",
    "filename",
    "comment",
];

fn is_prop(name: &[u8]) -> bool {
    PROPS.iter().any(|p| p.as_bytes() == name)
}

/// php_zip_status() and friends
fn prop_value(o: &ZipObj, name: &[u8]) -> Value {
    match name {
        b"lastId" => Value::Long(o.last_id),
        b"status" => Value::Long(match &o.za {
            Some(za) => za.error.zip_err as i64,
            None => o.err_zip as i64,
        }),
        b"statusSys" => Value::Long(match &o.za {
            Some(za) => za.error.sys_err as i64,
            None => o.err_sys as i64,
        }),
        b"numFiles" => Value::Long(o.za.as_ref().map_or(0, |za| za.num_entries(0))),
        b"filename" => Value::String(PhpString::from_bytes(
            o.filename.as_deref().unwrap_or_default().as_bytes(),
        )),
        b"comment" => Value::String(PhpString::from_vec(
            o.za.as_ref()
                .map(|za| za.get_archive_comment(0))
                .unwrap_or_default(),
        )),
        _ => Value::Null,
    }
}

fn h_read_property(_vm: &mut Vm, o: &Obj, name: &[u8], _mode: u8) -> Option<Value> {
    if !is_prop(name) {
        return None;
    }
    let d = data_or_attach(o);
    Some(prop_value(&d.obj.borrow(), name))
}

/// The NativeClassHandler property hook (property_exists() & co.)
fn prop_read_hook(_vm: &mut Vm, o: &Obj, name: &[u8]) -> Option<Option<Value>> {
    if !is_prop(name) {
        return None;
    }
    let d = data_or_attach(o);
    let v = prop_value(&d.obj.borrow(), name);
    Some(Some(v))
}

fn ce_name(o: &Obj) -> String {
    goro_core::value::display_class_name(&o.borrow().class_name).to_string()
}

fn h_write_property(vm: &mut Vm, o: &Obj, name: &[u8], _v: &Value) -> bool {
    if !is_prop(name) {
        return false;
    }
    let m = format!(
        "Cannot write read-only property {}::${}",
        ce_name(o),
        String::from_utf8_lossy(name)
    );
    zpp::throw(vm, b"Error", &m);
    true
}

fn h_has_property(_vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    if !is_prop(name) {
        return None;
    }
    if check_empty == 2 {
        return Some(true);
    }
    let d = data_or_attach(o);
    let v = prop_value(&d.obj.borrow(), name);
    Some(if check_empty == 1 {
        v.is_truthy()
    } else {
        !matches!(v, Value::Null)
    })
}

/// php_zip_get_properties()
fn h_get_properties(vm: &mut Vm, o: &Obj, _purpose: u8) -> Option<PhpArray> {
    let d = data_or_attach(o);
    let mut arr = PhpArray::new();
    {
        let zo = d.obj.borrow();
        for p in PROPS {
            arr.set(
                ArrayKey::String(PhpString::from_bytes(p.as_bytes())),
                prop_value(&zo, p.as_bytes()),
            );
        }
    }
    for e in vm.object_prop_table(o) {
        if is_prop(&e.key) {
            continue;
        }
        arr.set(ArrayKey::String(PhpString::from_vec(e.key)), e.value);
    }
    Some(arr)
}

fn h_count(vm: &mut Vm, o: &Obj) -> Option<i64> {
    let d = data_or_attach(o);
    let n = d.obj.borrow().za.as_ref().map(|za| za.num_entries(0));
    match n {
        Some(n) => Some(n),
        None => {
            zpp::throw(vm, b"ValueError", "Invalid or uninitialized Zip object");
            Some(0)
        }
    }
}

fn h_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        ce_name(src)
    );
    zpp::throw(vm, b"Error", &m);
}

// ---------------------------------------------------------------------------
// registration

const CLASS_CONSTS: &[(&str, i64)] = &[
    ("CREATE", 1),
    ("EXCL", 2),
    ("CHECKCONS", 4),
    ("OVERWRITE", 8),
    ("RDONLY", 16),
    ("FL_NOCASE", 1),
    ("FL_NODIR", 2),
    ("FL_COMPRESSED", 4),
    ("FL_UNCHANGED", 8),
    ("FL_RECOMPRESS", 16),
    ("FL_ENCRYPTED", 32),
    ("FL_OVERWRITE", 8192),
    ("FL_LOCAL", 256),
    ("FL_CENTRAL", 512),
    ("FL_ENC_GUESS", 0),
    ("FL_ENC_RAW", 64),
    ("FL_ENC_STRICT", 128),
    ("FL_ENC_UTF_8", 2048),
    ("FL_ENC_CP437", 4096),
    ("FL_OPEN_FILE_NOW", 1 << 30),
    ("CM_DEFAULT", -1),
    ("CM_STORE", 0),
    ("CM_SHRINK", 1),
    ("CM_REDUCE_1", 2),
    ("CM_REDUCE_2", 3),
    ("CM_REDUCE_3", 4),
    ("CM_REDUCE_4", 5),
    ("CM_IMPLODE", 6),
    ("CM_DEFLATE", 8),
    ("CM_DEFLATE64", 9),
    ("CM_PKWARE_IMPLODE", 10),
    ("CM_BZIP2", 12),
    ("CM_LZMA", 14),
    ("CM_LZMA2", 33),
    ("CM_ZSTD", 93),
    ("CM_XZ", 95),
    ("CM_TERSE", 18),
    ("CM_LZ77", 19),
    ("CM_WAVPACK", 97),
    ("CM_PPMD", 98),
    ("ER_OK", 0),
    ("ER_MULTIDISK", 1),
    ("ER_RENAME", 2),
    ("ER_CLOSE", 3),
    ("ER_SEEK", 4),
    ("ER_READ", 5),
    ("ER_WRITE", 6),
    ("ER_CRC", 7),
    ("ER_ZIPCLOSED", 8),
    ("ER_NOENT", 9),
    ("ER_EXISTS", 10),
    ("ER_OPEN", 11),
    ("ER_TMPOPEN", 12),
    ("ER_ZLIB", 13),
    ("ER_MEMORY", 14),
    ("ER_CHANGED", 15),
    ("ER_COMPNOTSUPP", 16),
    ("ER_EOF", 17),
    ("ER_INVAL", 18),
    ("ER_NOZIP", 19),
    ("ER_INTERNAL", 20),
    ("ER_INCONS", 21),
    ("ER_REMOVE", 22),
    ("ER_DELETED", 23),
    ("ER_ENCRNOTSUPP", 24),
    ("ER_RDONLY", 25),
    ("ER_NOPASSWD", 26),
    ("ER_WRONGPASSWD", 27),
    ("ER_OPNOTSUPP", 28),
    ("ER_INUSE", 29),
    ("ER_TELL", 30),
    ("ER_COMPRESSED_DATA", 31),
    ("ER_CANCELLED", 32),
    ("ER_DATA_LENGTH", 33),
    ("ER_NOT_ALLOWED", 34),
    ("ER_TRUNCATED_ZIP", 35),
    ("AFL_RDONLY", 2),
    ("AFL_IS_TORRENTZIP", 4),
    ("AFL_WANT_TORRENTZIP", 8),
    ("AFL_CREATE_OR_KEEP_FILE_FOR_EMPTY_ARCHIVE", 16),
    ("OPSYS_DOS", 0),
    ("OPSYS_AMIGA", 1),
    ("OPSYS_OPENVMS", 2),
    ("OPSYS_UNIX", 3),
    ("OPSYS_VM_CMS", 4),
    ("OPSYS_ATARI_ST", 5),
    ("OPSYS_OS_2", 6),
    ("OPSYS_MACINTOSH", 7),
    ("OPSYS_Z_SYSTEM", 8),
    ("OPSYS_CPM", 9),
    ("OPSYS_WINDOWS_NTFS", 10),
    ("OPSYS_MVS", 11),
    ("OPSYS_VSE", 12),
    ("OPSYS_ACORN_RISC", 13),
    ("OPSYS_VFAT", 14),
    ("OPSYS_ALTERNATE_MVS", 15),
    ("OPSYS_BEOS", 16),
    ("OPSYS_TANDEM", 17),
    ("OPSYS_OS_400", 18),
    ("OPSYS_OS_X", 19),
    ("OPSYS_DEFAULT", 3),
    ("EM_NONE", 0),
    ("EM_TRAD_PKWARE", 1),
    ("EM_AES_128", 257),
    ("EM_AES_192", 258),
    ("EM_AES_256", 259),
    ("EM_UNKNOWN", 65535),
    ("LENGTH_TO_END", 0),
    ("LENGTH_UNCHECKED", -2),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    procedural::register(vm);
    let mut ce = ClassEntry::new(b"ZipArchive".to_vec());
    ce.interfaces.push(b"Countable".to_vec());
    for &(n, v) in CLASS_CONSTS {
        ce.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
    }
    ce.constants.insert(
        b"LIBZIP_VERSION".to_vec(),
        Value::String(PhpString::from_bytes(LIBZIP_VERSION.as_bytes())),
    );
    vm.classes.insert(b"ziparchive".to_vec(), ce);
    let h = NativeClassHandler {
        has_method: methods::has_method,
        call: methods::call_method,
        prop_added: None,
        prop_write: None,
        compare_check: None,
        prop_read: Some(prop_read_hook),
        param_names: Some(methods::param_names),
    };
    vm.native_class_handlers.insert(b"ziparchive".to_vec(), h);
    vm.object_create_handlers
        .insert(b"ziparchive".to_vec(), create_object);
    stream::register(vm);
    vm.shutdown_hooks.push(shutdown);
}

// ---------------------------------------------------------------------------
// shared helpers

fn vstr(b: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(b.as_ref()))
}

fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

/// expand_filepath(): an absolute, lexically normalized path.
pub fn expand_filepath(path: &[u8]) -> Option<String> {
    let p = String::from_utf8_lossy(path).into_owned();
    let full = if p.starts_with('/') {
        p
    } else {
        let cwd = std::env::current_dir().ok()?;
        format!("{}/{}", cwd.to_string_lossy(), p)
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in full.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let r = format!("/{}", parts.join("/"));
    if r.len() >= 4096 {
        return None;
    }
    Some(r)
}

/// ZIP_OPENBASEDIR_CHECKPATH(): true when refused
pub fn basedir_refused(vm: &mut Vm, func: &str, path: &[u8]) -> bool {
    let p = String::from_utf8_lossy(path).into_owned();
    !vm.check_open_basedir(func, &p)
}

/// RETURN_SB()
pub fn stat_array(st: &Stat) -> Value {
    let mut a = PhpArray::new();
    let k = |s: &str| ArrayKey::String(PhpString::from_bytes(s.as_bytes()));
    a.set(k("name"), vstr(c_str(&st.name)));
    a.set(k("index"), Value::Long(st.index as i64));
    a.set(k("crc"), Value::Long(st.crc as i64));
    a.set(k("size"), Value::Long(st.size as i64));
    a.set(k("mtime"), Value::Long(st.mtime));
    a.set(k("comp_size"), Value::Long(st.comp_size as i64));
    a.set(k("comp_method"), Value::Long(st.comp_method as i64));
    a.set(
        k("encryption_method"),
        Value::Long(st.encryption_method as i64),
    );
    Value::Array(Rc::new(RefCell::new(a)))
}
