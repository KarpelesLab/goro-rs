//! The gd extension (PHP manual: GD and Image Functions): the GdImage and
//! GdFont classes, the image*() functions and the IMG_* constants. The
//! image model, drawing, filters, fonts and codecs live in the
//! goro-ext-gd crate; this module checks arguments with PHP's messages
//! and does the file, stream and output I/O.
//!
//! Behaviour sources: the PHP manual and the expected output of
//! ext/gd/tests/*.phpt.

use std::cell::RefCell;
use std::rc::Rc;

use goro_ext_gd::font::{self, Font, FontError};
use goro_ext_gd::formats::{self, DecodeError, DecodeResult, Kind};
use goro_ext_gd::image::{self as gi, Image, Shared};
use goro_ext_gd::transform as tr;
use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::{Class, NativeHooks, cflags};
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

macro_rules! sig {
    ($($s:literal => $f:ident),* $(,)?) => { &[$(($s, $f)),*] };
}

pub static MODULE: Module = Module {
    name: "gd",
    functions: sig![
        "function gd_info(): array" => gd_info,
        "function imageloadfont(string $filename): GdFont|false" => imageloadfont,
        "function imagesetstyle(GdImage $image, array $style): bool" => imagesetstyle,
        "function imagecreatetruecolor(int $width, int $height): GdImage|false" => imagecreatetruecolor,
        "function imageistruecolor(GdImage $image): bool" => imageistruecolor,
        "function imagetruecolortopalette(GdImage $image, bool $dither, int $num_colors): bool" => imagetruecolortopalette,
        "function imagepalettetotruecolor(GdImage $image): bool" => imagepalettetotruecolor,
        "function imagecolormatch(GdImage $image1, GdImage $image2): true" => imagecolormatch,
        "function imagesetthickness(GdImage $image, int $thickness): bool" => imagesetthickness,
        "function imagefilledellipse(GdImage $image, int $center_x, int $center_y, int $width, int $height, int $color): bool" => imagefilledellipse,
        "function imagefilledarc(GdImage $image, int $center_x, int $center_y, int $width, int $height, int $start_angle, int $end_angle, int $color, int $style): bool" => imagefilledarc,
        "function imagealphablending(GdImage $image, bool $enable): bool" => imagealphablending,
        "function imagesavealpha(GdImage $image, bool $enable): bool" => imagesavealpha,
        "function imagelayereffect(GdImage $image, int $effect): bool" => imagelayereffect,
        "function imagecolorallocatealpha(GdImage $image, int $red, int $green, int $blue, int $alpha): int|false" => imagecolorallocatealpha,
        "function imagecolorresolvealpha(GdImage $image, int $red, int $green, int $blue, int $alpha): int" => imagecolorresolvealpha,
        "function imagecolorclosestalpha(GdImage $image, int $red, int $green, int $blue, int $alpha): int" => imagecolorclosestalpha,
        "function imagecolorexactalpha(GdImage $image, int $red, int $green, int $blue, int $alpha): int" => imagecolorexactalpha,
        "function imagecopyresampled(GdImage $dst_image, GdImage $src_image, int $dst_x, int $dst_y, int $src_x, int $src_y, int $dst_width, int $dst_height, int $src_width, int $src_height): bool" => imagecopyresampled,
        "function imagerotate(GdImage $image, float $angle, int $background_color): GdImage|false" => imagerotate,
        "function imagesettile(GdImage $image, GdImage $tile): bool" => imagesettile,
        "function imagesetbrush(GdImage $image, GdImage $brush): bool" => imagesetbrush,
        "function imagecreate(int $width, int $height): GdImage|false" => imagecreate,
        "function imagetypes(): int" => imagetypes,
        "function imagecreatefromstring(string $data): GdImage|false" => imagecreatefromstring,
        "function imagecreatefromavif(string $filename): GdImage|false" => imagecreatefromavif,
        "function imagecreatefromgif(string $filename): GdImage|false" => imagecreatefromgif,
        "function imagecreatefromjpeg(string $filename): GdImage|false" => imagecreatefromjpeg,
        "function imagecreatefrompng(string $filename): GdImage|false" => imagecreatefrompng,
        "function imagecreatefromwebp(string $filename): GdImage|false" => imagecreatefromwebp,
        "function imagecreatefromxbm(string $filename): GdImage|false" => imagecreatefromxbm,
        "function imagecreatefromxpm(string $filename): GdImage|false" => imagecreatefromxpm,
        "function imagecreatefromwbmp(string $filename): GdImage|false" => imagecreatefromwbmp,
        "function imagecreatefromgd(string $filename): GdImage|false" => imagecreatefromgd,
        "function imagecreatefromgd2(string $filename): GdImage|false" => imagecreatefromgd2,
        "function imagecreatefromgd2part(string $filename, int $x, int $y, int $width, int $height): GdImage|false" => imagecreatefromgd2part,
        "function imagecreatefrombmp(string $filename): GdImage|false" => imagecreatefrombmp,
        "function imagecreatefromtga(string $filename): GdImage|false" => imagecreatefromtga,
        "function imagexbm(GdImage $image, ?string $filename, ?int $foreground_color = null): bool" => imagexbm,
        "function imageavif(GdImage $image, mixed $file = null, int $quality = -1, int $speed = -1): bool" => imageavif,
        "function imagegif(GdImage $image, mixed $file = null): bool" => imagegif,
        "function imagepng(GdImage $image, mixed $file = null, int $quality = -1, int $filters = -1): bool" => imagepng,
        "function imagewebp(GdImage $image, mixed $file = null, int $quality = -1): bool" => imagewebp,
        "function imagejpeg(GdImage $image, mixed $file = null, int $quality = -1): bool" => imagejpeg,
        "function imagewbmp(GdImage $image, mixed $file = null, ?int $foreground_color = null): bool" => imagewbmp,
        "function imagegd(GdImage $image, ?string $file = null): bool" => imagegd,
        "function imagegd2(GdImage $image, ?string $file = null, int $chunk_size = 128, int $mode = IMG_GD2_RAW): bool" => imagegd2,
        "function imagebmp(GdImage $image, mixed $file = null, bool $compressed = true): bool" => imagebmp,
        "function imagedestroy(GdImage $image): bool" => imagedestroy,
        "function imagecolorallocate(GdImage $image, int $red, int $green, int $blue): int|false" => imagecolorallocate,
        "function imagepalettecopy(GdImage $dst, GdImage $src): void" => imagepalettecopy,
        "function imagecolorat(GdImage $image, int $x, int $y): int|false" => imagecolorat,
        "function imagecolorclosest(GdImage $image, int $red, int $green, int $blue): int" => imagecolorclosest,
        "function imagecolorclosesthwb(GdImage $image, int $red, int $green, int $blue): int" => imagecolorclosesthwb,
        "function imagecolordeallocate(GdImage $image, int $color): true" => imagecolordeallocate,
        "function imagecolorresolve(GdImage $image, int $red, int $green, int $blue): int" => imagecolorresolve,
        "function imagecolorexact(GdImage $image, int $red, int $green, int $blue): int" => imagecolorexact,
        "function imagecolorset(GdImage $image, int $color, int $red, int $green, int $blue, int $alpha = 0): ?false" => imagecolorset,
        "function imagecolorsforindex(GdImage $image, int $color): array" => imagecolorsforindex,
        "function imagegammacorrect(GdImage $image, float $input_gamma, float $output_gamma): true" => imagegammacorrect,
        "function imagesetpixel(GdImage $image, int $x, int $y, int $color): true" => imagesetpixel,
        "function imageline(GdImage $image, int $x1, int $y1, int $x2, int $y2, int $color): true" => imageline,
        "function imagedashedline(GdImage $image, int $x1, int $y1, int $x2, int $y2, int $color): true" => imagedashedline,
        "function imagerectangle(GdImage $image, int $x1, int $y1, int $x2, int $y2, int $color): true" => imagerectangle,
        "function imagefilledrectangle(GdImage $image, int $x1, int $y1, int $x2, int $y2, int $color): true" => imagefilledrectangle,
        "function imagearc(GdImage $image, int $center_x, int $center_y, int $width, int $height, int $start_angle, int $end_angle, int $color): true" => imagearc,
        "function imageellipse(GdImage $image, int $center_x, int $center_y, int $width, int $height, int $color): true" => imageellipse,
        "function imagefilltoborder(GdImage $image, int $x, int $y, int $border_color, int $color): true" => imagefilltoborder,
        "function imagefill(GdImage $image, int $x, int $y, int $color): true" => imagefill,
        "function imagecolorstotal(GdImage $image): int" => imagecolorstotal,
        "function imagecolortransparent(GdImage $image, ?int $color = null): int" => imagecolortransparent,
        "function imageinterlace(GdImage $image, ?bool $enable = null): bool" => imageinterlace,
        "function imagepolygon(GdImage $image, array $points, int $num_points_or_color, ?int $color = null): true" => imagepolygon,
        "function imageopenpolygon(GdImage $image, array $points, int $num_points_or_color, ?int $color = null): true" => imageopenpolygon,
        "function imagefilledpolygon(GdImage $image, array $points, int $num_points_or_color, ?int $color = null): true" => imagefilledpolygon,
        "function imagefontwidth(GdFont|int $font): int" => imagefontwidth,
        "function imagefontheight(GdFont|int $font): int" => imagefontheight,
        "function imagechar(GdImage $image, GdFont|int $font, int $x, int $y, string $char, int $color): true" => imagechar,
        "function imagecharup(GdImage $image, GdFont|int $font, int $x, int $y, string $char, int $color): true" => imagecharup,
        "function imagestring(GdImage $image, GdFont|int $font, int $x, int $y, string $string, int $color): true" => imagestring,
        "function imagestringup(GdImage $image, GdFont|int $font, int $x, int $y, string $string, int $color): true" => imagestringup,
        "function imagecopy(GdImage $dst_image, GdImage $src_image, int $dst_x, int $dst_y, int $src_x, int $src_y, int $src_width, int $src_height): true" => imagecopy,
        "function imagecopymerge(GdImage $dst_image, GdImage $src_image, int $dst_x, int $dst_y, int $src_x, int $src_y, int $src_width, int $src_height, int $pct): true" => imagecopymerge,
        "function imagecopymergegray(GdImage $dst_image, GdImage $src_image, int $dst_x, int $dst_y, int $src_x, int $src_y, int $src_width, int $src_height, int $pct): true" => imagecopymergegray,
        "function imagecopyresized(GdImage $dst_image, GdImage $src_image, int $dst_x, int $dst_y, int $src_x, int $src_y, int $dst_width, int $dst_height, int $src_width, int $src_height): true" => imagecopyresized,
        "function imagesx(GdImage $image): int" => imagesx,
        "function imagesy(GdImage $image): int" => imagesy,
        "function imagesetclip(GdImage $image, int $x1, int $y1, int $x2, int $y2): true" => imagesetclip,
        "function imagegetclip(GdImage $image): array" => imagegetclip,
        "function imageftbbox(float $size, float $angle, string $font_filename, string $string, array $options = []): array|false" => imageftbbox,
        "function imagefttext(GdImage $image, float $size, float $angle, int $x, int $y, int $color, string $font_filename, string $text, array $options = []): array|false" => imagefttext,
        "function imagettfbbox(float $size, float $angle, string $font_filename, string $string, array $options = []): array|false" => imageftbbox,
        "function imagettftext(GdImage $image, float $size, float $angle, int $x, int $y, int $color, string $font_filename, string $text, array $options = []): array|false" => imagefttext,
        "function imagefilter(GdImage $image, mixed ...$args): bool" => imagefilter,
        "function imageconvolution(GdImage $image, array $matrix, float $divisor, float $offset): bool" => imageconvolution,
        "function imageflip(GdImage $image, int $mode): true" => imageflip,
        "function imageantialias(GdImage $image, bool $enable): true" => imageantialias,
        "function imagecrop(GdImage $image, array $rectangle): GdImage|false" => imagecrop,
        "function imagecropauto(GdImage $image, int $mode = IMG_CROP_DEFAULT, float $threshold = 0.5, int $color = -1): GdImage|false" => imagecropauto,
        "function imagescale(GdImage $image, int $width, int $height = -1, int $mode = IMG_BILINEAR_FIXED): GdImage|false" => imagescale,
        "function imageaffine(GdImage $image, array $affine, ?array $clip = null): GdImage|false" => imageaffine,
        "function imageaffinematrixget(int $type, mixed $options): array|false" => imageaffinematrixget,
        "function imageaffinematrixconcat(array $matrix1, array $matrix2): array|false" => imageaffinematrixconcat,
        "function imagegetinterpolation(GdImage $image): int" => imagegetinterpolation,
        "function imagesetinterpolation(GdImage $image, int $method = IMG_BILINEAR_FIXED): bool" => imagesetinterpolation,
        "function imageresolution(GdImage $image, ?int $resolution_x = null, ?int $resolution_y = null): array|true" => imageresolution,
    ],
    classes: &[
        ("gdimage", "<?php final class GdImage {}"),
        ("gdfont", "<?php final class GdFont {}"),
    ],
    hooks: &[("gdimage", gd_hooks), ("gdfont", gd_hooks)],
    constants: CONSTANTS,
    ini: &[("gd.jpeg_ignore_warning", "1")],
    ..Module::EMPTY
};

