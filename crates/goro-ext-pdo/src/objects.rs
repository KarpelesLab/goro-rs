//! The native payloads of the PDO objects (pdo_dbh_object_t, pdo_stmt_t,
//! pdo_row_t, the statement iterator) and their object handlers.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::NativeData;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::core::*;

// ── PDO ───────────────────────────────────────────────────────────────

/// pdo_dbh_object_t: `inner` may be switched to a persistent handle.
pub struct DbhData {
    pub inner: RefCell<DbhRef>,
}

impl Drop for DbhData {
    fn drop(&mut self) {
        let dbh = self.inner.get_mut().clone();
        dbh_free_storage(&dbh);
    }
}

/// pdo_dbh_free_storage
fn dbh_free_storage(dbh: &DbhRef) {
    let (methods, has_data, persistent) = match dbh.try_borrow() {
        Ok(d) => (d.methods, d.driver_data.is_some(), d.is_persistent),
        Err(_) => return,
    };
    if let Some(m) = methods {
        if let (true, Some(rollback)) = (has_data, m.rollback) {
            if in_transaction(dbh) {
                rollback(dbh);
                if let Ok(mut d) = dbh.try_borrow_mut() {
                    d.in_txn = false;
                }
            }
        }
        if persistent {
            if let Some(ps) = m.persistent_shutdown {
                ps(dbh);
            }
        }
    }
    dbh_free(dbh, false);
}

/// pdo_is_in_transaction
pub fn in_transaction(dbh: &DbhRef) -> bool {
    let (m, in_txn) = {
        let d = dbh.borrow();
        (d.methods, d.in_txn)
    };
    match m.and_then(|m| m.in_transaction) {
        Some(f) => f(dbh),
        None => in_txn,
    }
}

/// dbh_free
pub fn dbh_free(dbh: &DbhRef, free_persistent: bool) {
    let old = match dbh.try_borrow_mut() {
        Ok(mut d) => d.query_stmt.take(),
        Err(_) => return,
    };
    drop(old);
    {
        let mut d = dbh.borrow_mut();
        if d.is_persistent {
            d.refcount = d.refcount.saturating_sub(1);
            if !free_persistent && d.refcount > 0 {
                return;
            }
        }
    }
    let m = dbh.borrow().methods;
    if let Some(m) = m {
        (m.closer)(dbh);
    }
    let mut d = dbh.borrow_mut();
    d.def_stmt_ctor_args = None;
    d.methods = None;
}

static DBH_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: None,
    write_property: None,
    has_property: None,
    unset_property: None,
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: None,
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(no_clone),
};

pub fn dbh_data(o: &Obj) -> Option<Rc<DbhData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<DbhData>().ok()
}

/// The (inner) handle of a PDO object
pub fn dbh_of(o: &Obj) -> Option<DbhRef> {
    dbh_data(o).map(|d| d.inner.borrow().clone())
}

/// pdo_dbh_new
pub fn attach_dbh(o: &Obj) {
    if dbh_data(o).is_some() {
        return;
    }
    let dbh = Rc::new(RefCell::new(Dbh::new()));
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(DbhData {
            inner: RefCell::new(dbh),
        }) as Rc<dyn Any>,
        handlers: &DBH_HANDLERS,
    });
}

// ── PDOStatement ──────────────────────────────────────────────────────

pub struct StmtData {
    pub stmt: StmtRef,
}

impl Drop for Stmt {
    fn drop(&mut self) {
        // php_pdo_free_statement
        self.bound_params = None;
        self.bound_param_map = None;
        self.bound_columns = None;
        if let Some(m) = self.methods {
            (m.dtor)(self);
        }
        self.methods = None;
        self.driver_data = None;
        self.columns = None;
        self.fetch = FetchData::default();
        self.database_object_handle = None;
    }
}

/// dbstmt_get_gc: the PDO object, and the FETCH_INTO object or the
/// FETCH_CLASS constructor arguments. Only reported while this object is
/// the sole owner of the statement (the iterator and PDORow objects share
/// it).
pub fn stmt_get_gc(data: &Rc<dyn std::any::Any>, visit: &mut dyn FnMut(&Value)) {
    if Rc::strong_count(data) != 1 {
        return;
    }
    let Some(d) = data.downcast_ref::<StmtData>() else {
        return;
    };
    if Rc::strong_count(&d.stmt) != 1 {
        return;
    }
    let Ok(st) = d.stmt.try_borrow() else {
        return;
    };
    if let Some(o) = &st.database_object_handle {
        visit(&Value::Object(o.clone()));
    }
    let mode = st.default_fetch_type & !crate::core::PDO_FETCH_FLAGS;
    if mode == crate::core::PDO_FETCH_INTO {
        if let Some(o) = &st.fetch.into {
            visit(&Value::Object(o.clone()));
        }
    } else if mode == crate::core::PDO_FETCH_CLASS {
        if let Some(a) = &st.fetch.cls_ctor_args {
            visit(&Value::Array(a.clone()));
        }
    }
}

