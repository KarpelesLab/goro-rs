//! gd_interpolation.c: the interpolation kernels, gdImageScale(),
//! gdImageRotateInterpolated() and the affine transformations.

// The kernel constants are kept digit for digit as in the C source.
#![allow(clippy::excessive_precision)]

use crate::matrix;
use crate::rotate::{rotate90, rotate180, rotate270};
use crate::*;
use std::f64::consts::PI;

// ---------------------------------------------------------------------------
// fixed point helpers (gdFixed is a long)

#[inline]
fn itofx(x: i64) -> i64 {
    ((x as u64) << 8) as i64
}
#[inline]
fn ftofx(x: f32) -> i64 {
    (x * 256.0) as i64
}
#[inline]
fn dtofx(x: f64) -> i64 {
    (x * 256.0) as i64
}
#[inline]
fn fxtoi(x: i64) -> i64 {
    x >> 8
}
#[inline]
fn fxtod(x: i64) -> f64 {
    x as f64 / 256.0
}
#[inline]
fn mulfx(x: i64, y: i64) -> i64 {
    x.wrapping_mul(y) >> 8
}
#[inline]
fn divfx(x: i64, y: i64) -> i64 {
    if y == 0 {
        return 0;
    }
    (((x as u64) << 8) as i64).wrapping_div(y)
}

#[inline]
fn clamp_i(x: i64, lo: i64, hi: i64) -> i64 {
    if x > hi {
        hi
    } else if x < lo {
        lo
    } else {
        x
    }
}

#[inline]
fn clamp_d(x: f64, lo: f64, hi: f64) -> f64 {
    if x > hi {
        hi
    } else if x < lo {
        lo
    } else {
        x
    }
}

/// A float literal promoted to double (C `0.42f` in a double expression)
#[inline]
fn f(x: f32) -> f64 {
    x as f64
}

// ---------------------------------------------------------------------------
// kernels

fn kernel_bessel_j1(x: f64) -> f64 {
    const PONE: [f64; 9] = [
        0.581199354001606143928050809e+21,
        -0.6672106568924916298020941484e+20,
        0.2316433580634002297931815435e+19,
        -0.3588817569910106050743641413e+17,
        0.2908795263834775409737601689e+15,
        -0.1322983480332126453125473247e+13,
        0.3413234182301700539091292655e+10,
        -0.4695753530642995859767162166e+7,
        0.270112271089232341485679099e+4,
    ];
    const QONE: [f64; 9] = [
        0.11623987080032122878585294e+22,
        0.1185770712190320999837113348e+20,
        0.6092061398917521746105196863e+17,
        0.2081661221307607351240184229e+15,
        0.5243710262167649715406728642e+12,
        0.1013863514358673989967045588e+10,
        0.1501793594998585505921097578e+7,
        0.1606931573481487801970916749e+4,
        0.1e+1,
    ];
    let mut p = PONE[8];
    let mut q = QONE[8];
    for i in (0..8).rev() {
        p = p * x * x + PONE[i];
        q = q * x * x + QONE[i];
    }
    p / q
}

fn kernel_bessel_p1(x: f64) -> f64 {
    const PONE: [f64; 6] = [
        0.352246649133679798341724373e+5,
        0.62758845247161281269005675e+5,
        0.313539631109159574238669888e+5,
        0.49854832060594338434500455e+4,
        0.2111529182853962382105718e+3,
        0.12571716929145341558495e+1,
    ];
    const QONE: [f64; 6] = [
        0.352246649133679798068390431e+5,
        0.626943469593560511888833731e+5,
        0.312404063819041039923015703e+5,
        0.4930396490181088979386097e+4,
        0.2030775189134759322293574e+3,
        0.1e+1,
    ];
    let mut p = PONE[5];
    let mut q = QONE[5];
    for i in (0..5).rev() {
        p = p * (8.0 / x) * (8.0 / x) + PONE[i];
        q = q * (8.0 / x) * (8.0 / x) + QONE[i];
    }
    p / q
}

fn kernel_bessel_q1(x: f64) -> f64 {
    const PONE: [f64; 6] = [
        0.3511751914303552822533318e+3,
        0.7210391804904475039280863e+3,
        0.4259873011654442389886993e+3,
        0.831898957673850827325226e+2,
        0.45681716295512267064405e+1,
        0.3532840052740123642735e-1,
    ];
    const QONE: [f64; 6] = [
        0.74917374171809127714519505e+4,
        0.154141773392650970499848051e+5,
        0.91522317015169922705904727e+4,
        0.18111867005523513506724158e+4,
        0.1038187585462133728776636e+3,
        0.1e+1,
    ];
    let mut p = PONE[5];
    let mut q = QONE[5];
    for i in (0..5).rev() {
        p = p * (8.0 / x) * (8.0 / x) + PONE[i];
        q = q * (8.0 / x) * (8.0 / x) + QONE[i];
    }
    p / q
}

fn kernel_bessel_order1(mut x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    let p = x;
    if x < 0.0 {
        x = -x;
    }
    if x < 8.0 {
        return p * kernel_bessel_j1(x);
    }
    let s2 = 2.0f64.sqrt();
    let mut q = (2.0 / (PI * x)).sqrt()
        * (kernel_bessel_p1(x) * (1.0 / s2 * (x.sin() - x.cos()))
            - 8.0 / x * kernel_bessel_q1(x) * (-1.0 / s2 * (x.sin() + x.cos())));
    if p < 0.0 {
        q = -q;
    }
    q
}

pub fn filter_bessel(x: f64) -> f64 {
    if x == 0.0 {
        return PI / 4.0;
    }
    kernel_bessel_order1(PI * x) / (2.0 * x)
}

pub fn filter_blackman(x: f64) -> f64 {
    f(0.42) + 0.5 * (PI * x).cos() + f(0.08) * (2.0 * PI * x).cos()
}

pub fn filter_linear(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 1.0 { 1.0 - ax } else { 0.0 }
}

