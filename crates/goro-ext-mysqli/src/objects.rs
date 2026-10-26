//! The mysqli objects (mysqli.c): their native payload (MYSQLI_RESOURCE),
//! object handlers and the property readers of mysqli_prop.c /
//! mysqli_driver.c / mysqli_warning.c.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::mysqlnd::conn::{CloseType, ConnRef};
use crate::mysqlnd::ps::Stmt;
use crate::mysqlnd::result::ResultSet;
use crate::zpp::{Obj, throw, vstr};

pub const MYSQLI_STATUS_UNKNOWN: u8 = 0;
pub const MYSQLI_STATUS_INITIALIZED: u8 = 1;
pub const MYSQLI_STATUS_VALID: u8 = 2;

pub const MYSQLI_REPORT_OFF: i64 = 0;
pub const MYSQLI_REPORT_ERROR: i64 = 1;
pub const MYSQLI_REPORT_STRICT: i64 = 2;
pub const MYSQLI_REPORT_INDEX: i64 = 4;

/// MY_MYSQL
pub struct MyMysql {
    pub mysql: Option<ConnRef>,
    pub hash_key: Option<String>,
    pub multi_query: bool,
    pub persistent: bool,
    pub async_result_fetch_type: i64,
}

impl MyMysql {
    pub fn new() -> MyMysql {
        MyMysql {
            mysql: None,
            hash_key: None,
            multi_query: false,
            persistent: false,
            async_result_fetch_type: 0,
        }
    }
}

impl Default for MyMysql {
    fn default() -> Self {
        MyMysql::new()
    }
}

/// MY_STMT
pub struct MyStmt {
    pub stmt: Option<Rc<RefCell<Stmt>>>,
    pub query: Option<Vec<u8>>,
}

/// st_mysqli_warning
#[derive(Clone, Debug)]
pub struct WarningItem {
    pub reason: Value,
    pub sqlstate: Value,
    pub errorno: i64,
}

pub enum Kind {
    Link(Rc<RefCell<MyMysql>>),
    Result(Rc<RefCell<ResultSet>>),
    Stmt(Rc<RefCell<MyStmt>>),
    Warning(Rc<Vec<WarningItem>>, Cell<usize>),
}

/// MYSQLI_RESOURCE
pub struct Resource {
    pub status: Cell<u8>,
    pub kind: Kind,
}

/// The class family of a mysqli object
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Base {
    Driver,
    Link,
    Result,
    Stmt,
    Warning,
}

/// mysqli_object: intern->ptr
pub struct MysqliData {
    pub base: Base,
    pub res: RefCell<Option<Rc<Resource>>>,
}

impl Drop for MysqliData {
    fn drop(&mut self) {
        // free_obj handlers
        if let Some(r) = self.res.get_mut().take() {
            if let Ok(r) = Rc::try_unwrap(r) {
                drop_resource(self.base, r);
            }
        }
    }
}

fn drop_resource(base: Base, r: Resource) {
    match (base, r.kind) {
        (Base::Link, Kind::Link(m)) => {
            // mysqli_link_free_storage
            let status = r.status.get();
            let mut m = m.borrow_mut();
            if m.mysql.is_some() {
                crate::link::php_mysqli_close(&mut m, CloseType::Explicit, status);
            }
        }
        (Base::Result, Kind::Result(res)) => {
            // mysql_free_result(res, false) via the ResultSet drop
            drop(res);
        }
        (Base::Stmt, Kind::Stmt(s)) => {
            // php_clear_stmt_bind: mysqli_stmt_close(stmt, true)
            let st = s.borrow_mut().stmt.take();
            if let Some(st) = st {
                if let Ok(mut st) = st.try_borrow_mut() {
                    st.dtor(true);
                }
            }
        }
        _ => {}
    }
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
    count_elements: None,
    cast: None,
    compare: None,
    clone_obj: Some(h_clone),
};

pub fn data_of(o: &Obj) -> Option<Rc<MysqliData>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<MysqliData>().ok()
}

/// mysqli_objects_new: the base class of `class_lower` (or a subclass).
pub fn base_of(vm: &Vm, class_lower: &[u8]) -> Option<Base> {
    let mut cur = class_lower.to_vec();
    for _ in 0..64 {
        match cur.as_slice() {
            b"mysqli" => return Some(Base::Link),
            b"mysqli_driver" => return Some(Base::Driver),
            b"mysqli_result" => return Some(Base::Result),
            b"mysqli_stmt" => return Some(Base::Stmt),
            b"mysqli_warning" => return Some(Base::Warning),
            _ => {}
        }
        let p = vm.classes.get(&cur).and_then(|c| c.parent.clone())?;
        cur = p.to_ascii_lowercase();
    }
    None
}