const CONSTANTS: &[(&str, ConstVal)] = &[
    ("IMG_AVIF", ConstVal::Int(256)),
    ("IMG_GIF", ConstVal::Int(1)),
    ("IMG_JPG", ConstVal::Int(2)),
    ("IMG_JPEG", ConstVal::Int(2)),
    ("IMG_PNG", ConstVal::Int(4)),
    ("IMG_WBMP", ConstVal::Int(8)),
    ("IMG_XPM", ConstVal::Int(16)),
    ("IMG_WEBP", ConstVal::Int(32)),
    ("IMG_BMP", ConstVal::Int(64)),
    ("IMG_TGA", ConstVal::Int(128)),
    ("IMG_WEBP_LOSSLESS", ConstVal::Int(101)),
    ("IMG_COLOR_TILED", ConstVal::Int(-5)),
    ("IMG_COLOR_STYLED", ConstVal::Int(-2)),
    ("IMG_COLOR_BRUSHED", ConstVal::Int(-3)),
    ("IMG_COLOR_STYLEDBRUSHED", ConstVal::Int(-4)),
    ("IMG_COLOR_TRANSPARENT", ConstVal::Int(-6)),
    ("IMG_ARC_ROUNDED", ConstVal::Int(0)),
    ("IMG_ARC_PIE", ConstVal::Int(0)),
    ("IMG_ARC_CHORD", ConstVal::Int(1)),
    ("IMG_ARC_NOFILL", ConstVal::Int(2)),
    ("IMG_ARC_EDGED", ConstVal::Int(4)),
    ("IMG_GD2_RAW", ConstVal::Int(1)),
    ("IMG_GD2_COMPRESSED", ConstVal::Int(2)),
    ("IMG_FLIP_HORIZONTAL", ConstVal::Int(1)),
    ("IMG_FLIP_VERTICAL", ConstVal::Int(2)),
    ("IMG_FLIP_BOTH", ConstVal::Int(3)),
    ("IMG_EFFECT_REPLACE", ConstVal::Int(0)),
    ("IMG_EFFECT_ALPHABLEND", ConstVal::Int(1)),
    ("IMG_EFFECT_NORMAL", ConstVal::Int(2)),
    ("IMG_EFFECT_OVERLAY", ConstVal::Int(3)),
    ("IMG_EFFECT_MULTIPLY", ConstVal::Int(4)),
    ("IMG_CROP_DEFAULT", ConstVal::Int(0)),
    ("IMG_CROP_TRANSPARENT", ConstVal::Int(1)),
    ("IMG_CROP_BLACK", ConstVal::Int(2)),
    ("IMG_CROP_WHITE", ConstVal::Int(3)),
    ("IMG_CROP_SIDES", ConstVal::Int(4)),
    ("IMG_CROP_THRESHOLD", ConstVal::Int(5)),
    ("IMG_BELL", ConstVal::Int(1)),
    ("IMG_BESSEL", ConstVal::Int(2)),
    ("IMG_BILINEAR_FIXED", ConstVal::Int(3)),
    ("IMG_BICUBIC", ConstVal::Int(4)),
    ("IMG_BICUBIC_FIXED", ConstVal::Int(5)),
    ("IMG_BLACKMAN", ConstVal::Int(6)),
    ("IMG_BOX", ConstVal::Int(7)),
    ("IMG_BSPLINE", ConstVal::Int(8)),
    ("IMG_CATMULLROM", ConstVal::Int(9)),
    ("IMG_GAUSSIAN", ConstVal::Int(10)),
    ("IMG_GENERALIZED_CUBIC", ConstVal::Int(11)),
    ("IMG_HERMITE", ConstVal::Int(12)),
    ("IMG_HAMMING", ConstVal::Int(13)),
    ("IMG_HANNING", ConstVal::Int(14)),
    ("IMG_MITCHELL", ConstVal::Int(15)),
    ("IMG_POWER", ConstVal::Int(17)),
    ("IMG_QUADRATIC", ConstVal::Int(18)),
    ("IMG_SINC", ConstVal::Int(19)),
    ("IMG_NEAREST_NEIGHBOUR", ConstVal::Int(16)),
    ("IMG_WEIGHTED4", ConstVal::Int(21)),
    ("IMG_TRIANGLE", ConstVal::Int(20)),
    ("IMG_AFFINE_TRANSLATE", ConstVal::Int(0)),
    ("IMG_AFFINE_SCALE", ConstVal::Int(1)),
    ("IMG_AFFINE_ROTATE", ConstVal::Int(2)),
    ("IMG_AFFINE_SHEAR_HORIZONTAL", ConstVal::Int(3)),
    ("IMG_AFFINE_SHEAR_VERTICAL", ConstVal::Int(4)),
    ("GD_BUNDLED", ConstVal::Int(1)),
    ("IMG_FILTER_NEGATE", ConstVal::Int(0)),
    ("IMG_FILTER_GRAYSCALE", ConstVal::Int(1)),
    ("IMG_FILTER_BRIGHTNESS", ConstVal::Int(2)),
    ("IMG_FILTER_CONTRAST", ConstVal::Int(3)),
    ("IMG_FILTER_COLORIZE", ConstVal::Int(4)),
    ("IMG_FILTER_EDGEDETECT", ConstVal::Int(5)),
    ("IMG_FILTER_EMBOSS", ConstVal::Int(6)),
    ("IMG_FILTER_GAUSSIAN_BLUR", ConstVal::Int(7)),
    ("IMG_FILTER_SELECTIVE_BLUR", ConstVal::Int(8)),
    ("IMG_FILTER_MEAN_REMOVAL", ConstVal::Int(9)),
    ("IMG_FILTER_SMOOTH", ConstVal::Int(10)),
    ("IMG_FILTER_PIXELATE", ConstVal::Int(11)),
    ("IMG_FILTER_SCATTER", ConstVal::Int(12)),
    ("GD_VERSION", ConstVal::Str("2.1.0-compat")),
    ("GD_MAJOR_VERSION", ConstVal::Int(2)),
    ("GD_MINOR_VERSION", ConstVal::Int(1)),
    ("GD_RELEASE_VERSION", ConstVal::Int(0)),
    ("GD_EXTRA_VERSION", ConstVal::Str("compat")),
    ("PNG_NO_FILTER", ConstVal::Int(0)),
    ("PNG_FILTER_NONE", ConstVal::Int(8)),
    ("PNG_FILTER_SUB", ConstVal::Int(16)),
    ("PNG_FILTER_UP", ConstVal::Int(32)),
    ("PNG_FILTER_AVG", ConstVal::Int(64)),
    ("PNG_FILTER_PAETH", ConstVal::Int(128)),
    ("PNG_ALL_FILTERS", ConstVal::Int(248)),
];

