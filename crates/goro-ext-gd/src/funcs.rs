//! The drawing, color and image-manipulation functions of gd.c.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_gd::{Font, Image, Point};

use crate::zpp::{self, Arg, Func, Img, err};
use crate::*;

const INT_MAX: i64 = i32::MAX as i64;
const INT_MIN: i64 = i32::MIN as i64;

fn args(
    vm: &mut Vm,
    name: &'static str,
    spec: &str,
    a: &[Value],
) -> Result<(Func, Vec<Arg>), VmError> {
    let f = Func::new(name);
    let v = zpp::parse(vm, &f, spec, a)?;
    Ok((f, v))
}

/// Run `op` on (dst, src); when both are the same image, `src` is a
/// snapshot taken before the operation.
pub fn with_two<T>(dst: &Img, src: &Img, op: impl FnOnce(&mut Image, &Image, bool) -> T) -> T {
    if Rc::ptr_eq(dst, src) {
        let snapshot = src.borrow().clone();
        op(&mut dst.borrow_mut(), &snapshot, true)
    } else {
        op(&mut dst.borrow_mut(), &src.borrow(), false)
    }
}

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("gd_info", gd_info),
    ("imagetypes", imagetypes),
    ("imagesetstyle", imagesetstyle),
    ("imagecreatetruecolor", imagecreatetruecolor),
    ("imagecreate", imagecreate),
    ("imageistruecolor", imageistruecolor),
    ("imagetruecolortopalette", imagetruecolortopalette),
    ("imagepalettetotruecolor", imagepalettetotruecolor),
    ("imagecolormatch", imagecolormatch),
    ("imagesetthickness", imagesetthickness),
    ("imagefilledellipse", imagefilledellipse),
    ("imagefilledarc", imagefilledarc),
    ("imagealphablending", imagealphablending),
    ("imagesavealpha", imagesavealpha),
    ("imagelayereffect", imagelayereffect),
    ("imagecolorallocatealpha", imagecolorallocatealpha),
    ("imagecolorresolvealpha", imagecolorresolvealpha),
    ("imagecolorclosestalpha", imagecolorclosestalpha),
    ("imagecolorexactalpha", imagecolorexactalpha),
    ("imagecopyresampled", imagecopyresampled),
    ("imagesettile", imagesettile),
    ("imagesetbrush", imagesetbrush),
    ("imagedestroy", imagedestroy),
    ("imagecolorallocate", imagecolorallocate),
    ("imagepalettecopy", imagepalettecopy),
    ("imagecolorat", imagecolorat),
    ("imagecolorclosest", imagecolorclosest),
    ("imagecolorclosesthwb", imagecolorclosesthwb),
    ("imagecolordeallocate", imagecolordeallocate),
    ("imagecolorresolve", imagecolorresolve),
    ("imagecolorexact", imagecolorexact),
    ("imagecolorset", imagecolorset),
    ("imagecolorsforindex", imagecolorsforindex),
    ("imagegammacorrect", imagegammacorrect),
    ("imagesetpixel", imagesetpixel),
    ("imageline", imageline),
    ("imagedashedline", imagedashedline),
    ("imagerectangle", imagerectangle),
    ("imagefilledrectangle", imagefilledrectangle),
    ("imagearc", imagearc),
    ("imageellipse", imageellipse),
    ("imagefilltoborder", imagefilltoborder),
    ("imagefill", imagefill),
    ("imagecolorstotal", imagecolorstotal),
    ("imagecolortransparent", imagecolortransparent),
    ("imageinterlace", imageinterlace),
    ("imagepolygon", imagepolygon),
    ("imageopenpolygon", imageopenpolygon),
    ("imagefilledpolygon", imagefilledpolygon),
    ("imagefontwidth", imagefontwidth),
    ("imagefontheight", imagefontheight),
    ("imagechar", imagechar),
    ("imagecharup", imagecharup),
    ("imagestring", imagestring),
    ("imagestringup", imagestringup),
    ("imagecopy", imagecopy),
    ("imagecopymerge", imagecopymerge),
    ("imagecopymergegray", imagecopymergegray),
    ("imagecopyresized", imagecopyresized),
    ("imagesx", imagesx),
    ("imagesy", imagesy),
    ("imagesetclip", imagesetclip),
    ("imagegetclip", imagegetclip),
    ("imagefilter", imagefilter),
    ("imageconvolution", imageconvolution),
    ("imageantialias", imageantialias),
    ("imageresolution", imageresolution),
];

pub fn register(vm: &mut Vm) {
    for &(name, f) in FUNCTIONS {
        register_fn(vm, name, f);
    }
}