pub fn filter_bicubic(t: f64) -> f64 {
    let abs_t = t.abs();
    let abs_t_sq = abs_t * abs_t;
    if abs_t < 1.0 {
        return 1.0 - 2.0 * abs_t_sq + abs_t_sq * abs_t;
    }
    if abs_t < 2.0 {
        return 4.0 - 8.0 * abs_t + 5.0 * abs_t_sq - abs_t_sq * abs_t;
    }
    0.0
}

pub fn filter_generalized_cubic(t: f64) -> f64 {
    let a = -f(0.5);
    let abs_t = t.abs();
    let abs_t_sq = abs_t * abs_t;
    if abs_t < 1.0 {
        return (a + 2.0) * abs_t_sq * abs_t - (a + 3.0) * abs_t_sq + 1.0;
    }
    if abs_t < 2.0 {
        return a * abs_t_sq * abs_t - 5.0 * a * abs_t_sq + 8.0 * a * abs_t - 4.0 * a;
    }
    0.0
}

pub fn filter_box(x: f64) -> f64 {
    if x < -f(0.5) {
        return 0.0;
    }
    if x < f(0.5) {
        return 1.0;
    }
    0.0
}

pub fn filter_catmullrom(x: f64) -> f64 {
    if x < -2.0 {
        return 0.0;
    }
    if x < -1.0 {
        return 0.5 * (4.0 + x * (8.0 + x * (5.0 + x)));
    }
    if x < 0.0 {
        return 0.5 * (2.0 + x * x * (-5.0 - 3.0 * x));
    }
    if x < 1.0 {
        return 0.5 * (2.0 + x * x * (-5.0 + 3.0 * x));
    }
    if x < 2.0 {
        return 0.5 * (4.0 + x * (-8.0 + x * (5.0 - x)));
    }
    0.0
}

pub fn filter_hermite(x1: f64) -> f64 {
    let x = if x1 < 0.0 { -x1 } else { x1 };
    if x < 1.0 {
        return (2.0 * x - 3.0) * x * x + 1.0;
    }
    0.0
}

pub fn filter_triangle(x1: f64) -> f64 {
    let x = if x1 < 0.0 { -x1 } else { x1 };
    if x < 1.0 {
        return 1.0 - x;
    }
    0.0
}

pub fn filter_bell(x1: f64) -> f64 {
    let x = if x1 < 0.0 { -x1 } else { x1 };
    if x < 0.5 {
        return 0.75 - x * x;
    }
    if x < 1.5 {
        return 0.5 * (x - 1.5).powf(2.0);
    }
    0.0
}

pub fn filter_mitchell(x: f64) -> f64 {
    const KM_B: f32 = 1.0f32 / 3.0f32;
    const KM_C: f32 = 1.0f32 / 3.0f32;
    let km_p0 = f((6.0f32 - 2.0f32 * KM_B) / 6.0f32);
    let km_p2 = f((-18.0f32 + 12.0f32 * KM_B + 6.0f32 * KM_C) / 6.0f32);
    let km_p3 = f((12.0f32 - 9.0f32 * KM_B - 6.0f32 * KM_C) / 6.0f32);
    let km_q0 = f((8.0f32 * KM_B + 24.0f32 * KM_C) / 6.0f32);
    let km_q1 = f((-12.0f32 * KM_B - 48.0f32 * KM_C) / 6.0f32);
    let km_q2 = f((6.0f32 * KM_B + 30.0f32 * KM_C) / 6.0f32);
    let km_q3 = f((-KM_B - 6.0f32 * KM_C) / 6.0f32);
    if x < -2.0 {
        return 0.0;
    }
    if x < -1.0 {
        return km_q0 - x * (km_q1 - x * (km_q2 - x * km_q3));
    }
    if x < 0.0 {
        return km_p0 + x * x * (km_p2 - x * km_p3);
    }
    if x < 1.0 {
        return km_p0 + x * x * (km_p2 + x * km_p3);
    }
    if x < 2.0 {
        return km_q0 + x * (km_q1 + x * (km_q2 + x * km_q3));
    }
    0.0
}

pub fn filter_quadratic(x1: f64) -> f64 {
    let x = if x1 < 0.0 { -x1 } else { x1 };
    if x <= 0.5 {
        return -2.0 * x * x + 1.0;
    }
    if x <= 1.5 {
        return x * x - 2.5 * x + 1.5;
    }
    0.0
}

#[allow(clippy::excessive_precision)]
pub fn filter_bspline(x: f64) -> f64 {
    if x > 2.0 {
        return 0.0;
    }
    let xm1 = x - 1.0;
    let xp1 = x + 1.0;
    let xp2 = x + 2.0;
    let a = if xp2 <= 0.0 { 0.0 } else { xp2 * xp2 * xp2 };
    let b = if xp1 <= 0.0 { 0.0 } else { xp1 * xp1 * xp1 };
    let c = if x <= 0.0 { 0.0 } else { x * x * x };
    let d = if xm1 <= 0.0 { 0.0 } else { xm1 * xm1 * xm1 };
    f(0.16666666666666666667) * (a - (4.0 * b) + (6.0 * c) - (4.0 * d))
}

#[allow(clippy::excessive_precision)]
pub fn filter_gaussian(x: f64) -> f64 {
    (-2.0 * x * x).exp() * f(0.79788456080287)
}

pub fn filter_hanning(x: f64) -> f64 {
    0.5 + 0.5 * (PI * x).cos()
}

pub fn filter_hamming(x: f64) -> f64 {
    if x < -1.0 {
        return 0.0;
    }
    if x < 0.0 {
        return f(0.92) * (-2.0 * x - 3.0) * x * x + 1.0;
    }
    if x < 1.0 {
        return f(0.92) * (2.0 * x - 3.0) * x * x + 1.0;
    }
    0.0
}

pub fn filter_power(x: f64) -> f64 {
    let a = 2.0;
    if x.abs() > 1.0 {
        return 0.0;
    }
    1.0 - x.powf(a).abs()
}

pub fn filter_sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    (PI * x).sin() / (PI * x)
}

