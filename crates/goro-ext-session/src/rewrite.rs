//! Trans-sid URL rewriting: the session side of url_scanner_ex.

use goro_core::vm::Vm;
use goro_ext_standard::url_scanner;

use crate::{E_DEPRECATED, err};

pub(crate) fn reset() {}

pub(crate) fn register(_vm: &mut Vm) {}

/// OnUpdateSessionTags / OnUpdateSessionHosts
pub(crate) fn on_update_ini(vm: &mut Vm, key: &str, value: &[u8], _startup: bool) -> bool {
    if key == "trans_sid_tags" {
        if !value.starts_with(b"a=href,area=href,frame=src,form=") {
            err(
                vm,
                E_DEPRECATED,
                "Usage of session.trans_sid_tags INI setting is deprecated",
            );
        }
        url_scanner::set_tags(true, value);
    } else {
        if !value.is_empty() {
            err(
                vm,
                E_DEPRECATED,
                "Usage of session.trans_sid_hosts INI setting is deprecated",
            );
        }
        url_scanner::set_hosts(true, value);
    }
    true
}

/// php_url_scanner_reset_session_var
pub(crate) fn reset_session_var(vm: &mut Vm, name: &[u8]) {
    url_scanner::reset_var(vm, name, true, true);
}

/// php_url_scanner_add_session_var
pub(crate) fn add_session_var(vm: &mut Vm, name: &[u8], id: &[u8]) {
    url_scanner::add_var(vm, name, id, true, true);
}
