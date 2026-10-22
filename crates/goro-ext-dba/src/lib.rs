//! php-src's ext/dba (dba.c): the dba_* functions and the Dba\Connection
//! class, with the handlers whose libraries php-src bundles: cdb and
//! cdb_make (libcdb), inifile (libinifile) and flatfile (libflatfile).
//! The handlers over external C libraries (gdbm, db4, qdbm, lmdb, tcadb,
//! dbm, ndbm) are not built in, as in a PHP configured without them.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};

mod cdb;
mod flatfile;
mod inifile;
mod stream;

use stream::PStream;

/// The extension name (`extension_loaded('dba')`).
pub const NAME: &[u8] = b"dba";

const CLASS_NAME: &[u8] = b"Dba\\Connection";

type R = Result<Value, VmError>;

const DBA_LOCK_READER: i32 = 0x0001;
const DBA_LOCK_WRITER: i32 = 0x0002;
const DBA_LOCK_CREAT: i32 = 0x0004;
const DBA_LOCK_TRUNC: i32 = 0x0008;
const DBA_LOCK_ALL: i32 = DBA_LOCK_READER | DBA_LOCK_WRITER | DBA_LOCK_CREAT | DBA_LOCK_TRUNC;
const DBA_STREAM_OPEN: i32 = 0x0010;
const DBA_CAST_AS_FD: i32 = 0x0050;
const DBA_NO_APPEND: i32 = 0x00D0;

const LOCK_SH: i32 = libc::LOCK_SH;
const LOCK_EX: i32 = libc::LOCK_EX;
const LOCK_NB: i32 = libc::LOCK_NB;

/// dba_mode_t
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Reader,
    Writer,
    Trunc,
    Creat,
}

/// The operations of a dba_handler on an open database.
pub trait Driver {
    fn close(&mut self);
    fn fetch(&mut self, vm: &mut Vm, key: &[u8], skip: i64) -> Option<Vec<u8>>;
    fn update(&mut self, vm: &mut Vm, key: &[u8], val: &[u8], mode: i32) -> bool;
    fn exists(&mut self, vm: &mut Vm, key: &[u8]) -> bool;
    fn delete(&mut self, vm: &mut Vm, key: &[u8]) -> bool;
    fn firstkey(&mut self, vm: &mut Vm) -> Option<Vec<u8>>;
    fn nextkey(&mut self, vm: &mut Vm) -> Option<Vec<u8>>;
    fn optimize(&mut self, _vm: &mut Vm) -> bool {
        true
    }
    fn sync(&mut self, _vm: &mut Vm) -> bool {
        true
    }
}

/// dba_handler
struct Handler {
    name: &'static str,
    flags: i32,
    open: fn(&Info) -> Result<Box<dyn Driver>, &'static str>,
    info: &'static str,
}

/// The handler table, in php-src's order.
static HANDLERS: &[Handler] = &[
    Handler {
        name: "cdb",
        flags: DBA_STREAM_OPEN | DBA_LOCK_ALL,
        open: cdb::open,
        info: cdb::CDB_VERSION,
    },
    Handler {
        name: "cdb_make",
        flags: DBA_STREAM_OPEN | DBA_LOCK_ALL,
        open: cdb::open,
        info: cdb::CDB_MAKE_VERSION,
    },
    Handler {
        name: "inifile",
        flags: DBA_STREAM_OPEN | DBA_LOCK_ALL | DBA_CAST_AS_FD,
        open: inifile::open,
        info: inifile::VERSION,
    },
    Handler {
        name: "flatfile",
        flags: DBA_STREAM_OPEN | DBA_LOCK_ALL | DBA_NO_APPEND,
        open: flatfile::open,
        info: flatfile::VERSION,
    },
];

/// DBA_DEFAULT
const DBA_DEFAULT: &str = "flatfile";

fn find_handler(name: &[u8]) -> Option<&'static Handler> {
    HANDLERS
        .iter()
        .find(|h| h.name.as_bytes().eq_ignore_ascii_case(name))
}

/// dba_info
pub struct Info {
    path: Vec<u8>,
    mode: Mode,
    fp: Option<Rc<RefCell<PStream>>>,
    lock_fp: Option<Rc<RefCell<PStream>>>,
    lock_mode: i32,
    hnd: Option<&'static Handler>,
    driver: Option<Box<dyn Driver>>,
    persistent: bool,
    closed: bool,
}

impl Info {
    /// dba_close_info()
    fn close(&mut self) {
        if let Some(mut d) = self.driver.take() {
            d.close();
        }
        self.hnd = None;
        self.fp = None;
        self.lock_fp = None;
        self.closed = true;
    }
}

impl Drop for Info {
    fn drop(&mut self) {
        self.close();
    }
}

type InfoRef = Rc<RefCell<Info>>;

/// dba_connection
struct Conn {
    info: RefCell<Option<InfoRef>>,
    hash: RefCell<Option<Vec<u8>>>,
}