// ---------------------------------------------------------------------------
// pixel access

#[inline]
fn color_index_to_rgba(im: &Image, c: i32) -> i32 {
    let c = (c as usize) & 0xff;
    true_color_alpha(im.red[c], im.green[c], im.blue[c], im.alpha[c])
}

/// getPixelOverflowTC()
#[inline]
fn get_pixel_overflow_tc(im: &Image, x: i32, y: i32, bg_color: i32) -> i32 {
    if im.bounds_safe(x, y) {
        let c = im.tpixels[y as usize][x as usize];
        if c == im.transparent {
            return if bg_color == -1 {
                true_color_alpha(0, 0, 0, 127)
            } else {
                bg_color
            };
        }
        c
    } else {
        bg_color
    }
}

/// getPixelOverflowPalette()
#[inline]
fn get_pixel_overflow_palette(im: &Image, x: i32, y: i32, bg_color: i32) -> i32 {
    if im.bounds_safe(x, y) {
        let c = im.pixels[y as usize][x as usize] as i32;
        if c == im.transparent {
            return if bg_color == -1 {
                true_color_alpha(0, 0, 0, 127)
            } else {
                bg_color
            };
        }
        color_index_to_rgba(im, c)
    } else {
        bg_color
    }
}

/// getPixelInterpolateWeight()
fn get_pixel_interpolate_weight(im: &Image, x: f64, y: f64, bg_color: i32) -> i32 {
    let sx = x as i32;
    let sy = y as i32;
    let xf = x - sx as f64;
    let yf = y - sy as f64;
    let nxf = 1.0 - xf;
    let nyf = 1.0 - yf;
    let m1 = xf * yf;
    let m2 = nxf * yf;
    let m3 = xf * nyf;
    let m4 = nxf * nyf;
    let tc = im.true_color;
    let c1 = if tc {
        get_pixel_overflow_tc(im, sx, sy, bg_color)
    } else {
        get_pixel_overflow_palette(im, sx, sy, bg_color)
    };
    let c2 = if tc {
        get_pixel_overflow_tc(im, sx - 1, sy, bg_color)
    } else {
        get_pixel_overflow_palette(im, sx - 1, sy, bg_color)
    };
    let c3 = if tc {
        get_pixel_overflow_tc(im, sx, sy - 1, bg_color)
    } else {
        get_pixel_overflow_palette(im, sx, sy - 1, bg_color)
    };
    let c4 = if tc {
        get_pixel_overflow_tc(im, sx - 1, sy - 1, bg_color)
    } else {
        get_pixel_overflow_palette(im, sx, sy - 1, bg_color)
    };
    let (r, g, b, a);
    if tc {
        r = (m1 * true_color_get_red(c1) as f64
            + m2 * true_color_get_red(c2) as f64
            + m3 * true_color_get_red(c3) as f64
            + m4 * true_color_get_red(c4) as f64) as i32;
        g = (m1 * true_color_get_green(c1) as f64
            + m2 * true_color_get_green(c2) as f64
            + m3 * true_color_get_green(c3) as f64
            + m4 * true_color_get_green(c4) as f64) as i32;
        b = (m1 * true_color_get_blue(c1) as f64
            + m2 * true_color_get_blue(c2) as f64
            + m3 * true_color_get_blue(c3) as f64
            + m4 * true_color_get_blue(c4) as f64) as i32;
        a = (m1 * true_color_get_alpha(c1) as f64
            + m2 * true_color_get_alpha(c2) as f64
            + m3 * true_color_get_alpha(c3) as f64
            + m4 * true_color_get_alpha(c4) as f64) as i32;
    } else {
        // (indexes the palette with the RGBA values, as the C code does;
        // unreachable in practice: callers convert to truecolor first)
        let ix = |c: i32| (c as usize) & 0xff;
        r = (m1 * im.red[ix(c1)] as f64
            + m2 * im.red[ix(c2)] as f64
            + m3 * im.red[ix(c3)] as f64
            + m4 * im.red[ix(c4)] as f64) as i32;
        g = (m1 * im.green[ix(c1)] as f64
            + m2 * im.green[ix(c2)] as f64
            + m3 * im.green[ix(c3)] as f64
            + m4 * im.green[ix(c4)] as f64) as i32;
        b = (m1 * im.blue[ix(c1)] as f64
            + m2 * im.blue[ix(c2)] as f64
            + m3 * im.blue[ix(c3)] as f64
            + m4 * im.blue[ix(c4)] as f64) as i32;
        a = (m1 * im.alpha[ix(c1)] as f64
            + m2 * im.alpha[ix(c2)] as f64
            + m3 * im.alpha[ix(c3)] as f64
            + m4 * im.alpha[ix(c4)] as f64) as i32;
    }
    let r = r.clamp(0, 255);
    let g = g.clamp(0, 255);
    let b = b.clamp(0, 255);
    let a = a.clamp(0, ALPHA_MAX);
    true_color_alpha(r, g, b, a)
}

