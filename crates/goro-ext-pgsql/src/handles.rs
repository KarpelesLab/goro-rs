//! The PgSql\Connection, PgSql\Result and PgSql\Lob objects (pgsql.c's
//! pgsql_link_handle / pgsql_result_handle / pgLofp), the module globals
//! (PGG: default link, connections, persistent links, counters) and the
//! notice processor.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use goro_core::object::{NativeData, PhpObject};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::libpq::{PgConn, PgResult};
use crate::zpp::{Obj, throw};

pub type ConnRef = Rc<RefCell<PgConn>>;

pub const CONNECTION_CLASS: &str = "PgSql\\Connection";
pub const RESULT_CLASS: &str = "PgSql\\Result";
pub const LOB_CLASS: &str = "PgSql\\Lob";

/// pgsql_link_handle
pub struct LinkHandle {
    pub conn: RefCell<Option<ConnRef>>,
    pub hash: RefCell<Vec<u8>>,
    pub notices: RefCell<Option<Vec<Vec<u8>>>>,
    pub persistent: Cell<bool>,
    /// the notice processor's argument
    pub id: usize,
}

/// pgsql_result_handle
pub struct ResultHandle {
    pub conn: RefCell<Option<ConnRef>>,
    pub result: RefCell<Option<PgResult>>,
    pub row: Cell<i64>,
}

/// pgLofp
pub struct LobHandle {
    pub conn: RefCell<Option<ConnRef>>,
    pub lofd: Cell<i32>,
}

/// zend_pgsql_globals
#[derive(Default)]
pub struct Globals {
    pub default_link: Option<Obj>,
    pub connections: HashMap<Vec<u8>, Weak<RefCell<PhpObject>>>,
    pub persistent: HashMap<Vec<u8>, ConnRef>,
    pub num_links: i64,
    pub num_persistent: i64,
    /// field_oids / table_oids caches
    pub field_oids: HashMap<u32, Vec<u8>>,
    pub table_oids: HashMap<u32, Vec<u8>>,
    pub links: HashMap<usize, Weak<LinkHandle>>,
    pub next_link_id: usize,
}