impl Conn {
    /// dba_close_connection()
    fn close_connection(&self) {
        let info = self.info.borrow_mut().take();
        if let Some(info) = info {
            let persistent = info.borrow().persistent;
            if !persistent {
                if let Ok(mut i) = info.try_borrow_mut() {
                    i.close();
                }
            }
        }
        if let Some(h) = self.hash.borrow_mut().take() {
            STATE.with(|s| {
                if let Ok(mut s) = s.try_borrow_mut() {
                    if let Some(p) = s.connections.iter().position(|c| c.0 == h) {
                        s.connections.remove(p);
                    }
                }
            });
        }
    }
}

impl Drop for Conn {
    /// dba_connection_free_obj()
    fn drop(&mut self) {
        if self.info.borrow().is_some() {
            self.close_connection();
        }
    }
}

#[derive(Default)]
struct State {
    /// DBA_G(connections): hash -> the connection object (not counted)
    connections: Vec<(Vec<u8>, Weak<Conn>, u64)>,
    /// EG(persistent_list) entries of type "dba persistent"
    persistent: Vec<(Vec<u8>, InfoRef)>,
    connection_counter: u32,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// zend_objects_not_comparable
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

/// dba_connection_cast_object(): (int) gives the object id.
fn cast(o: &Rc<RefCell<PhpObject>>, ty: u8) -> Option<Value> {
    match ty {
        1 => Some(Value::Long(o.borrow().object_id as i64)),
        _ => None,
    }
}

/// @strict-properties
fn write_property(vm: &mut Vm, _o: &Rc<RefCell<PhpObject>>, name: &[u8], _v: &Value) -> bool {
    throw(
        vm,
        b"Error",
        &format!(
            "Cannot create dynamic property Dba\\Connection::${}",
            String::from_utf8_lossy(name)
        ),
    );
    true
}

static DBA_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    cast: Some(cast),
    write_property: Some(write_property),
    ..NativeObjectHandlers::EMPTY
};

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn, u8)] = &[
    ("dba_popen", dba_popen, 3),
    ("dba_open", dba_open, 3),
    ("dba_close", dba_close, 1),
    ("dba_exists", dba_exists, 3),
    ("dba_fetch", dba_fetch, 3),
    ("dba_key_split", dba_key_split, 1),
    ("dba_firstkey", dba_firstkey, 1),
    ("dba_nextkey", dba_nextkey, 1),
    ("dba_delete", dba_delete, 3),
    ("dba_insert", dba_insert, 3),
    ("dba_replace", dba_replace, 3),
    ("dba_optimize", dba_optimize, 1),
    ("dba_sync", dba_sync, 1),
    ("dba_handlers", dba_handlers, 3),
    ("dba_list", dba_list, 1),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);
    let mut ce = match goro_core::stubs::class_entry(vm, CLASS_NAME) {
        Some(stub) => stub.clone(),
        None => ClassEntry::new(CLASS_NAME.to_vec()),
    };
    ce.is_final = true;
    let lower = CLASS_NAME.to_ascii_lowercase();
    vm.classes.insert(lower.clone(), ce);
    vm.uncloneable_classes.push(lower.clone());
    vm.native_class_handlers.insert(
        lower,
        goro_core::vm::NativeClassHandler {
            has_method: |_c, m, _s| m.eq_ignore_ascii_case(b"__construct"),
            call: ctor_call,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: None,
        },
    );
    for &(name, f, flags) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified
            .insert(name.as_bytes().to_vec(), flags);
    }
    vm.constants.insert(
        b"dba.default_handler".to_vec(),
        Value::String(PhpString::from_bytes(DBA_DEFAULT.as_bytes())),
    );
    vm.ini_validators
        .insert(b"dba.default_handler".to_vec(), on_update_default_handler);
    vm.shutdown_hooks.push(shutdown);
}

/// PHP_MINFO_FUNCTION(dba)
fn minfo(vm: &mut Vm) -> String {
    let mut handlers = String::new();
    for h in HANDLERS {
        handlers.push_str(h.name);
        handlers.push(' ');
    }
    let v = ini_default_handler(vm);
    let v = String::from_utf8_lossy(&v).into_owned();
    format!(
        "\ndba\n\nDBA support => enabled\nSupported handlers => {}\n\nDirective => Local Value => Master Value\ndba.default_handler => {} => {}\n",
        handlers,
        if v.is_empty() { "no value" } else { &v },
        DBA_DEFAULT
    )
}

/// Objects and persistent connections are freed at shutdown (which
/// closes the databases: cdb_make writes its tables then).
fn shutdown(_vm: &mut Vm) {
    let (conns, persistent) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let conns: Vec<Rc<Conn>> = s.connections.iter().filter_map(|c| c.1.upgrade()).collect();
        let p = std::mem::take(&mut s.persistent);
        (conns, p)
    });
    for c in &conns {
        c.close_connection();
    }
    drop(conns);
    for (_, info) in persistent {
        if let Ok(mut i) = info.try_borrow_mut() {
            i.close();
        }
    }
    STATE.with(|s| *s.borrow_mut() = State::default());
}

/// OnUpdateDefaultHandler
fn on_update_default_handler(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    if value.is_empty() || find_handler(value).is_some() {
        return true;
    }
    docref_warning(
        vm,
        &format!("No such handler: {}", String::from_utf8_lossy(value)),
    );
    false
}