/// getPixelInterpolated(): the color at (x, y) using the image's
/// interpolation method; -1 for nearest neighbour / no kernel
pub fn get_pixel_interpolated(im: &Image, x: f64, y: f64, bg_color: i32) -> i32 {
    let xi = x as i32;
    let yi = y as i32;
    if im.interpolation_id == interp::NEAREST_NEIGHBOUR {
        return -1;
    }
    if im.interpolation_id == interp::WEIGHTED4 {
        return get_pixel_interpolate_weight(im, x, y, bg_color);
    }
    let Some(k) = im.interpolation else {
        return -1;
    };
    let mut kernel_x = [0f64; 4];
    let mut kernel_y = [0f64; 4];
    for i in 0..4 {
        kernel_x[i] = k((xi.wrapping_add(i as i32 - 1)) as f64 - x);
        kernel_y[i] = k((yi.wrapping_add(i as i32 - 1)) as f64 - y);
    }
    let (mut new_r, mut new_g, mut new_b, mut new_a) = (0f64, 0f64, 0f64, 0f64);
    for (ky, yii) in (yi.wrapping_sub(1)..).take(4).enumerate() {
        let kernel_cache_y = kernel_y[ky];
        for (kx, xii) in (xi.wrapping_sub(1)..).take(4).enumerate() {
            let rgbs = if im.true_color {
                get_pixel_overflow_tc(im, xii, yii, bg_color)
            } else {
                get_pixel_overflow_palette(im, xii, yii, bg_color)
            };
            let kernel = kernel_cache_y * kernel_x[kx];
            new_r += kernel * true_color_get_red(rgbs) as f64;
            new_g += kernel * true_color_get_green(rgbs) as f64;
            new_b += kernel * true_color_get_blue(rgbs) as f64;
            new_a += kernel * true_color_get_alpha(rgbs) as f64;
        }
    }
    let new_r = clamp_d(new_r, 0.0, 255.0);
    let new_g = clamp_d(new_g, 0.0, 255.0);
    let new_b = clamp_d(new_b, 0.0, 255.0);
    let new_a = clamp_d(new_a, 0.0, ALPHA_MAX as f64);
    true_color_alpha(new_r as i32, new_g as i32, new_b as i32, new_a as i32)
}

// ---------------------------------------------------------------------------
// two pass scaling

struct Contribution {
    weights: Vec<f64>,
    left: i32,
    right: i32,
}

/// _gdContributionsCalc()
fn contributions_calc(
    line_size: u32,
    src_size: u32,
    scale_d: f64,
    filter: InterpolationFn,
) -> Option<Vec<Contribution>> {
    let filter_width_d = f(0.5);
    let mut scale_f_d = 1.0;
    let width_d = if scale_d < 1.0 {
        scale_f_d = scale_d;
        filter_width_d / scale_d
    } else {
        filter_width_d
    };
    let windows_size = 2 * (width_d.ceil() as i32) + 1;
    if overflow2(windows_size, 8) {
        return None;
    }
    if overflow2(line_size as i32, 16) {
        return None;
    }
    let mut res = Vec::with_capacity(line_size as usize);
    for u in 0..line_size {
        let d_center = u as f64 / scale_d;
        let mut i_left = 0.max((d_center - width_d).floor() as i32);
        let mut i_right = ((d_center + width_d).ceil() as i32).min(src_size as i32 - 1);
        let mut total = 0.0;
        if i_right - i_left + 1 > windows_size {
            if i_left < src_size as i32 {
                i_left += 1;
            } else {
                i_right -= 1;
            }
        }
        let mut weights = vec![0f64; windows_size as usize];
        let mut src = i_left;
        while src <= i_right {
            let w = scale_f_d * filter(scale_f_d * (d_center - src as f64));
            weights[(src - i_left) as usize] = w;
            total += w;
            src += 1;
        }
        if total < 0.0 {
            return None;
        }
        if total > 0.0 {
            let mut src = i_left;
            while src <= i_right {
                weights[(src - i_left) as usize] /= total;
                src += 1;
            }
        }
        res.push(Contribution {
            weights,
            left: i_left,
            right: i_right,
        });
    }
    Some(res)
}

/// uchar_clamp()
#[inline]
fn uchar_clamp(clr: f64, max: u8) -> i32 {
    let mut result = (clr + 0.5) as i32 as i16 as u16;
    if result > max as u16 {
        result = if clr < 0.0 { 0 } else { max as u16 };
    }
    result as u8 as i32
}

fn accumulate(acc: &mut [f64; 4], w: f64, p: i32) {
    acc[0] += w * true_color_get_red(p) as f64;
    acc[1] += w * true_color_get_green(p) as f64;
    acc[2] += w * true_color_get_blue(p) as f64;
    acc[3] += w * true_color_get_alpha(p) as f64;
}

fn pack(acc: &[f64; 4]) -> i32 {
    true_color_alpha(
        uchar_clamp(acc[0], 0xFF),
        uchar_clamp(acc[1], 0xFF),
        uchar_clamp(acc[2], 0xFF),
        uchar_clamp(acc[3], 0x7F),
    )
}

/// _gdScaleHoriz()
fn scale_horiz(
    src: &Image,
    src_width: u32,
    dst: &mut Image,
    dst_width: u32,
    dst_height: u32,
) -> bool {
    let Some(filter) = src.interpolation else {
        return false;
    };
    let Some(contrib) = contributions_calc(
        dst_width,
        src_width,
        dst_width as f64 / src_width as f64,
        filter,
    ) else {
        return false;
    };
    for row in 0..dst_height as usize {
        let src_row = &src.tpixels[row];
        let dst_row = &mut dst.tpixels[row];
        for (x, c) in contrib.iter().enumerate() {
            let mut acc = [0f64; 4];
            let mut i = c.left;
            while i <= c.right {
                accumulate(
                    &mut acc,
                    c.weights[(i - c.left) as usize],
                    src_row[i as usize],
                );
                i += 1;
            }
            dst_row[x] = pack(&acc);
        }
    }
    true
}

/// _gdScaleVert()
fn scale_vert(
    src: &Image,
    src_height: u32,
    dst: &mut Image,
    dst_width: u32,
    dst_height: u32,
) -> bool {
    let Some(filter) = src.interpolation else {
        return false;
    };
    let Some(contrib) = contributions_calc(
        dst_height,
        src_height,
        dst_height as f64 / src_height as f64,
        filter,
    ) else {
        return false;
    };
    for col in 0..dst_width as usize {
        for (y, c) in contrib.iter().enumerate() {
            let mut acc = [0f64; 4];
            let mut i = c.left;
            while i <= c.right {
                accumulate(
                    &mut acc,
                    c.weights[(i - c.left) as usize],
                    src.tpixels[i as usize][col],
                );
                i += 1;
            }
            dst.tpixels[y][col] = pack(&acc);
        }
    }
    true
}