fn gd_info(vm: &mut Vm, a: &[Value]) -> R {
    args(vm, "gd_info", "", a)?;
    let mut r = PhpArray::new();
    r.set(key("GD Version"), vstr(PHP_GD_VERSION_STRING));
    r.set(key("FreeType Support"), vbool(HAVE_FREETYPE));
    if HAVE_FREETYPE {
        r.set(key("FreeType Linkage"), vstr("with freetype"));
    }
    r.set(key("GIF Read Support"), Value::True);
    r.set(key("GIF Create Support"), Value::True);
    r.set(key("JPEG Support"), vbool(HAVE_JPG));
    r.set(key("PNG Support"), Value::True);
    r.set(key("WBMP Support"), Value::True);
    r.set(key("XPM Support"), vbool(HAVE_XPM));
    r.set(key("XBM Support"), Value::True);
    r.set(key("WebP Support"), vbool(HAVE_WEBP));
    r.set(key("BMP Support"), Value::True);
    r.set(key("AVIF Support"), vbool(HAVE_AVIF));
    r.set(key("TGA Read Support"), Value::True);
    r.set(key("JIS-mapped Japanese Font Support"), Value::False);
    Ok(varr(r))
}

fn imagetypes(vm: &mut Vm, a: &[Value]) -> R {
    let mut ret = 1 | 4 | 8 | 64 | 128;
    if HAVE_JPG {
        ret |= 2;
    }
    if HAVE_XPM {
        ret |= 16;
    }
    if HAVE_WEBP {
        ret |= 32;
    }
    if HAVE_AVIF {
        ret |= 256;
    }
    args(vm, "imagetypes", "", a)?;
    Ok(Value::Long(ret))
}

fn imagesetstyle(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagesetstyle", "Oa", a)?;
    let im = a[0].img();
    let styles = a[1].arr();
    let styles = styles.borrow();
    if styles.is_empty() {
        return Err(f.value_error(vm, 2, "must not be empty"));
    }
    let v: Vec<i32> = styles
        .iter()
        .map(|(_, v)| v.deref().to_long_coerced() as i32)
        .collect();
    im.borrow_mut().set_style(&v);
    Ok(Value::True)
}

fn create_common(vm: &mut Vm, name: &'static str, a: &[Value], truecolor: bool) -> R {
    let (f, a) = args(vm, name, "ll", a)?;
    let x = a[0].long_or(0);
    let y = a[1].long_or(0);
    if x <= 0 || x >= INT_MAX {
        return Err(f.value_error(vm, 1, "must be greater than 0"));
    }
    if y <= 0 || y >= INT_MAX {
        return Err(f.value_error(vm, 2, "must be greater than 0"));
    }
    let im = if truecolor {
        Image::create_true_color(x as i32, y as i32)
    } else {
        Image::create(x as i32, y as i32)
    };
    flush_errors(vm, &f);
    match im {
        Some(im) => Ok(wrap_image(vm, im)),
        None => Ok(Value::False),
    }
}

fn imagecreatetruecolor(vm: &mut Vm, a: &[Value]) -> R {
    create_common(vm, "imagecreatetruecolor", a, true)
}

fn imagecreate(vm: &mut Vm, a: &[Value]) -> R {
    create_common(vm, "imagecreate", a, false)
}

fn imageistruecolor(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imageistruecolor", "O", a)?;
    Ok(vbool(a[0].img().borrow().true_color))
}

fn imagetruecolortopalette(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagetruecolortopalette", "Obl", a)?;
    let im = a[0].img();
    let dither = a[1].bool_or(false);
    let n = a[2].long_or(0);
    if n <= 0 || n > INT_MAX {
        return Err(f.value_error(
            vm,
            3,
            &format!("must be greater than 0 and less than {}", i32::MAX),
        ));
    }
    let ok = goro_gd::topal::true_color_to_palette(&mut im.borrow_mut(), dither, n as i32);
    flush_errors(vm, &f);
    if ok {
        Ok(Value::True)
    } else {
        f.warn(vm, "Couldn't convert to palette");
        Ok(Value::False)
    }
}

fn imagepalettetotruecolor(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagepalettetotruecolor", "O", a)?;
    let ok = a[0].img().borrow_mut().palette_to_true_color();
    Ok(vbool(ok))
}

fn imagecolormatch(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolormatch", "OO", a)?;
    let (im1, im2) = (a[0].img(), a[1].img());
    let r = if Rc::ptr_eq(&im1, &im2) {
        let snap = im1.borrow().clone();
        goro_gd::color_match::color_match(&snap, &mut im2.borrow_mut())
    } else {
        goro_gd::color_match::color_match(&im1.borrow(), &mut im2.borrow_mut())
    };
    flush_errors(vm, &f);
    match r {
        -1 => Err(f.value_error(vm, 1, "must be TrueColor")),
        -2 => Err(f.value_error(vm, 2, "must be Palette")),
        -3 => Err(f.value_error(vm, 2, "must be the same size as argument #1 ($im1)")),
        -4 => Err(f.value_error(vm, 2, "must have at least one color")),
        _ => Ok(Value::True),
    }
}

fn imagesetthickness(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesetthickness", "Ol", a)?;
    a[0].img()
        .borrow_mut()
        .set_thickness(a[1].long_or(0) as i32);
    Ok(Value::True)
}

fn l(a: &[Arg], i: usize) -> i32 {
    a[i].long_or(0) as i32
}

