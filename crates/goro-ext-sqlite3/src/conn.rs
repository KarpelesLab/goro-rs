//! The native payloads of the SQLite3, SQLite3Stmt and SQLite3Result
//! objects (php_sqlite3_structs.h) and the access to the connection.
//!
//! The connection (`Box<Sqlite3>`) lives in `DbData::conn` and is borrowed
//! for the duration of each engine call.  PHP code run by the engine (user
//! functions, aggregates, collations, the authorizer) may use the same
//! SQLite3 object again, as C code may: user functions receive the
//! connection in their `Context`, which is published in `NESTED` while the
//! PHP callback runs and used instead of the (borrowed) cell.  Callbacks
//! that get no connection from the engine (collations, the authorizer)
//! cannot use their own connection; such a call fails with an Error.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use goro_core::object::NativeData;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use goro_sqlite::int::{Sqlite3, StmtId};

use crate::zpp::{Obj, throw};

thread_local! {
    /// The VM running the current engine call (for the callbacks).
    static VM: Cell<*mut Vm> = const { Cell::new(std::ptr::null_mut()) };
    /// The connections handed to the PHP callbacks running now (innermost
    /// last), from their `Context`.
    static NESTED: RefCell<Vec<*mut Sqlite3>> = const { RefCell::new(Vec::new()) };
    /// The names of the sqlite3 methods running (innermost last), for the
    /// php_error_docref() prefix of messages raised from callbacks.
    static ACTIVE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The VM of the engine call in progress (None outside of one).
pub fn current_vm<'a>() -> Option<&'a mut Vm> {
    let p = VM.with(|v| v.get());
    if p.is_null() {
        None
    } else {
        // SAFETY: the pointer is the VM that made the engine call that is
        // running the callback; that call does not use the VM meanwhile.
        Some(unsafe { &mut *p })
    }
}

/// Run `f` with the connection `db` published for re-entrant use.
pub fn with_nested<R>(db: *mut Sqlite3, f: impl FnOnce() -> R) -> R {
    NESTED.with(|n| n.borrow_mut().push(db));
    let r = f();
    NESTED.with(|n| {
        n.borrow_mut().pop();
    });
    r
}

pub fn push_active(name: &str) {
    ACTIVE.with(|a| a.borrow_mut().push(name.to_string()));
}

pub fn pop_active() {
    ACTIVE.with(|a| {
        a.borrow_mut().pop();
    });
}

/// get_active_function_name() for messages raised by the callbacks.
pub fn active_name(vm: &Vm) -> String {
    ACTIVE
        .with(|a| a.borrow().last().cloned())
        .or_else(|| vm.current_builtin_name())
        .unwrap_or_default()
}

/// php_sqlite3_func
pub struct FuncEntry {
    pub func_name: Vec<u8>,
    pub argc: i32,
    pub func: RefCell<Option<Value>>,
    pub step: RefCell<Option<Value>>,
    pub fini: RefCell<Option<Value>>,
}

/// php_sqlite3_collation
pub struct CollEntry {
    pub collation_name: Vec<u8>,
    pub cmp_func: RefCell<Option<Value>>,
}

/// The parts of php_sqlite3_db_object the engine callbacks need.
#[derive(Default)]
pub struct Shared {
    pub exception: Cell<bool>,
    pub authorizer_fcc: RefCell<Option<Value>>,
}

/// An element of the free list: a statement to finalize when the database
/// is closed.
pub struct FreeEntry {
    pub stmt_obj: Weak<StmtData>,
    pub stmt: Option<StmtId>,
}

#[derive(Default)]
pub struct DbState {
    pub initialised: bool,
    /// db_obj->db != NULL (stays set after close(), as in C)
    pub has_db: bool,
    pub funcs: Vec<Rc<FuncEntry>>,
    pub collations: Vec<Rc<CollEntry>>,
    pub free_list: Vec<FreeEntry>,
}

/// php_sqlite3_db_object
#[derive(Default)]
pub struct DbData {
    pub st: RefCell<DbState>,
    pub shared: Rc<Shared>,
    pub conn: RefCell<Option<Box<Sqlite3>>>,
    /// The address of the connection (to recognize it in NESTED).
    pub conn_ptr: Cell<usize>,
    /// Statements freed while the connection was in use.
    pub pending_finalize: RefCell<Vec<StmtId>>,
}

impl DbData {
    /// Run `f` on the connection, without a VM (object destruction).
    /// None when there is no connection or it is in use.
    pub fn with_conn_raw<R>(&self, f: impl FnOnce(&mut Sqlite3) -> R) -> Option<R> {
        match self.conn.try_borrow_mut() {
            Ok(mut g) => {
                let db = g.as_deref_mut()?;
                flush_pending(self, db);
                Some(f(db))
            }
            Err(_) => {
                let want = self.conn_ptr.get();
                let p = NESTED.with(|n| {
                    n.borrow()
                        .iter()
                        .rev()
                        .find(|p| **p as usize == want)
                        .copied()
                })?;
                // SAFETY: `p` is the connection the engine handed to the
                // callback running now (derived from its `&mut Sqlite3`),
                // which the engine does not use until the callback returns.
                Some(f(unsafe { &mut *p }))
            }
        }
    }

    /// Run `f` on the connection for a method: Err after an Error when the
    /// connection is not available.
    pub fn with_conn<R>(&self, vm: &mut Vm, f: impl FnOnce(&mut Sqlite3) -> R) -> Result<R, ()> {
        let prev = VM.with(|v| v.replace(vm as *mut Vm));
        let r = self.with_conn_raw(f);
        VM.with(|v| v.set(prev));
        match r {
            Some(r) => Ok(r),
            None => {
                throw(
                    vm,
                    b"Error",
                    "The SQLite3 object cannot be used while one of its callbacks is running",
                );
                Err(())
            }
        }
    }

    /// Run `f` on two connections (backup): `other` is None when both
    /// objects are the same.
    pub fn with_two_conns<R>(
        &self,
        vm: &mut Vm,
        other: &DbData,
        f: impl FnOnce(&mut Sqlite3, Option<&mut Sqlite3>) -> R,
    ) -> Result<R, ()> {
        if std::ptr::eq(self, other) {
            return self.with_conn(vm, |db| f(db, None));
        }
        let prev = VM.with(|v| v.replace(vm as *mut Vm));
        let r = (|| {
            let mut a = self.conn.try_borrow_mut().ok()?;
            let mut b = other.conn.try_borrow_mut().ok()?;
            let a = a.as_deref_mut()?;
            let b = b.as_deref_mut()?;
            Some(f(a, Some(b)))
        })();
        VM.with(|v| v.set(prev));
        match r {
            Some(r) => Ok(r),
            None => {
                throw(
                    vm,
                    b"Error",
                    "The SQLite3 object cannot be used while one of its callbacks is running",
                );
                Err(())
            }
        }
    }
}

fn flush_pending(d: &DbData, db: &mut Sqlite3) {
    let pending = std::mem::take(&mut *d.pending_finalize.borrow_mut());
    for s in pending {
        goro_sqlite::vdbeapi::sqlite3_finalize(db, s);
    }
}

/// php_sqlite3_free_list_dtor() of one element
fn free_list_dtor(d: &DbData, e: &FreeEntry) {
    if let Some(s) = e.stmt_obj.upgrade() {
        let (init, stmt) = {
            let st = s.st.borrow();
            (st.initialised, st.stmt)
        };
        if init {
            if let Some(id) = stmt {
                finalize_or_defer(d, id);
            }
            s.st.borrow_mut().initialised = false;
        }
    } else if let Some(id) = e.stmt {
        finalize_or_defer(d, id);
    }
}

fn finalize_or_defer(d: &DbData, id: StmtId) {
    if d.with_conn_raw(|db| goro_sqlite::vdbeapi::sqlite3_finalize(db, id))
        .is_none()
        && d.conn.try_borrow().map(|c| c.is_some()).unwrap_or(true)
    {
        d.pending_finalize.borrow_mut().push(id);
    }
}

/// zend_llist_clean(&db_obj->free_list)
pub fn free_list_clean(d: &DbData) {
    let list = std::mem::take(&mut d.st.borrow_mut().free_list);
    for e in &list {
        free_list_dtor(d, e);
    }
}

/// zend_llist_del_element(&db_obj->free_list, stmt,
/// php_sqlite3_compare_stmt_free): the first element whose statement is
/// initialised and is `stmt` is removed and finalized.
pub fn free_list_del(d: &DbData, stmt: Option<StmtId>, self_init: Option<bool>) {
    let found = {
        let mut st = d.st.borrow_mut();
        let pos = st.free_list.iter().position(|e| {
            let (init, s) = match e.stmt_obj.upgrade() {
                Some(o) => {
                    let o = o.st.borrow();
                    (o.initialised, o.stmt)
                }
                // the statement being freed
                None => (self_init.unwrap_or(false), e.stmt),
            };
            init && s == stmt
        });
        pos.map(|p| st.free_list.remove(p))
    };
    if let Some(e) = found {
        free_list_dtor(d, &e);
    }
}

impl Drop for DbData {
    /// php_sqlite3_object_free_storage()
    fn drop(&mut self) {
        *self.shared.authorizer_fcc.borrow_mut() = None;
        let (initialised, funcs, collations) = {
            let st = self.st.get_mut();
            (
                st.initialised,
                std::mem::take(&mut st.funcs),
                std::mem::take(&mut st.collations),
            )
        };
        let conn = self.conn.get_mut();
        for func in funcs.iter().rev() {
            if let (true, Some(db)) = (initialised, conn.as_deref_mut()) {
                goro_sqlite::mainapi::sqlite3_create_function(
                    db,
                    Some(&func.func_name),
                    func.argc,
                    goro_sqlite::consts::SQLITE_UTF8 as i32,
                    Default::default(),
                    None,
                    None,
                    None,
                );
            }
            *func.func.borrow_mut() = None;
            *func.step.borrow_mut() = None;
            *func.fini.borrow_mut() = None;
        }
        for coll in collations.iter().rev() {
            if let (true, Some(db)) = (initialised, conn.as_deref_mut()) {
                goro_sqlite::mainapi::sqlite3_create_collation(
                    db,
                    Some(&coll.collation_name),
                    goro_sqlite::consts::SQLITE_UTF8 as i32,
                    None,
                );
            }
            *coll.cmp_func.borrow_mut() = None;
        }
        let pending = std::mem::take(self.pending_finalize.get_mut());
        if let Some(mut db) = conn.take() {
            if initialised {
                for s in pending {
                    goro_sqlite::vdbeapi::sqlite3_finalize(&mut db, s);
                }
                goro_sqlite::mainapi::sqlite3_close(&mut db);
            }
        }
        self.st.get_mut().initialised = false;
    }
}

/// struct php_sqlite3_bound_param
#[derive(Clone)]
pub struct BoundParam {
    pub param_number: i64,
    pub name: Option<Vec<u8>>,
    pub type_: i64,
    pub parameter: Value,
}

/// The keys of stmt_obj->bound_params
#[derive(Clone, PartialEq)]
pub enum ParamKey {
    Name(Vec<u8>),
    Index(i64),
}

#[derive(Default)]
pub struct StmtState {
    pub stmt: Option<StmtId>,
    /// The SQLite3 object (held, as the C Z_ADDREF) and its payload.
    pub db_obj: Option<(Obj, Rc<DbData>)>,
    pub initialised: bool,
    pub bound_params: Option<Vec<(ParamKey, BoundParam)>>,
}

/// php_sqlite3_stmt
#[derive(Default)]
pub struct StmtData {
    pub st: RefCell<StmtState>,
}

impl StmtData {
    pub fn db(&self) -> Option<Rc<DbData>> {
        self.st.borrow().db_obj.as_ref().map(|(_, d)| d.clone())
    }
}

impl Drop for StmtData {
    /// php_sqlite3_stmt_object_free_storage()
    fn drop(&mut self) {
        let st = self.st.get_mut();
        st.bound_params = None;
        if st.initialised {
            if let Some((_, d)) = &st.db_obj {
                free_list_del(d, st.stmt, Some(true));
            }
        }
        st.db_obj = None;
    }
}

#[derive(Default)]
pub struct ResultState {
    pub db_obj: Option<Rc<DbData>>,
    /// The SQLite3Stmt object (held).
    pub stmt_obj: Option<Obj>,
    pub is_prepared_statement: bool,
    pub column_count: i32,
    pub column_names: Option<Vec<Vec<u8>>>,
}

/// php_sqlite3_result
#[derive(Default)]
pub struct ResultData {
    pub st: RefCell<ResultState>,
}

impl Drop for ResultData {
    /// php_sqlite3_result_object_free_storage()
    fn drop(&mut self) {
        let st = self.st.get_mut();
        st.column_names = None;
        st.column_count = -1;
        if let Some(o) = st.stmt_obj.take() {
            if let Some(s) = stmt_data(&o) {
                let (init, stmt, d) = {
                    let s = s.st.borrow();
                    (
                        s.initialised,
                        s.stmt,
                        s.db_obj.as_ref().map(|x| x.1.clone()),
                    )
                };
                if let (true, Some(id), Some(d)) = (init, stmt, d) {
                    d.with_conn_raw(|db| goro_sqlite::vdbeapi::sqlite3_reset(db, id));
                }
            }
            drop(o);
        }
    }
}

// ── object handlers ─────────────────────────────────────────────────────

fn no_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        goro_core::value::display_class_name(&src.borrow().class_name)
    );
    throw(vm, b"Error", &m);
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(no_clone),
    ..NativeObjectHandlers::EMPTY
};