/// gdImageScaleTwoPass()
fn scale_two_pass(src: &mut Image, new_width: u32, new_height: u32) -> Option<Image> {
    let src_width = src.sx as u32;
    let src_height = src.sy as u32;
    if !src.true_color {
        src.palette_to_true_color();
    }
    let mut tmp = Image::create_true_color(new_width as i32, src_height as i32)?;
    set_interpolation_method(&mut tmp, src.interpolation_id);
    if !scale_horiz(src, src_width, &mut tmp, new_width, src_height) {
        return None;
    }
    let mut dst = Image::create_true_color(new_width as i32, new_height as i32)?;
    set_interpolation_method(&mut dst, src.interpolation_id);
    if !scale_vert(&tmp, src_height, &mut dst, new_width, new_height) {
        return None;
    }
    Some(dst)
}

// ---------------------------------------------------------------------------
// fixed point scaling

/// gdImageScaleNearestNeighbour()
fn scale_nearest_neighbour(im: &Image, width: u32, height: u32) -> Option<Image> {
    let new_width = width.max(1) as u64;
    let new_height = height.max(1) as u64;
    let dx = im.sx as f32 / new_width as f32;
    let dy = im.sy as f32 / new_height as f32;
    let f_dx = ftofx(dx);
    let f_dy = ftofx(dy);
    let mut dst = Image::create_true_color(new_width as i32, new_height as i32)?;
    for i in 0..new_height {
        let f_a = mulfx(itofx(i as i64), f_dy);
        let m = fxtoi(f_a) as usize;
        for j in 0..new_width {
            let f_b = mulfx(itofx(j as i64), f_dx);
            let n = fxtoi(f_b) as usize;
            let v = if im.true_color {
                im.tpixels
                    .get(m)
                    .and_then(|r| r.get(n))
                    .copied()
                    .unwrap_or(0)
            } else {
                let p = im
                    .pixels
                    .get(m)
                    .and_then(|r| r.get(n))
                    .copied()
                    .unwrap_or(0);
                color_index_to_rgba(im, p as i32)
            };
            dst.tpixels[i as usize][j as usize] = v;
        }
    }
    Some(dst)
}

/// The bilinear fixed point body of gdImageScaleBilinearPalette/TC()
fn scale_bilinear(im: &Image, new_width: u32, new_height: u32) -> Option<Image> {
    let w = new_width.max(1) as i64;
    let h = new_height.max(1) as i64;
    let dx = im.sx as f32 / w as f32;
    let dy = im.sy as f32 / h as f32;
    let f_dx = ftofx(dx);
    let f_dy = ftofx(dy);
    let f_1 = itofx(1);
    let mut new_img = Image::create_true_color(new_width as i32, new_height as i32)?;
    let tc = im.true_color;
    if !tc {
        let t = im.transparent;
        new_img.transparent = if t < 0 {
            -1
        } else {
            color_index_to_rgba(im, t)
        };
    }
    let px = |x: i32, y: i32, bg: i32| -> i32 {
        if tc {
            get_pixel_overflow_tc(im, x, y, bg)
        } else {
            get_pixel_overflow_palette(im, x, y, bg)
        }
    };
    for i in 0..h {
        let f_a = mulfx(itofx(i), f_dy);
        let m = fxtoi(f_a);
        for j in 0..w {
            let f_b = mulfx(itofx(j), f_dx);
            let n = fxtoi(f_b);
            let f_f = f_a - itofx(m);
            let f_g = f_b - itofx(n);
            let f_w1 = mulfx(f_1 - f_f, f_1 - f_g);
            let f_w2 = mulfx(f_1 - f_f, f_g);
            let f_w3 = mulfx(f_f, f_1 - f_g);
            let f_w4 = mulfx(f_f, f_g);
            let (n, m) = (n as i32, m as i32);
            let p1 = px(n, m, 0);
            let p2 = px(n + 1, m, p1);
            let p3 = px(n, m + 1, p1);
            let p4 = px(n + 1, m + 1, p1);
            let ch = |get: fn(i32) -> i32| -> i32 {
                let v = mulfx(f_w1, itofx(get(p1) as i64))
                    + mulfx(f_w2, itofx(get(p2) as i64))
                    + mulfx(f_w3, itofx(get(p3) as i64))
                    + mulfx(f_w4, itofx(get(p4) as i64));
                fxtoi(v) as u8 as i32
            };
            let red = ch(true_color_get_red);
            let green = ch(true_color_get_green);
            let blue = ch(true_color_get_blue);
            let alpha = ch(true_color_get_alpha);
            new_img.tpixels[i as usize][j as usize] = true_color_alpha(red, green, blue, alpha);
        }
    }
    Some(new_img)
}