fn imagefilledellipse(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagefilledellipse", "Olllll", a)?;
    let w = a[3].long_or(0);
    if !(0..=INT_MAX).contains(&w) {
        return Err(f.value_error(vm, 4, &format!("must be between 0 and {}", i32::MAX)));
    }
    a[0].img()
        .borrow_mut()
        .filled_ellipse(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), l(&a, 5));
    Ok(Value::True)
}

fn arc_angle(v: i64) -> i32 {
    let mut e = v as i32;
    if e < 0 {
        e %= 360;
    }
    e
}

fn imagefilledarc(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagefilledarc", "Olllllllll", a)?;
    let e = arc_angle(a[6].long_or(0));
    let st = arc_angle(a[5].long_or(0));
    a[0].img().borrow_mut().filled_arc(
        l(&a, 1),
        l(&a, 2),
        l(&a, 3),
        l(&a, 4),
        st,
        e,
        l(&a, 7),
        l(&a, 8),
    );
    Ok(Value::True)
}

fn imagealphablending(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagealphablending", "Ob", a)?;
    a[0].img()
        .borrow_mut()
        .set_alpha_blending(a[1].bool_or(false) as i32);
    Ok(Value::True)
}

fn imagesavealpha(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesavealpha", "Ob", a)?;
    a[0].img()
        .borrow_mut()
        .set_save_alpha(a[1].bool_or(false) as i32);
    Ok(Value::True)
}

fn imagelayereffect(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagelayereffect", "Ol", a)?;
    a[0].img().borrow_mut().set_alpha_blending(l(&a, 1));
    Ok(Value::True)
}

/// CHECK_RGBA_RANGE()
fn rgba_check(vm: &mut Vm, f: &Func, a: &[Arg], first: usize, alpha: bool) -> Result<(), VmError> {
    let n = if alpha { 4 } else { 3 };
    for i in 0..n {
        let v = a[first + i].long_or(0);
        let max = if alpha && i == 3 { 127 } else { 255 };
        if v < 0 || v > max {
            return Err(f.value_error(
                vm,
                first + i + 1,
                &format!("must be between 0 and {} (inclusive)", max),
            ));
        }
    }
    Ok(())
}

fn imagecolorallocatealpha(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorallocatealpha", "Ollll", a)?;
    rgba_check(vm, &f, &a, 1, true)?;
    let ct = a[0]
        .img()
        .borrow_mut()
        .color_allocate_alpha(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4));
    if ct < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(ct as i64))
}

fn imagecolorresolvealpha(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorresolvealpha", "Ollll", a)?;
    rgba_check(vm, &f, &a, 1, true)?;
    let c = a[0]
        .img()
        .borrow_mut()
        .color_resolve_alpha(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4));
    Ok(Value::Long(c as i64))
}

fn imagecolorclosestalpha(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorclosestalpha", "Ollll", a)?;
    rgba_check(vm, &f, &a, 1, true)?;
    let c = a[0]
        .img()
        .borrow()
        .color_closest_alpha(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4));
    Ok(Value::Long(c as i64))
}

fn imagecolorexactalpha(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorexactalpha", "Ollll", a)?;
    rgba_check(vm, &f, &a, 1, true)?;
    let c = a[0]
        .img()
        .borrow()
        .color_exact_alpha(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4));
    Ok(Value::Long(c as i64))
}

fn imagecopyresampled(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecopyresampled", "OOllllllll", a)?;
    let (dst, src) = (a[0].img(), a[1].img());
    with_two(&dst, &src, |d, s, same| {
        goro_gd::gd::copy_resampled(
            d,
            s,
            same,
            l(&a, 2),
            l(&a, 3),
            l(&a, 4),
            l(&a, 5),
            l(&a, 6),
            l(&a, 7),
            l(&a, 8),
            l(&a, 9),
        )
    });
    flush_errors(vm, &f);
    Ok(Value::True)
}

/// The brush / tile image: a snapshot when it is the image itself.
fn aux_image(im: &Img, other: &Img) -> Img {
    if Rc::ptr_eq(im, other) {
        Rc::new(RefCell::new(other.borrow().clone()))
    } else {
        other.clone()
    }
}

fn imagesettile(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesettile", "OO", a)?;
    let (im, tile) = (a[0].img(), a[1].img());
    let t = aux_image(&im, &tile);
    im.borrow_mut().set_tile(t);
    Ok(Value::True)
}

fn imagesetbrush(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesetbrush", "OO", a)?;
    let (im, brush) = (a[0].img(), a[1].img());
    let b = aux_image(&im, &brush);
    im.borrow_mut().set_brush(b);
    Ok(Value::True)
}

fn imagedestroy(vm: &mut Vm, a: &[Value]) -> R {
    vm.emit_deprecated(
        "Function imagedestroy() is deprecated since 8.5, as it has no effect since PHP 8.0",
    );
    if vm.current_exception.is_some() {
        return Err(err(vm));
    }
    args(vm, "imagedestroy", "O", a)?;
    Ok(Value::True)
}