fn native<T: Any>(o: &Obj) -> Option<Rc<T>> {
    let b = o.try_borrow().ok()?;
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<T>().ok()
}

fn attach<T: Any + Default>(o: &Obj) -> Rc<T> {
    if let Some(d) = native::<T>(o) {
        return d;
    }
    let d = Rc::new(T::default());
    o.borrow_mut().native = Some(NativeData {
        data: d.clone() as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    d
}

pub fn stmt_data(o: &Obj) -> Option<Rc<StmtData>> {
    native::<StmtData>(o)
}

pub fn attach_db(o: &Obj) -> Rc<DbData> {
    attach::<DbData>(o)
}

pub fn attach_stmt(o: &Obj) -> Rc<StmtData> {
    attach::<StmtData>(o)
}

pub fn attach_result(o: &Obj) -> Rc<ResultData> {
    let r = attach::<ResultData>(o);
    r.st.borrow_mut().column_count = -1;
    r
}

/// object_init_ex() of an internal class of this extension
pub fn new_object(vm: &mut Vm, class: &[u8]) -> Obj {
    let id = vm.alloc_object_id();
    let obj = goro_core::object::PhpObject::new(class.to_vec(), id);
    let o = Rc::new(RefCell::new(obj));
    goro_core::gc::track_object(&o);
    vm.run_object_create_hook(&o);
    o
}
