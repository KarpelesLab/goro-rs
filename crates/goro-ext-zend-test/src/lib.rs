//! php-src's `zend_test` extension (ext/zend_test): helpers the engine test
//! suite uses to exercise internal APIs (zpp variants, internal classes with
//! typed properties, internal enums and attributes, the observer API...).
//!
//! Like a shared php-src build, the extension is only loaded on request
//! (`extension=zend_test`, which run-tests.php passes for `--EXTENSIONS--
//! zend_test`): see [`requested`].
//!
//! Functions are native builtins described by their stub arginfo
//! (test.stub.php, in stub_sigs.rs). Classes are declared from PHP source
//! (prelude.php) and then made internal (see `classes`).

use goro_core::value::Value;
use goro_core::vm::Vm;

mod classes;
mod functions;
mod handlers;
mod ini;
mod observer;
mod support;

/// The extension name (`extension_loaded('zend_test')`).
pub const NAME: &[u8] = b"zend_test";

/// Whether INI settings ask for the extension (`extension=zend_test`,
/// possibly as a file name like `zend_test.so`).
pub fn requested<'a>(ini: impl IntoIterator<Item = (&'a str, &'a str)>) -> bool {
    ini.into_iter().any(|(k, v)| {
        k == "extension" && {
            let v = v.trim();
            let base = v.rsplit(['/', '\\']).next().unwrap_or(v);
            let base = base.strip_prefix("php_").unwrap_or(base);
            let stem = base.split('.').next().unwrap_or(base);
            stem.eq_ignore_ascii_case("zend_test")
        }
    })
}

/// MINIT + RINIT: register the functions, constants, INI entries and
/// classes of the extension.
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    functions::register(vm);
    ini::register(vm);
    register_constants(vm);
    classes::register(vm);
    handlers::register(vm);
    observer::register(vm);
}

fn register_constants(vm: &mut Vm) {
    use goro_core::string::PhpString;
    let s = |v: &str| Value::String(PhpString::from_string(v.to_string()));
    // In declaration order (ReflectionExtension::getConstants()).
    vm.constants
        .insert(b"ZEND_TEST_DEPRECATED".to_vec(), Value::Long(42));
    vm.internal_deprecated_constants.insert(
        b"ZEND_TEST_DEPRECATED".to_vec(),
        "Constant ZEND_TEST_DEPRECATED is deprecated".to_string(),
    );
    vm.constants
        .insert(b"ZEND_CONSTANT_A".to_vec(), s("global"));
    // ZEND_TEST_ATTRIBUTED_CONSTANT (#[\Deprecated]) is declared by the
    // prelude, with its attribute.
    vm.constants
        .insert(b"ZendTestNS2\\ZEND_CONSTANT_A".to_vec(), s("namespaced"));
    vm.constants.insert(
        b"ZendTestNS2\\ZendSubNS\\ZEND_CONSTANT_A".to_vec(),
        s("namespaced"),
    );
    vm.module_constants.push((
        NAME.to_vec(),
        [
            "ZEND_TEST_DEPRECATED",
            "ZEND_CONSTANT_A",
            "ZEND_TEST_ATTRIBUTED_CONSTANT",
            "ZendTestNS2\\ZEND_CONSTANT_A",
            "ZendTestNS2\\ZendSubNS\\ZEND_CONSTANT_A",
        ]
        .iter()
        .map(|n| n.as_bytes().to_vec())
        .collect(),
    ));
}