fn ini_default_handler(vm: &Vm) -> Vec<u8> {
    vm.constants
        .get(b"dba.default_handler".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

/// dba_connection_get_constructor()
fn ctor_call(
    vm: &mut Vm,
    _class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    _args: &[Value],
) -> Value {
    if method.eq_ignore_ascii_case(b"__construct") {
        throw(
            vm,
            b"Error",
            "Cannot directly construct Dba\\Connection, use dba_open() or dba_popen() instead",
        );
    }
    Value::Null
}

// ---------------------------------------------------------------------------
// helpers

fn throw(vm: &mut Vm, class: &[u8], msg: &str) {
    let e = vm.create_exception(class, msg, vm.current_line);
    vm.current_exception = Some(e);
}

/// php_error_docref(NULL, E_WARNING, ...)
pub(crate) fn docref_warning(vm: &mut Vm, msg: &str) {
    let full = match vm.current_builtin_name() {
        Some(f) => format!("{}(): {}", f, msg),
        None => msg.to_string(),
    };
    vm.emit_warning(&full);
}

fn docref_notice(vm: &mut Vm, msg: &str) {
    let full = match vm.current_builtin_name() {
        Some(f) => format!("{}(): {}", f, msg),
        None => msg.to_string(),
    };
    let line = vm.current_line;
    vm.emit_notice_at(&full, line);
}

fn param_name(fname: &str, n: usize) -> &'static str {
    goro_core::stubs::find_function(fname.as_bytes())
        .and_then(|f| f.args.get(n - 1))
        .map(|a| a.name)
        .unwrap_or("")
}

fn arg_prefix(fname: &str, n: usize) -> String {
    format!("{}(): Argument #{} (${})", fname, n, param_name(fname, n))
}

fn value_error(vm: &mut Vm, fname: &str, n: usize, msg: &str) -> R {
    let m = format!("{} {}", arg_prefix(fname, n), msg);
    throw(vm, b"ValueError", &m);
    Ok(Value::Null)
}

fn type_mismatch(vm: &mut Vm, fname: &str, n: usize, ty: &str, v: &Value) -> R {
    let m = format!(
        "{} must be of type {}, {} given",
        arg_prefix(fname, n),
        ty,
        Vm::value_type_name(v)
    );
    throw(vm, b"TypeError", &m);
    Ok(Value::Null)
}

fn arg(args: &[Value], i: usize) -> Value {
    match args.get(i) {
        Some(v) => v.deref(),
        None => Value::Undef,
    }
}

fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn bval(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

fn conn_of(v: &Value) -> Option<Rc<Conn>> {
    let Value::Object(o) = v else { return None };
    let o = o.borrow();
    if !o.class_name.eq_ignore_ascii_case(CLASS_NAME) {
        return None;
    }
    let d = o.native.as_ref()?.data.clone();
    d.downcast::<Conn>().ok()
}

/// Z_PARAM_OBJECT_OF_CLASS(dba_connection_ce)
fn conn_arg(vm: &mut Vm, fname: &str, n: usize, args: &[Value]) -> Option<Rc<Conn>> {
    let v = arg(args, n - 1);
    match conn_of(&v) {
        Some(c) => Some(c),
        None => {
            let _ = type_mismatch(vm, fname, n, "Dba\\Connection", &v);
            None
        }
    }
}

/// CHECK_DBA_CONNECTION: the open database.
fn info_of(vm: &mut Vm, c: &Conn) -> Option<InfoRef> {
    let i = c.info.borrow().clone();
    match i {
        Some(i) if !i.borrow().closed => Some(i),
        _ => {
            throw(vm, b"Error", "DBA connection has already been closed");
            None
        }
    }
}

/// DBA_WRITE_CHECK
fn write_check(vm: &mut Vm, info: &InfoRef) -> bool {
    if info.borrow().mode == Mode::Reader {
        docref_warning(vm, "Cannot perform a modification on a readonly database");
        return false;
    }
    true
}

/// Run a handler operation (the driver is taken out of the info while it
/// runs, so that diagnostics may re-enter the extension).
fn with_driver<T>(
    vm: &mut Vm,
    info: &InfoRef,
    default: T,
    f: impl FnOnce(&mut Vm, &mut dyn Driver) -> T,
) -> T {
    let d = info.borrow_mut().driver.take();
    let Some(mut d) = d else { return default };
    let r = f(vm, d.as_mut());
    let mut i = info.borrow_mut();
    if i.closed {
        d.close();
    } else {
        i.driver = Some(d);
    }
    r
}

/// php_dba_make_key(): the key argument as a string (None: thrown).
fn make_key(vm: &mut Vm, fname: &str, v: &Value) -> Option<Vec<u8>> {
    let Value::Array(a) = v else {
        return Some(v.to_php_string().as_bytes().to_vec());
    };
    let (group, name) = {
        let a = a.borrow();
        if a.len() != 2 {
            drop(a);
            throw(
                vm,
                b"Error",
                &format!(
                    "{} must have exactly two elements: \"key\" and \"name\"",
                    arg_prefix(fname, 1)
                ),
            );
            return None;
        }
        let mut it = a.values();
        (
            it.next().cloned().unwrap_or(Value::Null).deref(),
            it.next().cloned().unwrap_or(Value::Null).deref(),
        )
    };
    let group = vm.value_to_string_checked(&group).ok()?;
    if vm.current_exception.is_some() {
        return None;
    }
    let name = vm.value_to_string_checked(&name).ok()?;
    if vm.current_exception.is_some() {
        return None;
    }
    if group.as_bytes().is_empty() {
        return Some(name.as_bytes().to_vec());
    }
    // "[%s]%s"
    let c = |b: &[u8]| -> Vec<u8> {
        match b.iter().position(|&x| x == 0) {
            Some(p) => b[..p].to_vec(),
            None => b.to_vec(),
        }
    };
    let mut k = b"[".to_vec();
    k.extend_from_slice(&c(group.as_bytes()));
    k.push(b']');
    k.extend_from_slice(&c(name.as_bytes()));
    Some(k)
}

fn strerror(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(code) => unsafe {
            let p = libc::strerror(code);
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        },
        None => e.to_string(),
    }
}

/// php_stream_open_wrapper() for the DBA streams: (stream, opened path).
fn open_stream(
    vm: &mut Vm,
    path: &[u8],
    mode: &str,
    report: bool,
) -> Option<(PStream, Option<Vec<u8>>)> {
    let fname = vm.current_builtin_name().unwrap_or_default();
    let shown = String::from_utf8_lossy(path).into_owned();
    let fail = |vm: &mut Vm, msg: &str| {
        if report {
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                fname, shown, msg
            ));
        }
    };
    if path.len() >= 5 && path[..5].eq_ignore_ascii_case(b"data:") {
        // The rfc2397 wrapper: a read-only memory stream without a path.
        let data = match path.iter().position(|&c| c == b',') {
            Some(p) => path[p + 1..].to_vec(),
            None => {
                fail(vm, "rfc2397: no comma in URL");
                return None;
            }
        };
        return Some((PStream::memory(data), None));
    }
    let p: &[u8] = if path.len() >= 7 && path[..7].eq_ignore_ascii_case(b"file://") {
        &path[7..]
    } else {
        path
    };
    let ps = String::from_utf8_lossy(p).into_owned();
    if !vm.check_open_basedir(&fname, &ps) {
        fail(vm, "Operation not permitted");
        return None;
    }
    let mut oo = std::fs::OpenOptions::new();
    match mode {
        "r" | "rb" => {
            oo.read(true);
        }
        "r+b" => {
            oo.read(true).write(true);
        }
        "w+b" => {
            oo.read(true).write(true).create(true).truncate(true);
        }
        _ => {
            // "a+b"
            oo.read(true).append(true).create(true);
        }
    }
    use std::os::unix::ffi::OsStrExt;
    match oo.open(std::ffi::OsStr::from_bytes(p)) {
        Ok(f) => {
            let opened = goro_core::basedir::expand_filepath(&ps).map(|s| s.into_bytes());
            Some((PStream::from_file(f), opened))
        }
        Err(e) => {
            fail(vm, &strerror(&e));
            None
        }
    }
}