// ---------------------------------------------------------------- objects

struct GdObj(Shared);
impl goro_runtime::NativeData for GdObj {}
struct FontObj(Rc<Font>);
impl goro_runtime::NativeData for FontObj {}

/// GdImage and GdFont objects can't be cloned or serialized (tests
/// gdimage_prevent_cloning, gdimage_prevent_serialization).
fn gd_hooks() -> NativeHooks {
    NativeHooks {
        create: Some(|vm, o| mark(&vm.class_of(o))),
        ..Default::default()
    }
}

fn mark(cls: &Class) {
    cls.flags.set(
        cls.flags.get() | cflags::NO_DYNAMIC | cflags::NOT_SERIALIZABLE | cflags::NOT_CLONEABLE,
    );
}

fn class(vm: &mut Vm, name: &str) -> R<Rc<Class>> {
    match vm.find_class(name.as_bytes(), false)? {
        Some(c) => {
            mark(&c);
            Ok(c)
        }
        None => Err(vm.fatal(&format!("Class \"{name}\" not found"))),
    }
}

fn new_image(vm: &mut Vm, im: Image) -> R<Value> {
    let cls = class(vm, "GdImage")?;
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(GdObj(Rc::new(RefCell::new(im)))));
    Ok(Value::Object(o))
}

fn shared_of(o: &ObjRef) -> Option<Shared> {
    let n = o.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<GdObj>().map(|g| g.0.clone())
}

/// The image of a `GdImage` parameter (the engine checked the type).
fn img(vm: &mut Vm, a: &Args, i: usize) -> R<Shared> {
    match a.val(i) {
        Value::Object(o) => match shared_of(&o) {
            Some(s) => Ok(s),
            None => vm.throw("Error", "GdImage object has been destroyed"),
        },
        _ => vm.throw("Error", "GdImage expected"),
    }
}

#[inline]
fn int(a: &Args, i: usize) -> i32 {
    a.int(i) as i32
}

fn font_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Rc<Font>> {
    match a.val(i) {
        Value::Object(o) => {
            let n = o.obj().native.borrow();
            if let Some(b) = n.as_ref() {
                let any: &dyn std::any::Any = &**b;
                if let Some(f) = any.downcast_ref::<FontObj>() {
                    return Ok(f.0.clone());
                }
            }
            drop(n);
            vm.throw("Error", "Invalid GdFont")
        }
        v => {
            let n = match v {
                Value::Int(n) => n,
                other => vm.to_int(&other)?,
            };
            Ok(Rc::new(font::builtin_font(n).clone()))
        }
    }
}

fn true_() -> R<Value> {
    Ok(Value::Bool(true))
}

// ---------------------------------------------------------------- info

fn gd_info(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let mut out = PhpArray::new();
    let mut put = |k: &str, v: Value| {
        out.insert(Key::from(k), v);
    };
    put("GD Version", Value::from("bundled (2.1.0 compatible)"));
    put("FreeType Support", Value::Bool(true));
    put("FreeType Linkage", Value::from("with freetype"));
    put("GIF Read Support", Value::Bool(true));
    put("GIF Create Support", Value::Bool(true));
    put("JPEG Support", Value::Bool(true));
    put("PNG Support", Value::Bool(true));
    put("WBMP Support", Value::Bool(true));
    put("XPM Support", Value::Bool(true));
    put("XBM Support", Value::Bool(true));
    put("WebP Support", Value::Bool(true));
    put("BMP Support", Value::Bool(true));
    put("AVIF Support", Value::Bool(true));
    put("TGA Read Support", Value::Bool(true));
    put("JIS-mapped Japanese Font Support", Value::Bool(false));
    Ok(Value::Array(out))
}

fn imagetypes(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Int(1 | 2 | 4 | 8 | 16 | 32 | 64 | 128 | 256))
}

// ---------------------------------------------------------------- creation

fn check_dims(vm: &mut Vm, a: &Args) -> R<Option<(i32, i32)>> {
    let w = a.int(0);
    let h = a.int(1);
    if w <= 0 || w > i32::MAX as i64 {
        return vm
            .fthrow(
                a,
                "ValueError",
                "Argument #1 ($width) must be greater than 0",
            )
            .map(|_: ()| None);
    }
    if h <= 0 || h > i32::MAX as i64 {
        return vm
            .fthrow(
                a,
                "ValueError",
                "Argument #2 ($height) must be greater than 0",
            )
            .map(|_: ()| None);
    }
    Ok(Some((w as i32, h as i32)))
}

fn imagecreate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((w, h)) = check_dims(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match Image::new_palette(w, h) {
        Some(im) => new_image(vm, im),
        None => {
            vm.fwarn(a, formats::OVERFLOW_MSG)?;
            Ok(Value::Bool(false))
        }
    }
}

fn imagecreatetruecolor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((w, h)) = check_dims(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    match Image::new_truecolor(w, h) {
        Some(im) => new_image(vm, im),
        None => {
            vm.fwarn(a, formats::OVERFLOW_MSG)?;
            Ok(Value::Bool(false))
        }
    }
}

fn imageistruecolor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    Ok(Value::Bool(im.borrow().truecolor))
}

fn imagesx(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    Ok(Value::Int(im.borrow().sx as i64))
}

fn imagesy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    Ok(Value::Int(im.borrow().sy as i64))
}

fn imagedestroy(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    vm.deprecated(
        "Function imagedestroy() is deprecated since 8.5, as it has no effect since PHP 8.0",
    )?;
    true_()
}

// ---------------------------------------------------------------- reading

/// Reads a file argument (null bytes refused like PHP's path parameters).
fn read_path(vm: &mut Vm, a: &Args, path: &[u8]) -> R<Option<Vec<u8>>> {
    if path.contains(&0) {
        return vm
            .fthrow(
                a,
                "ValueError",
                "Argument #1 ($filename) must not contain any null bytes",
            )
            .map(|_: ()| None);
    }
    let Some(mut s) = super::file::open_stream(vm, a, path, b"rb", false)? else {
        return Ok(None);
    };
    let data = super::file::read_rest(vm, &mut s);
    super::streams::close_stream(vm, &mut s)?;
    Ok(Some(data))
}

/// Reports a decoding failure: the codec's warnings, then "... is not a
/// valid X file".
fn decode_failed(vm: &mut Vm, a: &Args, e: DecodeError, path: &[u8], kind: &str) -> R<Value> {
    for w in &e.warnings {
        vm.fwarn(a, w)?;
    }
    vm.fwarn(
        a,
        &format!(
            "\"{}\" is not a valid {} file",
            String::from_utf8_lossy(path),
            kind
        ),
    )?;
    Ok(Value::Bool(false))
}

fn create_from(vm: &mut Vm, a: &Args, kind: &str, dec: fn(&[u8]) -> DecodeResult) -> R<Value> {
    let path = a.str(0);
    let Some(data) = read_path(vm, a, &path)? else {
        return Ok(Value::Bool(false));
    };
    match dec(&data) {
        Ok(im) => new_image(vm, im),
        Err(e) => decode_failed(vm, a, e, &path, kind),
    }
}

fn imagecreatefrompng(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "PNG", formats::png::decode_gd)
}
fn imagecreatefromgif(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "GIF", |d| {
        formats::gif::decode(d).ok_or_else(DecodeError::silent)
    })
}
fn imagecreatefromjpeg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "JPEG", formats::jpeg::decode)
}
fn imagecreatefromwebp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "WEBP", formats::webp::decode)
}
fn imagecreatefromavif(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "AVIF", formats::avif::decode)
}
fn imagecreatefromxbm(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "XBM", formats::xbm::decode)
}
fn imagecreatefromxpm(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "XPM", formats::xpm::decode)
}
fn imagecreatefromwbmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "WBMP", formats::wbmp::decode)
}
fn imagecreatefromgd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "GD", formats::gd::decode_gd)
}
fn imagecreatefromgd2(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "GD2", |d| formats::gd::decode_gd2(d, None))
}
fn imagecreatefrombmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "BMP", formats::bmp::decode)
}
fn imagecreatefromtga(vm: &mut Vm, a: &mut Args) -> R<Value> {
    create_from(vm, a, "TGA", formats::tga::decode)
}

fn imagecreatefromgd2part(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (x, y, w, h) = (a.int(1), a.int(2), a.int(3), a.int(4));
    if w < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($width) must be greater than or equal to 1",
        );
    }
    if h < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #5 ($height) must be greater than or equal to 1",
        );
    }
    let path = a.str(0);
    let Some(data) = read_path(vm, a, &path)? else {
        return Ok(Value::Bool(false));
    };
    match formats::gd::decode_gd2(&data, Some((x as i32, y as i32, w as i32, h as i32))) {
        Ok(im) => new_image(vm, im),
        Err(e) => decode_failed(vm, a, e, &path, "GD2"),
    }
}

fn imagecreatefromstring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(0);
    let Some(kind) = formats::sniff(&data) else {
        vm.fwarn(a, "Data is not in a recognized format")?;
        return Ok(Value::Bool(false));
    };
    let r = match kind {
        Kind::Png => formats::png::decode_gd(&data),
        Kind::Jpeg => formats::jpeg::decode(&data),
        Kind::Gif => formats::gif::decode(&data).ok_or_else(DecodeError::silent),
        Kind::Gd2 => formats::gd::decode_gd2(&data, None),
        Kind::Gd => formats::gd::decode_gd(&data),
        Kind::Wbmp => formats::wbmp::decode(&data),
        Kind::Webp => formats::webp::decode(&data),
        Kind::Bmp => formats::bmp::decode(&data),
        Kind::Avif => formats::avif::decode(&data),
    };
    match r {
        Ok(im) => new_image(vm, im),
        Err(e) => {
            for w in &e.warnings {
                vm.fwarn(a, w)?;
            }
            if kind == Kind::Bmp {
                vm.fwarn(a, "Passed data is not in \"BMP\" format")?;
            }
            vm.fwarn(a, "Couldn't create GD Image Stream out of Data")?;
            Ok(Value::Bool(false))
        }
    }
}

