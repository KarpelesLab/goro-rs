//! php-src's ext/gd (gd.c) on top of goro-gd, a port of the bundled libgd:
//! the GdImage and GdFont classes, the image*() functions, gd_info() and
//! the GD_* / IMG_* constants.

// C-port style: index loops and explicit range checks mirror gd.c.
#![allow(clippy::needless_range_loop, clippy::manual_range_contains)]

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, NativeData, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm, VmError};
use goro_gd::{Font, Image};

mod funcs;
mod io;
mod xform;
mod zpp;

use zpp::{Func, Img, Obj};

/// The extension name (`extension_loaded('gd')`).
pub const NAME: &[u8] = b"gd";

/// PHP_GD_VERSION_STRING of a bundled build
pub const PHP_GD_VERSION_STRING: &str = "bundled (2.1.0 compatible)";

pub type R = Result<Value, VmError>;

/// The image format support compiled in.
pub const HAVE_JPG: bool = false;
pub const HAVE_WEBP: bool = false;
pub const HAVE_AVIF: bool = false;
pub const HAVE_XPM: bool = true;
pub const HAVE_FREETYPE: bool = false;

// ---------------------------------------------------------------------------
// GdImage / GdFont objects

/// php_gd_image_object
pub struct GdImageData {
    pub img: Img,
}

/// php_gd_font_object
pub struct GdFontData {
    pub font: Rc<Font>,
}

fn h_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        goro_core::value::display_class_name(&src.borrow().class_name)
    );
    zpp::throw(vm, b"Error", &m);
}

/// zend_objects_not_comparable
fn h_compare(_a: &Value, _b: &Value) -> Option<i64> {
    Some(2)
}

static IMAGE_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(h_clone),
    compare: Some(h_compare),
    ..NativeObjectHandlers::EMPTY
};

static FONT_HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(h_clone),
    ..NativeObjectHandlers::EMPTY
};

/// The image of a GdImage object.
pub fn image_of(o: &Obj) -> Option<Img> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    let d = n.data.clone().downcast::<GdImageData>().ok()?;
    Some(d.img.clone())
}

/// The font of a GdFont object.
pub fn font_of(o: &Obj) -> Option<Rc<Font>> {
    let b = o.borrow();
    let n = b.native.as_ref()?;
    let d = n.data.clone().downcast::<GdFontData>().ok()?;
    Some(d.font.clone())
}

thread_local! {
    /// Set while the extension itself instantiates its classes.
    static CREATING: RefCell<bool> = const { RefCell::new(false) };
}

fn new_native_object(
    vm: &mut Vm,
    class: &[u8],
    data: Rc<dyn Any>,
    handlers: &'static NativeObjectHandlers,
) -> Value {
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(class.to_vec(), id);
    o.native = Some(NativeData { data, handlers });
    let o = Rc::new(RefCell::new(o));
    goro_core::gc::track_object(&o);
    CREATING.with(|c| *c.borrow_mut() = true);
    vm.run_object_create_hook(&o);
    CREATING.with(|c| *c.borrow_mut() = false);
    Value::Object(o)
}

/// php_gd_assign_libgdimageptr_as_extgdimage()
pub fn wrap_image(vm: &mut Vm, im: Image) -> Value {
    let d = Rc::new(GdImageData {
        img: Rc::new(RefCell::new(im)),
    });
    new_native_object(vm, b"GdImage", d, &IMAGE_HANDLERS)
}

pub fn wrap_font(vm: &mut Vm, font: Font) -> Value {
    let d = Rc::new(GdFontData {
        font: Rc::new(font),
    });
    new_native_object(vm, b"GdFont", d, &FONT_HANDLERS)
}

/// get_constructor(): the classes cannot be instantiated with `new`.
fn create_object(vm: &mut Vm, o: &Obj) {
    if CREATING.with(|c| *c.borrow()) {
        return;
    }
    let class = goro_core::value::display_class_name(&o.borrow().class_name).to_string();
    let m = format!(
        "You cannot initialize a {} object except through helper functions",
        class
    );
    zpp::throw(vm, b"Error", &m);
}

// ---------------------------------------------------------------------------
// libgd diagnostics

