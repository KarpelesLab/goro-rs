//! dir() and the Directory class (PHP manual: dir(), "The Directory
//! class"; messages from tests ext/standard/tests/dir/*.phpt and
//! ext/standard/tests/directory/*.phpt).

use goro_runtime::Value;

use super::file::{Backing, Stream};
use crate::class::{NativeHooks, cflags};
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[(
        "function dir(string $directory, $context = null): Directory|false",
        dir,
    )],
    classes: &[(
        "directory",
        r#"<?php
final class Directory {
    public protected(set) readonly string $path;
    public protected(set) readonly mixed $handle;
    public function close(): void {}
    public function rewind(): void {}
    public function read(): string|false {}
}"#,
    )],
    methods: &[
        ("directory::close", close),
        ("directory::rewind", rewind),
        ("directory::read", read),
    ],
    hooks: &[("directory", hooks)],
    listed: false,
    ..Module::EMPTY
};

fn hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| {
            let cls = vm.class_of(o);
            cls.flags
                .set(cls.flags.get() | cflags::NOT_CLONEABLE | cflags::NOT_SERIALIZABLE);
        }),
        ..Default::default()
    }
}

fn dir(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let r = super::file::opendir(vm, a)?;
    let Value::Resource(h) = r else {
        return Ok(r);
    };
    let cls = super::date::class(vm, "Directory")?;
    let o = vm.instantiate_raw(&cls)?;
    vm.set_prop_raw(&o, b"path", Value::Str(a.str(0)));
    vm.set_prop_raw(&o, b"handle", Value::Resource(h));
    Ok(Value::Object(o))
}

/// Calls `f` with the object's directory handle as its only argument.
fn with_handle(vm: &mut Vm, a: &mut Args, f: fn(&mut Vm, &mut Args) -> R<Value>) -> R<Value> {
    let o = a.this_obj().clone();
    let h = vm.get_prop_raw(&o, b"handle");
    let Value::Resource(r) = &h else {
        return vm.throw("Error", "Internal directory stream has been altered");
    };
    if r.is_closed() {
        return vm.fthrow(
            a,
            "TypeError",
            "cannot use Directory resource after it has been closed",
        );
    }
    let is_dir = {
        let data = r.res().data.borrow();
        data.as_ref()
            .and_then(|d| d.downcast_ref::<std::cell::RefCell<Stream>>())
            .is_some_and(|c| {
                c.try_borrow().is_ok_and(|s| {
                    matches!(s.backing, Backing::Dir { .. } | Backing::UserDir { .. })
                })
            })
    };
    if !is_dir {
        return vm.throw("Error", "Internal directory stream has been altered");
    }
    let mut tmp = vec![h.clone()];
    let mut args = Args {
        this: None,
        vals: &mut tmp,
        func: a.func,
        static_class: std::ptr::null(),
        named: None,
    };
    f(vm, &mut args)
}

fn close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_handle(vm, a, super::file::closedir)?;
    Ok(Value::Null)
}

fn rewind(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_handle(vm, a, super::file::rewinddir)?;
    Ok(Value::Null)
}

fn read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    with_handle(vm, a, super::file::readdir)
}