fn imagecolorallocate(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorallocate", "Olll", a)?;
    rgba_check(vm, &f, &a, 1, false)?;
    let ct = a[0]
        .img()
        .borrow_mut()
        .color_allocate(l(&a, 1), l(&a, 2), l(&a, 3));
    if ct < 0 {
        return Ok(Value::False);
    }
    Ok(Value::Long(ct as i64))
}

fn imagepalettecopy(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagepalettecopy", "OO", a)?;
    let (dst, src) = (a[0].img(), a[1].img());
    with_two(&dst, &src, |d, s, _| Image::palette_copy(d, s));
    Ok(Value::Null)
}

fn imagecolorat(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorat", "Oll", a)?;
    let im = a[0].img();
    let im = im.borrow();
    let (x, y) = (a[1].long_or(0), a[2].long_or(0));
    let inside =
        x >= im.cx1 as i64 && x <= im.cx2 as i64 && y >= im.cy1 as i64 && y <= im.cy2 as i64;
    if inside {
        let (xu, yu) = (x as usize, y as usize);
        if im.true_color {
            if let Some(p) = im.tpixels.get(yu).and_then(|r| r.get(xu)) {
                return Ok(Value::Long(*p as i64));
            }
        } else if let Some(p) = im.pixels.get(yu).and_then(|r| r.get(xu)) {
            return Ok(Value::Long(*p as i64));
        }
    }
    drop(im);
    f.notice(vm, &format!("{},{} is out of bounds", x, y));
    Ok(Value::False)
}

fn imagecolorclosest(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorclosest", "Olll", a)?;
    rgba_check(vm, &f, &a, 1, false)?;
    let c = a[0]
        .img()
        .borrow()
        .color_closest(l(&a, 1), l(&a, 2), l(&a, 3));
    Ok(Value::Long(c as i64))
}

fn imagecolorclosesthwb(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorclosesthwb", "Olll", a)?;
    rgba_check(vm, &f, &a, 1, false)?;
    let c = a[0]
        .img()
        .borrow()
        .color_closest_hwb(l(&a, 1), l(&a, 2), l(&a, 3));
    Ok(Value::Long(c as i64))
}

fn imagecolordeallocate(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolordeallocate", "Ol", a)?;
    let im = a[0].img();
    if im.borrow().true_color {
        return Ok(Value::True);
    }
    let col = a[1].long_or(0) as i32;
    let total = im.borrow().colors_total;
    if col >= 0 && col < total {
        im.borrow_mut().color_deallocate(col);
        Ok(Value::True)
    } else {
        Err(f.value_error(vm, 2, &format!("must be between 0 and {}", total)))
    }
}

fn imagecolorresolve(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorresolve", "Olll", a)?;
    rgba_check(vm, &f, &a, 1, false)?;
    let c = a[0]
        .img()
        .borrow_mut()
        .color_resolve(l(&a, 1), l(&a, 2), l(&a, 3));
    Ok(Value::Long(c as i64))
}

fn imagecolorexact(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorexact", "Olll", a)?;
    rgba_check(vm, &f, &a, 1, false)?;
    let c = a[0]
        .img()
        .borrow()
        .color_exact(l(&a, 1), l(&a, 2), l(&a, 3));
    Ok(Value::Long(c as i64))
}

fn imagecolorset(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorset", "Ollll|l", a)?;
    let mut v = a.clone();
    if matches!(v[5], Arg::Absent) {
        v[5] = Arg::Long(0);
    }
    rgba_check(vm, &f, &v, 2, true)?;
    let im = a[0].img();
    let mut im = im.borrow_mut();
    let col = a[1].long_or(0) as i32;
    if col >= 0 && col < im.colors_total {
        let c = col as usize;
        im.red[c] = l(&v, 2);
        im.green[c] = l(&v, 3);
        im.blue[c] = l(&v, 4);
        im.alpha[c] = l(&v, 5);
        Ok(Value::Null)
    } else {
        Ok(Value::False)
    }
}

fn imagecolorsforindex(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecolorsforindex", "Ol", a)?;
    let im = a[0].img();
    let im = im.borrow();
    let col = a[1].long_or(0) as i32;
    if (col >= 0 && im.true_color) || (!im.true_color && col >= 0 && col < im.colors_total) {
        let mut r = PhpArray::new();
        r.set(key("red"), Value::Long(im.red_of(col) as i64));
        r.set(key("green"), Value::Long(im.green_of(col) as i64));
        r.set(key("blue"), Value::Long(im.blue_of(col) as i64));
        r.set(key("alpha"), Value::Long(im.alpha_of(col) as i64));
        Ok(varr(r))
    } else {
        drop(im);
        Err(f.value_error(vm, 2, "is out of range"))
    }
}

