//! ImagickPixel (imagickpixel_class.c).

use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_imagick::color::clamp_to_quantum;
use goro_imagick::{PixelInfo, QUANTUM_RANGE, QUANTUM_SCALE};

use crate::zpp::{self, Arg, Func, Obj};
use crate::{
    Cls, Impl, PixelData, R, arr_val, bool_val, convert_exception, pixel_data, set, str_val,
    throw_cls, zval_to_pixel,
};

pub(crate) const METHODS: &[(&str, Impl, bool)] = &[
    ("__construct", construct, false),
    ("getHSL", get_hsl, false),
    ("setHSL", set_hsl, false),
    ("getColorValueQuantum", get_color_value_quantum, false),
    ("setColorValueQuantum", set_color_value_quantum, false),
    ("getIndex", get_index, false),
    ("setIndex", set_index, false),
    ("setColor", set_color, false),
    ("clear", clear, false),
    ("destroy", clear, false),
    ("isPixelSimilarQuantum", is_similar_quantum, false),
    ("isPixelSimilar", is_similar, false),
    ("isSimilar", is_similar_quantum, false),
    ("getColorValue", get_color_value, false),
    ("setColorValue", set_color_value, false),
    ("getColor", get_color, false),
    ("getColorQuantum", get_color_quantum, false),
    ("getColorAsString", get_color_as_string, false),
    ("clone", clone_method, false),
    ("getColorCount", get_color_count, false),
    ("setColorCount", set_color_count, false),
    ("setColorFromPixel", set_color_from_pixel, false),
];

const COLOR_MIN: i64 = 10;
const COLOR_BLACK: i64 = 11;
const COLOR_BLUE: i64 = 12;
const COLOR_CYAN: i64 = 13;
const COLOR_GREEN: i64 = 14;
const COLOR_RED: i64 = 15;
const COLOR_YELLOW: i64 = 16;
const COLOR_MAGENTA: i64 = 17;
const COLOR_ALPHA: i64 = 18;
const COLOR_FUZZ: i64 = 19;
const COLOR_MAX: i64 = 20;

fn this(o: Option<&Obj>) -> Rc<PixelData> {
    pixel_data(o.expect("method with $this"))
}

/// IMAGICK_PIXEL_NOT_EMPTY
fn color(vm: &mut Vm, d: &PixelData) -> Result<PixelInfo, ()> {
    match d.color.borrow().clone() {
        Some(c) => Ok(c),
        None => {
            throw_cls(vm, Cls::Pixel, "Can not process empty ImagickPixel object")?;
            Err(())
        }
    }
}

fn update(vm: &mut Vm, d: &PixelData, f: impl FnOnce(&mut PixelInfo)) -> Result<(), ()> {
    let mut c = color(vm, d)?;
    f(&mut c);
    *d.color.borrow_mut() = Some(c);
    Ok(())
}

fn construct(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|s!", args)?;
    let d = this(o);
    let mut c = PixelInfo::default();
    if let Arg::Str(s) = &a[0] {
        if !s.is_empty() {
            match PixelInfo::parse(&String::from_utf8_lossy(s)) {
                Some(p) => c = p,
                None => {
                    *d.color.borrow_mut() = Some(PixelInfo::default());
                    return throw_cls(vm, Cls::Pixel, "Unable to construct ImagickPixel");
                }
            }
        }
    }
    *d.color.borrow_mut() = Some(c);
    Ok(Value::Null)
}

fn get_hsl(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = color(vm, &this(o))?;
    // the stored channels as they are (a CMYK colour's C, M, Y too)
    let (h, s, l) = goro_imagick::color::rgb_to_hsl(
        clamp_to_quantum(c.red) * QUANTUM_SCALE,
        clamp_to_quantum(c.green) * QUANTUM_SCALE,
        clamp_to_quantum(c.blue) * QUANTUM_SCALE,
    );
    let mut a = PhpArray::new();
    set(&mut a, "hue", Value::Double(h));
    set(&mut a, "saturation", Value::Double(s));
    set(&mut a, "luminosity", Value::Double(l));
    Ok(arr_val(a))
}

fn set_hsl(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd", args)?;
    let (h, s, l) = (
        a[0].double_or(0.0),
        a[1].double_or(0.0),
        a[2].double_or(0.0),
    );
    update(vm, &this(o), |c| {
        let (r, g, b) = goro_imagick::color::hsl_to_rgb(h, s, l);
        c.red = r * QUANTUM_RANGE;
        c.green = g * QUANTUM_RANGE;
        c.blue = b * QUANTUM_RANGE;
    })?;
    Ok(Value::True)
}

