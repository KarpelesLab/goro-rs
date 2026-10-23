//! PECL imagick 3.8 for goro-rs: the Imagick, ImagickPixel,
//! ImagickPixelIterator, ImagickDraw and ImagickKernel classes and their
//! exceptions, on goro-imagick (a pure-Rust engine with MagickWand
//! semantics). The PHP binding glue (argument handling, exception
//! messages, method semantics) follows imagick's C sources (PHP License);
//! the method signatures come from its stubs.

#![allow(
    clippy::chunks_exact_to_as_chunks,
    clippy::needless_range_loop,
    clippy::manual_range_contains
)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeClassHandler, NativeObjectHandlers, Vm, VmError};
use goro_imagick::{PixelInfo, Wand};

mod consts;
mod draw;
mod imagick;
mod kernel;
mod morph;
mod ops;
mod ops2;
mod pixel;
mod pixel_iter;
mod zpp;

use zpp::{Func, Obj};

/// The extension name (`extension_loaded('imagick')`).
pub const NAME: &[u8] = b"imagick";
/// PHP_IMAGICK_VERSION
pub const VERSION: &str = "3.8.1";

pub(crate) type R = Result<Value, ()>;
pub(crate) type Impl = fn(&mut Vm, &Func, Option<&Obj>, &[Value]) -> R;

// ---------------------------------------------------------------------------
// object data

/// php_imagick_object
pub struct ImagickData {
    pub wand: RefCell<Wand>,
    pub next_out_of_bound: Cell<bool>,
    /// setProgressMonitor()'s callback
    pub progress: RefCell<Option<Value>>,
}

/// php_imagickpixel_object
pub struct PixelData {
    pub color: RefCell<Option<PixelInfo>>,
}

impl PixelData {
    pub fn new(c: PixelInfo) -> Rc<PixelData> {
        Rc::new(PixelData {
            color: RefCell::new(Some(c)),
        })
    }
}

pub(crate) const C_IMAGICK: &[u8] = b"imagick";
pub(crate) const C_DRAW: &[u8] = b"imagickdraw";
pub(crate) const C_PIXEL: &[u8] = b"imagickpixel";
pub(crate) const C_ITER: &[u8] = b"imagickpixeliterator";
pub(crate) const C_KERNEL: &[u8] = b"imagickkernel";

/// php_imagick_class_type_t: which exception class / code an error gets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cls {
    Imagick,
    Draw,
    Iter,
    Pixel,
    Kernel,
}

impl Cls {
    fn exception(self) -> (&'static [u8], i64) {
        match self {
            Cls::Imagick => (b"ImagickException", 1),
            Cls::Draw => (b"ImagickDrawException", 2),
            Cls::Iter => (b"ImagickPixelIteratorException", 3),
            Cls::Pixel => (b"ImagickPixelException", 4),
            Cls::Kernel => (b"ImagickKernelException", 5),
        }
    }
}

// ---------------------------------------------------------------------------
// helpers

pub(crate) fn throw(vm: &mut Vm, class: &[u8], msg: &str, code: i64) {
    let e = vm.create_exception(class, msg, vm.current_line);
    if code != 0 {
        if let Value::Object(o) = &e {
            o.borrow_mut()
                .set_property(b"code".to_vec(), Value::Long(code));
        }
    }
    vm.current_exception = Some(e);
}

/// php_imagick_throw_exception()
pub(crate) fn throw_cls(vm: &mut Vm, cls: Cls, msg: &str) -> R {
    let (c, code) = cls.exception();
    throw(vm, c, msg, code);
    Err(())
}

/// php_imagick_convert_*_exception(): the wand's exception (its message,
/// severity as code), or the default message (code by class) — always an
/// ImagickException, as the C code throws.
pub(crate) fn convert_exception(
    vm: &mut Vm,
    e: Option<goro_imagick::MagickError>,
    cls: Cls,
    default: &str,
) -> R {
    match e {
        Some(e) if !e.message.is_empty() => {
            throw(vm, b"ImagickException", &e.message, e.severity as i64);
        }
        _ => {
            let (_, code) = cls.exception();
            throw(vm, b"ImagickException", default, code);
        }
    }
    Err(())
}

