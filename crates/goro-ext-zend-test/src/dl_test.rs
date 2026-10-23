//! ext/dl_test: the extension php-src builds shared to test dl() (and
//! `extension=` loading). goro has it built in, loadable with dl() or the
//! `extension` INI entry (see `Vm::loadable_modules`).

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

pub const NAME: &[u8] = b"dl_test";

const CLASSES: &[u8] = b"<?php
class DlTest {
    public function test(string $str = 'World'): string {
        return 'Hello ' . $str;
    }
}
class DlTestSuperClass {
    public int $a;
    public function test(string $str = \"\") {
        return null;
    }
}
class DlTestSubClass extends DlTestSuperClass {
}
class DlTestAliasedClass {
}
";

fn debug(msg: &str) {
    if std::env::var_os("PHP_DL_TEST_MODULE_DEBUG").is_some() {
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "{}", msg);
    }
}

/// PHP_MINIT_FUNCTION(dl_test)
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.register_function(b"dl_test_test1", dl_test_test1);
    vm.register_function(b"dl_test_test2", dl_test_test2);
    if std::env::var_os("PHP_DL_TEST_USE_REGISTER_FUNCTIONS_DIRECTLY").is_some() {
        vm.register_function(
            b"dl_test_use_register_functions_directly",
            dl_test_use_register_functions_directly,
        );
    }
    for (k, v) in [("dl_test.long", "0"), ("dl_test.string", "hello")] {
        vm.constants
            .entry(k.as_bytes().to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(v.as_bytes())));
    }
    crate::classes::declare_internal_classes(vm, CLASSES);
    debug("DL TEST MINIT");
    vm.module_shutdown_hooks
        .push(|_| debug("DL TEST MSHUTDOWN"));
    // PHP_RINIT_FUNCTION / PHP_RSHUTDOWN_FUNCTION
    vm.request_startup_hooks.push(|_| debug("DL TEST RINIT"));
    vm.request_shutdown_hooks
        .push(|_| debug("DL TEST RSHUTDOWN"));
}

/// PHP_FUNCTION(dl_test_test1)
fn dl_test_test1(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    vm.write_output(b"The extension dl_test is loaded and working!\r\n");
    Ok(Value::Null)
}

/// PHP_FUNCTION(dl_test_test2)
fn dl_test_test2(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut out = b"Hello ".to_vec();
    match args.first() {
        Some(v) => out.extend_from_slice(v.to_php_string().as_bytes()),
        None => out.extend_from_slice(b"World"),
    }
    Ok(Value::String(PhpString::from_vec(out)))
}

fn dl_test_use_register_functions_directly(
    _vm: &mut Vm,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::String(PhpString::from_bytes(b"OK")))
}
