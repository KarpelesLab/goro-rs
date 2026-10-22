//! pg_meta_data, pg_convert and the pg_insert / pg_update / pg_delete /
//! pg_select helpers of pgsql.c.

use goro_core::value::Value;
use goro_core::vm::Vm;

pub fn pg_meta_data(_vm: &mut Vm, _f: &'static str, _args: &[Value]) -> Value {
    Value::False
}

pub fn pg_convert(_vm: &mut Vm, _f: &'static str, _args: &[Value]) -> Value {
    Value::False
}

pub fn pg_insert(_vm: &mut Vm, _f: &'static str, _args: &[Value]) -> Value {
    Value::False
}

pub fn pg_update(_vm: &mut Vm, _f: &'static str, _args: &[Value]) -> Value {
    Value::False
}

pub fn pg_delete(_vm: &mut Vm, _f: &'static str, _args: &[Value]) -> Value {
    Value::False
}

pub fn pg_select(_vm: &mut Vm, _f: &'static str, _args: &[Value]) -> Value {
    Value::False
}