pub(crate) fn str_val(s: &str) -> Value {
    Value::String(PhpString::from_bytes(s.as_bytes()))
}

pub(crate) fn bytes_val(s: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(s))
}

pub(crate) fn bool_val(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub(crate) fn arr_val(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

pub(crate) fn set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v);
}

pub(crate) fn obj_class_lower(o: &Obj) -> Vec<u8> {
    o.borrow().class_name.to_ascii_lowercase()
}

/// instanceof against a built-in class (lower-case name).
pub(crate) fn instance_of(vm: &Vm, v: &Value, class_lower: &[u8]) -> Option<Obj> {
    match v.deref() {
        Value::Object(o) => {
            let c = obj_class_lower(&o);
            if c == class_lower || vm.is_subclass_of(&c, class_lower) {
                Some(o)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// An "O" argument: an object of the class, else a TypeError.
pub(crate) fn arg_obj(
    vm: &mut Vm,
    f: &Func,
    n: usize,
    v: &Value,
    class_lower: &[u8],
    class_name: &str,
) -> Result<Obj, ()> {
    match instance_of(vm, v, class_lower) {
        Some(o) => Ok(o),
        None => {
            f.type_error(vm, n, class_name, &v.deref());
            Err(())
        }
    }
}

fn data_any<T: 'static>(o: &Obj) -> Option<Rc<T>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    n.data.clone().downcast::<T>().ok()
}

pub(crate) fn imagick_data(o: &Obj) -> Rc<ImagickData> {
    if let Some(d) = data_any::<ImagickData>(o) {
        return d;
    }
    attach_imagick(o, Wand::new());
    data_any::<ImagickData>(o).unwrap()
}

pub(crate) fn pixel_data(o: &Obj) -> Rc<PixelData> {
    if let Some(d) = data_any::<PixelData>(o) {
        return d;
    }
    let d = Rc::new(PixelData {
        color: RefCell::new(None),
    });
    attach(o, d.clone() as Rc<dyn Any>, &PIXEL_HANDLERS);
    d
}

fn attach(o: &Obj, d: Rc<dyn Any>, h: &'static NativeObjectHandlers) {
    o.borrow_mut().native = Some(NativeData {
        data: d,
        handlers: h,
    });
}

pub(crate) fn attach_imagick(o: &Obj, w: Wand) {
    let d = Rc::new(ImagickData {
        wand: RefCell::new(w),
        next_out_of_bound: Cell::new(false),
        progress: RefCell::new(None),
    });
    attach(o, d as Rc<dyn Any>, &IMAGICK_HANDLERS);
}

pub(crate) fn attach_pixel(o: &Obj, d: Rc<PixelData>) {
    attach(o, d as Rc<dyn Any>, &PIXEL_HANDLERS);
}

/// A new object of a built-in class (object_init_ex + create_object).
pub(crate) fn new_object(vm: &mut Vm, class: &[u8]) -> Obj {
    let o = vm.spl_instantiate(class);
    vm.run_object_create_hook(&o);
    o
}

/// A new Imagick object holding `w`.
pub(crate) fn new_imagick(vm: &mut Vm, w: Wand) -> Value {
    let o = new_object(vm, b"Imagick");
    attach_imagick(&o, w);
    Value::Object(o)
}

/// A new ImagickPixel object for a colour (or sharing a PixelData).
pub(crate) fn new_pixel_shared(vm: &mut Vm, d: Rc<PixelData>) -> Value {
    let o = new_object(vm, b"ImagickPixel");
    attach_pixel(&o, d);
    Value::Object(o)
}

pub(crate) fn new_pixel(vm: &mut Vm, c: PixelInfo) -> Value {
    new_pixel_shared(vm, PixelData::new(c))
}

/// php_imagick_zval_to_pixelwand(): a colour string, number or ImagickPixel.
pub(crate) fn zval_to_pixel(vm: &mut Vm, v: &Value, caller: Cls) -> Result<PixelInfo, ()> {
    let v = v.deref();
    match &v {
        Value::String(_) | Value::Long(_) | Value::Double(_) => {
            let s = v.to_php_string();
            let s = String::from_utf8_lossy(s.as_bytes()).into_owned();
            match PixelInfo::parse(&s) {
                Some(c) => Ok(c),
                None => {
                    throw_cls(vm, caller, "Unrecognized color string")?;
                    Err(())
                }
            }
        }
        Value::Object(o) => {
            if instance_of(vm, &v, C_PIXEL).is_some() {
                let d = pixel_data(o);
                let c = d.color.borrow().clone();
                match c {
                    Some(c) => Ok(c),
                    None => {
                        throw_cls(vm, Cls::Pixel, "Can not process empty ImagickPixel object")?;
                        Err(())
                    }
                }
            } else {
                throw_cls(
                    vm,
                    caller,
                    "The parameter must be an instance of ImagickPixel or a string",
                )?;
                Err(())
            }
        }
        _ => {
            throw_cls(vm, caller, "Invalid color parameter provided")?;
            Err(())
        }
    }
}

/// php_imagick_zval_to_opacity(): a number (the alpha) or an ImagickPixel.
pub(crate) fn zval_to_opacity(vm: &mut Vm, v: &Value, caller: Cls) -> Result<PixelInfo, ()> {
    let v = v.deref();
    match &v {
        Value::String(_) | Value::Long(_) | Value::Double(_) => {
            let a = v.to_double();
            let c = PixelInfo {
                alpha: a * goro_imagick::QUANTUM_RANGE,
                ..Default::default()
            };
            Ok(c)
        }
        Value::Object(_) => zval_to_pixel(vm, &v, caller),
        _ => {
            throw_cls(vm, caller, "Invalid color parameter provided")?;
            Err(())
        }
    }
}

// ---------------------------------------------------------------------------
// dispatch

/// (method name, implementation, static)
pub(crate) type MethodTable = &'static [(&'static str, Impl, bool)];

fn table_for(class: &[u8]) -> Option<(&'static [MethodTable], &'static str, Cls)> {
    Some(match class {
        C_IMAGICK => (
            &[imagick::CORE, ops::OPS, ops2::OPS2],
            "Imagick",
            Cls::Imagick,
        ),
        C_DRAW => (&[draw::METHODS], "ImagickDraw", Cls::Draw),
        C_PIXEL => (&[pixel::METHODS], "ImagickPixel", Cls::Pixel),
        C_ITER => (&[pixel_iter::METHODS], "ImagickPixelIterator", Cls::Iter),
        C_KERNEL => (&[kernel::METHODS], "ImagickKernel", Cls::Kernel),
        _ => return None,
    })
}

fn stub_method(class_name: &str, method: &[u8]) -> Option<&'static goro_core::stubs::StubFunc> {
    let (c, m) = goro_core::stubs::find_method(class_name.as_bytes(), method)?;
    if c.name.eq_ignore_ascii_case(class_name) {
        Some(m)
    } else {
        None
    }
}