/// gdImageScaleBicubicFixed()
fn scale_bicubic_fixed(src: &mut Image, width: u32, height: u32) -> Option<Image> {
    let new_width = width.max(1) as i64;
    let new_height = height.max(1) as i64;
    let src_w = src.sx as i64;
    let src_h = src.sy as i64;
    let f_dx = ftofx(src_w as f32 / new_width as f32);
    let f_dy = ftofx(src_h as f32 / new_height as f32);
    let f_1 = itofx(1);
    let f_2 = itofx(2);
    let f_4 = itofx(4);
    let f_6 = itofx(6);
    let f_gamma = ftofx(1.04f32);
    if !src.true_color {
        src.palette_to_true_color();
    }
    let mut dst = Image::create_true_color(new_width as i32, new_height as i32)?;
    dst.save_alpha_flag = 1;
    let weight = |f: i64| -> i64 {
        let f_fm1 = f - f_1;
        let f_fp1 = f + f_1;
        let f_fp2 = f + f_2;
        let (mut f_a, mut f_b, mut f_c, mut f_d) = (0i64, 0i64, 0i64, 0i64);
        if f_fp2 > 0 {
            f_a = mulfx(f_fp2, mulfx(f_fp2, f_fp2));
        }
        if f_fp1 > 0 {
            f_b = mulfx(f_fp1, mulfx(f_fp1, f_fp1));
        }
        if f > 0 {
            f_c = mulfx(f, mulfx(f, f));
        }
        if f_fm1 > 0 {
            f_d = mulfx(f_fm1, mulfx(f_fm1, f_fm1));
        }
        divfx(
            f_a - mulfx(f_4, f_b) + mulfx(f_6, f_c) - mulfx(f_4, f_d),
            f_6,
        )
    };
    for i in 0..new_height {
        for j in 0..new_width {
            let f_a = mulfx(itofx(i), f_dy);
            let f_b = mulfx(itofx(j), f_dx);
            let m = fxtoi(f_a);
            let n = fxtoi(f_b);
            let f_f = f_a - itofx(m);
            let f_g = f_b - itofx(n);
            let mut ox = [0i64; 16];
            let oy = [m; 16];
            ox[0] = if m < 1 || n < 1 { n } else { n - 1 };
            ox[1] = n;
            ox[2] = if m < 1 || n >= src_w - 1 { n } else { n + 1 };
            ox[3] = if m < 1 || n >= src_w - 2 { n } else { n + 2 };
            ox[4] = if n < 1 { n } else { n - 1 };
            ox[5] = n;
            ox[6] = if n >= src_w - 1 { n } else { n + 1 };
            ox[7] = if n >= src_w - 2 { n } else { n + 2 };
            ox[8] = if m >= src_h - 1 || n < 1 { n } else { n - 1 };
            ox[9] = n;
            ox[10] = if m >= src_h - 1 || n >= src_w - 1 {
                n
            } else {
                n + 1
            };
            ox[11] = if m >= src_h - 1 || n >= src_w - 2 {
                n
            } else {
                n + 2
            };
            ox[12] = if m >= src_h - 2 || n < 1 { n } else { n - 1 };
            ox[13] = n;
            ox[14] = if m >= src_h - 2 || n >= src_w - 1 {
                n
            } else {
                n + 1
            };
            ox[15] = if m >= src_h - 2 || n >= src_w - 2 {
                n
            } else {
                n + 2
            };
            let (mut f_red, mut f_green, mut f_blue, mut f_alpha) = (0i64, 0i64, 0i64, 0i64);
            for k in -1i64..3 {
                let f_ry = weight(itofx(k) - f_f);
                for l in -1i64..3 {
                    let f_rx = weight(itofx(l) - f_g);
                    let f_r = mulfx(f_ry, f_rx);
                    let kk = ((k + 1) * 4 + (l + 1)) as usize;
                    let c = src
                        .tpixels
                        .get(oy[kk] as u32 as usize)
                        .and_then(|r| r.get(ox[kk] as u32 as usize))
                        .copied()
                        .unwrap_or(0);
                    f_red += mulfx(itofx(true_color_get_red(c) as i64), f_r);
                    f_green += mulfx(itofx(true_color_get_green(c) as i64), f_r);
                    f_blue += mulfx(itofx(true_color_get_blue(c) as i64), f_r);
                    f_alpha += mulfx(itofx(true_color_get_alpha(c) as i64), f_r);
                }
            }
            let red = clamp_i(fxtoi(mulfx(f_red, f_gamma)), 0, 255) as i32;
            let green = clamp_i(fxtoi(mulfx(f_green, f_gamma)), 0, 255) as i32;
            let blue = clamp_i(fxtoi(mulfx(f_blue, f_gamma)), 0, 255) as i32;
            let alpha = clamp_i(fxtoi(mulfx(f_alpha, f_gamma)), 0, 127) as i32;
            dst.tpixels[i as usize][j as usize] = true_color_alpha(red, green, blue, alpha);
        }
    }
    Some(dst)
}

/// gdImageScale(). The source is converted to truecolor in place by the
/// bicubic and generic methods, as libgd does.
pub fn scale(src: &mut Image, new_width: u32, new_height: u32) -> Option<Image> {
    if src.interpolation_id < 0 || src.interpolation_id > interp::METHOD_COUNT {
        return None;
    }
    if new_width == 0 || new_height == 0 {
        return None;
    }
    if new_width == src.sx as u32 && new_height == src.sy as u32 {
        return src.clone_image();
    }
    match src.interpolation_id {
        interp::NEAREST_NEIGHBOUR => scale_nearest_neighbour(src, new_width, new_height),
        interp::BILINEAR_FIXED | interp::LINEAR => scale_bilinear(src, new_width, new_height),
        interp::BICUBIC_FIXED | interp::BICUBIC => scale_bicubic_fixed(src, new_width, new_height),
        _ => {
            src.interpolation?;
            scale_two_pass(src, new_width, new_height)
        }
    }
}

// ---------------------------------------------------------------------------
// rotation

/// gdRotatedImageSize()
fn rotated_image_size(src: &Image, angle: f32) -> Rect {
    let m = matrix::rotate(angle as f64);
    let src_area = Rect {
        x: 0,
        y: 0,
        width: src.sx,
        height: src.sy,
    };
    transform_affine_bounding_box(&src_area, &m)
}

fn rotate_setup(src: &Image, degrees: f32) -> (i64, i64, i64, i64, i64) {
    let angle: f32 = (-degrees / 180.0f32) * std::f32::consts::PI;
    let f_0_5 = ftofx(0.5f32);
    let f_h = itofx((src.sy / 2) as i64);
    let f_w = itofx((src.sx / 2) as i64);
    let f_cos = dtofx((-angle as f64).cos());
    let f_sin = dtofx((-angle as f64).sin());
    (f_0_5, f_h, f_w, f_cos, f_sin)
}

