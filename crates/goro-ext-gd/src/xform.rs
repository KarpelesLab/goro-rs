//! imagerotate(), imagescale(), imagecrop*(), imageaffine*(), imageflip()
//! and the interpolation method functions of gd.c.

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_gd::{Rect, interp, interpolation as gi, matrix};

use crate::zpp::{self, Arg, Func};
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

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("imagerotate", imagerotate),
    ("imageflip", imageflip),
    ("imagecrop", imagecrop),
    ("imagecropauto", imagecropauto),
    ("imagescale", imagescale),
    ("imageaffine", imageaffine),
    ("imageaffinematrixget", imageaffinematrixget),
    ("imageaffinematrixconcat", imageaffinematrixconcat),
    ("imagegetinterpolation", imagegetinterpolation),
    ("imagesetinterpolation", imagesetinterpolation),
];

pub fn register(vm: &mut Vm) {
    for &(name, f) in FUNCTIONS {
        register_fn(vm, name, f);
    }
}

fn imagerotate(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagerotate", "Odl", a)?;
    let degrees = a[1].double_or(0.0);
    let lo = (i32::MIN / 100) as f64;
    let hi = (i32::MAX / 100) as f64;
    if degrees < lo || degrees > hi {
        return Err(f.value_error(
            vm,
            2,
            &format!("must be between {} and {}", i32::MIN / 100, i32::MAX / 100),
        ));
    }
    let im = a[0].img();
    let r = gi::rotate_interpolated(&mut im.borrow_mut(), degrees as f32, a[2].long_or(0) as i32);
    flush_errors(vm, &f);
    match r {
        Some(d) => wrap_image(vm, d),
        None => Ok(Value::False),
    }
}

fn imageflip(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageflip", "Ol", a)?;
    let im = a[0].img();
    match a[1].long_or(0) {
        2 => goro_gd::transform::flip_vertical(&mut im.borrow_mut()),
        1 => goro_gd::transform::flip_horizontal(&mut im.borrow_mut()),
        3 => goro_gd::transform::flip_both(&mut im.borrow_mut()),
        _ => {
            return Err(f.value_error(
                vm,
                2,
                "must be one of IMG_FLIP_VERTICAL, IMG_FLIP_HORIZONTAL, or IMG_FLIP_BOTH",
            ));
        }
    }
    Ok(Value::True)
}

fn str_key(a: &PhpArray, k: &str) -> Option<Value> {
    a.get(&ArrayKey::String(PhpString::from_bytes(k.as_bytes())))
        .map(|v| v.deref())
}

/// The x/y/width/height keys of a rectangle array (argument `n`).
fn rect_arg(vm: &mut Vm, f: &Func, n: usize, arr: &PhpArray) -> Result<Rect, VmError> {
    let mut r = [0i32; 4];
    for (i, (k, art)) in [("x", "an"), ("y", "a"), ("width", "a"), ("height", "a")]
        .iter()
        .enumerate()
    {
        match str_key(arr, k) {
            Some(v) => r[i] = zval_get_long(vm, &v) as i32,
            None => return Err(f.value_error(vm, n, &format!("must have {} \"{}\" key", art, k))),
        }
    }
    Ok(Rect {
        x: r[0],
        y: r[1],
        width: r[2],
        height: r[3],
    })
}

fn imagecrop(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecrop", "Oa", a)?;
    let rect = {
        let arr = a[1].arr();
        let arr = arr.borrow();
        rect_arg(vm, &f, 2, &arr)?
    };
    let (x, y, w, h) = (
        rect.x as i64,
        rect.y as i64,
        rect.width as i64,
        rect.height as i64,
    );
    if w > 0 && x > INT_MAX - w {
        return Err(f.value_error(vm, 2, "overflow with \"x\" and \"width\" keys"));
    }
    if w < 0 && x < INT_MIN - w {
        return Err(f.value_error(vm, 2, "underflow with \"x\" and \"width\" keys"));
    }
    if h > 0 && y > INT_MAX - h {
        return Err(f.value_error(vm, 2, "overflow with \"y\" and \"height\" keys"));
    }
    if h < 0 && y < INT_MIN - h {
        return Err(f.value_error(vm, 2, "underflow with \"y\" and \"height\" keys"));
    }
    let r = goro_gd::crop::crop(&a[0].img().borrow(), &rect);
    flush_errors(vm, &f);
    match r {
        Some(d) => wrap_image(vm, d),
        None => Ok(Value::False),
    }
}