fn has_method(class: &[u8], method: &[u8], is_static: bool) -> bool {
    let Some((_, name, _)) = table_for(class) else {
        return false;
    };
    match stub_method(name, method) {
        Some(m) => !is_static || m.flags & goro_core::stubs::F_STATIC != 0,
        None => false,
    }
}

fn this_obj(this: Option<&Value>) -> Option<Obj> {
    match this.map(|t| t.deref()) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}

fn call_method(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    this: Option<&Value>,
    args: &[Value],
) -> Value {
    let Some((table, cname, cls)) = table_for(class) else {
        return Value::Null;
    };
    let Some(stub) = stub_method(cname, method) else {
        return Value::Null;
    };
    let is_static = stub.flags & goro_core::stubs::F_STATIC != 0;
    let this_o = this_obj(this);
    if !is_static && this_o.is_none() {
        let msg = format!(
            "Non-static method {}::{}() cannot be called statically",
            cname, stub.name
        );
        throw(vm, b"Error", &msg, 0);
        return Value::Null;
    }
    let f = Func::method(cname, stub.name);
    let frame = if is_static {
        format!("{}::{}", cname, stub.name)
    } else {
        format!("{}->{}", cname, stub.name)
    };
    let has = vm
        .call_stack
        .last()
        .map(|fr| fr.0.eq_ignore_ascii_case(&frame))
        .unwrap_or(false);
    if !has {
        let line = vm.current_line;
        let file = vm.current_file.clone();
        vm.call_stack.push((frame, file, line, args.to_vec(), true));
    }
    let imp = table
        .iter()
        .flat_map(|t| t.iter())
        .find(|(n, _, _)| n.as_bytes().eq_ignore_ascii_case(method))
        .map(|e| e.1);
    let r = match imp {
        Some(imp) => imp(vm, &f, if is_static { None } else { this_o.as_ref() }, args),
        None => {
            let _ = throw_cls(
                vm,
                cls,
                &format!("{}::{}() is not implemented yet", cname, stub.name),
            );
            Err(())
        }
    };
    if !has {
        vm.call_stack.pop();
    }
    match r {
        Ok(v) => v,
        Err(()) => Value::Null,
    }
}

