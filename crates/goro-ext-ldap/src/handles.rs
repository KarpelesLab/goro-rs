//! Native data for LDAP\Connection, LDAP\Result, LDAP\ResultEntry and the
//! object create/fetch helpers.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::object::{NativeData, PhpObject};
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use goro_ldap::{Conn, LdapMessage};

use crate::zpp::{CONN_CLASS, ENTRY_CLASS, Obj, RESULT_CLASS};

/// ldap_linkdata
pub struct ConnData {
    pub conn: RefCell<Option<Conn>>,
}

/// ldap_resultdata
pub struct ResultData {
    pub msg: RefCell<Option<Rc<LdapMessage>>>,
}

/// ldap_result_entry
pub struct EntryData {
    /// the Result object this entry belongs to (ZVAL_COPY of res)
    pub res: Obj,
    pub msg: Rc<LdapMessage>,
    pub is_ref: bool,
    pub index: usize,
    /// ber state: None before ldap_first_attribute()
    pub attr_cursor: Cell<Option<usize>>,
}

fn not_comparable(a: &Value, b: &Value) -> Option<i64> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) if Rc::ptr_eq(x, y) => Some(0),
        (Value::Object(_), Value::Object(_)) => Some(1),
        _ => None,
    }
}

pub static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    compare: Some(not_comparable),
    ..NativeObjectHandlers::EMPTY
};

fn new_object(vm: &mut Vm, class: &[u8], data: Rc<dyn Any>) -> Value {
    let id = vm.next_object_id();
    let mut obj = PhpObject::new(class.to_vec(), id);
    obj.native = Some(NativeData {
        data,
        handlers: &HANDLERS,
    });
    Value::Object(Rc::new(RefCell::new(obj)))
}

pub fn new_conn(vm: &mut Vm, conn: Conn) -> Value {
    new_object(
        vm,
        CONN_CLASS,
        Rc::new(ConnData {
            conn: RefCell::new(Some(conn)),
        }),
    )
}

pub fn new_result(vm: &mut Vm, msg: Rc<LdapMessage>) -> Value {
    new_object(
        vm,
        RESULT_CLASS,
        Rc::new(ResultData {
            msg: RefCell::new(Some(msg)),
        }),
    )
}

pub fn new_entry(vm: &mut Vm, res: Obj, msg: Rc<LdapMessage>, is_ref: bool, index: usize) -> Value {
    new_object(
        vm,
        ENTRY_CLASS,
        Rc::new(EntryData {
            res,
            msg,
            is_ref,
            index,
            attr_cursor: Cell::new(None),
        }),
    )
}

pub fn conn_data(o: &Obj) -> Option<Rc<ConnData>> {
    o.borrow()
        .native
        .as_ref()
        .map(|n| n.data.clone())?
        .downcast::<ConnData>()
        .ok()
}

pub fn result_data(o: &Obj) -> Option<Rc<ResultData>> {
    o.borrow()
        .native
        .as_ref()
        .map(|n| n.data.clone())?
        .downcast::<ResultData>()
        .ok()
}

pub fn entry_data(o: &Obj) -> Option<Rc<EntryData>> {
    o.borrow()
        .native
        .as_ref()
        .map(|n| n.data.clone())?
        .downcast::<EntryData>()
        .ok()
}