fn channel_ref(c: &mut PixelInfo, which: i64) -> Option<&mut f64> {
    Some(match which {
        COLOR_BLACK => &mut c.black,
        COLOR_BLUE | COLOR_YELLOW => &mut c.blue,
        COLOR_CYAN | COLOR_RED => &mut c.red,
        COLOR_GREEN | COLOR_MAGENTA => &mut c.green,
        COLOR_ALPHA => &mut c.alpha,
        COLOR_FUZZ => &mut c.fuzz,
        _ => return None,
    })
}

fn get_color_value_quantum(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let mut c = color(vm, &this(o))?;
    let which = a[0].long_or(0);
    if which == COLOR_FUZZ {
        return throw_cls(vm, Cls::Pixel, "Unknown color type");
    }
    match channel_ref(&mut c, which) {
        Some(v) => Ok(Value::Long(clamp_to_quantum(*v) as i64)),
        None => throw_cls(vm, Cls::Pixel, "Unknown color type"),
    }
}

fn set_color_value_quantum(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let which = a[0].long_or(0);
    let v = a[1].long_or(0) as f64;
    let d = this(o);
    let mut c = color(vm, &d)?;
    if which == COLOR_FUZZ {
        return throw_cls(vm, Cls::Pixel, "Unknown color type");
    }
    match channel_ref(&mut c, which) {
        Some(slot) => *slot = clamp_to_quantum(v),
        None => return throw_cls(vm, Cls::Pixel, "Unknown color type"),
    }
    if which == COLOR_ALPHA {
        c.alpha_trait = true;
    }
    *d.color.borrow_mut() = Some(c);
    Ok(Value::True)
}

fn get_index(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = color(vm, &this(o))?;
    Ok(Value::Long(c.index as i64))
}

fn set_index(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let i = a[0].long_or(0) as f64;
    update(vm, &this(o), |c| c.index = i)?;
    Ok(Value::True)
}

fn set_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let d = this(o);
    let old = color(vm, &d)?;
    let s = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    match PixelInfo::parse(&s) {
        Some(mut c) => {
            c.fuzz = old.fuzz;
            c.count = old.count;
            *d.color.borrow_mut() = Some(c);
            Ok(Value::True)
        }
        None => {
            let e = goro_imagick::MagickError::with_desc(
                goro_imagick::error::severity::OPTION_WARNING,
                "unrecognized color",
                &s,
                "warning/color.c/GetColorCompliance/1064",
            );
            convert_exception(vm, Some(e), Cls::Pixel, "Unable to set ImagickPixel color")
        }
    }
}

fn clear(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    color(vm, &d)?;
    *d.color.borrow_mut() = Some(PixelInfo::default());
    Ok(Value::True)
}

fn similar(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value], quantum: bool) -> R {
    let a = zpp::parse(vm, f, "zd", args)?;
    let c = color(vm, &this(o))?;
    let other = match &a[0] {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Pixel)?,
        _ => return Err(()),
    };
    let fuzz = a[1].double_or(0.0);
    let fuzz = if quantum { fuzz * QUANTUM_RANGE } else { fuzz };
    Ok(bool_val(pixel_similar(&c, &other, fuzz)))
}

/// isPixelSimilar() as pecl imagick 3.8 on ImageMagick 7 answers it
/// (black-box): the alpha-weighted RGB distance over √6 against the
/// fuzz (in quantum units), and alphas that differ by more than the
/// unscaled fuzz never match.
fn pixel_similar(a: &PixelInfo, b: &PixelInfo, fuzz: f64) -> bool {
    let aa = if a.alpha_trait {
        a.alpha
    } else {
        QUANTUM_RANGE
    };
    let ba = if b.alpha_trait {
        b.alpha
    } else {
        QUANTUM_RANGE
    };
    let mut scale = 1.0;
    if a.alpha_trait || b.alpha_trait {
        let p = aa - ba;
        if p * p > fuzz * fuzz / (QUANTUM_RANGE * QUANTUM_RANGE) {
            return false;
        }
        scale = (aa * QUANTUM_SCALE) * (ba * QUANTUM_SCALE);
    }
    let (r1, g1, b1) = a.rgb_q();
    let (r2, g2, b2) = b.rgb_q();
    let d: f64 = [(r1, r2), (g1, g2), (b1, b2)]
        .iter()
        .map(|(x, y)| scale * (x - y) * (x - y))
        .sum();
    d <= 6.0 * fuzz * fuzz
}

fn is_similar(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    similar(vm, f, o, args, true)
}

fn is_similar_quantum(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    similar(vm, f, o, args, false)
}