fn param_names(class: &[u8], method: &[u8]) -> Option<&'static [&'static str]> {
    let (_, name, _) = table_for(class)?;
    let m = stub_method(name, method)?;
    thread_local! {
        static CACHE: RefCell<std::collections::HashMap<(Vec<u8>, Vec<u8>), &'static [&'static str]>> =
            RefCell::new(std::collections::HashMap::new());
    }
    let key = (class.to_vec(), method.to_ascii_lowercase());
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return Some(v);
    }
    let names: Vec<&'static str> = m.args.iter().map(|a| a.name).collect();
    let leaked: &'static [&'static str] = Box::leak(names.into_boxed_slice());
    CACHE.with(|c| c.borrow_mut().insert(key, leaked));
    Some(leaked)
}

// ---------------------------------------------------------------------------
// object handlers

static IMAGICK_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    read_property: Some(imagick::read_property),
    count_elements: Some(imagick::count_elements),
    cast: Some(imagick::cast),
    clone_obj: Some(imagick::clone_obj),
    ..NativeObjectHandlers::EMPTY
};

static PIXEL_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(pixel::clone_obj),
    ..NativeObjectHandlers::EMPTY
};

fn create_imagick(_vm: &mut Vm, o: &Obj) {
    if data_any::<ImagickData>(o).is_none() {
        attach_imagick(o, Wand::new());
    }
}

fn create_pixel(_vm: &mut Vm, o: &Obj) {
    if data_any::<PixelData>(o).is_none() {
        attach_pixel(
            o,
            Rc::new(PixelData {
                color: RefCell::new(None),
            }),
        );
    }
}

// ---------------------------------------------------------------------------
// registration

const INI: &[(&str, &str)] = &[
    ("imagick.locale_fix", "0"),
    ("imagick.skip_version_check", "0"),
    ("imagick.progress_monitor", "0"),
    ("imagick.set_single_thread", "1"),
    ("imagick.shutdown_sleep_count", "10"),
    ("imagick.allow_zero_dimension_images", "0"),
];

/// INI value of a directive.
pub(crate) fn ini_bool(vm: &Vm, name: &str) -> bool {
    match vm.constants.get(name.as_bytes()) {
        Some(v) => {
            let s = v.to_php_string();
            let s = s.as_bytes();
            !(s.is_empty()
                || s == b"0"
                || s.eq_ignore_ascii_case(b"off")
                || s.eq_ignore_ascii_case(b"false")
                || s.eq_ignore_ascii_case(b"no"))
        }
        None => false,
    }
}