fn new_object(vm: &mut Vm, conn: Conn) -> (Value, Rc<Conn>, u64) {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(CLASS_NAME.to_vec(), id);
    let rc = Rc::new(conn);
    obj.native = Some(NativeData {
        data: rc.clone() as Rc<dyn Any>,
        handlers: &DBA_HANDLERS,
    });
    (Value::Object(Rc::new(RefCell::new(obj))), rc, id)
}

/// zend_hash_add(&DBA_G(connections), hash, obj), replacing an entry of
/// the same key.
fn add_connection(hash: &[u8], conn: &Rc<Conn>, id: u64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(p) = s.connections.iter().position(|c| c.0 == hash) {
            s.connections.remove(p);
        }
        s.connections.push((hash.to_vec(), Rc::downgrade(conn), id));
    });
}

// ---------------------------------------------------------------------------
// dba_open / dba_popen

/// php_dba_open()
fn php_dba_open(vm: &mut Vm, args: &[Value], persistent: bool) -> R {
    let fname = if persistent { "dba_popen" } else { "dba_open" };
    // "PS|S!lll!" (the types were checked from the stub)
    let path = arg(args, 0).to_php_string().as_bytes().to_vec();
    let mode = arg(args, 1).to_php_string().as_bytes().to_vec();
    let handler_str = match arg(args, 2) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_php_string().as_bytes().to_vec()),
    };
    let map_size = match arg(args, 4) {
        Value::Undef => 0,
        v => v.to_long(),
    };
    let flags = match arg(args, 5) {
        Value::Undef | Value::Null => None,
        v => Some(v.to_long()),
    };
    if path.contains(&0) {
        return value_error(vm, fname, 1, "must not contain any null bytes");
    }
    if path.is_empty() {
        return value_error(vm, fname, 1, "must not be empty");
    }
    if mode.is_empty() {
        return value_error(vm, fname, 2, "must not be empty");
    }
    if handler_str.as_ref().is_some_and(|h| h.is_empty()) {
        return value_error(vm, fname, 3, "must not be empty");
    }
    if map_size < 0 {
        return value_error(vm, fname, 5, "must be greater than or equal to 0");
    }
    if flags.is_some_and(|f| f < 0) {
        return value_error(vm, fname, 6, "must be greater than or equal to 0");
    }

    // "dba_%d_%u_%s_%s_%s"
    let counter = if persistent {
        0
    } else {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let c = s.connection_counter;
            s.connection_counter = c.wrapping_add(1);
            c
        })
    };
    let mut resource_key = format!("dba_{}_{}_", persistent as i32, counter).into_bytes();
    resource_key.extend_from_slice(&path);
    resource_key.push(b'_');
    resource_key.extend_from_slice(&mode);
    resource_key.push(b'_');
    if let Some(h) = &handler_str {
        resource_key.extend_from_slice(h);
    }

    if persistent {
        let found = STATE.with(|s| {
            s.borrow()
                .persistent
                .iter()
                .find(|p| p.0 == resource_key)
                .map(|p| p.1.clone())
        });
        if let Some(info) = found {
            let conn = Conn {
                info: RefCell::new(Some(info)),
                hash: RefCell::new(Some(resource_key.clone())),
            };
            let (v, rc, id) = new_object(vm, conn);
            add_connection(&resource_key, &rc, id);
            return Ok(v);
        }
    }

    let hptr: &'static Handler = match &handler_str {
        None => {
            let d = ini_default_handler(vm);
            match find_handler(&d).filter(|_| !d.is_empty()) {
                Some(h) => h,
                None => {
                    docref_warning(vm, "No default handler selected");
                    return Ok(Value::False);
                }
            }
        }
        Some(h) => match find_handler(h) {
            Some(h) => h,
            None => {
                docref_warning(
                    vm,
                    &format!(
                        "Handler \"{}\" is not available",
                        String::from_utf8_lossy(h)
                    ),
                );
                return Ok(Value::False);
            }
        },
    };

    // Check mode: [rwnc][dl-]?t?
    let mut is_test_lock = false;
    let mut is_db_lock = false;
    let mut is_lock_ignored = false;
    let lock_flag;
    if mode.len() > 3 {
        return value_error(vm, fname, 2, "must be at most 3 characters");
    }
    if mode.len() == 3 {
        if mode[2] != b't' {
            return value_error(vm, fname, 2, "third character must be \"t\"");
        }
        is_test_lock = true;
    }
    if mode.len() >= 2 {
        match mode[1] {
            b't' => {
                is_test_lock = true;
                lock_flag = 0;
            }
            b'-' => {
                if hptr.flags & DBA_LOCK_ALL == 0 {
                    docref_warning(
                        vm,
                        &format!("Locking cannot be disabled for handler {}", hptr.name),
                    );
                    return Ok(Value::False);
                }
                is_lock_ignored = true;
                lock_flag = 0;
            }
            b'd' | b'l' => {
                if mode[1] == b'd' {
                    is_db_lock = true;
                }
                if mode[1] == b'd' && hptr.flags & DBA_LOCK_ALL == 0 {
                    lock_flag = hptr.flags & DBA_LOCK_ALL;
                } else {
                    lock_flag = DBA_LOCK_ALL;
                    if hptr.flags & DBA_LOCK_ALL == 0 {
                        docref_notice(
                            vm,
                            &format!("Handler {} does locking internally", hptr.name),
                        );
                    }
                }
            }
            _ => {
                return value_error(
                    vm,
                    fname,
                    2,
                    "second character must be one of \"d\", \"l\", \"-\", or \"t\"",
                );
            }
        }
    } else {
        lock_flag = hptr.flags & DBA_LOCK_ALL;
        is_db_lock = true;
    }

    let modenr;
    let mut lock_mode;
    let file_mode: &str;
    let mut lock_file_mode: Option<&str> = None;
    match mode[0] {
        b'r' => {
            modenr = Mode::Reader;
            lock_mode = if lock_flag & DBA_LOCK_READER != 0 {
                LOCK_SH
            } else {
                0
            };
            file_mode = "r";
        }
        b'w' => {
            modenr = Mode::Writer;
            lock_mode = if lock_flag & DBA_LOCK_WRITER != 0 {
                LOCK_EX
            } else {
                0
            };
            file_mode = "r+b";
        }
        b'c' => {
            modenr = Mode::Creat;
            lock_mode = if lock_flag & DBA_LOCK_CREAT != 0 {
                LOCK_EX
            } else {
                0
            };
            if lock_mode != 0 {
                if is_db_lock {
                    // the create/append check is done on the lock: when the
                    // library opens the file it is already created
                    file_mode = "r+b";
                    lock_file_mode = Some("a+b");
                } else {
                    file_mode = "a+b";
                    lock_file_mode = Some("w+b");
                }
            } else {
                file_mode = "a+b";
            }
        }
        b'n' => {
            modenr = Mode::Trunc;
            lock_mode = if lock_flag & DBA_LOCK_TRUNC != 0 {
                LOCK_EX
            } else {
                0
            };
            file_mode = "w+b";
        }
        _ => {
            return value_error(
                vm,
                fname,
                2,
                "first character must be one of \"r\", \"w\", \"c\", or \"n\"",
            );
        }
    }
    let mut lock_file_mode = lock_file_mode.unwrap_or(file_mode);
    if is_test_lock {
        if is_lock_ignored {
            return value_error(
                vm,
                fname,
                2,
                "cannot combine mode \"-\" (no lock) and \"t\" (test lock)",
            );
        }
        if lock_mode == 0 {
            if hptr.flags & DBA_LOCK_ALL == 0 {
                docref_warning(
                    vm,
                    &format!(
                        "Handler {} uses its own locking which doesn't support mode modifier t (test lock)",
                        hptr.name
                    ),
                );
            } else {
                docref_warning(
                    vm,
                    &format!(
                        "Handler {} doesn't uses locking for this mode which makes modifier t (test lock) obsolete",
                        hptr.name
                    ),
                );
            }
            return Ok(Value::False);
        }
        lock_mode |= LOCK_NB;
    }

    let mut info = Info {
        path: path.clone(),
        mode: modenr,
        fp: None,
        lock_fp: None,
        lock_mode,
        hnd: None,
        driver: None,
        persistent,
        closed: false,
    };

    let mut error: Option<&'static str> = None;
    // A locking open conflicting with another open of the same file.
    if hptr.flags & DBA_LOCK_ALL != 0 {
        let other_mode = STATE.with(|s| {
            let s = s.borrow();
            for (_, w, _) in &s.connections {
                let Some(c) = w.upgrade() else { continue };
                let i = c.info.borrow();
                if let Some(i) = i.as_ref() {
                    if let Ok(i) = i.try_borrow() {
                        if i.path == info.path {
                            return Some(i.lock_mode);
                        }
                    }
                }
            }
            None
        });
        if let Some(om) = other_mode {
            if (lock_mode & LOCK_EX != 0 && om & (LOCK_EX | LOCK_SH) != 0)
                || (om & LOCK_EX != 0 && lock_mode & (LOCK_EX | LOCK_SH) != 0)
            {
                error = Some("Unable to establish lock (database file already open)");
            }
        }
    }

    if error.is_none() && lock_mode != 0 {
        let lock_name: Vec<u8> = if is_db_lock {
            path.clone()
        } else {
            let mut n = info.path.clone();
            n.extend_from_slice(b".lck");
            if file_mode == "r" {
                // read-only: try an existing .lck file first, quietly
                lock_file_mode = "rb";
                if let Some((s, _)) = open_stream(vm, &n, lock_file_mode, false) {
                    info.lock_fp = Some(Rc::new(RefCell::new(s)));
                }
            }
            if info.lock_fp.is_none() {
                lock_file_mode = "a+b";
            }
            n
        };
        if info.lock_fp.is_none() {
            if let Some((s, opened)) = open_stream(vm, &lock_name, lock_file_mode, true) {
                info.lock_fp = Some(Rc::new(RefCell::new(s)));
                if is_db_lock {
                    match opened {
                        // the real path of the opened file
                        Some(p) => info.path = p,
                        None => error = Some("Unable to determine path for locking"),
                    }
                }
            }
        }
        match &info.lock_fp {
            // (the stream layer reported the error)
            None => return Ok(Value::False),
            Some(l) => {
                if error.is_none() {
                    let mut l = l.borrow_mut();
                    if !l.is_plain_file() {
                        error = Some("Stream does not support locking");
                    } else if !l.lock(lock_mode) {
                        error = Some("Unable to establish lock");
                    }
                }
            }
        }
    }

    // the centralised stream open of the builtin handlers
    if error.is_none() && hptr.flags & DBA_STREAM_OPEN == DBA_STREAM_OPEN {
        if info.lock_fp.is_some() && is_db_lock {
            // the same stream for locking and database access
            info.fp = info.lock_fp.clone();
        } else {
            let p = info.path.clone();
            match open_stream(vm, &p, file_mode, true) {
                Some((s, _)) => info.fp = Some(Rc::new(RefCell::new(s))),
                None => return Ok(Value::False),
            }
        }
        if hptr.flags & (DBA_NO_APPEND | DBA_CAST_AS_FD) != 0 {
            // some systems do not allow writing to the original contents
            // with O_APPEND set
            let fp = info.fp.clone().unwrap();
            let mut fp = fp.borrow_mut();
            if !fp.is_plain_file() {
                drop(fp);
                docref_warning(vm, "Could not cast stream");
                return Ok(Value::False);
            } else if modenr == Mode::Creat {
                fp.clear_append();
            }
        }
    }

    let opened = match error {
        Some(e) => Err(e),
        None => (hptr.open)(&info),
    };
    match opened {
        Ok(d) => info.driver = Some(d),
        Err(e) => {
            if vm.current_exception.is_none() {
                if e.is_empty() {
                    docref_warning(
                        vm,
                        &format!("Driver initialization failed for handler: {}", hptr.name),
                    );
                } else {
                    docref_warning(
                        vm,
                        &format!(
                            "Driver initialization failed for handler: {}: {}",
                            hptr.name, e
                        ),
                    );
                }
            }
            return Ok(Value::False);
        }
    }
    info.hnd = Some(hptr);

    let info = Rc::new(RefCell::new(info));
    if persistent {
        STATE.with(|s| {
            s.borrow_mut()
                .persistent
                .push((resource_key.clone(), info.clone()))
        });
    }
    let conn = Conn {
        info: RefCell::new(Some(info)),
        hash: RefCell::new(Some(resource_key.clone())),
    };
    let (v, rc, id) = new_object(vm, conn);
    add_connection(&resource_key, &rc, id);
    Ok(v)
}