fn imagegammacorrect(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagegammacorrect", "Odd", a)?;
    let input = a[1].double_or(0.0);
    let output = a[2].double_or(0.0);
    if input <= 0.0 {
        return Err(f.value_error(vm, 2, "must be greater than 0"));
    }
    if !input.is_finite() {
        return Err(f.value_error(vm, 2, "must be finite"));
    }
    if output <= 0.0 {
        return Err(f.value_error(vm, 3, "must be greater than 0"));
    }
    if !output.is_finite() {
        return Err(f.value_error(vm, 3, "must be finite"));
    }
    let gamma = input / output;
    let im = a[0].img();
    let mut im = im.borrow_mut();
    let g = |c: i32| -> i32 { ((c as f64 / 255.0).powf(gamma) * 255.0 + 0.5) as i32 };
    if im.true_color {
        for y in 0..im.sy {
            for x in 0..im.sx {
                let c = im.get_pixel(x, y);
                let nc = goro_gd::true_color_alpha(
                    g(goro_gd::true_color_get_red(c)),
                    g(goro_gd::true_color_get_green(c)),
                    g(goro_gd::true_color_get_blue(c)),
                    goro_gd::true_color_get_alpha(c),
                );
                im.set_pixel(x, y, nc);
            }
        }
        return Ok(Value::True);
    }
    for i in 0..im.colors_total as usize {
        im.red[i] = g(im.red[i]);
        im.green[i] = g(im.green[i]);
        im.blue[i] = g(im.blue[i]);
    }
    Ok(Value::True)
}

fn imagesetpixel(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesetpixel", "Olll", a)?;
    a[0].img()
        .borrow_mut()
        .set_pixel(l(&a, 1), l(&a, 2), l(&a, 3));
    Ok(Value::True)
}

fn imageline(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imageline", "Olllll", a)?;
    let im = a[0].img();
    let mut im = im.borrow_mut();
    let mut col = l(&a, 5);
    if im.aa != 0 {
        im.set_anti_aliased(col);
        col = goro_gd::ANTI_ALIASED;
    }
    im.line(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), col);
    Ok(Value::True)
}

fn imagedashedline(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagedashedline", "Olllll", a)?;
    a[0].img()
        .borrow_mut()
        .dashed_line(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), l(&a, 5));
    Ok(Value::True)
}

fn imagerectangle(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagerectangle", "Olllll", a)?;
    a[0].img()
        .borrow_mut()
        .rectangle(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), l(&a, 5));
    Ok(Value::True)
}

fn imagefilledrectangle(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagefilledrectangle", "Olllll", a)?;
    a[0].img()
        .borrow_mut()
        .filled_rectangle(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), l(&a, 5));
    Ok(Value::True)
}

fn imagearc(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagearc", "Olllllll", a)?;
    let e = arc_angle(a[6].long_or(0));
    let st = arc_angle(a[5].long_or(0));
    a[0].img()
        .borrow_mut()
        .arc(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), st, e, l(&a, 7));
    Ok(Value::True)
}

fn imageellipse(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imageellipse", "Olllll", a)?;
    a[0].img()
        .borrow_mut()
        .ellipse(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4), l(&a, 5));
    Ok(Value::True)
}

fn imagefilltoborder(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagefilltoborder", "Ollll", a)?;
    a[0].img()
        .borrow_mut()
        .fill_to_border(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4));
    Ok(Value::True)
}

fn imagefill(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagefill", "Olll", a)?;
    a[0].img().borrow_mut().fill(l(&a, 1), l(&a, 2), l(&a, 3));
    flush_errors(vm, &f);
    Ok(Value::True)
}

fn imagecolorstotal(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagecolorstotal", "O", a)?;
    Ok(Value::Long(a[0].img().borrow().colors_total as i64))
}

fn imagecolortransparent(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagecolortransparent", "O|l!", a)?;
    let im = a[0].img();
    let mut im = im.borrow_mut();
    if let Arg::Long(c) = a[1] {
        im.color_transparent(c as i32);
    }
    Ok(Value::Long(im.transparent as i64))
}

fn imageinterlace(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imageinterlace", "O|b!", a)?;
    let im = a[0].img();
    let mut im = im.borrow_mut();
    if let Arg::Bool(b) = a[1] {
        im.set_interlace(b as i32);
    }
    Ok(vbool(im.interlace != 0))
}

/// php_imagepolygon(): mode -1 open, 0 normal, 1 filled
fn polygon_common(vm: &mut Vm, name: &'static str, a: &[Value], mode: i32) -> R {
    let (f, a) = args(vm, name, "Oal|l!", a)?;
    let points = a[1].arr();
    let mut npoints = a[2].long_or(0);
    let col;
    let nelem_all = points.borrow().len() as i64;
    if a[3].is_null() {
        col = npoints;
        npoints = nelem_all;
        if npoints % 2 != 0 {
            return Err(f.value_error(vm, 2, "must have an even number of elements"));
        }
        npoints /= 2;
    } else {
        col = a[3].long_or(0);
        f.deprecated(vm, "Using the $num_points parameter is deprecated")?;
    }
    let npoints = npoints as i32;
    let mut col = col as i32;
    let nelem = nelem_all as i32;
    if npoints < 3 {
        return Err(f.value_error(vm, 3, "must be greater than or equal to 3"));
    }
    if nelem < npoints.wrapping_mul(2) {
        let m = format!(
            "Trying to use {} points in array with only {} points",
            npoints,
            nelem / 2
        );
        return Err(f.plain_value_error(vm, &m));
    }
    let pts = {
        let p = points.borrow();
        let get = |i: i64| -> i32 {
            p.get(&ArrayKey::Int(i))
                .map(|v| v.deref().to_long_coerced() as i32)
                .unwrap_or(0)
        };
        (0..npoints as i64)
            .map(|i| Point {
                x: get(i * 2),
                y: get(i * 2 + 1),
            })
            .collect::<Vec<_>>()
    };
    let im = a[0].img();
    let mut im = im.borrow_mut();
    if im.aa != 0 {
        im.set_anti_aliased(col);
        col = goro_gd::ANTI_ALIASED;
    }
    match mode {
        -1 => im.open_polygon(&pts, col),
        0 => im.polygon(&pts, col),
        _ => im.filled_polygon(&pts, col),
    }
    drop(im);
    flush_errors(vm, &f);
    Ok(Value::True)
}