/// gdImageRotateNearestNeighbour()
fn rotate_nearest_neighbour(src: &Image, degrees: f32, bg_color: i32) -> Option<Image> {
    let src_w = src.sx as i64;
    let src_h = src.sy as i64;
    let (f_0_5, f_h, f_w, f_cos, f_sin) = rotate_setup(src, degrees);
    let bbox = rotated_image_size(src, degrees);
    let new_width = bbox.width;
    let new_height = bbox.height;
    let mut dst = Image::create_true_color(new_width, new_height)?;
    dst.save_alpha_flag = 1;
    for i in 0..new_height as u32 {
        for j in 0..new_width as u32 {
            let f_i = itofx((i as i32 - new_height / 2) as i64);
            let f_j = itofx((j as i32 - new_width / 2) as i64);
            let f_m = mulfx(f_j, f_sin) + mulfx(f_i, f_cos) + f_0_5 + f_h;
            let f_n = mulfx(f_j, f_cos) - mulfx(f_i, f_sin) + f_0_5 + f_w;
            let m = fxtoi(f_m);
            let n = fxtoi(f_n);
            dst.tpixels[i as usize][j as usize] =
                if m > 0 && m < src_h - 1 && n > 0 && n < src_w - 1 {
                    src.tpixels[m as usize][n as usize]
                } else {
                    bg_color
                };
        }
    }
    Some(dst)
}

/// gdImageRotateGeneric()
fn rotate_generic(src: &mut Image, degrees: f32, bg_color: i32) -> Option<Image> {
    let src_w = src.sx as i64;
    let src_h = src.sy as i64;
    let (f_0_5, f_h, f_w, f_cos, f_sin) = rotate_setup(src, degrees);
    if bg_color < 0 {
        return None;
    }
    if src.interpolation.is_none() {
        set_interpolation_method(src, interp::DEFAULT);
    }
    let bbox = rotated_image_size(src, degrees);
    let new_width = bbox.width;
    let new_height = bbox.height;
    let mut dst = Image::create_true_color(new_width, new_height)?;
    dst.save_alpha_flag = 1;
    for i in 0..new_height as u32 {
        for j in 0..new_width as u32 {
            let f_i = itofx((i as i32 - new_height / 2) as i64);
            let f_j = itofx((j as i32 - new_width / 2) as i64);
            let f_m = mulfx(f_j, f_sin) + mulfx(f_i, f_cos) + f_0_5 + f_h;
            let f_n = mulfx(f_j, f_cos) - mulfx(f_i, f_sin) + f_0_5 + f_w;
            let m = fxtoi(f_m);
            let n = fxtoi(f_n);
            dst.tpixels[i as usize][j as usize] = if m < -1 || n < -1 || m >= src_h || n >= src_w {
                bg_color
            } else {
                get_pixel_interpolated(src, fxtod(f_n), fxtod(f_m), bg_color)
            };
        }
    }
    Some(dst)
}

/// gdImageRotateInterpolated(). A palette source is converted to
/// truecolor in place, as libgd does.
pub fn rotate_interpolated(src: &mut Image, angle: f32, mut bgcolor: i32) -> Option<Image> {
    let angle_rounded = ((angle * 100.0).floor() as i32) % (360 * 100);
    if bgcolor < 0 {
        return None;
    }
    if !src.true_color {
        if bgcolor < MAX_COLORS as i32 {
            let c = bgcolor as usize;
            bgcolor = true_color_alpha(src.red[c], src.green[c], src.blue[c], src.alpha[c]);
        }
        src.palette_to_true_color();
    }
    match angle_rounded {
        0 => {
            let mut dst = Image::create_true_color(src.sx, src.sy)?;
            dst.transparent = src.transparent;
            dst.save_alpha_flag = 1;
            dst.alpha_blending_flag = EFFECT_REPLACE;
            gd::copy(&mut dst, src, false, 0, 0, 0, 0, src.sx, src.sy);
            return Some(dst);
        }
        -27000 | 9000 => return rotate90(src, false),
        -18000 | 18000 => return rotate180(src, false),
        -9000 | 27000 => return rotate270(src, false),
        _ => {}
    }
    if src.interpolation_id < 1 || src.interpolation_id > interp::METHOD_COUNT {
        return None;
    }
    match src.interpolation_id {
        interp::NEAREST_NEIGHBOUR => rotate_nearest_neighbour(src, angle, bgcolor),
        _ => rotate_generic(src, angle, bgcolor),
    }
}

// ---------------------------------------------------------------------------
// affine transformations

/// gdImageClipRectangle()
fn clip_rectangle(im: &Image, r: &mut Rect) {
    let (c1x, c1y, c2x, c2y) = im.get_clip();
    let x1 = r.x + r.width - 1;
    let y1 = r.y + r.height - 1;
    r.x = clamp_i32(r.x, c1x, c2x);
    r.y = clamp_i32(r.y, c1y, c2y);
    r.width = clamp_i32(x1, c1x, c2x) - r.x + 1;
    r.height = clamp_i32(y1, c1y, c2y) - r.y + 1;
}

/// CLAMP() of gd_intern.h
#[inline]
fn clamp_i32(x: i32, low: i32, high: i32) -> i32 {
    if x > high {
        high
    } else if x < low {
        low
    } else {
        x
    }
}

/// gdTransformAffineGetImage(): the source is converted to truecolor in
/// place and `src_area` (when given) is clipped, as libgd does.
pub fn transform_affine_get_image(
    src: &mut Image,
    src_area: Option<&mut Rect>,
    affine: &[f64; 6],
) -> Option<Image> {
    let mut area_full = Rect {
        x: 0,
        y: 0,
        width: src.sx,
        height: src.sy,
    };
    let src_area = match src_area {
        Some(a) => a,
        None => &mut area_full,
    };
    let bbox = transform_affine_bounding_box(src_area, affine);
    let mut dst = Image::create_true_color(bbox.width, bbox.height)?;
    dst.save_alpha_flag = 1;
    if !src.true_color {
        src.palette_to_true_color();
    }
    let m = matrix::translate(-bbox.x as f64, -bbox.y as f64);
    let m = matrix::concat(affine, &m);
    dst.set_alpha_blending(0);
    if transform_affine_copy(&mut dst, 0, 0, src, src_area, &m) {
        Some(dst)
    } else {
        None
    }
}