// ---------------------------------------------------------------- writing

/// Where an image goes.
enum Sink {
    Output,
    Path(PhpStr),
    Stream(Value),
}

/// The `$file` argument of imagepng() and friends: null (output), a file
/// name or a stream resource.
fn sink_arg(vm: &mut Vm, a: &Args, i: usize) -> R<Option<Sink>> {
    match a.val(i) {
        Value::Null => Ok(Some(Sink::Output)),
        v @ Value::Resource(_) => Ok(Some(Sink::Stream(v))),
        Value::Str(s) => {
            if s.contains(&0) {
                return vm
                    .fthrow(
                        a,
                        "TypeError",
                        &format!("Argument #{} ($file) must not contain null bytes", i + 1),
                    )
                    .map(|_: ()| None);
            }
            Ok(Some(Sink::Path(s)))
        }
        v => {
            let given = crate::vm::Vm::type_name_given(&v);
            vm.fthrow(
                a,
                "TypeError",
                &format!(
                    "Argument #{} ($file) must be a file name or a stream resource, {} given",
                    i + 1,
                    given
                ),
            )
            .map(|_: ()| None)
        }
    }
}

/// A `?string $file` path argument (imagegd, imagegd2, imagexbm).
fn path_arg(vm: &mut Vm, a: &Args, i: usize, name: &str) -> R<Option<Sink>> {
    match a.val(i) {
        Value::Null => Ok(Some(Sink::Output)),
        Value::Str(s) => {
            if s.contains(&0) {
                return vm
                    .fthrow(
                        a,
                        "ValueError",
                        &format!(
                            "Argument #{} (${}) must not contain any null bytes",
                            i + 1,
                            name
                        ),
                    )
                    .map(|_: ()| None);
            }
            Ok(Some(Sink::Path(s)))
        }
        _ => Ok(Some(Sink::Output)),
    }
}

fn emit(vm: &mut Vm, a: &Args, sink: Sink, bytes: &[u8]) -> R<Value> {
    match sink {
        Sink::Output => {
            vm.echo(bytes);
            true_()
        }
        Sink::Path(p) => {
            let Some(mut s) = super::file::open_stream(vm, a, &p, b"wb", false)? else {
                return Ok(Value::Bool(false));
            };
            let r = s.write_all(vm, bytes);
            super::streams::close_stream(vm, &mut s)?;
            Ok(Value::Bool(r.is_ok()))
        }
        Sink::Stream(v) => {
            let mut vals = vec![v];
            let tmp = Args {
                this: None,
                vals: &mut vals,
                func: a.func,
                static_class: std::ptr::null(),
                named: None,
            };
            let ok = super::file::with_stream(vm, &tmp, 0, |vm, st| {
                Ok(st.write_all(vm, bytes).is_ok())
            })?;
            Ok(Value::Bool(ok))
        }
    }
}

fn imagepng(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let q = a.int_or(2, -1);
    if !(-1..=9).contains(&q) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($quality) must be between -1 and 9",
        );
    }
    let filters = a.int_or(3, -1) as i32;
    let bytes = {
        let im = im.borrow();
        if !im.truecolor && im.colors_total == 0 {
            drop(im);
            vm.fwarn(a, "gd-png error: no colors in palette")?;
            return Ok(Value::Bool(false));
        }
        formats::png::encode(&im, q as i32, filters)
    };
    emit(vm, a, sink, &bytes)
}

fn imagegif(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let bytes = {
        let im = im.borrow();
        let p = formats::gif::to_palette(&im);
        formats::gif::encode(&p)
    };
    emit(vm, a, sink, &bytes)
}

fn imagejpeg(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let q = a.int_or(2, -1);
    if !(-1..=100).contains(&q) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($quality) must be at between -1 and 100",
        );
    }
    let r = formats::jpeg::encode(&im.borrow(), q as i32);
    match r {
        Ok(b) => emit(vm, a, sink, &b),
        Err(e) => {
            vm.fwarn(
                a,
                &format!("gd-jpeg: JPEG library reports unrecoverable error: {e}"),
            )?;
            Ok(Value::Bool(false))
        }
    }
}

fn imagewebp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let q = a.int_or(2, -1);
    if q < -1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($quality) must be greater than or equal to -1",
        );
    }
    let r = formats::webp::encode(&im.borrow(), q.min(i32::MAX as i64) as i32);
    match r {
        Ok(b) => emit(vm, a, sink, &b),
        Err(e) => {
            vm.fwarn(a, &format!("gd-webp encoding failed: {e}"))?;
            Ok(Value::Bool(false))
        }
    }
}

fn imageavif(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let q = a.int_or(2, -1);
    if !(-1..=100).contains(&q) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($quality) must be between -1 and 100",
        );
    }
    let sp = a.int_or(3, -1);
    if !(-1..=10).contains(&sp) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($speed) must be between -1 and 10",
        );
    }
    let r = formats::avif::encode(&im.borrow(), q as i32, sp as i32);
    match r {
        Ok(b) => emit(vm, a, sink, &b),
        Err(e) => {
            vm.fwarn(a, &format!("avif error - could not encode image: {e}"))?;
            Ok(Value::Bool(false))
        }
    }
}

/// The default foreground of imagewbmp/imagexbm: black.
fn default_fg(im: &Image) -> i32 {
    if im.truecolor {
        return 0;
    }
    for i in 0..im.colors_total as usize {
        if !im.open[i] && im.red[i] == 0 && im.green[i] == 0 && im.blue[i] == 0 {
            return i as i32;
        }
    }
    0
}

fn imagewbmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let r = {
        let im = im.borrow();
        let fg = a
            .opt_int(2)
            .map(|v| v as i32)
            .unwrap_or_else(|| default_fg(&im));
        formats::wbmp::encode(&im, fg)
    };
    match r {
        Ok(b) => emit(vm, a, sink, &b),
        Err(e) => {
            vm.fwarn(a, &e)?;
            vm.fwarn(a, "Could not create WBMP")?;
            Ok(Value::Bool(false))
        }
    }
}

fn imagexbm(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = path_arg(vm, a, 1, "filename")? else {
        return Ok(Value::Bool(false));
    };
    let name = match &sink {
        Sink::Path(p) => {
            let base = super::file::basename_bytes(p, b"");
            let stem = match base.iter().position(|&c| c == b'.') {
                Some(i) => base[..i].to_vec(),
                None => base,
            };
            let s: String = String::from_utf8_lossy(&stem)
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            if s.is_empty() { "image".to_string() } else { s }
        }
        _ => "image".to_string(),
    };
    let bytes = {
        let im = im.borrow();
        let fg = a
            .opt_int(2)
            .map(|v| v as i32)
            .unwrap_or_else(|| default_fg(&im));
        formats::xbm::encode(&im, &name, fg)
    };
    emit(vm, a, sink, &bytes)
}

fn imagegd(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = path_arg(vm, a, 1, "file")? else {
        return Ok(Value::Bool(false));
    };
    let bytes = formats::gd::encode_gd(&im.borrow());
    emit(vm, a, sink, &bytes)
}

fn imagegd2(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = path_arg(vm, a, 1, "file")? else {
        return Ok(Value::Bool(false));
    };
    let cs = a.int_or(2, 128);
    let mode = a.int_or(3, 1);
    let bytes = formats::gd::encode_gd2(&im.borrow(), cs, mode);
    emit(vm, a, sink, &bytes)
}

fn imagebmp(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some(sink) = sink_arg(vm, a, 1)? else {
        return Ok(Value::Bool(false));
    };
    let compressed = a.bool_or(2, true);
    let bytes = formats::bmp::encode(&im.borrow(), compressed);
    emit(vm, a, sink, &bytes)
}

// ---------------------------------------------------------------- colors

fn check_range(vm: &mut Vm, a: &Args, i: usize, name: &str, max: i64) -> R<bool> {
    let v = a.int(i);
    if !(0..=max).contains(&v) {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!(
                "Argument #{} (${}) must be between 0 and {} (inclusive)",
                i + 1,
                name,
                max
            ),
        )?;
        return Ok(false);
    }
    Ok(true)
}

/// Reads red/green/blue (and alpha) from arguments `first..`.
fn rgba(vm: &mut Vm, a: &Args, first: usize, alpha: bool) -> R<(i32, i32, i32, i32)> {
    check_range(vm, a, first, "red", 255)?;
    check_range(vm, a, first + 1, "green", 255)?;
    check_range(vm, a, first + 2, "blue", 255)?;
    let al = if alpha {
        check_range(vm, a, first + 3, "alpha", 127)?;
        int(a, first + 3)
    } else {
        0
    };
    Ok((int(a, first), int(a, first + 1), int(a, first + 2), al))
}

fn imagecolorallocate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (r, g, b, al) = rgba(vm, a, 1, false)?;
    let c = im.borrow_mut().color_allocate(r, g, b, al);
    Ok(if c < 0 {
        Value::Bool(false)
    } else {
        Value::Int(c as i64)
    })
}

fn imagecolorallocatealpha(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (r, g, b, al) = rgba(vm, a, 1, true)?;
    let c = im.borrow_mut().color_allocate(r, g, b, al);
    Ok(if c < 0 {
        Value::Bool(false)
    } else {
        Value::Int(c as i64)
    })
}