fn imagepolygon(vm: &mut Vm, a: &[Value]) -> R {
    polygon_common(vm, "imagepolygon", a, 0)
}

fn imageopenpolygon(vm: &mut Vm, a: &[Value]) -> R {
    polygon_common(vm, "imageopenpolygon", a, -1)
}

fn imagefilledpolygon(vm: &mut Vm, a: &[Value]) -> R {
    polygon_common(vm, "imagefilledpolygon", a, 1)
}

/// php_find_gd_font()
fn find_font(a: &Arg) -> Rc<Font> {
    match a {
        Arg::Font(f) => f.clone(),
        _ => {
            let n = a.long_or(0);
            Rc::new(Font::builtin(n.clamp(1, 5) as i32).clone())
        }
    }
}

fn fontsize(vm: &mut Vm, name: &'static str, a: &[Value], h: bool) -> R {
    let (_, a) = args(vm, name, "F", a)?;
    let f = find_font(&a[0]);
    Ok(Value::Long(if h { f.h } else { f.w } as i64))
}

fn imagefontwidth(vm: &mut Vm, a: &[Value]) -> R {
    fontsize(vm, "imagefontwidth", a, false)
}

fn imagefontheight(vm: &mut Vm, a: &[Value]) -> R {
    fontsize(vm, "imagefontheight", a, true)
}

/// php_gdimagecharup(): workaround for a bug in gd 1.2
fn php_char_up(im: &mut Image, f: &Font, x: i32, y: i32, c: i32, color: i32) {
    if c < f.offset || c >= f.offset + f.nchars {
        return;
    }
    let fline = ((c - f.offset) * f.h * f.w) as usize;
    let mut cx = 0usize;
    let mut py = y as i64;
    while py > y as i64 - f.w as i64 {
        let mut cy = 0usize;
        let mut px = x as i64;
        while px < x as i64 + f.h as i64 {
            if f.data
                .get(fline + cy * f.w as usize + cx)
                .copied()
                .unwrap_or(0)
                != 0
            {
                im.set_pixel(px as i32, py as i32, color);
            }
            cy += 1;
            px += 1;
        }
        cx += 1;
        py -= 1;
    }
}

/// php_imagechar(): 0 imagechar, 1 imagecharup, 2 imagestring, 3 imagestringup
fn imagechar_common(vm: &mut Vm, name: &'static str, a: &[Value], mode: i32) -> R {
    let (_, a) = args(vm, name, "OFllsl", a)?;
    let im = a[0].img();
    let font = find_font(&a[1]);
    let mut x = a[2].long_or(0) as u32;
    let mut y = a[3].long_or(0) as u32;
    let s = a[4].str_or(b"").to_vec();
    let col = l(&a, 5);
    let mut im = im.borrow_mut();
    match mode {
        0 => {
            let ch = s.first().copied().unwrap_or(0) as i32;
            im.char(&font, x as i32, y as i32, ch, col);
        }
        1 => {
            let ch = s.first().copied().unwrap_or(0) as i32;
            php_char_up(&mut im, &font, x as i32, y as i32, ch, col);
        }
        2 => {
            for &c in s.iter().take_while(|&&c| c != 0) {
                im.char(&font, x as i32, y as i32, c as i32, col);
                x = x.wrapping_add(font.w as u32);
            }
        }
        _ => {
            for &c in s.iter().take_while(|&&c| c != 0) {
                im.char_up(&font, x as i32, y as i32, c as i32, col);
                y = y.wrapping_sub(font.w as u32);
            }
        }
    }
    Ok(Value::True)
}

fn imagechar(vm: &mut Vm, a: &[Value]) -> R {
    imagechar_common(vm, "imagechar", a, 0)
}

fn imagecharup(vm: &mut Vm, a: &[Value]) -> R {
    imagechar_common(vm, "imagecharup", a, 1)
}

fn imagestring(vm: &mut Vm, a: &[Value]) -> R {
    imagechar_common(vm, "imagestring", a, 2)
}

fn imagestringup(vm: &mut Vm, a: &[Value]) -> R {
    imagechar_common(vm, "imagestringup", a, 3)
}