pub fn attach(o: &Obj, base: Base) {
    o.borrow_mut().native = Some(NativeData {
        data: Rc::new(MysqliData {
            base,
            res: RefCell::new(None),
        }) as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

/// create_object handler
pub fn create_object(vm: &mut Vm, o: &Obj) {
    if data_of(o).is_some() {
        return;
    }
    let cl = o.borrow().class_name.to_ascii_lowercase();
    if let Some(b) = base_of(vm, &cl) {
        attach(o, b);
        // object_properties_init: the declared properties with a default
        if b == Base::Driver && !o.borrow().has_property(b"report_mode") {
            o.borrow_mut()
                .set_property(b"report_mode".to_vec(), Value::Long(0));
        }
    }
}

/// A new object of an internal mysqli class (MYSQLI_RETVAL_RESOURCE)
pub fn new_object(vm: &mut Vm, class: &[u8], base: Base, res: Option<Resource>) -> Obj {
    let id = vm.alloc_object_id();
    let o = Rc::new(RefCell::new(PhpObject::new(class.to_vec(), id)));
    attach(&o, base);
    if let Some(r) = res {
        set_res(&o, Some(r));
    }
    o
}

pub fn set_res(o: &Obj, r: Option<Resource>) {
    if let Some(d) = data_of(o) {
        let old = d.res.replace(r.map(Rc::new));
        drop(old);
    }
}

pub fn get_res(o: &Obj) -> Option<Rc<Resource>> {
    data_of(o).and_then(|d| d.res.borrow().clone())
}

/// The display name of the object's class (intern->zo.ce->name)
pub fn ce_name(o: &Obj) -> String {
    goro_core::value::display_class_name(&o.borrow().class_name).to_string()
}

/// MYSQLI_FETCH_RESOURCE: Err after throwing.
pub fn fetch_resource(vm: &mut Vm, o: &Obj, check: u8) -> Result<Rc<Resource>, ()> {
    let r = match get_res(o) {
        Some(r) => r,
        None => {
            let m = format!("{} object is already closed", ce_name(o));
            throw(vm, b"Error", &m);
            return Err(());
        }
    };
    if r.status.get() < check {
        let m = format!("{} object is not fully initialized", ce_name(o));
        throw(vm, b"Error", &m);
        return Err(());
    }
    Ok(r)
}

/// MYSQLI_FETCH_RESOURCE_CONN: (MY_MYSQL, MYSQLND)
pub fn fetch_conn(vm: &mut Vm, o: &Obj, check: u8) -> Result<(Rc<RefCell<MyMysql>>, ConnRef), ()> {
    let r = fetch_resource(vm, o, check)?;
    let m = match &r.kind {
        Kind::Link(m) => m.clone(),
        _ => {
            let m = format!("{} object is not fully initialized", ce_name(o));
            throw(vm, b"Error", &m);
            return Err(());
        }
    };
    let c = m.borrow().mysql.clone();
    match c {
        Some(c) => Ok((m, c)),
        None => {
            let msg = format!("{} object is not fully initialized", ce_name(o));
            throw(vm, b"Error", &msg);
            Err(())
        }
    }
}

pub fn fetch_result(vm: &mut Vm, o: &Obj, check: u8) -> Result<Rc<RefCell<ResultSet>>, ()> {
    let r = fetch_resource(vm, o, check)?;
    match &r.kind {
        Kind::Result(res) => Ok(res.clone()),
        _ => Err(()),
    }
}

/// MYSQLI_FETCH_RESOURCE_STMT
pub fn fetch_stmt(
    vm: &mut Vm,
    o: &Obj,
    check: u8,
) -> Result<(Rc<RefCell<MyStmt>>, Rc<RefCell<Stmt>>), ()> {
    let r = fetch_resource(vm, o, check)?;
    match &r.kind {
        Kind::Stmt(s) => {
            let st = s.borrow().stmt.clone();
            match st {
                Some(st) => Ok((s.clone(), st)),
                None => Err(()),
            }
        }
        _ => Err(()),
    }
}

/// MYSQLI_CLEAR_RESOURCE (the payload was already released by the caller)
pub fn clear_resource(o: &Obj) {
    if let Some(d) = data_of(o) {
        let old = d.res.replace(None);
        drop(old);
    }
}

// ── property handlers ─────────────────────────────────────────────────

const LINK_PROPS: [&str; 18] = [
    "affected_rows",
    "client_info",
    "client_version",
    "connect_errno",
    "connect_error",
    "errno",
    "error",
    "error_list",
    "field_count",
    "host_info",
    "info",
    "insert_id",
    "server_info",
    "server_version",
    "sqlstate",
    "protocol_version",
    "thread_id",
    "warning_count",
];
const RESULT_PROPS: [&str; 5] = [
    "current_field",
    "field_count",
    "lengths",
    "num_rows",
    "type",
];
const STMT_PROPS: [&str; 10] = [
    "affected_rows",
    "insert_id",
    "num_rows",
    "param_count",
    "field_count",
    "errno",
    "error",
    "error_list",
    "sqlstate",
    "id",
];
const DRIVER_PROPS: [&str; 4] = [
    "client_info",
    "client_version",
    "driver_version",
    "report_mode",
];
const WARNING_PROPS: [&str; 3] = ["message", "sqlstate", "errno"];

fn props_of(base: Base) -> &'static [&'static str] {
    match base {
        Base::Link => &LINK_PROPS,
        Base::Result => &RESULT_PROPS,
        Base::Stmt => &STMT_PROPS,
        Base::Driver => &DRIVER_PROPS,
        Base::Warning => &WARNING_PROPS,
    }
}

fn is_prop(base: Base, name: &[u8]) -> bool {
    props_of(base).iter().any(|p| p.as_bytes() == name)
}

/// The read handler of property `name`: Ok(value) or Err(()) (FAILURE,
/// with the error thrown unless `quiet`).
pub fn read_prop(vm: &mut Vm, o: &Obj, base: Base, name: &[u8], quiet: bool) -> Result<Value, ()> {
    match base {
        Base::Driver => crate::link::driver_prop(vm, name, quiet),
        Base::Link => crate::link::link_prop(vm, o, name, quiet),
        Base::Result => crate::res::result_prop(vm, o, name, quiet),
        Base::Stmt => crate::stmt::stmt_prop(vm, o, name, quiet),
        Base::Warning => crate::stmt::warning_prop(vm, o, name, quiet),
    }
}

fn h_read_property(vm: &mut Vm, o: &Obj, name: &[u8], mode: u8) -> Option<Value> {
    let d = data_of(o)?;
    if !is_prop(d.base, name) {
        return None;
    }
    // BP_VAR_IS reads are quiet
    match read_prop(vm, o, d.base, name, mode == 2) {
        Ok(v) => Some(v),
        Err(()) => Some(Value::Null),
    }
}

fn h_write_property(vm: &mut Vm, o: &Obj, name: &[u8], v: &Value) -> bool {
    let d = match data_of(o) {
        Some(d) => d,
        None => return false,
    };
    if !is_prop(d.base, name) {
        return false;
    }
    if d.base == Base::Driver && name == b"report_mode" {
        // typed int property
        let strict = vm.caller_strict_types;
        let line = vm.current_line;
        match vm.zpp_coerce_scalar(goro_core::arginfo::T_INT, v, strict, line) {
            Ok(Some(Value::Long(n))) => crate::link::set_report_mode(n),
            Ok(_) => {
                let m = format!(
                    "Cannot assign {} to property mysqli_driver::$report_mode of type int",
                    Vm::value_type_name(v)
                );
                throw(vm, b"TypeError", &m);
            }
            Err(()) => {}
        }
        return true;
    }
    let m = format!(
        "Cannot write read-only property {}::${}",
        ce_name(o),
        String::from_utf8_lossy(name)
    );
    throw(vm, b"Error", &m);
    true
}

fn h_has_property(vm: &mut Vm, o: &Obj, name: &[u8], check_empty: u8) -> Option<bool> {
    let d = data_of(o)?;
    if !is_prop(d.base, name) {
        return None;
    }
    let v = read_prop(vm, o, d.base, name, true).ok();
    Some(match v {
        None => false,
        Some(v) => {
            if check_empty == 1 {
                v.is_truthy()
            } else {
                !matches!(v, Value::Null)
            }
        }
    })
}

/// mysqli_object_get_debug_info
fn h_get_properties(vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let d = data_of(o)?;
    let mut arr = PhpArray::new();
    for p in props_of(d.base) {
        if let Ok(v) = read_prop(vm, o, d.base, p.as_bytes(), true) {
            arr.set(ArrayKey::String(PhpString::from_bytes(p.as_bytes())), v);
        }
    }
    Some(arr)
}

fn h_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        ce_name(src)
    );
    throw(vm, b"Error", &m);
}

/// MYSQLI_RETURN_LONG_INT
pub fn long_or_str(v: u64) -> Value {
    if v < i64::MAX as u64 {
        Value::Long(v as i64)
    } else {
        vstr(v.to_string())
    }
}

/// An error_list array
pub fn error_list(list: &[crate::mysqlnd::ErrorListElem]) -> Value {
    let mut arr = PhpArray::new();
    for e in list {
        let mut a = PhpArray::new();
        a.set(
            ArrayKey::String(PhpString::from_bytes(b"errno")),
            Value::Long(e.error_no as i64),
        );
        a.set(
            ArrayKey::String(PhpString::from_bytes(b"sqlstate")),
            vstr(&e.sqlstate),
        );
        a.set(
            ArrayKey::String(PhpString::from_bytes(b"error")),
            vstr(&e.error),
        );
        arr.push(Value::Array(Rc::new(RefCell::new(a))));
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn empty_array() -> Value {
    Value::Array(Rc::new(RefCell::new(PhpArray::new())))
}