fn dba_popen(vm: &mut Vm, args: &[Value]) -> R {
    php_dba_open(vm, args, true)
}

fn dba_open(vm: &mut Vm, args: &[Value]) -> R {
    php_dba_open(vm, args, false)
}

fn dba_close(vm: &mut Vm, args: &[Value]) -> R {
    let Some(c) = conn_arg(vm, "dba_close", 1, args) else {
        return Ok(Value::Null);
    };
    if info_of(vm, &c).is_none() {
        return Ok(Value::Null);
    }
    // (a persistent database stays open: php-src looks it up in the
    // persistent list by the pointer it has just cleared)
    c.close_connection();
    Ok(Value::Null)
}

/// The key, connection and open database of the key functions.
fn key_conn(
    vm: &mut Vm,
    fname: &str,
    args: &[Value],
    conn_n: usize,
    write: bool,
) -> Option<(InfoRef, Vec<u8>)> {
    let c = conn_arg(vm, fname, conn_n, args)?;
    let info = info_of(vm, &c)?;
    if write && !write_check(vm, &info) {
        return None;
    }
    let key = make_key(vm, fname, &arg(args, 0))?;
    Some((info, key))
}

fn dba_exists(vm: &mut Vm, args: &[Value]) -> R {
    let Some((info, key)) = key_conn(vm, "dba_exists", args, 2, false) else {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    };
    let r = with_driver(vm, &info, false, |vm, d| d.exists(vm, &key));
    Ok(bval(r))
}

