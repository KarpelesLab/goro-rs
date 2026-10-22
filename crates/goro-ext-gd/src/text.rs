//! FreeType text: imagettftext(), imagettfbbox(), imagefttext() and
//! imageftbbox() (php_imagettftext_common()).

use goro_core::array::ArrayKey;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_gd::ft;

use crate::zpp::{self, Arg, Func};
use crate::*;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("imageftbbox", imageftbbox),
    ("imagefttext", imagefttext),
    ("imagettfbbox", imagettfbbox),
    ("imagettftext", imagettftext),
];

pub fn register(vm: &mut Vm) {
    for &(name, f) in FUNCTIONS {
        register_fn(vm, name, f);
    }
    vm.shutdown_hooks.push(|_| ft::font_cache_shutdown());
}

fn imageftbbox(vm: &mut Vm, a: &[Value]) -> R {
    common(vm, "imageftbbox", a, false)
}

fn imagefttext(vm: &mut Vm, a: &[Value]) -> R {
    common(vm, "imagefttext", a, true)
}

fn imagettfbbox(vm: &mut Vm, a: &[Value]) -> R {
    common(vm, "imagettfbbox", a, false)
}

fn imagettftext(vm: &mut Vm, a: &[Value]) -> R {
    common(vm, "imagettftext", a, true)
}

/// php_imagettftext_common()
fn common(vm: &mut Vm, name: &'static str, a: &[Value], draw: bool) -> R {
    let f = Func::new(name);
    let (im, ptsize, angle, x, y, col, fontname, text, ext) = if draw {
        let a = zpp::parse(vm, &f, "Oddlllss|a", a)?;
        (
            Some(a[0].img()),
            a[1].double_or(0.0),
            a[2].double_or(0.0),
            a[3].long_or(0),
            a[4].long_or(0),
            a[5].long_or(0),
            a[6].str_or(b"").to_vec(),
            a[7].str_or(b"").to_vec(),
            a[8].clone(),
        )
    } else {
        let a = zpp::parse(vm, &f, "ddss|a", a)?;
        (
            None,
            a[0].double_or(0.0),
            a[1].double_or(0.0),
            0,
            0,
            -1,
            a[2].str_or(b"").to_vec(),
            a[3].str_or(b"").to_vec(),
            a[4].clone(),
        )
    };
    // FT_F26Dot6 is a signed long alias
    let lo = i64::MIN as f64 / 64.0;
    let hi = i64::MAX as f64 / 64.0;
    if ptsize < lo || ptsize > hi {
        let m = format!("must be between {} and {}", lo as i64, hi as i64);
        return Err(f.value_error(vm, 2, &m));
    }
    if !ptsize.is_finite() {
        return Err(f.value_error(vm, 2, "must be finite"));
    }
    let angle = angle * (std::f64::consts::PI / 180.0);
    let mut strex = ft::FtStringExtra::default();
    let have_ext = matches!(ext, Arg::Arr(_));
    if let Arg::Arr(arr) = &ext {
        let entries: Vec<(ArrayKey, Value)> = arr
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // (a packed array has no string keys)
        for (k, v) in &entries {
            if let ArrayKey::String(s) = k {
                if s.as_bytes() == b"linespacing" {
                    strex.flags |= ft::FTEX_LINESPACE;
                    strex.linespacing = zval_get_double(vm, v);
                }
            }
        }
    }
    let mut brect = [0i32; 8];
    let font = String::from_utf8_lossy(&fontname).into_owned();
    let r = match &im {
        Some(img) => {
            let mut g = img.borrow_mut();
            ft::image_string_ft_ex(
                Some(&mut g),
                &mut brect,
                col as i32,
                &font,
                ptsize,
                angle,
                x as i32,
                y as i32,
                &text,
                if have_ext { Some(&mut strex) } else { None },
            )
        }
        None => ft::image_string_ft_ex(
            None,
            &mut brect,
            col as i32,
            &font,
            ptsize,
            angle,
            x as i32,
            y as i32,
            &text,
            if have_ext { Some(&mut strex) } else { None },
        ),
    };
    flush_errors(vm, &f);
    if let Err(e) = r {
        f.warn(vm, &e);
        return Ok(Value::False);
    }
    let v: Vec<i64> = brect.iter().map(|&n| n as i64).collect();
    Ok(long_list(&v))
}