/// gdTransformAffineCopy()
pub fn transform_affine_copy(
    dst: &mut Image,
    dst_x: i32,
    dst_y: i32,
    src: &mut Image,
    src_region: &mut Rect,
    affine: &[f64; 6],
) -> bool {
    let mut interpolation_id_bak = src.interpolation_id;
    if src.interpolation_id == interp::BILINEAR_FIXED
        || src.interpolation_id == interp::BICUBIC_FIXED
        || src.interpolation_id == interp::NEAREST_NEIGHBOUR
    {
        interpolation_id_bak = src.interpolation_id;
        set_interpolation_method(src, interp::BICUBIC);
    }
    clip_rectangle(src, src_region);
    let mut backclip = None;
    if src_region.x > 0
        || src_region.y > 0
        || src_region.width < src.sx
        || src_region.height < src.sy
    {
        backclip = Some(src.get_clip());
        src.set_clip(
            src_region.x,
            src_region.y,
            src_region.x + src_region.width - 1,
            src_region.y + src_region.height - 1,
        );
    }
    let bbox = transform_affine_bounding_box(src_region, affine);
    let end_x = bbox.width.wrapping_add(bbox.x.wrapping_abs());
    let end_y = bbox.height.wrapping_add(bbox.y.wrapping_abs());
    let Some(inv) = matrix::invert(affine) else {
        set_interpolation_method(src, interpolation_id_bak);
        return false;
    };
    let src_offset_x = src_region.x;
    let src_offset_y = src_region.y;
    if dst.alpha_blending_flag != 0 {
        let mut y = bbox.y;
        while y <= end_y {
            let pty = y as f64 + 0.5;
            let mut x = 0;
            while x <= end_x {
                let pt = PointF {
                    x: x as f64 + 0.5,
                    y: pty,
                };
                let sp = matrix::apply_to_point_f(&pt, &inv);
                let c = get_pixel_interpolated(
                    src,
                    src_offset_x as f64 + sp.x,
                    src_offset_y as f64 + sp.y,
                    0,
                );
                dst.set_pixel(dst_x + x, dst_y + y, c);
                x += 1;
            }
            y += 1;
        }
    } else {
        let mut y = 0;
        while y <= end_y {
            let pty = y as f64 + 0.5 + bbox.y as f64;
            let dy = dst_y + y;
            if dy < 0 || dy > dst.sy - 1 {
                y += 1;
                continue;
            }
            let mut x = 0;
            while x <= end_x {
                let pt = PointF {
                    x: x as f64 + 0.5 + bbox.x as f64,
                    y: pty,
                };
                let sp = matrix::apply_to_point_f(&pt, &inv);
                let dx = dst_x + x;
                if dx < 0 || dx > dst.sx - 1 {
                    break;
                }
                let c = get_pixel_interpolated(
                    src,
                    src_offset_x as f64 + sp.x,
                    src_offset_y as f64 + sp.y,
                    -1,
                );
                if dst.true_color {
                    dst.tpixels[dy as usize][dx as usize] = c;
                } else {
                    dst.pixels[dy as usize][dx as usize] = c as u8;
                }
                x += 1;
            }
            y += 1;
        }
    }
    if let Some((x1, y1, x2, y2)) = backclip {
        src.set_clip(x1, y1, x2, y2);
    }
    set_interpolation_method(src, interpolation_id_bak);
    true
}

/// gdTransformAffineBoundingBox()
pub fn transform_affine_bounding_box(src: &Rect, affine: &[f64; 6]) -> Rect {
    let extent = [
        PointF { x: 0.0, y: 0.0 },
        PointF {
            x: src.width as f64,
            y: 0.0,
        },
        PointF {
            x: src.width as f64,
            y: src.height as f64,
        },
        PointF {
            x: 0.0,
            y: src.height as f64,
        },
    ]
    .map(|p| matrix::apply_to_point_f(&p, affine));
    let mut min = extent[0];
    let mut max = extent[0];
    for e in &extent[1..] {
        if min.x > e.x {
            min.x = e.x;
        }
        if min.y > e.y {
            min.y = e.y;
        }
        if max.x < e.x {
            max.x = e.x;
        }
        if max.y < e.y {
            max.y = e.y;
        }
    }
    Rect {
        x: min.x as i32,
        y: min.y as i32,
        width: (max.x - min.x).floor() as i32 - 1,
        height: (max.y - min.y).floor() as i32,
    }
}

/// gdImageSetInterpolationMethod()
pub fn set_interpolation_method(im: &mut Image, id: i32) -> bool {
    if !(0..=interp::METHOD_COUNT).contains(&id) {
        return false;
    }
    let mut id = id;
    let k: Option<InterpolationFn> = match id {
        interp::NEAREST_NEIGHBOUR | interp::WEIGHTED4 => None,
        interp::BILINEAR_FIXED | interp::LINEAR => Some(filter_linear),
        interp::BELL => Some(filter_bell),
        interp::BESSEL => Some(filter_bessel),
        interp::BICUBIC_FIXED | interp::BICUBIC => Some(filter_bicubic),
        interp::BLACKMAN => Some(filter_blackman),
        interp::BOX => Some(filter_box),
        interp::BSPLINE => Some(filter_bspline),
        interp::CATMULLROM => Some(filter_catmullrom),
        interp::GAUSSIAN => Some(filter_gaussian),
        interp::GENERALIZED_CUBIC => Some(filter_generalized_cubic),
        interp::HERMITE => Some(filter_hermite),
        interp::HAMMING => Some(filter_hamming),
        interp::HANNING => Some(filter_hanning),
        interp::MITCHELL => Some(filter_mitchell),
        interp::POWER => Some(filter_power),
        interp::QUADRATIC => Some(filter_quadratic),
        interp::SINC => Some(filter_sinc),
        interp::TRIANGLE => Some(filter_triangle),
        interp::DEFAULT => {
            id = interp::LINEAR;
            Some(filter_linear)
        }
        _ => return false,
    };
    im.interpolation = k;
    im.interpolation_id = id;
    true
}

/// gdImageGetInterpolationMethod()
pub fn get_interpolation_method(im: &Image) -> i32 {
    im.interpolation_id
}