fn imagecropauto(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagecropauto", "O|ldl", a)?;
    let im = a[0].img();
    let mode = a[1].long_or(0);
    let threshold = a[2].double_or(0.5);
    let color = a[3].long_or(-1);
    let r = match mode {
        0..=4 => goro_gd::crop::crop_auto(&im.borrow(), mode as u32),
        5 => {
            let (tc, total) = {
                let i = im.borrow();
                (i.true_color, i.colors_total as i64)
            };
            if color < 0 || (!tc && color >= total) {
                return Err(f.value_error(
                    vm,
                    4,
                    "must be greater than or equal to 0 when using the threshold mode",
                ));
            }
            goro_gd::crop::crop_threshold(&im.borrow(), color as u32, threshold as f32)
        }
        _ => return Err(f.value_error(vm, 2, "must be a valid mode")),
    };
    flush_errors(vm, &f);
    match r {
        Some(d) => wrap_image(vm, d),
        None => Ok(Value::False),
    }
}

fn imagescale(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagescale", "Ol|ll", a)?;
    let mut tmp_w = a[1].long_or(0);
    let mut tmp_h = a[2].long_or(-1);
    let tmp_m = a[3].long_or(interp::BILINEAR_FIXED as i64);
    if tmp_m < interp::DEFAULT as i64 || tmp_m >= interp::METHOD_COUNT as i64 {
        return Err(f.value_error(vm, 4, "must be one of the GD_* constants"));
    }
    let im = a[0].img();
    if tmp_h < 0 && tmp_w < 0 {
        return Err(f.plain_value_error(
            vm,
            "Argument #2 ($width) and argument #3 ($height) cannot be both negative",
        ));
    }
    if tmp_h < 0 || tmp_w < 0 {
        let (src_x, src_y) = {
            let i = im.borrow();
            (i.sx as i64, i.sy as i64)
        };
        if src_x != 0 && tmp_h < 0 {
            if tmp_w > i64::MAX / src_y {
                return Err(f.value_error(
                    vm,
                    2,
                    &format!("must be less than or equal to {}", i64::MAX / src_y),
                ));
            }
            tmp_h = tmp_w * src_y / src_x;
        }
        if src_y != 0 && tmp_w < 0 {
            if tmp_h > i64::MAX / src_x {
                return Err(f.value_error(
                    vm,
                    3,
                    &format!("must be less than or equal to {}", i64::MAX / src_x),
                ));
            }
            tmp_w = tmp_h * src_x / src_y;
        }
    }
    if tmp_w <= 0 || tmp_w > INT_MAX {
        return Err(f.value_error(vm, 2, &format!("must be between 1 and {}", i32::MAX)));
    }
    if tmp_h <= 0 || tmp_h > INT_MAX {
        return Err(f.value_error(vm, 3, &format!("must be between 1 and {}", i32::MAX)));
    }
    let r = {
        let mut i = im.borrow_mut();
        let old = i.interpolation_id;
        let mut r = None;
        if gi::set_interpolation_method(&mut i, tmp_m as i32) {
            r = gi::scale(&mut i, tmp_w as u32, tmp_h as u32);
        }
        gi::set_interpolation_method(&mut i, old);
        r
    };
    flush_errors(vm, &f);
    match r {
        Some(d) => wrap_image(vm, d),
        None => Ok(Value::False),
    }
}

/// An element of a matrix array as IS_LONG / IS_DOUBLE / IS_STRING
fn matrix_elem(v: &Value) -> Option<f64> {
    match v {
        Value::Long(_) | Value::Double(_) | Value::String(_) => Some(v.to_double()),
        _ => None,
    }
}