/// The handlers of PDOStatement objects (for gc registration).
pub fn stmt_handlers() -> &'static NativeObjectHandlers {
    &STMT_HANDLERS
}

static STMT_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: None,
    write_property: Some(stmt_prop_write),
    has_property: None,
    unset_property: Some(stmt_prop_delete),
    read_dimension: None,
    write_dimension: None,
    has_dimension: None,
    unset_dimension: None,
    get_properties: None,
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(no_clone),
};

pub fn stmt_of(o: &Obj) -> Option<StmtRef> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data
        .clone()
        .downcast::<StmtData>()
        .ok()
        .map(|d| d.stmt.clone())
}

/// pdo_dbstmt_new
pub fn attach_stmt(o: &Obj) {
    if stmt_of(o).is_some() {
        return;
    }
    let mut s = Stmt::new();
    s.self_obj = Some(Rc::downgrade(o));
    // the typed property starts uninitialized
    if !o.borrow().has_property(b"queryString") {
        o.borrow_mut()
            .set_property(b"queryString".to_vec(), Value::Undef);
    }
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(StmtData {
            stmt: Rc::new(RefCell::new(s)),
        }) as Rc<dyn Any>,
        handlers: &STMT_HANDLERS,
    });
}

/// dbstmt_prop_write
fn stmt_prop_write(vm: &mut Vm, o: &Obj, name: &[u8], _v: &Value) -> bool {
    if name == b"queryString" && !matches!(o.borrow().get_property(b"queryString"), Value::Undef) {
        throw(vm, b"Error", "Property queryString is read only");
        return true;
    }
    false
}

/// dbstmt_prop_delete
fn stmt_prop_delete(vm: &mut Vm, _o: &Obj, name: &[u8]) -> bool {
    if name == b"queryString" {
        throw(vm, b"Error", "Property queryString is read only");
        return true;
    }
    false
}

// ── PDORow ────────────────────────────────────────────────────────────

/// pdo_row_t
pub struct RowData {
    pub stmt: StmtRef,
    pub stmt_obj: Obj,
}

impl Drop for RowData {
    fn drop(&mut self) {
        if let Ok(mut s) = self.stmt.try_borrow_mut() {
            s.lazy_object_ref = None;
        }
    }
}

static ROW_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(row_prop_read),
    write_property: Some(row_prop_write),
    has_property: Some(row_prop_exists),
    unset_property: Some(row_prop_delete),
    read_dimension: Some(row_dim_read),
    write_dimension: Some(row_dim_write),
    has_dimension: Some(row_dim_exists),
    unset_dimension: Some(row_dim_delete),
    get_properties: Some(row_get_properties),
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(no_clone),
};

pub fn row_of(o: &Obj) -> Option<Rc<RowData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<RowData>().ok()
}

/// pdo_get_lazy_object
pub fn lazy_object(vm: &mut Vm, stmt: &StmtRef) -> Value {
    let existing = stmt
        .borrow()
        .lazy_object_ref
        .as_ref()
        .and_then(|w| w.upgrade());
    if let Some(o) = existing {
        return Value::Object(o);
    }
    let stmt_obj = match stmt.borrow().self_obj.as_ref().and_then(|w| w.upgrade()) {
        Some(o) => o,
        None => return Value::Null,
    };
    let id = vm.alloc_object_id();
    let mut obj = goro_core::object::PhpObject::new(b"PDORow".to_vec(), id);
    obj.native = Some(NativeData {
        data: Rc::new(RowData {
            stmt: stmt.clone(),
            stmt_obj,
        }) as Rc<dyn Any>,
        handlers: &ROW_HANDLERS,
    });
    let o = Rc::new(RefCell::new(obj));
    stmt.borrow_mut().lazy_object_ref = Some(Rc::downgrade(&o));
    Value::Object(o)
}

fn row_read_column_name(vm: &mut Vm, stmt: &StmtRef, name: &[u8]) -> Option<Value> {
    let n = stmt.borrow().column_count;
    for colno in 0..n.max(0) as usize {
        let matches = stmt
            .borrow()
            .columns
            .as_ref()
            .and_then(|c| c.get(colno))
            .map(|c| c.name() == name)
            .unwrap_or(false);
        if matches {
            return Some(crate::stmt::fetch_value(vm, stmt, colno as i64, None));
        }
    }
    None
}

fn row_read_column_number(vm: &mut Vm, stmt: &StmtRef, column: i64) -> Option<Value> {
    let n = stmt.borrow().column_count as i64;
    if column >= 0 && column < n {
        return Some(crate::stmt::fetch_value(vm, stmt, column, None));
    }
    None
}

