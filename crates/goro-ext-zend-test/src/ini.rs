//! INI entries of ext/zend_test (test.c and observer.c).

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

/// PHP_INI_SYSTEM: settable from php.ini / -d only.
const SYSTEM: u8 = 0;
/// PHP_INI_ALL
const ALL: u8 = 1;
/// PHP_INI_ALL with OnUpdateStrNotEmpty
const ALL_NOT_EMPTY: u8 = 2;

pub(crate) const ENTRIES: &[(&str, &str, u8)] = &[
    ("zend_test.replace_zend_execute_ex", "0", SYSTEM),
    ("zend_test.register_passes", "0", SYSTEM),
    ("zend_test.print_stderr_mshutdown", "0", SYSTEM),
    ("zend_test.limit_copy_file_range", "-1", ALL),
    ("zend_test.quantity_value", "0", ALL),
    ("zend_test.str_test", "", ALL),
    ("zend_test.not_empty_str_test", "val", ALL_NOT_EMPTY),
    ("zend_test.observe_opline_in_zendmm", "0", ALL),
    ("zend_test.observer.enabled", "0", SYSTEM),
    ("zend_test.observer.show_output", "1", SYSTEM),
    ("zend_test.observer.observe_all", "0", SYSTEM),
    ("zend_test.observer.observe_includes", "0", SYSTEM),
    ("zend_test.observer.observe_functions", "0", SYSTEM),
    ("zend_test.observer.observe_declaring", "0", SYSTEM),
    ("zend_test.observer.observe_function_names", "", ALL),
    ("zend_test.observer.show_return_type", "0", SYSTEM),
    ("zend_test.observer.show_return_value", "0", SYSTEM),
    ("zend_test.observer.show_init_backtrace", "0", SYSTEM),
    ("zend_test.observer.show_opcode", "0", SYSTEM),
    ("zend_test.observer.show_opcode_in_user_handler", "", SYSTEM),
    ("zend_test.observer.fiber_init", "0", SYSTEM),
    ("zend_test.observer.fiber_switch", "0", SYSTEM),
    ("zend_test.observer.fiber_destroy", "0", SYSTEM),
    ("zend_test.observer.execute_internal", "0", SYSTEM),
];

pub(crate) fn register(vm: &mut Vm) {
    for &(name, default, kind) in ENTRIES {
        vm.constants.insert(
            name.as_bytes().to_vec(),
            Value::String(PhpString::from_string(default.to_string())),
        );
        match kind {
            SYSTEM => {
                vm.ini_set_hooks.insert(name.as_bytes().to_vec(), reject);
            }
            ALL_NOT_EMPTY => {
                vm.ini_set_hooks
                    .insert(name.as_bytes().to_vec(), reject_empty);
            }
            _ => {}
        }
    }
    vm.ini_set_hooks.insert(
        b"zend_test.observer.observe_function_names".to_vec(),
        crate::observer::on_update_function_names,
    );
    vm.ini_validators.insert(
        b"zend_test.quantity_value".to_vec(),
        on_update_quantity_value,
    );
    vm.ini_validators.insert(
        b"zend_test.limit_copy_file_range".to_vec(),
        on_update_limit_copy_file_range,
    );
}

fn on_update_quantity_value(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    on_update_long(vm, "zend_test.quantity_value", value)
}

fn on_update_limit_copy_file_range(vm: &mut Vm, value: &[u8], _startup: bool) -> bool {
    on_update_long(vm, "zend_test.limit_copy_file_range", value)
}

/// OnUpdateLong (zend_ini_parse_quantity_warn()): an invalid quantity warns
/// and is used as interpreted.
fn on_update_long(vm: &mut Vm, name: &str, value: &[u8]) -> bool {
    let (_, err) = goro_core::ini_quantity::parse_quantity(value, true);
    if let Some(e) = err {
        vm.emit_warning(&format!("Invalid \"{}\" setting. {}", name, e));
    }
    true
}

fn reject(_vm: &mut Vm, _name: &[u8], _value: &Value) -> bool {
    false
}

fn reject_empty(_vm: &mut Vm, _name: &[u8], value: &Value) -> bool {
    !value.to_php_string().as_bytes().is_empty()
}

/// zend_ini_parse_bool()
pub(crate) fn ini_bool(vm: &Vm, name: &str) -> bool {
    match vm.constants.get(name.as_bytes()) {
        Some(Value::String(s)) => parse_bool(s.as_bytes()),
        Some(Value::True) => true,
        Some(Value::Long(n)) => *n != 0,
        Some(Value::Double(d)) => *d != 0.0,
        _ => false,
    }
}

#[allow(dead_code)]
pub(crate) fn ini_str(vm: &Vm, name: &str) -> Vec<u8> {
    vm.constants
        .get(name.as_bytes())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default()
}

fn parse_bool(s: &[u8]) -> bool {
    let t = s.trim_ascii();
    if t.eq_ignore_ascii_case(b"true")
        || t.eq_ignore_ascii_case(b"yes")
        || t.eq_ignore_ascii_case(b"on")
    {
        return true;
    }
    // atoi()
    let mut n: i64 = 0;
    let mut neg = false;
    let mut it = t.iter().peekable();
    if let Some(&&c) = it.peek() {
        if c == b'-' || c == b'+' {
            neg = c == b'-';
            it.next();
        }
    }
    for &c in it {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((c - b'0') as i64);
    }
    if neg {
        n = -n;
    }
    n != 0
}