fn imageaffine(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageaffine", "Oa|a!", a)?;
    let src = a[0].img();
    let mut affine = [0f64; 6];
    {
        let z = a[1].arr();
        let z = z.borrow();
        if z.len() != 6 {
            drop(z);
            return Err(f.value_error(vm, 2, "must have 6 elements"));
        }
        for (i, slot) in affine.iter_mut().enumerate() {
            if let Some(v) = z.get(&ArrayKey::Int(i as i64)) {
                let v = v.deref();
                match matrix_elem(&v) {
                    Some(d) => {
                        if d < INT_MIN as f64 || d > INT_MAX as f64 {
                            return Err(f.value_error(
                                vm,
                                2,
                                &format!(
                                    "element {} must be between {} and {}",
                                    i,
                                    i32::MIN,
                                    i32::MAX
                                ),
                            ));
                        }
                        *slot = d;
                    }
                    None => {
                        return Err(f.arg_type_error(
                            vm,
                            3,
                            &format!("contains invalid type for element {}", i),
                        ));
                    }
                }
            }
        }
    }
    let mut rect = match &a[2] {
        Arg::Arr(r) => {
            let r = r.borrow();
            Some(rect_arg(vm, &f, 3, &r)?)
        }
        _ => None,
    };
    let r = gi::transform_affine_get_image(&mut src.borrow_mut(), rect.as_mut(), &affine);
    flush_errors(vm, &f);
    match r {
        Some(d) => wrap_image(vm, d),
        None => Ok(Value::False),
    }
}

fn double_list(v: &[f64]) -> Value {
    let mut a = PhpArray::new();
    for &d in v {
        a.push(Value::Double(d));
    }
    varr(a)
}

fn imageaffinematrixget(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageaffinematrixget", "lz", a)?;
    let ty = a[0].long_or(0);
    let options = match &a[1] {
        Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };
    let m = match ty {
        0 | 1 => {
            let Value::Array(arr) = &options else {
                return Err(f.arg_type_error(
                    vm,
                    1,
                    "must be of type array when using translate or scale",
                ));
            };
            let arr = arr.borrow();
            let x = match str_key(&arr, "x") {
                Some(v) => zval_get_double(vm, &v),
                None => return Err(f.value_error(vm, 2, "must have an \"x\" key")),
            };
            let y = match str_key(&arr, "y") {
                Some(v) => zval_get_double(vm, &v),
                None => return Err(f.value_error(vm, 2, "must have a \"y\" key")),
            };
            if ty == 0 {
                matrix::translate(x, y)
            } else {
                matrix::scale(x, y)
            }
        }
        2..=4 => {
            let angle = zval_get_double(vm, &options);
            match ty {
                3 => matrix::shear_horizontal(angle),
                4 => matrix::shear_vertical(angle),
                _ => matrix::rotate(angle),
            }
        }
        _ => return Err(f.value_error(vm, 1, "must be a valid element type")),
    };
    Ok(double_list(&m))
}

fn imageaffinematrixconcat(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imageaffinematrixconcat", "aa", a)?;
    let (z1, z2) = (a[0].arr(), a[1].arr());
    let (z1, z2) = (z1.borrow(), z2.borrow());
    if z1.len() != 6 || z2.len() != 6 {
        return Err(f.value_error(vm, 1, "must have 6 elements"));
    }
    let mut m1 = [0f64; 6];
    let mut m2 = [0f64; 6];
    for i in 0..6 {
        if let Some(v) = z1.get(&ArrayKey::Int(i as i64)) {
            match matrix_elem(&v.deref()) {
                Some(d) => m1[i] = d,
                None => {
                    return Err(f.arg_type_error(
                        vm,
                        1,
                        &format!("contains invalid type for element {}", i),
                    ));
                }
            }
        }
        if let Some(v) = z2.get(&ArrayKey::Int(i as i64)) {
            match matrix_elem(&v.deref()) {
                Some(d) => m2[i] = d,
                None => {
                    return Err(f.arg_type_error(
                        vm,
                        2,
                        &format!("contains invalid type for element {}", i),
                    ));
                }
            }
        }
    }
    Ok(double_list(&matrix::concat(&m1, &m2)))
}

fn imagegetinterpolation(vm: &mut Vm, a: &[Value]) -> R {
    let (_, a) = args(vm, "imagegetinterpolation", "O", a)?;
    Ok(Value::Long(
        gi::get_interpolation_method(&a[0].img().borrow()) as i64,
    ))
}

fn imagesetinterpolation(vm: &mut Vm, a: &[Value]) -> R {
    let (f, a) = args(vm, "imagesetinterpolation", "O|l", a)?;
    let mut method = a[1].long_or(interp::BILINEAR_FIXED as i64);
    if method == -1 {
        method = interp::BILINEAR_FIXED as i64;
    }
    let r = gi::set_interpolation_method(&mut a[0].img().borrow_mut(), method as i32);
    flush_errors(vm, &f);
    Ok(vbool(r))
}