thread_local! {
    pub static PGG: RefCell<Globals> = RefCell::new(Globals::default());
    /// Notices libpq reported during the current call: (link id, message)
    static PENDING: RefCell<Vec<(usize, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    /// The PHP function being run (for docref messages)
    static ACTIVE: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

pub fn push_active(name: &'static str) {
    ACTIVE.with(|a| a.borrow_mut().push(name));
}

pub fn pop_active() {
    ACTIVE.with(|a| {
        a.borrow_mut().pop();
    });
}

pub fn active() -> &'static str {
    ACTIVE.with(|a| a.borrow().last().copied().unwrap_or(""))
}

/// php_error_docref(NULL, E_WARNING, ...)
pub fn warning(vm: &mut Vm, msg: &str) {
    let m = format!("{}(): {}", active(), msg);
    vm.emit_warning(&m);
}

pub fn notice(vm: &mut Vm, msg: &str) {
    let m = format!("{}(): {}", active(), msg);
    let line = vm.current_line;
    vm.emit_notice_at(&m, line);
}

pub fn deprecated(vm: &mut Vm, msg: &str) {
    let m = format!("{}(): {}", active(), msg);
    vm.emit_deprecated(&m);
}

/// _php_pgsql_trim_message
pub fn trim_message(message: &[u8]) -> Vec<u8> {
    let n = message
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(message.len());
    let m = &message[..n];
    let mut i = m.len();
    if i > 2 && (m[i - 2] == b'\r' || m[i - 2] == b'\n') && m[i - 1] == b'.' {
        i -= 1;
    }
    while i > 1 && (m[i - 1] == b'\r' || m[i - 1] == b'\n') {
        i -= 1;
    }
    m[..i].to_vec()
}

/// PHP_PQ_ERROR(text, pgsql): `text` holds one "%s"
pub fn pq_error(vm: &mut Vm, text: &str, conn: &ConnRef) {
    let msg = trim_message(conn.borrow().error_message());
    let s = text.replacen("%s", &String::from_utf8_lossy(&msg), 1);
    warning(vm, &s);
}

/// _php_pgsql_notice_handler, as libpq's notice processor: queued, then
/// handled by flush_notices() when the libpq call is over.
pub fn notice_processor(arg: usize, msg: &[u8]) {
    if arg == 0 {
        return;
    }
    PENDING.with(|p| p.borrow_mut().push((arg, msg.to_vec())));
}

fn ini_bool(vm: &Vm, name: &[u8]) -> bool {
    match vm.constants.get(name) {
        Some(v) => {
            let s = v.to_php_string().to_string_lossy().to_ascii_lowercase();
            matches!(s.as_str(), "1" | "on" | "yes" | "true")
                || s.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
        None => false,
    }
}

pub fn ignore_notices(vm: &Vm) -> bool {
    ini_bool(vm, b"pgsql.ignore_notice")
}

/// Run the notice handler on the queued notices.
pub fn flush_notices(vm: &mut Vm) {
    let pending = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    if pending.is_empty() {
        return;
    }
    let ignore = ignore_notices(vm);
    let log = ini_bool(vm, b"pgsql.log_notice");
    for (id, msg) in pending {
        if ignore {
            continue;
        }
        let link = PGG.with(|g| g.borrow().links.get(&id).and_then(|w| w.upgrade()));
        let Some(link) = link else {
            continue;
        };
        let trimmed = trim_message(&msg);
        if log {
            notice(vm, &String::from_utf8_lossy(&trimmed));
        }
        link.notices
            .borrow_mut()
            .get_or_insert_with(Vec::new)
            .push(trimmed);
    }
}

/// pgsql_link_free
pub fn link_free(link: &LinkHandle) {
    let conn = link.conn.borrow_mut().take();
    if let Some(conn) = conn {
        {
            let mut c = conn.borrow_mut();
            while c.get_result().is_some() {}
            if !link.persistent.get() {
                c.untrace();
                c.finish();
            }
        }
        let hash = link.hash.borrow().clone();
        PGG.with(|g| {
            let mut g = g.borrow_mut();
            g.num_links -= 1;
            g.connections.remove(&hash);
        });
    }
    *link.notices.borrow_mut() = None;
}

impl Drop for LinkHandle {
    fn drop(&mut self) {
        if self.conn.borrow().is_some() {
            link_free(self);
        }
        let id = self.id;
        let _ = PGG.try_with(|g| {
            if let Ok(mut g) = g.try_borrow_mut() {
                g.links.remove(&id);
            }
        });
    }
}

/// zend_objects_not_comparable
fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

fn new_object(vm: &mut Vm, class: &str, data: Rc<dyn Any>) -> Obj {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(class.as_bytes().to_vec(), id);
    obj.native = Some(NativeData {
        data,
        handlers: &HANDLERS,
    });
    Rc::new(RefCell::new(obj))
}

/// object_init_ex(return_value, pgsql_link_ce) with its handle
pub fn new_link(
    vm: &mut Vm,
    conn: ConnRef,
    hash: Vec<u8>,
    persistent: bool,
) -> (Obj, Rc<LinkHandle>) {
    let id = PGG.with(|g| {
        let mut g = g.borrow_mut();
        g.next_link_id += 1;
        g.next_link_id
    });
    let h = Rc::new(LinkHandle {
        conn: RefCell::new(Some(conn)),
        hash: RefCell::new(hash),
        notices: RefCell::new(None),
        persistent: Cell::new(persistent),
        id,
    });
    PGG.with(|g| g.borrow_mut().links.insert(id, Rc::downgrade(&h)));
    let o = new_object(vm, CONNECTION_CLASS, h.clone() as Rc<dyn Any>);
    (o, h)
}

/// A PgSql\Result for `res`
pub fn new_result(vm: &mut Vm, conn: &ConnRef, res: PgResult) -> Value {
    let h = Rc::new(ResultHandle {
        conn: RefCell::new(Some(conn.clone())),
        result: RefCell::new(Some(res)),
        row: Cell::new(0),
    });
    Value::Object(new_object(vm, RESULT_CLASS, h as Rc<dyn Any>))
}

/// A PgSql\Lob
pub fn new_lob(vm: &mut Vm, conn: &ConnRef, lofd: i32) -> Value {
    let h = Rc::new(LobHandle {
        conn: RefCell::new(Some(conn.clone())),
        lofd: Cell::new(lofd),
    });
    Value::Object(new_object(vm, LOB_CLASS, h as Rc<dyn Any>))
}

fn data_of<T: 'static>(o: &Obj) -> Option<Rc<T>> {
    let d = o.borrow().native.as_ref().map(|n| n.data.clone())?;
    d.downcast::<T>().ok()
}

pub fn link_of(o: &Obj) -> Option<Rc<LinkHandle>> {
    data_of::<LinkHandle>(o)
}

pub fn result_of(o: &Obj) -> Option<Rc<ResultHandle>> {
    data_of::<ResultHandle>(o)
}

pub fn lob_of(o: &Obj) -> Option<Rc<LobHandle>> {
    data_of::<LobHandle>(o)
}

/// php_pgsql_set_default_link
pub fn set_default_link(o: &Obj) {
    // (the previous link is released outside the borrow: freeing it
    // updates the globals)
    let old = PGG.with(|g| g.borrow_mut().default_link.replace(o.clone()));
    drop(old);
}

/// Release the default link (zend_object_release(PGG(default_link)))
pub fn clear_default_link() {
    let old = PGG.with(|g| g.borrow_mut().default_link.take());
    drop(old);
}

pub fn default_link() -> Option<Obj> {
    PGG.with(|g| g.borrow().default_link.clone())
}

/// FETCH_DEFAULT_LINK() + CHECK_DEFAULT_LINK(): the default connection,
/// with the deprecation; None once an exception is pending.
pub fn fetch_default_link(vm: &mut Vm) -> Option<(Rc<LinkHandle>, Option<ConnRef>)> {
    let l = default_link().and_then(|o| link_of(&o));
    deprecated(
        vm,
        "Automatic fetching of PostgreSQL connection is deprecated",
    );
    if vm.current_exception.is_some() {
        return None;
    }
    match l {
        Some(l) => {
            let c = l.conn.borrow().clone();
            Some((l, c))
        }
        None => {
            throw(vm, b"Error", "No PostgreSQL connection opened yet");
            None
        }
    }
}

/// The connection of `o`, or the "has already been closed" Error.
pub fn link_conn(vm: &mut Vm, o: &Obj) -> Option<(Rc<LinkHandle>, ConnRef)> {
    let l = link_of(o)?;
    let c = l.conn.borrow().clone();
    match c {
        Some(c) => Some((l, c)),
        None => {
            throw(
                vm,
                b"Error",
                "PostgreSQL connection has already been closed",
            );
            None
        }
    }
}

/// The connection of an optional link argument (the default link when
/// absent), with CHECK_DEFAULT_LINK / CHECK_PGSQL_LINK.
pub fn link_or_default(vm: &mut Vm, o: Option<Obj>) -> Option<(Rc<LinkHandle>, ConnRef)> {
    match o {
        Some(o) => link_conn(vm, &o),
        None => {
            let (l, c) = fetch_default_link(vm)?;
            match c {
                Some(c) => Some((l, c)),
                None => {
                    throw(
                        vm,
                        b"Error",
                        "PostgreSQL connection has already been closed",
                    );
                    None
                }
            }
        }
    }
}

/// CHECK_PGSQL_RESULT
pub fn result_handle(vm: &mut Vm, o: &Obj) -> Option<Rc<ResultHandle>> {
    let h = result_of(o)?;
    if h.result.borrow().is_none() {
        throw(vm, b"Error", "PostgreSQL result has already been closed");
        return None;
    }
    Some(h)
}

/// CHECK_PGSQL_LOB
pub fn lob_handle(vm: &mut Vm, o: &Obj) -> Option<(Rc<LobHandle>, ConnRef)> {
    let h = lob_of(o)?;
    let c = h.conn.borrow().clone();
    match c {
        Some(c) => Some((h, c)),
        None => {
            throw(
                vm,
                b"Error",
                "PostgreSQL large object has already been closed",
            );
            None
        }
    }
}