macro_rules! color_query {
    ($name:ident, $alpha:expr, $m:ident) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> R<Value> {
            let im = img(vm, a, 0)?;
            let (r, g, b, al) = rgba(vm, a, 1, $alpha)?;
            let c = im.borrow_mut().$m(r, g, b, al);
            Ok(Value::Int(c as i64))
        }
    };
}
color_query!(imagecolorresolve, false, color_resolve);
color_query!(imagecolorresolvealpha, true, color_resolve);
color_query!(imagecolorclosestalpha, true, color_closest_mut);
color_query!(imagecolorclosest, false, color_closest_mut);
color_query!(imagecolorexact, false, color_exact_mut);
color_query!(imagecolorexactalpha, true, color_exact_mut);

trait ColorMut {
    fn color_closest_mut(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32;
    fn color_exact_mut(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32;
}
impl ColorMut for Image {
    fn color_closest_mut(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        self.color_closest(r, g, b, a)
    }
    fn color_exact_mut(&mut self, r: i32, g: i32, b: i32, a: i32) -> i32 {
        self.color_exact(r, g, b, a)
    }
}

fn imagecolorclosesthwb(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (r, g, b, _) = rgba(vm, a, 1, false)?;
    let c = im.borrow().color_closest_hwb(r, g, b);
    Ok(Value::Int(c as i64))
}

fn imagecolordeallocate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let c = a.int(1);
    let mut im = im.borrow_mut();
    if im.truecolor {
        return true_();
    }
    if c >= 0 && c < im.colors_total as i64 {
        im.color_deallocate(c as i32);
        return true_();
    }
    let total = im.colors_total;
    drop(im);
    vm.fthrow(
        a,
        "ValueError",
        &format!("Argument #2 ($color) must be between 0 and {total}"),
    )
}

fn imagecolorset(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (r, g, b, al) = {
        check_range(vm, a, 2, "red", 255)?;
        check_range(vm, a, 3, "green", 255)?;
        check_range(vm, a, 4, "blue", 255)?;
        if a.has(5) {
            check_range(vm, a, 5, "alpha", 127)?;
        }
        (
            int(a, 2),
            int(a, 3),
            int(a, 4),
            if a.has(5) { int(a, 5) } else { 0 },
        )
    };
    let c = a.int(1);
    let mut im = im.borrow_mut();
    if c >= 0 && c < im.colors_total as i64 {
        let i = c as usize;
        im.red[i] = r;
        im.green[i] = g;
        im.blue[i] = b;
        im.alpha[i] = al;
        return Ok(Value::Null);
    }
    Ok(Value::Bool(false))
}

fn imagecolorsforindex(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let c = a.int(1);
    let im = im.borrow();
    let (r, g, b, al) = if im.truecolor {
        let c = c as i32;
        (gi::tc_r(c), gi::tc_g(c), gi::tc_b(c), gi::tc_a(c))
    } else if c >= 0 && c < im.colors_total as i64 {
        let i = c as usize;
        (im.red[i], im.green[i], im.blue[i], im.alpha[i])
    } else {
        drop(im);
        return vm.fthrow(a, "ValueError", "Argument #2 ($color) is out of range");
    };
    let mut out = PhpArray::new();
    out.insert(Key::from("red"), Value::Int(r as i64));
    out.insert(Key::from("green"), Value::Int(g as i64));
    out.insert(Key::from("blue"), Value::Int(b as i64));
    out.insert(Key::from("alpha"), Value::Int(al as i64));
    Ok(Value::Array(out))
}

fn imagecolorat(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (x, y) = (a.int(1), a.int(2));
    let im = im.borrow();
    if x < 0 || y < 0 || x >= im.sx as i64 || y >= im.sy as i64 {
        let (sx, sy) = (im.sx, im.sy);
        drop(im);
        let _ = (sx, sy);
        vm.fnotice(a, &format!("{x},{y} is out of bounds"))?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(im.get_pixel(x as i32, y as i32) as i64))
}

fn imagecolorstotal(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let im = im.borrow();
    Ok(Value::Int(if im.truecolor {
        0
    } else {
        im.colors_total as i64
    }))
}

fn imagecolortransparent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mut im = im.borrow_mut();
    if let Some(c) = a.opt_int(1) {
        im.set_transparent(c as i32);
    }
    Ok(Value::Int(im.transparent as i64))
}

fn imagepalettecopy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let dst = img(vm, a, 0)?;
    let src = img(vm, a, 1)?;
    if Rc::ptr_eq(&dst, &src) {
        return Ok(Value::Null);
    }
    let src = src.borrow();
    let mut dst = dst.borrow_mut();
    if src.truecolor || dst.truecolor {
        return Ok(Value::Null);
    }
    dst.red = src.red;
    dst.green = src.green;
    dst.blue = src.blue;
    dst.alpha = src.alpha;
    dst.open = src.open;
    dst.colors_total = src.colors_total;
    dst.transparent = src.transparent;
    Ok(Value::Null)
}

fn imagecolormatch(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let i1 = img(vm, a, 0)?;
    let i2 = img(vm, a, 1)?;
    let (tc1, tc2, s1, s2, n2) = {
        let x = i1.borrow();
        let y = i2.borrow();
        (
            x.truecolor,
            y.truecolor,
            (x.sx, x.sy),
            (y.sx, y.sy),
            y.colors_total,
        )
    };
    if !tc1 {
        return vm.fthrow(a, "ValueError", "Argument #1 ($image1) must be TrueColor");
    }
    if tc2 {
        return vm.fthrow(a, "ValueError", "Argument #2 ($image2) must be Palette");
    }
    if s1 != s2 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($image2) must be the same size as argument #1 ($im1)",
        );
    }
    if n2 < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($image2) must have at least one color",
        );
    }
    let x = i1.borrow();
    let mut y = i2.borrow_mut();
    let mut sums = vec![[0i64; 5]; 256];
    for py in 0..x.sy {
        for px in 0..x.sx {
            let c = x.get_pixel(px, py);
            let i = y.get_pixel(px, py) as usize;
            sums[i][0] += gi::tc_r(c) as i64;
            sums[i][1] += gi::tc_g(c) as i64;
            sums[i][2] += gi::tc_b(c) as i64;
            sums[i][3] += gi::tc_a(c) as i64;
            sums[i][4] += 1;
        }
    }
    let total = y.colors_total as usize;
    for (i, sum) in sums.iter().enumerate().take(total) {
        let n = sum[4];
        if n > 0 {
            y.red[i] = (sum[0] / n) as i32;
            y.green[i] = (sum[1] / n) as i32;
            y.blue[i] = (sum[2] / n) as i32;
            y.alpha[i] = (sum[3] / n) as i32;
        }
    }
    true_()
}

fn imagetruecolortopalette(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let n = a.int(2);
    if n <= 0 || n >= i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($num_colors) must be greater than 0 and less than 2147483647",
        );
    }
    let dither = a.bool(1);
    goro_ext_gd::quant::truecolor_to_palette(&mut im.borrow_mut(), dither, n as usize);
    true_()
}

fn imagepalettetotruecolor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    goro_ext_gd::copy::palette_to_truecolor(&mut im.borrow_mut());
    true_()
}

fn imagegammacorrect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (i, o) = (a.float(1), a.float(2));
    if i.is_nan() || i == f64::INFINITY {
        return vm.fthrow(a, "ValueError", "Argument #2 ($input_gamma) must be finite");
    }
    if i <= 0.0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($input_gamma) must be greater than 0",
        );
    }
    if o.is_nan() || o == f64::INFINITY {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($output_gamma) must be finite",
        );
    }
    if o <= 0.0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #3 ($output_gamma) must be greater than 0",
        );
    }
    im.borrow_mut().gamma_correct(i, o);
    true_()
}

// ---------------------------------------------------------------- state

fn imagesetthickness(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut().thick = int(a, 1);
    true_()
}

fn imagealphablending(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut().alpha_blending = if a.bool(1) {
        gi::EFFECT_ALPHABLEND
    } else {
        gi::EFFECT_REPLACE
    };
    true_()
}

fn imagesavealpha(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut().save_alpha = a.bool(1);
    true_()
}

fn imagelayereffect(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut().alpha_blending = int(a, 1);
    true_()
}

fn imageantialias(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mut im = im.borrow_mut();
    if im.truecolor {
        im.aa = a.bool(1);
    }
    true_()
}

fn imageinterlace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mut im = im.borrow_mut();
    if a.has(1) && !matches!(a.val(1), Value::Null) {
        im.interlace = a.bool(1);
    }
    Ok(Value::Bool(im.interlace))
}

fn imagesetstyle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let arr = a.array(1);
    if arr.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #2 ($style) must not be empty");
    }
    let mut style = Vec::with_capacity(arr.len());
    for (_, v) in arr.iter() {
        let v = v.deref();
        style.push(vm.to_int(&v)? as i32);
    }
    let mut im = im.borrow_mut();
    im.style = style;
    im.style_pos = 0;
    true_()
}

fn imagesettile(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let t = img(vm, a, 1)?;
    im.borrow_mut().tile = Some(snapshot_if_same(&im, &t));
    true_()
}

fn imagesetbrush(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let b = img(vm, a, 1)?;
    im.borrow_mut().brush = Some(snapshot_if_same(&im, &b));
    true_()
}

/// A brush or tile that is the image itself is copied (the image can't
/// read itself while being drawn on).
fn snapshot_if_same(im: &Shared, other: &Shared) -> Shared {
    if Rc::ptr_eq(im, other) {
        Rc::new(RefCell::new(other.borrow().clone()))
    } else {
        other.clone()
    }
}

fn imagesetclip(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut()
        .set_clip(int(a, 1), int(a, 2), int(a, 3), int(a, 4));
    true_()
}

fn imagegetclip(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let im = im.borrow();
    Ok(Value::Array(PhpArray::from_values([
        Value::Int(im.cx1 as i64),
        Value::Int(im.cy1 as i64),
        Value::Int(im.cx2 as i64),
        Value::Int(im.cy2 as i64),
    ])))
}

fn imagegetinterpolation(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    Ok(Value::Int(im.borrow().interpolation as i64))
}