fn get_color_value(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let mut c = color(vm, &this(o))?;
    let which = a[0].long_or(0);
    if which <= COLOR_MIN || which >= COLOR_MAX {
        return throw_cls(vm, Cls::Pixel, "Unknown color type");
    }
    let fuzz = which == COLOR_FUZZ;
    match channel_ref(&mut c, which) {
        Some(v) => Ok(Value::Double(if fuzz { *v } else { *v * QUANTUM_SCALE })),
        None => throw_cls(vm, Cls::Pixel, "Unknown color type"),
    }
}

fn set_color_value(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ld", args)?;
    let which = a[0].long_or(0);
    let v = a[1].double_or(0.0);
    let d = this(o);
    let mut c = color(vm, &d)?;
    if which <= COLOR_MIN || which >= COLOR_MAX {
        return throw_cls(vm, Cls::Pixel, "Unknown color type");
    }
    let fuzz = which == COLOR_FUZZ;
    match channel_ref(&mut c, which) {
        Some(slot) => *slot = if fuzz { v } else { v * QUANTUM_RANGE },
        None => return throw_cls(vm, Cls::Pixel, "Unknown color type"),
    }
    *d.color.borrow_mut() = Some(c);
    Ok(Value::True)
}

fn round_c(v: f64) -> i64 {
    (if v > 0.0 { v + 0.5 } else { v - 0.5 }) as i64
}

fn get_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|l", args)?;
    let c = color(vm, &this(o))?;
    let norm = a[0].long_or(0);
    let (r, g, b) = (
        c.red * QUANTUM_SCALE,
        c.green * QUANTUM_SCALE,
        c.blue * QUANTUM_SCALE,
    );
    let al = c.alpha * QUANTUM_SCALE;
    let mut arr = PhpArray::new();
    match norm {
        0 => {
            set(&mut arr, "r", Value::Long(round_c(r * 255.0)));
            set(&mut arr, "g", Value::Long(round_c(g * 255.0)));
            set(&mut arr, "b", Value::Long(round_c(b * 255.0)));
            set(&mut arr, "a", Value::Long(al as i64));
        }
        1 => {
            set(&mut arr, "r", Value::Double(r));
            set(&mut arr, "g", Value::Double(g));
            set(&mut arr, "b", Value::Double(b));
            set(&mut arr, "a", Value::Double(al));
        }
        2 => {
            set(&mut arr, "r", Value::Long(round_c(r * 255.0)));
            set(&mut arr, "g", Value::Long(round_c(g * 255.0)));
            set(&mut arr, "b", Value::Long(round_c(b * 255.0)));
            set(&mut arr, "a", Value::Long(round_c(al * 255.0)));
        }
        _ => {}
    }
    Ok(arr_val(arr))
}

fn get_color_quantum(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = color(vm, &this(o))?;
    let mut arr = PhpArray::new();
    set(&mut arr, "r", Value::Long(clamp_to_quantum(c.red) as i64));
    set(&mut arr, "g", Value::Long(clamp_to_quantum(c.green) as i64));
    set(&mut arr, "b", Value::Long(clamp_to_quantum(c.blue) as i64));
    set(&mut arr, "a", Value::Long(clamp_to_quantum(c.alpha) as i64));
    Ok(arr_val(arr))
}

fn get_color_as_string(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = color(vm, &this(o))?;
    Ok(str_val(&c.to_color_string()))
}

fn clone_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    vm.emit_deprecated("ImagickPixel::clone method is deprecated and it's use should be avoided");
    let c = this(o).color.borrow().clone().unwrap_or_default();
    Ok(crate::new_pixel(vm, c))
}

fn get_color_count(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = color(vm, &this(o))?;
    Ok(Value::Long(c.count as i64))
}

fn set_color_count(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let n = a[0].long_or(0).max(0) as u64;
    update(vm, &this(o), |c| c.count = n)?;
    Ok(Value::True)
}

fn set_color_from_pixel(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let d = this(o);
    color(vm, &d)?;
    let src = match &a[0] {
        Arg::Any(v) => crate::arg_obj(vm, f, 1, v, crate::C_PIXEL, "ImagickPixel")?,
        _ => return Err(()),
    };
    let s = color(vm, &pixel_data(&src))?;
    *d.color.borrow_mut() = Some(s);
    Ok(Value::True)
}

/// clone handler (php_imagick_clone_imagickpixel_object): a copy of the
/// colour, not shared.
pub(crate) fn clone_obj(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    let c = pixel_data(src).color.borrow().clone();
    crate::attach_pixel(
        dst,
        Rc::new(PixelData {
            color: std::cell::RefCell::new(c),
        }),
    );
}
