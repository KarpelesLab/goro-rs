//! Trans-sid URL rewriting (ext/standard/url_scanner_ex.re, session part).

use goro_core::vm::Vm;

pub(crate) fn reset() {}

pub(crate) fn register(_vm: &mut Vm) {}

pub(crate) fn on_update_ini(_vm: &mut Vm, _key: &str, _value: &[u8], _startup: bool) -> bool {
    true
}

pub(crate) fn reset_session_var(_vm: &mut Vm, _name: &[u8]) {}

pub(crate) fn add_session_var(_vm: &mut Vm, _name: &[u8], _id: &[u8]) {}