fn register_class(vm: &mut Vm, lower: &[u8], name: &str, interfaces: &[&[u8]]) {
    let mut ce = match goro_core::stubs::class_entry(vm, name.as_bytes()) {
        Some(stub) => stub.clone(),
        None => ClassEntry::new(name.as_bytes().to_vec()),
    };
    // the methods are served by the native class handler
    ce.methods.clear();
    for i in interfaces {
        if !ce.interfaces.iter().any(|x| x.eq_ignore_ascii_case(i)) {
            ce.interfaces.push(i.to_vec());
        }
    }
    vm.classes.insert(lower.to_vec(), ce);
    vm.native_class_handlers.insert(
        lower.to_vec(),
        NativeClassHandler {
            has_method,
            call: call_method,
            prop_added: None,
            prop_write: None,
            compare_check: None,
            prop_read: None,
            param_names: Some(param_names),
        },
    );
}

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    vm.phpinfo_hooks.push(minfo);

    for (name, _) in [
        ("ImagickException", 1),
        ("ImagickDrawException", 2),
        ("ImagickPixelIteratorException", 3),
        ("ImagickPixelException", 4),
        ("ImagickKernelException", 5),
    ] {
        let mut ce = ClassEntry::new(name.as_bytes().to_vec());
        ce.parent = Some(b"Exception".to_vec());
        vm.classes
            .insert(name.to_ascii_lowercase().into_bytes(), ce);
    }

    register_class(
        vm,
        C_IMAGICK,
        "Imagick",
        &[b"Stringable", b"Iterator", b"Countable"],
    );
    if let Some(ce) = vm.classes.get_mut(C_IMAGICK) {
        for &(n, v) in consts::IMAGICK_CONSTANTS {
            ce.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
        }
        ce.constants
            .insert(b"IMAGICK_EXTVER".to_vec(), str_val(VERSION));
    }
    register_class(vm, C_DRAW, "ImagickDraw", &[]);
    register_class(vm, C_PIXEL, "ImagickPixel", &[]);
    register_class(vm, C_ITER, "ImagickPixelIterator", &[b"Iterator"]);
    register_class(vm, C_KERNEL, "ImagickKernel", &[]);

    vm.object_create_handlers
        .insert(C_IMAGICK.to_vec(), create_imagick);
    vm.object_create_handlers
        .insert(C_PIXEL.to_vec(), create_pixel);
    vm.object_create_handlers
        .insert(C_ITER.to_vec(), pixel_iter::create);
    vm.object_create_handlers
        .insert(C_DRAW.to_vec(), draw::create);
    vm.object_create_handlers
        .insert(C_KERNEL.to_vec(), kernel::create);

    for &(k, v) in INI {
        vm.constants
            .entry(k.as_bytes().to_vec())
            .or_insert_with(|| str_val(v));
    }
}

/// PHP_MINFO(imagick)
fn minfo(vm: &mut Vm) -> String {
    let formats: Vec<&str> = goro_imagick::codec::FORMATS
        .iter()
        .map(|f| f.name)
        .collect();
    let mut s = format!(
        "\nimagick\n\nimagick module => enabled\nimagick module version => {}\nimagick classes => Imagick, ImagickDraw, ImagickPixel, ImagickPixelIterator, ImagickKernel\nImagick compiled with ImageMagick version => {}\nImagick using ImageMagick library version => {}\nImageMagick copyright => (C) 1999 ImageMagick Studio LLC\nImageMagick release date => {}\nImageMagick number of supported formats:  => {}\nImageMagick supported formats => {}\n\nDirective => Local Value => Master Value\n",
        VERSION,
        goro_imagick::VERSION_STRING,
        goro_imagick::VERSION_STRING,
        goro_imagick::RELEASE_DATE,
        formats.len(),
        formats.join(", ")
    );
    for &(k, d) in INI {
        let v = vm
            .constants
            .get(k.as_bytes())
            .map(|v| String::from_utf8_lossy(v.to_php_string().as_bytes()).into_owned())
            .unwrap_or_else(|| d.to_string());
        s.push_str(&format!("{} => {} => {}\n", k, v, d));
    }
    s
}

/// The VmError a native call returns after setting an exception.
#[allow(dead_code)]
pub(crate) fn err(vm: &Vm) -> VmError {
    VmError {
        message: String::new(),
        line: vm.current_line,
    }
}

#[allow(dead_code)]
pub(crate) fn php_object(o: &Obj) -> std::cell::Ref<'_, PhpObject> {
    o.borrow()
}