fn imagesetinterpolation(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let m = a.int_or(1, tr::IMG_BILINEAR_FIXED as i64);
    let m = if m == -1 {
        tr::IMG_BILINEAR_FIXED as i64
    } else {
        m
    };
    if !tr::valid_interpolation(m) {
        return Ok(Value::Bool(false));
    }
    im.borrow_mut().interpolation = m as i32;
    true_()
}

fn imageresolution(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let x = a.opt_int(1);
    let y = a.opt_int(2);
    if x.is_none() && y.is_none() {
        let im = im.borrow();
        return Ok(Value::Array(PhpArray::from_values([
            Value::Int(im.res_x as i64),
            Value::Int(im.res_y as i64),
        ])));
    }
    let max = i32::MAX as i64;
    if let Some(v) = x
        && !(0..=max).contains(&v)
    {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #2 ($resolution_x) must be between 0 and {max}"),
        );
    }
    if let Some(v) = y
        && !(0..=max).contains(&v)
    {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #3 ($resolution_y) must be between 0 and {max}"),
        );
    }
    let mut im = im.borrow_mut();
    let xv = x.unwrap_or(0);
    let yv = y.unwrap_or(xv);
    if xv > 0 {
        im.res_x = xv as u32;
    }
    if yv > 0 {
        im.res_y = yv as u32;
    }
    true_()
}

// ---------------------------------------------------------------- drawing

fn imagesetpixel(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let (x, y, c) = (a.int(1), a.int(2), int(a, 3));
    if x < i32::MIN as i64 || x > i32::MAX as i64 || y < i32::MIN as i64 || y > i32::MAX as i64 {
        return true_();
    }
    im.borrow_mut().set_pixel(x as i32, y as i32, c);
    true_()
}

fn imageline(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mut im = im.borrow_mut();
    let c = int(a, 5);
    if im.aa && c >= 0 {
        im.aa_color = c;
        im.line(int(a, 1), int(a, 2), int(a, 3), int(a, 4), gi::ANTIALIASED);
    } else {
        im.line(int(a, 1), int(a, 2), int(a, 3), int(a, 4), c);
    }
    true_()
}

fn imagedashedline(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut()
        .dashed_line(int(a, 1), int(a, 2), int(a, 3), int(a, 4), int(a, 5));
    true_()
}

fn imagerectangle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut()
        .rectangle(int(a, 1), int(a, 2), int(a, 3), int(a, 4), int(a, 5));
    true_()
}

fn imagefilledrectangle(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut()
        .filled_rectangle(int(a, 1), int(a, 2), int(a, 3), int(a, 4), int(a, 5));
    true_()
}

fn imagearc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mut im = im.borrow_mut();
    let c = int(a, 7);
    let c = if im.aa && c >= 0 {
        im.aa_color = c;
        gi::ANTIALIASED
    } else {
        c
    };
    im.arc(
        int(a, 1),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        c,
    );
    true_()
}

fn imageellipse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut()
        .ellipse(int(a, 1), int(a, 2), int(a, 3), int(a, 4), int(a, 5));
    true_()
}

fn imagefilledellipse(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let w = a.int(3);
    let h = a.int(4);
    let max = i32::MAX as i64;
    if !(0..=max).contains(&w) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #4 ($width) must be between 0 and {max}"),
        );
    }
    if !(0..=max).contains(&h) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #5 ($height) must be between 0 and {max}"),
        );
    }
    im.borrow_mut()
        .filled_ellipse(int(a, 1), int(a, 2), w as i32, h as i32, int(a, 5));
    true_()
}

fn imagefilledarc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mut im = im.borrow_mut();
    let c = int(a, 7);
    let c = if im.aa && c >= 0 {
        im.aa_color = c;
        gi::ANTIALIASED
    } else {
        c
    };
    im.filled_arc(
        int(a, 1),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        c,
        int(a, 8),
    );
    true_()
}

fn imagefill(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut().fill(int(a, 1), int(a, 2), int(a, 3));
    true_()
}

fn imagefilltoborder(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    im.borrow_mut()
        .fill_to_border(int(a, 1), int(a, 2), int(a, 3), int(a, 4));
    true_()
}

/// imagepolygon & co: the points and color, with PHP 8.1's deprecated
/// $num_points form.
fn polygon_args(vm: &mut Vm, a: &Args, min: usize) -> R<Option<(Vec<(i32, i32)>, i32)>> {
    let arr = a.array(1);
    let (npoints, color) = match a.opt_int(3) {
        Some(c) => {
            vm.fdeprecated(a, "Using the $num_points parameter is deprecated")?;
            (a.int(2), c as i32)
        }
        None => ((arr.len() / 2) as i64, int(a, 2)),
    };
    if npoints < min as i64 {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!("Argument #3 ($num_points_or_color) must be greater than or equal to {min}"),
        )?;
        return Ok(None);
    }
    if (arr.len() as i64) < npoints * 2 {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!("Argument #2 ($points) must have at least {npoints} points"),
        )?;
        return Ok(None);
    }
    let mut vals = Vec::with_capacity(arr.len());
    for (_, v) in arr.iter() {
        let v = v.deref();
        vals.push(vm.to_int(&v)? as i32);
    }
    let pts = (0..npoints as usize)
        .map(|i| (vals[2 * i], vals[2 * i + 1]))
        .collect();
    Ok(Some((pts, color)))
}

fn imagepolygon(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some((pts, c)) = polygon_args(vm, a, 3)? else {
        return true_();
    };
    let mut im = im.borrow_mut();
    let c = if im.aa && c >= 0 {
        im.aa_color = c;
        gi::ANTIALIASED
    } else {
        c
    };
    im.polygon(&pts, c, true);
    true_()
}

fn imageopenpolygon(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some((pts, c)) = polygon_args(vm, a, 2)? else {
        return true_();
    };
    let mut im = im.borrow_mut();
    let c = if im.aa && c >= 0 {
        im.aa_color = c;
        gi::ANTIALIASED
    } else {
        c
    };
    im.polygon(&pts, c, false);
    true_()
}

fn imagefilledpolygon(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let Some((pts, c)) = polygon_args(vm, a, 3)? else {
        return true_();
    };
    let mut im = im.borrow_mut();
    let c = if im.aa && c >= 0 {
        im.aa_color = c;
        gi::ANTIALIASED
    } else {
        c
    };
    im.filled_polygon(&pts, c);
    true_()
}

// ---------------------------------------------------------------- fonts

fn imageloadfont(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let path = a.str(0);
    let Some(data) = read_path(vm, a, &path)? else {
        return Ok(Value::Bool(false));
    };
    match font::load_gdf(&data) {
        Ok(f) => {
            let cls = class(vm, "GdFont")?;
            let o = vm.instantiate_raw(&cls)?;
            *o.obj().native.borrow_mut() = Some(Box::new(FontObj(Rc::new(f))));
            Ok(Value::Object(o))
        }
        Err(e) => {
            match e {
                FontError::HeaderEof => vm.fwarn(a, "End of file while reading header")?,
                FontError::Overflow => {
                    vm.fwarn(a, formats::OVERFLOW_MSG)?;
                    vm.fwarn(a, "Error reading font, invalid font header")?;
                }
                FontError::BadHeader => vm.fwarn(a, "Error reading font, invalid font header")?,
                FontError::DataEof => vm.fwarn(a, "End of file while reading body")?,
            }
            Ok(Value::Bool(false))
        }
    }
}

fn imagefontwidth(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = font_arg(vm, a, 0)?;
    Ok(Value::Int(f.w as i64))
}

fn imagefontheight(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = font_arg(vm, a, 0)?;
    Ok(Value::Int(f.h as i64))
}

fn text_args(vm: &mut Vm, a: &Args) -> R<(Shared, Rc<Font>, i32, i32, PhpStr, i32)> {
    let im = img(vm, a, 0)?;
    let f = font_arg(vm, a, 1)?;
    Ok((im, f, int(a, 2), int(a, 3), a.str(4), int(a, 5)))
}

fn imagechar(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (im, f, x, y, s, c) = text_args(vm, a)?;
    if let Some(&ch) = s.first() {
        im.borrow_mut().draw_char(&f, x, y, ch as i32, c);
    }
    true_()
}

fn imagecharup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (im, f, x, y, s, c) = text_args(vm, a)?;
    if let Some(&ch) = s.first() {
        im.borrow_mut().draw_char_up(&f, x, y, ch as i32, c);
    }
    true_()
}

fn imagestring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (im, f, x, y, s, c) = text_args(vm, a)?;
    im.borrow_mut().draw_string(&f, x, y, &s, c);
    true_()
}

fn imagestringup(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (im, f, x, y, s, c) = text_args(vm, a)?;
    im.borrow_mut().draw_string_up(&f, x, y, &s, c);
    true_()
}

// ---------------------------------------------------------------- TrueType

fn ft_options(vm: &mut Vm, a: &Args, i: usize) -> R<f64> {
    let opts = a.array(i);
    let mut ls = 1.0;
    if let Some(v) = opts.get_str(&PhpStr::from("linespacing")) {
        let v = v.deref();
        ls = vm.to_float(&v)?;
    }
    Ok(ls)
}

fn bbox_value(b: [i32; 8]) -> Value {
    Value::Array(PhpArray::from_values(
        b.iter().map(|&v| Value::Int(v as i64)),
    ))
}