/// row_prop_read
fn row_prop_read(vm: &mut Vm, o: &Obj, name: &[u8], _mode: u8) -> Option<Value> {
    let r = row_of(o)?;
    if name == b"queryString" {
        return Some(r.stmt_obj.borrow().get_property(b"queryString"));
    }
    let v = match numeric_str_long(name) {
        Some(n) => row_read_column_number(vm, &r.stmt, n),
        None => row_read_column_name(vm, &r.stmt, name),
    };
    Some(v.unwrap_or(Value::Null))
}

fn row_prop_write(vm: &mut Vm, _o: &Obj, _name: &[u8], _v: &Value) -> bool {
    throw(vm, b"Error", "Cannot write to PDORow property");
    true
}

fn row_prop_exists(vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    let r = row_of(o)?;
    let v = match numeric_str_long(name) {
        Some(n) => row_read_column_number(vm, &r.stmt, n),
        None => row_read_column_name(vm, &r.stmt, name),
    };
    Some(match v {
        None => false,
        Some(v) => {
            if check_empty != 0 {
                v.is_truthy()
            } else {
                !matches!(v, Value::Null)
            }
        }
    })
}

fn row_prop_delete(vm: &mut Vm, _o: &Obj, _name: &[u8]) -> bool {
    throw(vm, b"Error", "Cannot unset PDORow property");
    true
}

fn row_dim_read(vm: &mut Vm, o: &Obj, offset: Option<&Value>, mode: u8) -> Option<Value> {
    let offset = match offset {
        Some(v) => v.deref(),
        None => {
            throw(vm, b"Error", "Cannot append to PDORow offset");
            return Some(Value::Null);
        }
    };
    if let Value::Long(n) = offset {
        let r = row_of(o)?;
        let cc = r.stmt.borrow().column_count as i64;
        if n >= 0 && n < cc {
            return Some(crate::stmt::fetch_value(vm, &r.stmt, n, None));
        }
        return Some(Value::Null);
    }
    let member = match vm.value_to_string_checked(&offset) {
        Ok(s) => s.as_bytes().to_vec(),
        Err(_) => return Some(Value::Null),
    };
    row_prop_read(vm, o, &member, mode)
}

fn row_dim_write(vm: &mut Vm, _o: &Obj, member: Option<&Value>, _v: &Value) -> bool {
    if member.is_none() {
        throw(vm, b"Error", "Cannot append to PDORow offset");
    } else {
        throw(vm, b"Error", "Cannot write to PDORow offset");
    }
    true
}

fn row_dim_exists(vm: &mut Vm, o: &Obj, offset: &Value, check_empty: u8) -> Option<bool> {
    let offset = offset.deref();
    if let Value::Long(column) = offset {
        let r = row_of(o)?;
        let cc = r.stmt.borrow().column_count as i64;
        if check_empty == 0 {
            return Some(column >= 0 && column < cc);
        }
        return Some(match row_read_column_number(vm, &r.stmt, column) {
            Some(v) => v.is_truthy(),
            None => false,
        });
    }
    let member = match vm.value_to_string_checked(&offset) {
        Ok(s) => s.as_bytes().to_vec(),
        Err(_) => return Some(false),
    };
    row_prop_exists(vm, o, &member, check_empty)
}

fn row_dim_delete(vm: &mut Vm, _o: &Obj, _offset: &Value) -> bool {
    throw(vm, b"Error", "Cannot unset PDORow offset");
    true
}

/// row_get_properties_for (ZEND_PROP_PURPOSE_DEBUG)
fn row_get_properties(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let r = row_of(o)?;
    let mut props = PhpArray::new();
    {
        let so = r.stmt_obj.borrow();
        let v = so.get_property(b"queryString");
        if !matches!(v, Value::Undef) {
            props.set(ArrayKey::String(PhpString::from_bytes(b"queryString")), v);
        }
    }
    let n = r.stmt.borrow().column_count.max(0) as usize;
    for i in 0..n {
        let name = r
            .stmt
            .borrow()
            .columns
            .as_ref()
            .and_then(|c| c.get(i))
            .map(|c| c.name().to_vec())
            .unwrap_or_default();
        if name.as_slice() == b"queryString" {
            continue;
        }
        let v = crate::stmt::fetch_value(vm, &r.stmt, i as i64, None);
        props.set(ArrayKey::String(PhpString::from_vec(name)), v);
    }
    Some(props)
}

// ── the statement iterator ────────────────────────────────────────────

/// struct php_pdo_iterator
pub struct StmtIter {
    pub stmt_obj: Obj,
    pub stmt: StmtRef,
    /// (zend_ulong)-1 is None
    pub key: Cell<Option<u64>>,
    pub fetch_ahead: RefCell<Value>,
}

pub static ITER_HANDLERS: NativeObjectHandlers = NativeObjectHandlers::EMPTY;

pub fn iter_of(o: &Obj) -> Option<Rc<StmtIter>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<StmtIter>().ok()
}

// ── shared handlers ───────────────────────────────────────────────────

fn no_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        goro_core::value::display_class_name(&src.borrow().class_name)
    );
    throw(vm, b"Error", &m);
}