/// php_gd_error_method(): report what libgd queued during the last call.
pub fn flush_errors(vm: &mut Vm, f: &Func) {
    for (prio, msg) in goro_gd::take_errors() {
        let line = vm.current_line;
        let m = format!("{}(): {}", f.name, msg);
        match prio {
            goro_gd::GD_WARNING => vm.emit_warning_at(&m, line),
            goro_gd::GD_NOTICE | goro_gd::GD_INFO | goro_gd::GD_DEBUG => {
                vm.emit_notice_at(&m, line)
            }
            _ => vm.emit_warning_at(&m, line),
        }
    }
}

// ---------------------------------------------------------------------------
// registration

fn long_consts() -> Vec<(&'static str, i64)> {
    use goro_gd::*;
    let mut v = vec![
        ("IMG_AVIF", 256),
        ("IMG_GIF", 1),
        ("IMG_JPG", 2),
        ("IMG_JPEG", 2),
        ("IMG_PNG", 4),
        ("IMG_WBMP", 8),
        ("IMG_XPM", 16),
        ("IMG_WEBP", 32),
        ("IMG_BMP", 64),
        ("IMG_TGA", 128),
        ("IMG_WEBP_LOSSLESS", 101),
        ("IMG_COLOR_TILED", TILED as i64),
        ("IMG_COLOR_STYLED", STYLED as i64),
        ("IMG_COLOR_BRUSHED", BRUSHED as i64),
        ("IMG_COLOR_STYLEDBRUSHED", STYLED_BRUSHED as i64),
        ("IMG_COLOR_TRANSPARENT", TRANSPARENT as i64),
        ("IMG_ARC_ROUNDED", ARC as i64),
        ("IMG_ARC_PIE", PIE as i64),
        ("IMG_ARC_CHORD", CHORD as i64),
        ("IMG_ARC_NOFILL", NO_FILL as i64),
        ("IMG_ARC_EDGED", EDGED as i64),
        ("IMG_GD2_RAW", GD2_FMT_RAW as i64),
        ("IMG_GD2_COMPRESSED", GD2_FMT_COMPRESSED as i64),
        ("IMG_FLIP_HORIZONTAL", 1),
        ("IMG_FLIP_VERTICAL", 2),
        ("IMG_FLIP_BOTH", 3),
        ("IMG_EFFECT_REPLACE", EFFECT_REPLACE as i64),
        ("IMG_EFFECT_ALPHABLEND", EFFECT_ALPHA_BLEND as i64),
        ("IMG_EFFECT_NORMAL", EFFECT_NORMAL as i64),
        ("IMG_EFFECT_OVERLAY", EFFECT_OVERLAY as i64),
        ("IMG_EFFECT_MULTIPLY", EFFECT_MULTIPLY as i64),
        ("IMG_CROP_DEFAULT", crop::DEFAULT as i64),
        ("IMG_CROP_TRANSPARENT", crop::TRANSPARENT as i64),
        ("IMG_CROP_BLACK", crop::BLACK as i64),
        ("IMG_CROP_WHITE", crop::WHITE as i64),
        ("IMG_CROP_SIDES", crop::SIDES as i64),
        ("IMG_CROP_THRESHOLD", crop::THRESHOLD as i64),
        ("IMG_BELL", interp::BELL as i64),
        ("IMG_BESSEL", interp::BESSEL as i64),
        ("IMG_BILINEAR_FIXED", interp::BILINEAR_FIXED as i64),
        ("IMG_BICUBIC", interp::BICUBIC as i64),
        ("IMG_BICUBIC_FIXED", interp::BICUBIC_FIXED as i64),
        ("IMG_BLACKMAN", interp::BLACKMAN as i64),
        ("IMG_BOX", interp::BOX as i64),
        ("IMG_BSPLINE", interp::BSPLINE as i64),
        ("IMG_CATMULLROM", interp::CATMULLROM as i64),
        ("IMG_GAUSSIAN", interp::GAUSSIAN as i64),
        ("IMG_GENERALIZED_CUBIC", interp::GENERALIZED_CUBIC as i64),
        ("IMG_HERMITE", interp::HERMITE as i64),
        ("IMG_HAMMING", interp::HAMMING as i64),
        ("IMG_HANNING", interp::HANNING as i64),
        ("IMG_MITCHELL", interp::MITCHELL as i64),
        ("IMG_POWER", interp::POWER as i64),
        ("IMG_QUADRATIC", interp::QUADRATIC as i64),
        ("IMG_SINC", interp::SINC as i64),
        ("IMG_NEAREST_NEIGHBOUR", interp::NEAREST_NEIGHBOUR as i64),
        ("IMG_WEIGHTED4", interp::WEIGHTED4 as i64),
        ("IMG_TRIANGLE", interp::TRIANGLE as i64),
        ("IMG_AFFINE_TRANSLATE", 0),
        ("IMG_AFFINE_SCALE", 1),
        ("IMG_AFFINE_ROTATE", 2),
        ("IMG_AFFINE_SHEAR_HORIZONTAL", 3),
        ("IMG_AFFINE_SHEAR_VERTICAL", 4),
        ("GD_BUNDLED", 1),
        ("IMG_FILTER_NEGATE", 0),
        ("IMG_FILTER_GRAYSCALE", 1),
        ("IMG_FILTER_BRIGHTNESS", 2),
        ("IMG_FILTER_CONTRAST", 3),
        ("IMG_FILTER_COLORIZE", 4),
        ("IMG_FILTER_EDGEDETECT", 5),
        ("IMG_FILTER_EMBOSS", 6),
        ("IMG_FILTER_GAUSSIAN_BLUR", 7),
        ("IMG_FILTER_SELECTIVE_BLUR", 8),
        ("IMG_FILTER_MEAN_REMOVAL", 9),
        ("IMG_FILTER_SMOOTH", 10),
        ("IMG_FILTER_PIXELATE", 11),
        ("IMG_FILTER_SCATTER", 12),
        ("GD_MAJOR_VERSION", 2),
        ("GD_MINOR_VERSION", 0),
        ("GD_RELEASE_VERSION", 35),
    ];
    v.extend([
        ("PNG_NO_FILTER", 0x00),
        ("PNG_FILTER_NONE", 0x08),
        ("PNG_FILTER_SUB", 0x10),
        ("PNG_FILTER_UP", 0x20),
        ("PNG_FILTER_AVG", 0x40),
        ("PNG_FILTER_PAETH", 0x80),
        ("PNG_ALL_FILTERS", 0xf8),
    ]);
    v
}

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    for class in [&b"GdImage"[..], b"GdFont"] {
        let mut ce = ClassEntry::new(class.to_vec());
        ce.is_final = true;
        let lower = class.to_ascii_lowercase();
        vm.classes.insert(lower.clone(), ce);
        vm.object_create_handlers.insert(lower, create_object);
    }
    let mut names = Vec::new();
    for (k, v) in long_consts() {
        vm.constants.insert(k.as_bytes().to_vec(), Value::Long(v));
        names.push(k.as_bytes().to_vec());
    }
    for (k, v) in [("GD_VERSION", "2.0.35"), ("GD_EXTRA_VERSION", "")] {
        vm.constants.insert(
            k.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(v.as_bytes())),
        );
        names.push(k.as_bytes().to_vec());
    }
    vm.module_constants.push((NAME.to_vec(), names));
    vm.constants
        .entry(b"gd.jpeg_ignore_warning".to_vec())
        .or_insert_with(|| Value::String(PhpString::from_bytes(b"1")));
    funcs::register(vm);
    io::register(vm);
    xform::register(vm);
}

/// Register a function with its stub's parameter names.
pub fn register_fn(vm: &mut Vm, name: &str, f: goro_core::vm::BuiltinFn) {
    let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
        .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
        .unwrap_or_default();
    vm.register_function_with_params(name.as_bytes(), f, &params);
}

// ---------------------------------------------------------------------------
// value helpers

pub fn vstr(b: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(b.as_ref()))
}

pub fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

pub fn varr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

pub fn key(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

/// A packed array of longs
pub fn long_list(v: &[i64]) -> Value {
    let mut a = PhpArray::new();
    for &n in v {
        a.push(Value::Long(n));
    }
    varr(a)
}