fn check_size(vm: &mut Vm, a: &Args, size: f64) -> R<bool> {
    if size.is_nan() {
        vm.fthrow::<()>(a, "ValueError", "Argument #2 ($size) must be finite")?;
        return Ok(false);
    }
    if !(i32::MIN as f64..=i32::MAX as f64).contains(&size) {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!(
                "Argument #2 ($size) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        )?;
        return Ok(false);
    }
    Ok(true)
}

fn load_font_file(vm: &mut Vm, a: &Args, path: &[u8]) -> R<Option<Vec<u8>>> {
    let p = vm.fs_path(path);
    match std::fs::read(&p) {
        Ok(d) => Ok(Some(d)),
        Err(_) => {
            vm.fwarn(a, "Could not find/open font")?;
            Ok(None)
        }
    }
}

fn imageftbbox(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let size = a.float(0);
    if size.is_nan() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($size) must be finite");
    }
    let angle = a.float(1);
    let path = a.str(2);
    let text = a.str(3);
    let ls = ft_options(vm, a, 4)?;
    let Some(font) = load_font_file(vm, a, &path)? else {
        return Ok(Value::Bool(false));
    };
    let req = goro_ext_gd::ttf::TextReq {
        font: &font,
        size,
        angle,
        x: 0,
        y: 0,
        color: 0,
        text: &text,
        linespacing: ls,
    };
    match goro_ext_gd::ttf::text(None, &req) {
        Ok(b) => Ok(bbox_value(b)),
        Err(e) => {
            vm.fwarn(a, &e)?;
            Ok(Value::Bool(false))
        }
    }
}

fn imagefttext(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let size = a.float(1);
    if !check_size(vm, a, size)? {
        return Ok(Value::Bool(false));
    }
    let angle = a.float(2);
    let (x, y, color) = (int(a, 3), int(a, 4), int(a, 5));
    let path = a.str(6);
    let text = a.str(7);
    let ls = ft_options(vm, a, 8)?;
    let Some(font) = load_font_file(vm, a, &path)? else {
        return Ok(Value::Bool(false));
    };
    let req = goro_ext_gd::ttf::TextReq {
        font: &font,
        size,
        angle,
        x,
        y,
        color,
        text: &text,
        linespacing: ls,
    };
    let r = goro_ext_gd::ttf::text(Some(&mut im.borrow_mut()), &req);
    match r {
        Ok(b) => Ok(bbox_value(b)),
        Err(e) => {
            vm.fwarn(a, &e)?;
            Ok(Value::Bool(false))
        }
    }
}

// ---------------------------------------------------------------- copying

/// The source image, copied when it is the destination itself.
fn pair(vm: &mut Vm, a: &Args) -> R<(Shared, Shared)> {
    let d = img(vm, a, 0)?;
    let s = img(vm, a, 1)?;
    let s = if Rc::ptr_eq(&d, &s) {
        Rc::new(RefCell::new(s.borrow().clone()))
    } else {
        s
    };
    Ok((d, s))
}

fn imagecopy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (d, s) = pair(vm, a)?;
    goro_ext_gd::copy::copy(
        &mut d.borrow_mut(),
        &s.borrow(),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        int(a, 7),
    );
    true_()
}

fn imagecopymerge(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (d, s) = pair(vm, a)?;
    goro_ext_gd::copy::copy_merge(
        &mut d.borrow_mut(),
        &s.borrow(),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        int(a, 7),
        int(a, 8),
    );
    true_()
}

fn imagecopymergegray(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (d, s) = pair(vm, a)?;
    goro_ext_gd::copy::copy_merge_gray(
        &mut d.borrow_mut(),
        &s.borrow(),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        int(a, 7),
        int(a, 8),
    );
    true_()
}

fn check_copy_sizes(vm: &mut Vm, a: &Args) -> R<bool> {
    let names = ["dst_width", "dst_height", "src_width", "src_height"];
    for (k, n) in names.iter().enumerate() {
        if a.int(6 + k) <= 0 {
            vm.fthrow::<()>(
                a,
                "ValueError",
                &format!("Argument #{} (${}) must be greater than 0", 7 + k, n),
            )?;
            return Ok(false);
        }
    }
    Ok(true)
}

fn imagecopyresized(vm: &mut Vm, a: &mut Args) -> R<Value> {
    if !check_copy_sizes(vm, a)? {
        return Ok(Value::Bool(false));
    }
    let (d, s) = pair(vm, a)?;
    goro_ext_gd::copy::copy_resized(
        &mut d.borrow_mut(),
        &s.borrow(),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        int(a, 7),
        int(a, 8),
        int(a, 9),
    );
    true_()
}

fn imagecopyresampled(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (d, s) = pair(vm, a)?;
    goro_ext_gd::copy::copy_resampled(
        &mut d.borrow_mut(),
        &s.borrow(),
        int(a, 2),
        int(a, 3),
        int(a, 4),
        int(a, 5),
        int(a, 6),
        int(a, 7),
        int(a, 8),
        int(a, 9),
    );
    true_()
}

// ---------------------------------------------------------------- filters

fn imagefilter(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let n = a.len();
    if n < 2 {
        return vm.throw(
            "ArgumentCountError",
            "Wrong parameter count for imagefilter()",
        );
    }
    let f = vm.to_int(&a.val(1))?;
    if !(0..=12).contains(&f) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($filter) must be one of the IMG_FILTER_* filter constants",
        );
    }
    // Number of extra arguments each filter takes (required, max).
    let (req, max) = match f {
        2 | 3 | 10 => (1, 1),
        4 => (3, 4),
        11 => (1, 2),
        12 => (2, 3),
        _ => (0, 0),
    };
    let extra = n - 2;
    if extra < req || extra > max {
        return vm.throw(
            "ArgumentCountError",
            "Wrong parameter count for imagefilter()",
        );
    }
    let iv = |vm: &mut Vm, i: usize| -> R<i64> { vm.to_int(&a.val(i)) };
    let fv = |vm: &mut Vm, i: usize| -> R<f64> { vm.to_float(&a.val(i)) };
    let mut im = im.borrow_mut();
    let ok = match f {
        0 => {
            im.filter_negate();
            true
        }
        1 => {
            im.filter_grayscale();
            true
        }
        2 => {
            let v = iv(vm, 2)?;
            im.filter_brightness(v.clamp(-255, 255) as i32);
            true
        }
        3 => {
            let v = fv(vm, 2)?;
            im.filter_contrast(v);
            true
        }
        4 => {
            let r = iv(vm, 2)?.clamp(-255, 255) as i32;
            let g = iv(vm, 3)?.clamp(-255, 255) as i32;
            let b = iv(vm, 4)?.clamp(-255, 255) as i32;
            let al = if n > 5 {
                iv(vm, 5)?.clamp(-127, 127) as i32
            } else {
                0
            };
            im.filter_colorize(r, g, b, al);
            true
        }
        5 => im.filter_edge_detect(),
        6 => im.filter_emboss(),
        7 => im.filter_gaussian_blur(),
        8 => im.filter_selective_blur(),
        9 => im.filter_mean_removal(),
        10 => {
            let w = fv(vm, 2)?;
            im.filter_smooth(w)
        }
        11 => {
            let size = iv(vm, 2)?;
            let adv = if n > 3 {
                let v = a.val(3);
                vm.to_bool(&v)
            } else {
                false
            };
            im.filter_pixelate(size as i32, adv)
        }
        _ => {
            let sub = iv(vm, 2)?;
            let plus = iv(vm, 3)?;
            let max = i32::MAX as i64;
            if !(0..=max).contains(&sub) {
                drop(im);
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!("Argument #3 must be between 0 and {max}"),
                );
            }
            if !(0..=max).contains(&plus) {
                drop(im);
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!("Argument #4 must be between 0 and {max}"),
                );
            }
            let mut colors = Vec::new();
            if n > 4
                && let Value::Array(arr) = a.val(4)
            {
                for (_, v) in arr.iter() {
                    let v = v.deref();
                    colors.push(vm.to_int(&v)? as i32);
                }
            }
            // Scatter offsets are random (PHP manual); a local xorshift
            // seeded from the clock is enough.
            let mut state = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9e37_79b9_7f4a_7c15)
                | 1;
            let mut rng = move || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 32) as u32
            };
            im.filter_scatter(sub as i32, plus as i32, &colors, &mut rng)
        }
    };
    Ok(Value::Bool(ok))
}

fn imageconvolution(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let arr = a.array(1);
    if arr.len() != 3 {
        return vm.fthrow(a, "ValueError", "Argument #2 ($matrix) must be a 3x3 array");
    }
    let mut m = [[0.0f64; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        let r = match arr.get_int(i as i64).map(|v| v.deref()) {
            Some(Value::Array(r)) if r.len() == 3 => r,
            Some(Value::Array(r)) => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #2 ($matrix) must be a 3x3 array, matrix[{}] only has {} elements",
                        i,
                        r.len()
                    ),
                );
            }
            _ => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    &format!(
                        "Argument #2 ($matrix) must be a 3x3 array, matrix[{i}] is not an array"
                    ),
                );
            }
        };
        for (j, cell) in row.iter_mut().enumerate() {
            match r.get_int(j as i64).map(|v| v.deref()) {
                Some(v) => *cell = vm.to_float(&v)?,
                None => {
                    return vm.fthrow(
                        a,
                        "ValueError",
                        &format!(
                            "Argument #2 ($matrix) must be a 3x3 array, matrix[{i}][{j}] cannot be found (missing integer key)"
                        ),
                    );
                }
            }
        }
    }
    let div = a.float(2);
    let off = a.float(3);
    if !div.is_finite() {
        return vm.fthrow(a, "ValueError", "Argument #3 ($divisor) must be finite");
    }
    if div.abs() < f64::from(f32::EPSILON) {
        return vm.fthrow(a, "ValueError", "Argument #3 ($divisor) must not be 0");
    }
    if !off.is_finite() {
        return vm.fthrow(a, "ValueError", "Argument #4 ($offset) must be finite");
    }
    let ok = im.borrow_mut().convolution(&m, div, off);
    Ok(Value::Bool(ok))
}

// ---------------------------------------------------------------- transforms

fn imageflip(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let m = a.int(1);
    if !(1..=3).contains(&m) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) must be one of IMG_FLIP_VERTICAL, IMG_FLIP_HORIZONTAL, or IMG_FLIP_BOTH",
        );
    }
    im.borrow_mut().flip(m as i32);
    true_()
}