fn imagecopy(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagecopy", "OOllllll", a)?;
    let (dst, src) = (a[0].img(), a[1].img());
    with_two(&dst, &src, |d, s, same| {
        goro_gd::gd::copy(
            d,
            s,
            same,
            l(&a, 2),
            l(&a, 3),
            l(&a, 4),
            l(&a, 5),
            l(&a, 6),
            l(&a, 7),
        )
    });
    Ok(Value::True)
}

fn imagecopymerge(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagecopymerge", "OOlllllll", a)?;
    let (dst, src) = (a[0].img(), a[1].img());
    with_two(&dst, &src, |d, s, same| {
        goro_gd::gd::copy_merge(
            d,
            s,
            same,
            l(&a, 2),
            l(&a, 3),
            l(&a, 4),
            l(&a, 5),
            l(&a, 6),
            l(&a, 7),
            l(&a, 8),
        )
    });
    Ok(Value::True)
}

fn imagecopymergegray(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagecopymergegray", "OOlllllll", a)?;
    let (dst, src) = (a[0].img(), a[1].img());
    with_two(&dst, &src, |d, s, same| {
        goro_gd::gd::copy_merge_gray(
            d,
            s,
            same,
            l(&a, 2),
            l(&a, 3),
            l(&a, 4),
            l(&a, 5),
            l(&a, 6),
            l(&a, 7),
            l(&a, 8),
        )
    });
    Ok(Value::True)
}

fn imagecopyresized(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecopyresized", "OOllllllll", a)?;
    for (i, n) in [(6, 7), (7, 8), (8, 9), (9, 10)] {
        if l(&a, i) <= 0 {
            return Err(f.value_error(vm, n, "must be greater than 0"));
        }
    }
    let (dst, src) = (a[0].img(), a[1].img());
    with_two(&dst, &src, |d, s, same| {
        goro_gd::gd::copy_resized(
            d,
            s,
            same,
            l(&a, 2),
            l(&a, 3),
            l(&a, 4),
            l(&a, 5),
            l(&a, 6),
            l(&a, 7),
            l(&a, 8),
            l(&a, 9),
        )
    });
    flush_errors(vm, &f);
    Ok(Value::True)
}

fn imagesx(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesx", "O", a)?;
    Ok(Value::Long(a[0].img().borrow().sx as i64))
}

fn imagesy(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesy", "O", a)?;
    Ok(Value::Long(a[0].img().borrow().sy as i64))
}

fn imagesetclip(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagesetclip", "Ollll", a)?;
    a[0].img()
        .borrow_mut()
        .set_clip(l(&a, 1), l(&a, 2), l(&a, 3), l(&a, 4));
    Ok(Value::True)
}

fn imagegetclip(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagegetclip", "O", a)?;
    let (x1, y1, x2, y2) = a[0].img().borrow().get_clip();
    Ok(long_list(&[x1 as i64, y1 as i64, x2 as i64, y2 as i64]))
}

fn imagefilter(vm: &mut Vm, av: &[Value]) -> R {
    let f = Func::new("imagefilter");
    let mut n = av.len();
    while n > 0 && matches!(av[n - 1], Value::Undef) {
        n -= 1;
    }
    if !(2..=6).contains(&n) {
        zpp::throw(
            vm,
            b"ArgumentCountError",
            "Wrong parameter count for imagefilter()",
        );
        return Err(err(vm));
    }
    let first = zpp::parse(vm, &f, "Ol", &av[..2])?;
    let filtertype = first[1].long_or(0);
    if !(0..=12).contains(&filtertype) {
        return Err(f.value_error(vm, 2, "must be one of the IMG_FILTER_* filter constants"));
    }
    let av = &av[..n];
    use goro_gd::filter as gf;
    // PHP_GD_SINGLE_RES: zend_parse_parameters(1, "O")
    let single = |vm: &mut Vm| -> Result<Img, VmError> {
        let a = zpp::parse(vm, &f, "O", &av[..1])?;
        Ok(a[0].img())
    };
    let r = match filtertype {
        0 => gf::negate(&mut single(vm)?.borrow_mut()),
        1 => gf::grayscale(&mut single(vm)?.borrow_mut()),
        2 => {
            let a = zpp::parse(vm, &f, "Oll", av)?;
            gf::brightness(&mut a[0].img().borrow_mut(), l(&a, 2))
        }
        3 => {
            let a = zpp::parse(vm, &f, "Oll", av)?;
            gf::contrast(&mut a[0].img().borrow_mut(), l(&a, 2) as f64)
        }
        4 => {
            let a = zpp::parse(vm, &f, "Ollll|l", av)?;
            gf::color(
                &mut a[0].img().borrow_mut(),
                l(&a, 2),
                l(&a, 3),
                l(&a, 4),
                l(&a, 5),
            )
        }
        5 => gf::edge_detect_quick(&mut single(vm)?.borrow_mut()),
        6 => gf::emboss(&mut single(vm)?.borrow_mut()),
        7 => gf::gaussian_blur(&mut single(vm)?.borrow_mut()),
        8 => gf::selective_blur(&mut single(vm)?.borrow_mut()),
        9 => gf::mean_removal(&mut single(vm)?.borrow_mut()),
        10 => {
            let a = zpp::parse(vm, &f, "Old", av)?;
            gf::smooth(&mut a[0].img().borrow_mut(), a[2].double_or(0.0) as f32)
        }
        11 => {
            let a = zpp::parse(vm, &f, "Oll|b", av)?;
            gf::pixelate(
                &mut a[0].img().borrow_mut(),
                l(&a, 2),
                a[3].bool_or(false) as u32,
            )
        }
        _ => {
            let a = zpp::parse(vm, &f, "Olll|a", av)?;
            let sub = a[2].long_or(0);
            let plus = a[3].long_or(0);
            if sub < 0 || sub > INT_MAX {
                return Err(f.value_error(vm, 3, &format!("must be between 0 and {}", i32::MAX)));
            }
            if plus < 0 || plus > INT_MAX {
                return Err(f.value_error(vm, 4, &format!("must be between 0 and {}", i32::MAX)));
            }
            let im = a[0].img();
            let mut im = im.borrow_mut();
            match &a[4] {
                Arg::Arr(colors) if !colors.borrow().is_empty() => {
                    let c: Vec<i32> = colors
                        .borrow()
                        .iter()
                        .map(|(_, v)| v.deref().to_long_coerced() as i32)
                        .collect();
                    gf::scatter_color(&mut im, sub as i32, plus as i32, &c)
                }
                _ => gf::scatter(&mut im, sub as i32, plus as i32),
            }
        }
    };
    flush_errors(vm, &f);
    Ok(vbool(r))
}

