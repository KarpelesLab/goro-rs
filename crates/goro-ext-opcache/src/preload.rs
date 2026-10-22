//! Preloading (opcache.preload): accel_finish_startup() and the
//! preload_statistics of opcache_get_status(). The preload request itself
//! is run by goro-core (`Vm::preload_run()`).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::script_cache::{self, LOG_FATAL, LOG_WARNING};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

/// accel_finish_startup(): false when the request must not run.
pub(crate) fn run(vm: &mut Vm, script: &str, file_cache_only: bool) -> bool {
    if file_cache_only {
        script_cache::log(
            LOG_WARNING,
            "Preloading doesn't work in \"file_cache_only\" mode",
        );
        return true;
    }
    // accel_finish_startup_preload_subprocess()
    let euid = unsafe { libc::geteuid() };
    let user = super::ini_str(vm, "opcache.preload_user");
    if euid != 0 {
        if !user.is_empty() {
            script_cache::log(
                LOG_WARNING,
                "\"opcache.preload_user\" is ignored because the current user is not \"root\"",
            );
        }
    } else if !user.is_empty() {
        let c = std::ffi::CString::new(user.clone()).unwrap_or_default();
        let pw = unsafe { libc::getpwnam(c.as_ptr()) };
        if pw.is_null() {
            script_cache::log(
                LOG_FATAL,
                &format!("Preloading failed to getpwnam(\"{}\")", user),
            );
            std::process::exit(254);
        }
    }
    vm.preload_run(script)
}

pub(crate) fn active() -> bool {
    goro_core::preload::active()
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// The preload_statistics entry of opcache_get_status().
pub(crate) fn statistics() -> Option<Value> {
    if !goro_core::preload::done() {
        return None;
    }
    let (functions, classes, scripts) = goro_core::preload::statistics();
    let mut st = PhpArray::new();
    let size: i64 = scripts
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len() as i64).unwrap_or(0) * 8 + 1024)
        .sum();
    st.set(
        ArrayKey::String(PhpString::from_bytes(b"memory_consumption")),
        Value::Long(size),
    );
    let list = |names: Vec<Vec<u8>>| {
        let mut a = PhpArray::new();
        for n in names {
            a.push(Value::String(PhpString::from_vec(n)));
        }
        arr(a)
    };
    if !functions.is_empty() {
        st.set(
            ArrayKey::String(PhpString::from_bytes(b"functions")),
            list(functions),
        );
    }
    if !classes.is_empty() {
        st.set(
            ArrayKey::String(PhpString::from_bytes(b"classes")),
            list(classes),
        );
    }
    st.set(
        ArrayKey::String(PhpString::from_bytes(b"scripts")),
        list(scripts.into_iter().map(|s| s.into_bytes()).collect()),
    );
    Some(arr(st))
}