/// Z_PARAM_LONG, quietly: Some when `v` is accepted as an int.
fn quiet_long(vm: &mut Vm, v: &Value) -> Option<i64> {
    match v {
        Value::Long(n) => Some(*n),
        Value::Null | Value::Undef | Value::Array(_) | Value::Object(_) => None,
        _ => {
            let strict = vm.caller_strict_types;
            let line = vm.current_line;
            match vm.zpp_coerce_scalar(goro_core::arginfo::T_INT, v, strict, line) {
                Ok(Some(Value::Long(n))) => Some(n),
                _ => {
                    vm.current_exception = None;
                    None
                }
            }
        }
    }
}

fn dba_fetch(vm: &mut Vm, args: &[Value]) -> R {
    let f = "dba_fetch";
    let mut skip: i64 = 0;
    let mut conn: Option<Rc<Conn>> = None;
    // the legacy signature (key, skip, dba)
    if args.len() == 3 {
        if let Some(c) = conn_of(&arg(args, 2)) {
            if let Some(n) = quiet_long(vm, &arg(args, 1)) {
                skip = n;
                conn = Some(c);
                let line = vm.current_line;
                vm.emit_deprecated_at(
                    "Calling dba_fetch() with $dba at the 3rd parameter is deprecated",
                    line,
                );
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
            }
        }
    }
    let c = match conn {
        Some(c) => c,
        None => {
            let Some(c) = conn_arg(vm, f, 2, args) else {
                return Ok(Value::Null);
            };
            match arg(args, 2) {
                Value::Undef => {}
                v => {
                    let strict = vm.caller_strict_types;
                    let line = vm.current_line;
                    if matches!(v, Value::Null) && !strict {
                        vm.emit_deprecated_at(
                            "dba_fetch(): Passing null to parameter #3 ($skip) of type int is deprecated",
                            line,
                        );
                        if vm.current_exception.is_some() {
                            return Ok(Value::Null);
                        }
                    } else {
                        match vm.zpp_coerce_scalar(goro_core::arginfo::T_INT, &v, strict, line) {
                            Ok(Some(Value::Long(n))) => skip = n,
                            Ok(_) => return type_mismatch(vm, f, 3, "int", &v),
                            Err(()) => return Ok(Value::Null),
                        }
                    }
                }
            }
            c
        }
    };
    if skip < i32::MIN as i64 || skip > i32::MAX as i64 {
        return value_error(
            vm,
            f,
            3,
            &format!("must be between {} and {}", i32::MIN, i32::MAX),
        );
    }
    let Some(info) = info_of(vm, &c) else {
        return Ok(Value::Null);
    };
    let Some(key) = make_key(vm, f, &arg(args, 0)) else {
        return Ok(Value::Null);
    };
    let hname = info.borrow().hnd.map(|h| h.name).unwrap_or("");
    if skip != 0 {
        if hname == "cdb" {
            if skip < 0 {
                docref_notice(
                    vm,
                    &format!(
                        "Handler {} accepts only skip values greater than or equal to zero, using skip=0",
                        hname
                    ),
                );
                skip = 0;
            }
        } else if hname == "inifile" {
            // -1 is like 0, allowing a faster lookup of the key found last
            if skip < -1 {
                docref_notice(
                    vm,
                    &format!(
                        "Handler {} accepts only skip value -1 and greater, using skip=0",
                        hname
                    ),
                );
                skip = 0;
            }
        } else {
            docref_notice(
                vm,
                &format!(
                    "Handler {} does not support optional skip parameter, the value will be ignored",
                    hname
                ),
            );
            skip = 0;
        }
    }
    let r = with_driver(vm, &info, None, |vm, d| d.fetch(vm, &key, skip));
    Ok(match r {
        Some(v) => sval(&v),
        None => Value::False,
    })
}