fn imageconvolution(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageconvolution", "Oadd", a)?;
    let im = a[0].img();
    let m = a[1].arr();
    let mut matrix = [[0f32; 3]; 3];
    {
        let m = m.borrow();
        if m.len() != 3 {
            drop(m);
            return Err(f.value_error(vm, 2, "must be a 3x3 array"));
        }
        for i in 0..3usize {
            let row = m.get(&ArrayKey::Int(i as i64)).map(|v| v.deref());
            if let Some(Value::Array(row)) = row {
                let row = row.borrow();
                if row.len() != 3 {
                    let msg = format!(
                        "must be a 3x3 array, matrix[{}] only has {} elements",
                        i,
                        row.len()
                    );
                    drop(row);
                    return Err(f.value_error(vm, 2, &msg));
                }
                for j in 0..3usize {
                    match row.get(&ArrayKey::Int(j as i64)) {
                        Some(v) => matrix[i][j] = v.deref().to_double() as f32,
                        None => {
                            let msg = format!(
                                "must be a 3x3 array, matrix[{}][{}] cannot be found (missing integer key)",
                                i, j
                            );
                            drop(row);
                            return Err(f.value_error(vm, 2, &msg));
                        }
                    }
                }
            }
        }
    }
    let div = a[2].double_or(0.0);
    let offset = a[3].double_or(0.0);
    if !div.is_finite() {
        return Err(f.value_error(vm, 3, "must be finite"));
    }
    let div_f = div as f32;
    if div_f == 0.0 {
        return Err(f.value_error(vm, 3, "must not be 0"));
    }
    if !offset.is_finite() {
        return Err(f.value_error(vm, 4, "must be finite"));
    }
    let r = goro_gd::filter::convolution(&mut im.borrow_mut(), &matrix, div_f, offset as f32);
    flush_errors(vm, &f);
    Ok(vbool(r))
}

fn imageantialias(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imageantialias", "Ob", a)?;
    let im = a[0].img();
    let mut im = im.borrow_mut();
    if im.true_color {
        im.aa = a[1].bool_or(false) as i32;
    }
    Ok(Value::True)
}

fn imageresolution(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageresolution", "O|l!l!", a)?;
    let im = a[0].img();
    let rx = match a[1] {
        Arg::Long(n) => Some(n),
        _ => None,
    };
    let ry = match a[2] {
        Arg::Long(n) => Some(n),
        _ => None,
    };
    let umax = u32::MAX as i64;
    let check = |vm: &mut Vm, v: i64, n: usize| -> Result<(), VmError> {
        if v < 0 || v > umax {
            return Err(f.value_error(vm, n, &format!("must be between 0 and {}", u32::MAX)));
        }
        Ok(())
    };
    match (rx, ry) {
        (Some(x), Some(y)) => {
            check(vm, x, 2)?;
            check(vm, y, 3)?;
            im.borrow_mut().set_resolution(x as u32, y as u32);
            Ok(Value::True)
        }
        (Some(x), None) => {
            check(vm, x, 2)?;
            im.borrow_mut().set_resolution(x as u32, x as u32);
            Ok(Value::True)
        }
        (None, Some(y)) => {
            check(vm, y, 3)?;
            im.borrow_mut().set_resolution(y as u32, y as u32);
            Ok(Value::True)
        }
        (None, None) => {
            let im = im.borrow();
            Ok(long_list(&[im.res_x as i64, im.res_y as i64]))
        }
    }
}

#[allow(dead_code)]
fn unused(_: i64) -> i64 {
    INT_MIN
}