fn size_warning(vm: &mut Vm, a: &Args, e: tr::SizeError) -> R<Value> {
    let m = match e {
        tr::SizeError::NegativeOrZero => formats::NEGATIVE_MSG,
        tr::SizeError::Overflow => formats::OVERFLOW_MSG,
    };
    vm.fwarn(a, m)?;
    Ok(Value::Bool(false))
}

fn rect_key(vm: &mut Vm, a: &Args, arr: &PhpArray, k: &str) -> R<Option<i64>> {
    match arr.get_str(&PhpStr::from(k)) {
        Some(v) => {
            let v = v.deref();
            Ok(Some(vm.to_int(&v)?))
        }
        None => {
            vm.fthrow::<()>(
                a,
                "ValueError",
                &format!("Argument #2 ($rectangle) must have an \"{k}\" key"),
            )?;
            Ok(None)
        }
    }
}

fn imagecrop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let arr = a.array(1);
    let mut v = [0i64; 4];
    for (i, k) in ["x", "y", "width", "height"].iter().enumerate() {
        match rect_key(vm, a, &arr, k)? {
            Some(n) => v[i] = n,
            None => return Ok(Value::Bool(false)),
        }
    }
    let [x, y, w, h] = v;
    let (x, y, w, h) = (
        x as i32 as i64,
        y as i32 as i64,
        w as i32 as i64,
        h as i32 as i64,
    );
    for (p, s, pk, sk) in [(x, w, "x", "width"), (y, h, "y", "height")] {
        if p + s > i32::MAX as i64 {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #2 ($rectangle) overflow with \"{pk}\" and \"{sk}\" keys"),
            );
        }
        if p + s < i32::MIN as i64 {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #2 ($rectangle) underflow with \"{pk}\" and \"{sk}\" keys"),
            );
        }
    }
    let r = im.borrow().crop(x as i32, y as i32, w as i32, h as i32);
    match r {
        Ok(d) => new_image(vm, d),
        Err(e) => size_warning(vm, a, e),
    }
}

fn imagecropauto(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let mode = a.int_or(1, 0);
    let threshold = if a.has(2) { a.float(2) } else { 0.5 };
    let color = a.int_or(3, -1);
    if !(0..=5).contains(&mode) {
        return vm.fthrow(a, "ValueError", "Argument #2 ($mode) must be a valid mode");
    }
    let palette_total = {
        let i = im.borrow();
        if i.truecolor {
            None
        } else {
            Some(i.colors_total as i64)
        }
    };
    if mode == tr::CROP_THRESHOLD as i64 && (color < 0 || palette_total.is_some_and(|n| color >= n))
    {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($color) must be greater than or equal to 0 when using the threshold mode",
        );
    }
    let r = im.borrow().crop_auto(mode as i32, threshold, color as i32);
    match r {
        Some(d) => new_image(vm, d),
        None => Ok(Value::Bool(false)),
    }
}

fn imagescale(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let w = a.int(1);
    let h = a.int_or(2, -1);
    let mode = a.int_or(3, tr::IMG_BILINEAR_FIXED as i64);
    if w < 0 && h < 0 {
        return vm.throw(
            "ValueError",
            "Argument #2 ($width) and argument #3 ($height) cannot be both negative",
        );
    }
    let max = i32::MAX as i64;
    let (sx, sy) = {
        let i = im.borrow();
        (i.sx as i64, i.sy as i64)
    };
    let (w, h) = if w < 0 {
        if h > max {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #3 ($height) must be less than or equal to {max}"),
            );
        }
        (sx * h / sy, h)
    } else if h < 0 {
        if w > max {
            return vm.fthrow(
                a,
                "ValueError",
                &format!("Argument #2 ($width) must be less than or equal to {max}"),
            );
        }
        (w, sy * w / sx)
    } else {
        (w, h)
    };
    if !(1..=max).contains(&w) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #2 ($width) must be between 1 and {max}"),
        );
    }
    if !(1..=max).contains(&h) {
        return vm.fthrow(
            a,
            "ValueError",
            &format!("Argument #3 ($height) must be between 1 and {max}"),
        );
    }
    if mode == -1 {
        // IMG_BILINEAR_FIXED default.
    } else if !tr::valid_interpolation(mode) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($mode) must be one of the GD_* constants",
        );
    }
    let mode = if mode == -1 {
        tr::IMG_BILINEAR_FIXED
    } else {
        mode as i32
    };
    let r = im.borrow().scale(w as i32, h as i32, mode);
    match r {
        Ok(d) => new_image(vm, d),
        Err(e) => size_warning(vm, a, e),
    }
}

fn imagerotate(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let angle = a.float(1);
    let lim = i32::MAX as f64 / 100.0;
    if !angle.is_finite() || angle < -lim || angle > lim {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #2 ($angle) must be between {} and {}",
                fmt_float(-lim),
                fmt_float(lim)
            ),
        );
    }
    let bg = a.int(2);
    if bg < 0 {
        return Ok(Value::Bool(false));
    }
    let r = im.borrow().rotate(angle, bg as i32);
    match r {
        Some(d) => new_image(vm, d),
        None => Ok(Value::Bool(false)),
    }
}

fn fmt_float(f: f64) -> String {
    format!("{f}")
}

fn affine_arg(
    vm: &mut Vm,
    a: &Args,
    arr: &PhpArray,
    argn: usize,
    name: &str,
) -> R<Option<tr::Affine>> {
    if arr.len() != 6 {
        vm.fthrow::<()>(
            a,
            "ValueError",
            &format!("Argument #{argn} (${name}) must have 6 elements"),
        )?;
        return Ok(None);
    }
    let mut m = [0.0f64; 6];
    for (i, cell) in m.iter_mut().enumerate() {
        let Some(v) = arr.get_int(i as i64) else {
            vm.fthrow::<()>(
                a,
                "ValueError",
                &format!("Argument #{argn} (${name}) must have 6 elements"),
            )?;
            return Ok(None);
        };
        let v = v.deref();
        *cell = match v {
            Value::Int(n) => n as f64,
            Value::Float(f) => f,
            Value::Str(_) | Value::Bool(_) | Value::Null => vm.to_float(&v)?,
            other => {
                vm.fthrow::<()>(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #{argn} (${name}) element {i} must be of type int|float|string, {} given",
                        crate::vm::Vm::type_name_given(&other)
                    ),
                )?;
                return Ok(None);
            }
        };
    }
    Ok(Some(m))
}

fn imageaffine(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let im = img(vm, a, 0)?;
    let arr = a.array(1);
    let Some(m) = affine_arg(vm, a, &arr, 2, "affine")? else {
        return Ok(Value::Bool(false));
    };
    for (i, v) in m.iter().enumerate() {
        if !v.is_finite() || v.abs() > i32::MAX as f64 {
            return vm.fthrow(
                a,
                "ValueError",
                &format!(
                    "Argument #2 ($affine) element {} must be between {} and {}",
                    i,
                    i32::MIN,
                    i32::MAX
                ),
            );
        }
    }
    let clip = match a.val(2) {
        Value::Array(c) => {
            let mut v = [0i64; 4];
            for (i, k) in ["x", "y", "width", "height"].iter().enumerate() {
                match c.get_str(&PhpStr::from(*k)) {
                    Some(x) => {
                        let x = x.deref();
                        v[i] = vm.to_int(&x)?;
                    }
                    None => {
                        return vm.fthrow(
                            a,
                            "ValueError",
                            &format!("Argument #3 ($clip) must have an \"{k}\" key"),
                        );
                    }
                }
            }
            Some((v[0] as i32, v[1] as i32, v[2] as i32, v[3] as i32))
        }
        _ => None,
    };
    let r = tr::affine(&im.borrow(), &m, clip);
    match r {
        Some(d) => new_image(vm, d),
        None => Ok(Value::Bool(false)),
    }
}

fn affine_value(m: &tr::Affine) -> Value {
    Value::Array(PhpArray::from_values(m.iter().map(|&v| Value::Float(v))))
}

fn imageaffinematrixget(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let t = a.int(0);
    let opt = a.val(1);
    match t {
        0 | 1 => {
            let Value::Array(arr) = opt else {
                return vm.fthrow(
                    a,
                    "TypeError",
                    "Argument #1 ($type) must be of type array when using translate or scale",
                );
            };
            let mut xy = [0.0; 2];
            for (i, k) in ["x", "y"].iter().enumerate() {
                match arr.get_str(&PhpStr::from(*k)) {
                    Some(v) => {
                        let v = v.deref();
                        xy[i] = vm.to_float(&v)?;
                    }
                    None => {
                        return vm.fthrow(
                            a,
                            "ValueError",
                            &format!("Argument #2 ($options) must have a \"{k}\" key"),
                        );
                    }
                }
            }
            let m = if t == 0 {
                [1.0, 0.0, 0.0, 1.0, xy[0], xy[1]]
            } else {
                [xy[0], 0.0, 0.0, xy[1], 0.0, 0.0]
            };
            Ok(affine_value(&m))
        }
        2..=4 => {
            let v = vm.to_float(&opt)?;
            let r = v.to_radians();
            let m = match t {
                2 => {
                    let (s, c) = r.sin_cos();
                    [c, s, -s, c, 0.0, 0.0]
                }
                3 => [1.0, 0.0, r.tan(), 1.0, 0.0, 0.0],
                _ => [1.0, r.tan(), 0.0, 1.0, 0.0, 0.0],
            };
            Ok(affine_value(&m))
        }
        _ => vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($type) must be a valid element type",
        ),
    }
}

fn imageaffinematrixconcat(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let a1 = a.array(0);
    let a2 = a.array(1);
    let Some(m1) = affine_arg(vm, a, &a1, 1, "matrix1")? else {
        return Ok(Value::Bool(false));
    };
    let Some(m2) = affine_arg(vm, a, &a2, 2, "matrix2")? else {
        return Ok(Value::Bool(false));
    };
    Ok(affine_value(&tr::affine_concat(&m1, &m2)))
}