fn dba_key_split(vm: &mut Vm, args: &[Value]) -> R {
    let f = "dba_key_split";
    let v = arg(args, 0);
    if matches!(v, Value::Null | Value::False) && args.len() == 1 {
        let line = vm.current_line;
        vm.emit_deprecated_at(
            "dba_key_split(): Passing false or null is deprecated since 8.4",
            line,
        );
        return Ok(Value::False);
    }
    // "s"
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    let key = match vm.zpp_coerce_scalar(goro_core::arginfo::T_STRING, &v, strict, line) {
        Ok(Some(Value::String(s))) => s.as_bytes().to_vec(),
        Ok(_) => return type_mismatch(vm, f, 1, "string", &v),
        Err(()) => return Ok(Value::Null),
    };
    let mut out = PhpArray::new();
    // (strchr() on the C string)
    let cpart = match key.iter().position(|&c| c == 0) {
        Some(p) => &key[..p],
        None => &key[..],
    };
    match cpart.iter().position(|&c| c == b']') {
        Some(p) if key.first() == Some(&b'[') => {
            out.push(sval(&key[1..p]));
            out.push(sval(&key[p + 1..]));
        }
        _ => {
            out.push(sval(b""));
            out.push(sval(&key));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

fn dba_firstkey(vm: &mut Vm, args: &[Value]) -> R {
    let Some(c) = conn_arg(vm, "dba_firstkey", 1, args) else {
        return Ok(Value::Null);
    };
    let Some(info) = info_of(vm, &c) else {
        return Ok(Value::Null);
    };
    let r = with_driver(vm, &info, None, |vm, d| d.firstkey(vm));
    Ok(r.map(|k| sval(&k)).unwrap_or(Value::False))
}

fn dba_nextkey(vm: &mut Vm, args: &[Value]) -> R {
    let Some(c) = conn_arg(vm, "dba_nextkey", 1, args) else {
        return Ok(Value::Null);
    };
    let Some(info) = info_of(vm, &c) else {
        return Ok(Value::Null);
    };
    let r = with_driver(vm, &info, None, |vm, d| d.nextkey(vm));
    Ok(r.map(|k| sval(&k)).unwrap_or(Value::False))
}

fn dba_delete(vm: &mut Vm, args: &[Value]) -> R {
    let Some((info, key)) = key_conn(vm, "dba_delete", args, 2, true) else {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    };
    let r = with_driver(vm, &info, false, |vm, d| d.delete(vm, &key));
    Ok(bval(r))
}

/// php_dba_update()
fn php_dba_update(vm: &mut Vm, args: &[Value], fname: &str, mode: i32) -> R {
    let Some((info, key)) = key_conn(vm, fname, args, 3, true) else {
        return Ok(if vm.current_exception.is_some() {
            Value::Null
        } else {
            Value::False
        });
    };
    let val = arg(args, 1).to_php_string().as_bytes().to_vec();
    let r = with_driver(vm, &info, false, |vm, d| d.update(vm, &key, &val, mode));
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    Ok(bval(r))
}

fn dba_insert(vm: &mut Vm, args: &[Value]) -> R {
    php_dba_update(vm, args, "dba_insert", 1)
}

fn dba_replace(vm: &mut Vm, args: &[Value]) -> R {
    php_dba_update(vm, args, "dba_replace", 0)
}

fn dba_optimize(vm: &mut Vm, args: &[Value]) -> R {
    let Some(c) = conn_arg(vm, "dba_optimize", 1, args) else {
        return Ok(Value::Null);
    };
    let Some(info) = info_of(vm, &c) else {
        return Ok(Value::Null);
    };
    if !write_check(vm, &info) {
        return Ok(Value::False);
    }
    let r = with_driver(vm, &info, false, |vm, d| d.optimize(vm));
    Ok(bval(r))
}

fn dba_sync(vm: &mut Vm, args: &[Value]) -> R {
    let Some(c) = conn_arg(vm, "dba_sync", 1, args) else {
        return Ok(Value::Null);
    };
    let Some(info) = info_of(vm, &c) else {
        return Ok(Value::Null);
    };
    let r = with_driver(vm, &info, false, |vm, d| d.sync(vm));
    Ok(bval(r))
}

fn dba_handlers(_vm: &mut Vm, args: &[Value]) -> R {
    let full = arg(args, 0).to_bool();
    let mut out = PhpArray::new();
    for h in HANDLERS {
        if full {
            out.set(
                ArrayKey::String(PhpString::from_bytes(h.name.as_bytes())),
                sval(h.info.as_bytes()),
            );
        } else {
            out.push(sval(h.name.as_bytes()));
        }
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}

fn dba_list(_vm: &mut Vm, _args: &[Value]) -> R {
    let mut out = PhpArray::new();
    let entries: Vec<(u64, Vec<u8>)> = STATE.with(|s| {
        let s = s.borrow();
        s.connections
            .iter()
            .filter_map(|(_, w, id)| {
                let c = w.upgrade()?;
                let i = c.info.borrow();
                let i = i.as_ref()?;
                let p = i.try_borrow().ok()?.path.clone();
                Some((*id, p))
            })
            .collect()
    });
    for (id, p) in entries {
        out.set(ArrayKey::Int(id as i64), sval(&p));
    }
    Ok(Value::Array(Rc::new(RefCell::new(out))))
}
