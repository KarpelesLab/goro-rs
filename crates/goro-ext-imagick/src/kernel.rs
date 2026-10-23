//! ImagickKernel (imagickkernel_class.c): convolution / morphology kernel
//! lists.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::NativeData;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::zpp::{self, Arg, Func, Obj};
use crate::{C_KERNEL, Cls, Impl, R, arr_val, throw, throw_cls};

pub(crate) const METHODS: &[(&str, Impl, bool)] = &[
    ("__construct", construct, false),
    ("fromMatrix", from_matrix, true),
    ("fromBuiltin", from_builtin, true),
    ("addKernel", add_kernel, false),
    ("separate", separate, false),
    ("getMatrix", get_matrix, false),
    ("scale", scale, false),
    ("addUnityKernel", add_unity_kernel, false),
];

/// One kernel (KernelInfo): values row-major, NaN = not used.
#[derive(Clone, Debug, PartialEq)]
pub struct Kernel {
    pub width: usize,
    pub height: usize,
    pub x: usize,
    pub y: usize,
    pub values: Vec<f64>,
    /// From fromMatrix(): its unused (false) entries poison the sums
    /// ScaleKernelInfo() normalises by, as pecl computes them.
    pub user: bool,
}

#[derive(Default)]
pub struct KernelData {
    pub list: RefCell<Vec<Kernel>>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    get_properties: Some(debug_info),
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

pub(crate) fn data(o: &Obj) -> Rc<KernelData> {
    {
        let b = o.borrow();
        if let Some(n) = b.native.as_ref() {
            if let Ok(d) = n.data.clone().downcast::<KernelData>() {
                return d;
            }
        }
    }
    let d = Rc::new(KernelData::default());
    o.borrow_mut().native = Some(NativeData {
        data: d.clone() as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    d
}

pub(crate) fn create(_vm: &mut Vm, o: &Obj) {
    data(o);
}

fn clone_obj(_vm: &mut Vm, src: &Obj, dst: &Obj) {
    let list = data(src).list.borrow().clone();
    let d = Rc::new(KernelData {
        list: RefCell::new(list),
    });
    dst.borrow_mut().native = Some(NativeData {
        data: d as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

fn values_to_array(k: &Kernel) -> PhpArray {
    let mut m = PhpArray::new();
    for y in 0..k.height {
        let mut row = PhpArray::new();
        for x in 0..k.width {
            let v = k.values[y * k.width + x];
            row.push(if v.is_nan() {
                Value::False
            } else {
                Value::Double(v)
            });
        }
        m.push(arr_val(row));
    }
    m
}

/// php_imagickkernel_get_debug_info()
fn debug_info(_vm: &mut Vm, o: &Obj, purpose: u8) -> Option<PhpArray> {
    if purpose != 0 {
        return None;
    }
    let mut a = PhpArray::new();
    for k in data(o).list.borrow().iter() {
        a.push(arr_val(values_to_array(k)));
    }
    Some(a)
}

/// The kernels of an ImagickKernel object (empty: an exception).
pub(crate) fn kernels_of(vm: &mut Vm, o: &Obj) -> Result<Vec<Kernel>, ()> {
    let l = data(o).list.borrow().clone();
    if l.is_empty() {
        throw_cls(vm, Cls::Kernel, "ImagickKernel is empty, cannot be used")?;
    }
    Ok(l)
}

fn new_kernel_object(vm: &mut Vm, list: Vec<Kernel>) -> Value {
    let o = crate::new_object(vm, b"ImagickKernel");
    *data(&o).list.borrow_mut() = list;
    Value::Object(o)
}

fn construct(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Null)
}

const MATRIX_ERROR_EMPTY: &str = "Cannot create kernel, matrix is empty.";
const MATRIX_ERROR_UNEVEN: &str =
    "Values must be matrix, with the same number of columns in each row.";
const MATRIX_ERROR_BAD_VALUE: &str = "Only numbers or false are valid values in a kernel matrix.";
const MATRIX_ORIGIN_REQUIRED: &str =
    "For kernels with even numbered rows or columns, the origin position must be specified.";

fn from_matrix(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a|a!", args)?;
    let Arg::Arr(m) = &a[0] else {
        return Err(());
    };
    let m = m.borrow().clone();
    let rows = m.len();
    if rows == 0 {
        return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_EMPTY);
    }
    let mut values = Vec::new();
    let mut cols: Option<usize> = None;
    for r in 0..rows {
        let Some(row) = m.get_int(r as i64) else {
            return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_UNEVEN);
        };
        let Value::Array(row) = row.deref() else {
            return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_UNEVEN);
        };
        let row = row.borrow();
        let n = row.len();
        if n == 0 {
            return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_EMPTY);
        }
        if cols.is_some_and(|c| c != n) {
            return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_UNEVEN);
        }
        cols = Some(n);
        for c in 0..n {
            let Some(v) = row.get_int(c as i64) else {
                return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_UNEVEN);
            };
            values.push(match v.deref() {
                Value::Double(d) => d,
                Value::Long(l) => l as f32 as f64,
                Value::False => f64::NAN,
                _ => return throw_cls(vm, Cls::Kernel, MATRIX_ERROR_BAD_VALUE),
            });
        }
    }
    let cols = cols.unwrap_or(0);
    let (ox, oy) = match &a[1] {
        Arg::Arr(o) => {
            let o = o.borrow();
            let Some(x) = o.get_int(0) else {
                return throw_cls(vm, Cls::Kernel, MATRIX_ORIGIN_REQUIRED);
            };
            let x = x.to_long();
            if x < 0 || x as usize >= cols {
                throw(
                    vm,
                    b"ImagickKernelException",
                    &format!("origin_x for matrix is outside bounds of columns: {}", x),
                    5,
                );
                return Err(());
            }
            let Some(y) = o.get_int(1) else {
                return throw_cls(vm, Cls::Kernel, MATRIX_ORIGIN_REQUIRED);
            };
            let y = y.to_long();
            if y < 0 || y as usize >= rows {
                throw(
                    vm,
                    b"ImagickKernelException",
                    &format!("origin_y for matrix is outside bounds of rows: {}", y),
                    5,
                );
                return Err(());
            }
            (x as usize, y as usize)
        }
        _ => {
            if cols.is_multiple_of(2) || rows.is_multiple_of(2) {
                return throw_cls(vm, Cls::Kernel, MATRIX_ORIGIN_REQUIRED);
            }
            ((cols - 1) / 2, (rows - 1) / 2)
        }
    };
    let k = Kernel {
        width: cols,
        height: rows,
        x: ox,
        y: oy,
        values,
        user: true,
    };
    Ok(new_kernel_object(vm, vec![k]))
}

/// A kernel from a pattern: rows split by '/', each char '0', '1' or '-'
/// (not used); the origin at the centre unless given.
fn pat(s: &str, origin: Option<(usize, usize)>) -> Kernel {
    let rows: Vec<&str> = s.split('/').collect();
    let height = rows.len();
    let width = rows[0].len();
    let values = rows
        .iter()
        .flat_map(|r| {
            r.chars().map(|c| match c {
                '0' => 0.0,
                '1' => 1.0,
                _ => f64::NAN,
            })
        })
        .collect();
    let (x, y) = origin.unwrap_or(((width - 1) / 2, (height - 1) / 2));
    Kernel {
        width,
        height,
        x,
        y,
        values,
        user: false,
    }
}

fn mat3(v: [f64; 9]) -> Kernel {
    Kernel {
        width: 3,
        height: 3,
        x: 1,
        y: 1,
        values: v.to_vec(),
        user: false,
    }
}

/// A 3×3 kernel turned clockwise by `n` eighths of a turn (the ring of
/// eight neighbours shifts round the centre).
fn rotate45(k: &Kernel, n: usize) -> Kernel {
    const RING: [usize; 8] = [0, 1, 2, 5, 8, 7, 6, 3];
    let mut out = k.clone();
    for (i, &dst) in RING.iter().enumerate() {
        let src = RING[(i + 8 - n % 8) % 8];
        out.values[dst] = k.values[src];
    }
    out
}

/// A kernel turned clockwise by a quarter turn, origin included.
fn rotate90(k: &Kernel) -> Kernel {
    let (w, h) = (k.width, k.height);
    let mut values = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            // (x, y) moves to (h-1-y, x) in the h-wide result
            values[x * h + (h - 1 - y)] = k.values[y * w + x];
        }
    }
    Kernel {
        width: h,
        height: w,
        x: h - 1 - k.y,
        y: k.x,
        values,
        user: false,
    }
}

/// RotateKernelInfo(): whole eighths for 3×3 kernels, quarter turns for
/// the others (an odd eighth leaves them as they are).
pub(crate) fn rotate_kernel(k: &Kernel, angle: f64) -> Kernel {
    if !angle.is_finite() {
        return k.clone();
    }
    let n = ((angle / 45.0).round() as i64).rem_euclid(8) as usize;
    if n == 0 {
        return k.clone();
    }
    if k.width == 3 && k.height == 3 {
        return rotate45(k, n);
    }
    if n % 2 == 1 {
        return k.clone();
    }
    let mut out = k.clone();
    for _ in 0..n / 2 {
        out = rotate90(&out);
    }
    out
}

/// Positive values scaled to sum 1, negative ones to sum -1.
fn correlate_normalize(k: &mut Kernel) {
    let pos: f64 = k.values.iter().filter(|v| **v > 0.0).sum();
    let neg: f64 = -k.values.iter().filter(|v| **v < 0.0).sum::<f64>();
    for v in k.values.iter_mut() {
        if *v > 0.0 && pos > 0.0 {
            *v /= pos;
        } else if *v < 0.0 && neg > 0.0 {
            *v /= neg;
        }
    }
}

fn square_of(r: usize, f: &dyn Fn(i64, i64) -> f64) -> Kernel {
    let n = 2 * r + 1;
    let mut v = Vec::with_capacity(n * n);
    for y in 0..n as i64 {
        for x in 0..n as i64 {
            v.push(f(x - r as i64, y - r as i64));
        }
    }
    Kernel {
        width: n,
        height: n,
        x: r,
        y: r,
        values: v,
        user: false,
    }
}

/// A sampled 2-D gaussian over [-w, w]²: normalised to unit sum, or by the
/// analytic 1/(2πσ²); sigma 0 is a unit impulse.
fn gauss2(w: usize, sigma: f64, unit_sum: bool) -> Kernel {
    if sigma <= 0.0 {
        return square_of(w, &|x, y| if x == 0 && y == 0 { 1.0 } else { 0.0 });
    }
    let s2 = 2.0 * sigma * sigma;
    let mut k = square_of(w, &|x, y| (-((x * x + y * y) as f64) / s2).exp());
    let div = if unit_sum {
        k.values.iter().sum()
    } else {
        std::f64::consts::PI * s2
    };
    for v in k.values.iter_mut() {
        *v /= div;
    }
    k
}

const FREI_CHEN: [[f64; 9]; 9] = {
    const R: f64 = std::f64::consts::FRAC_1_SQRT_2;
    const S: f64 = 1.0 / 6.0;
    const T: f64 = 1.0 / 3.0;
    [
        [R, 0.0, -R, 1.0, 0.0, -1.0, R, 0.0, -R],
        [R, 1.0, R, 0.0, 0.0, 0.0, R, 1.0, R],
        [1.0, -R, 0.0, -R, 0.0, R, 0.0, R, -1.0],
        [0.0, R, -1.0, -R, 0.0, R, 1.0, -R, 0.0],
        [0.0, -0.5, 0.0, 0.5, 0.0, 0.5, 0.0, -0.5, 0.0],
        [0.5, 0.0, -0.5, 0.0, 0.0, 0.0, -0.5, 0.0, 0.5],
        [S, -T, S, -T, 2.0 * T, -T, -S, -T, S],
        [-T, S, -T, S, 2.0 * T, S, -T, S, -T],
        [T, T, T, T, T, T, T, T, T],
    ]
};

/// The built-in kernels (AcquireKernelBuiltIn() as pecl's fromBuiltin()
/// drives it), established black-box against ImageMagick 7.1.2. `args`
/// are rho, sigma, xi, psi (NaN when absent).
fn builtin(kind: i64, args: &[f64]) -> Option<Vec<Kernel>> {
    let a = |i: usize| args.get(i).copied().filter(|v| !v.is_nan());
    let arg = |i: usize, d: f64| a(i).unwrap_or(d);
    let one = |k: Kernel| Some(vec![k]);
    let shape_r = |d: f64| {
        let r = arg(0, d);
        if r < 1.0 { d } else { r }
    };
    match kind {
        // Unity
        1 => one(Kernel {
            width: 1,
            height: 1,
            x: 0,
            y: 0,
            values: vec![arg(0, 1.0)],
            user: false,
        }),
        // Gaussian
        2 => {
            let sigma = arg(1, 0.0).abs();
            let w = kernel_width(arg(0, 0.0), sigma, true);
            one(gauss2(w, sigma, true))
        }
        // DoG: two analytic gaussians, then positive/negative unit sums
        3 => {
            let (s1, s2) = (arg(1, 0.0).abs(), arg(2, 0.0).abs());
            let w = kernel_width(arg(0, 0.0), s1.max(s2), true);
            let (g1, g2) = (gauss2(w, s1, false), gauss2(w, s2, false));
            let mut k = g1.clone();
            for (v, b) in k.values.iter_mut().zip(&g2.values) {
                *v -= b;
            }
            correlate_normalize(&mut k);
            one(k)
        }
        // LoG
        4 => {
            let sigma = arg(1, 0.0).abs();
            let w = kernel_width(arg(0, 0.0), sigma, true);
            if sigma <= 0.0 {
                return one(gauss2(w, 0.0, true));
            }
            let s2 = 2.0 * sigma * sigma;
            let mut k = square_of(w, &|x, y| {
                let r2 = (x * x + y * y) as f64;
                (1.0 - r2 / s2) * (-r2 / s2).exp()
            });
            correlate_normalize(&mut k);
            one(k)
        }
        // Blur (a row; xi turns it) / Comet (one-sided)
        5 | 6 => {
            let sigma = arg(1, 0.0).abs();
            let rho = arg(0, 0.0);
            let s = sigma.max(1e-12);
            let tap = |t: f64| {
                if sigma <= 0.0 {
                    if t == 0.0 { 1.0 } else { 0.0 }
                } else {
                    (-(t * t) / (2.0 * s * s)).exp()
                }
            };
            let mut k = if kind == 5 {
                let w = kernel_width(rho, sigma, false);
                let n = 2 * w + 1;
                let values = (0..n)
                    .map(|i| {
                        let x = i as f64 - w as f64;
                        if sigma <= 0.0 {
                            tap(x)
                        } else {
                            tap(x - 1.0 / 3.0) + tap(x) + tap(x + 1.0 / 3.0)
                        }
                    })
                    .collect();
                Kernel {
                    width: n,
                    height: 1,
                    x: w,
                    y: 0,
                    values,
                    user: false,
                }
            } else {
                let n = if rho > 0.0 {
                    (rho as usize).max(1)
                } else {
                    kernel_width(0.0, sigma, false) + 1
                };
                let values = (0..n)
                    .map(|i| {
                        let x = i as f64;
                        if sigma <= 0.0 {
                            tap(x)
                        } else {
                            tap(x) + tap(x + 1.0 / 3.0) + tap(x + 2.0 / 3.0)
                        }
                    })
                    .collect();
                Kernel {
                    width: n,
                    height: 1,
                    x: 0,
                    y: 0,
                    values,
                    user: false,
                }
            };
            let sum: f64 = k.values.iter().sum();
            for v in k.values.iter_mut() {
                *v /= sum;
            }
            one(rotate_kernel(&k, arg(2, 0.0)))
        }
        // Laplacian
        8 => {
            let v: Vec<f64> = match arg(0, 0.0) as i64 {
                1 => vec![0., -1., 0., -1., 4., -1., 0., -1., 0.],
                2 => vec![-2., 1., -2., 1., 4., 1., -2., 1., -2.],
                3 => vec![1., -2., 1., -2., 4., -2., 1., -2., 1.],
                5 => {
                    return one(Kernel {
                        width: 5,
                        height: 5,
                        x: 2,
                        y: 2,
                        values: vec![
                            -4., -1., 0., -1., -4., -1., 2., 3., 2., -1., 0., 3., 4., 3., 0., -1.,
                            2., 3., 2., -1., -4., -1., 0., -1., -4.,
                        ],
                        user: false,
                    });
                }
                7 => {
                    return one(Kernel {
                        width: 7,
                        height: 7,
                        x: 3,
                        y: 3,
                        values: vec![
                            -10., -5., -2., -1., -2., -5., -10., -5., 0., 3., 4., 3., 0., -5., -2.,
                            3., 6., 7., 6., 3., -2., -1., 4., 7., 8., 7., 4., -1., -2., 3., 6., 7.,
                            6., 3., -2., -5., 0., 3., 4., 3., 0., -5., -10., -5., -2., -1., -2.,
                            -5., -10.,
                        ],
                        user: false,
                    });
                }
                15 => {
                    return one(Kernel {
                        width: 5,
                        height: 5,
                        x: 2,
                        y: 2,
                        values: vec![
                            0., 0., -1., 0., 0., 0., -1., -2., -1., 0., -1., -2., 16., -2., -1.,
                            0., -1., -2., -1., 0., 0., 0., -1., 0., 0.,
                        ],
                        user: false,
                    });
                }
                19 => {
                    return one(Kernel {
                        width: 9,
                        height: 9,
                        x: 4,
                        y: 4,
                        values: vec![
                            0., -1., -1., -2., -2., -2., -1., -1., 0., -1., -2., -4., -5., -5.,
                            -5., -4., -2., -1., -1., -4., -5., -3., 0., -3., -5., -4., -1., -2.,
                            -5., -3., 12., 24., 12., -3., -5., -2., -2., -5., 0., 24., 40., 24.,
                            0., -5., -2., -2., -5., -3., 12., 24., 12., -3., -5., -2., -1., -4.,
                            -5., -3., 0., -3., -5., -4., -1., -1., -2., -4., -5., -5., -5., -4.,
                            -2., -1., 0., -1., -1., -2., -2., -2., -1., -1., 0.,
                        ],
                        user: false,
                    });
                }
                _ => vec![-1., -1., -1., -1., 8., -1., -1., -1., -1.],
            };
            one(Kernel {
                width: 3,
                height: 3,
                x: 1,
                y: 1,
                values: v,
                user: false,
            })
        }
        // Sobel, Roberts, Prewitt, Compass, Kirsch (rho turns them)
        9 | 11 | 12 | 13 | 14 => {
            let base = match kind {
                9 => [1., 0., -1., 2., 0., -2., 1., 0., -1.],
                11 => [0., 0., 0., 1., -1., 0., 0., 0., 0.],
                12 => [1., 0., -1., 1., 0., -1., 1., 0., -1.],
                13 => [1., 1., -1., 1., -2., -1., 1., 1., -1.],
                _ => [5., -3., -3., 5., 0., -3., 5., -3., -3.],
            };
            one(rotate_kernel(&mat3(base), arg(0, 0.0)))
        }
        // Frei-Chen: rho the type (10 the whole set, 11-19 one of it),
        // sigma turns it
        10 => {
            let angle = arg(1, 0.0);
            let r2 = std::f64::consts::SQRT_2;
            let t = arg(0, 0.0) as i64;
            let ks: Vec<Kernel> = match t {
                1 => vec![mat3(FREI_CHEN[0])],
                2 => {
                    let r = std::f64::consts::FRAC_1_SQRT_2;
                    vec![mat3([r, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, -1.0, -r])]
                }
                10 => FREI_CHEN.iter().map(|v| mat3(*v)).collect(),
                11..=19 => vec![mat3(FREI_CHEN[(t - 11) as usize])],
                _ => vec![mat3([1.0, 0.0, -1.0, r2, 0.0, -r2, 1.0, 0.0, -1.0])],
            };
            Some(ks.iter().map(|k| rotate_kernel(k, angle)).collect())
        }
        // Diamond
        15 => {
            let r = shape_r(1.0);
            let s = arg(1, 1.0);
            one(square_of(r as usize, &|x, y| {
                if ((x.abs() + y.abs()) as f64) <= r {
                    s
                } else {
                    f64::NAN
                }
            }))
        }
        // Square
        16 => {
            let r = shape_r(1.0) as usize;
            let s = arg(1, 1.0);
            one(square_of(r, &|_, _| s))
        }
        // Rectangle: a geometry WxH+x+y
        17 => {
            let w = a(0).filter(|v| *v >= 1.0).unwrap_or(3.0) as usize;
            let h = a(1).filter(|v| *v >= 1.0).unwrap_or(w as f64) as usize;
            let x = a(2).map_or((w - 1) / 2, |v| (v.max(0.0) as usize).min(w - 1));
            let y = a(3).map_or((h - 1) / 2, |v| (v.max(0.0) as usize).min(h - 1));
            one(Kernel {
                width: w,
                height: h,
                x,
                y,
                values: vec![1.0; w * h],
                user: false,
            })
        }
        // Octagon
        18 => {
            let r = shape_r(2.0);
            let s = arg(1, 1.0);
            let lim = (1.5 * r).floor();
            one(square_of(r as usize, &|x, y| {
                if ((x.abs() + y.abs()) as f64) <= lim {
                    s
                } else {
                    f64::NAN
                }
            }))
        }
        // Disk
        19 => {
            let r = {
                let r = arg(0, 4.3);
                if r < 0.5 { 4.3 } else { r }
            };
            let s = arg(1, 1.0);
            one(square_of(r as usize, &|x, y| {
                if ((x * x + y * y) as f64) <= r * r {
                    s
                } else {
                    f64::NAN
                }
            }))
        }
        // Plus / Cross
        20 | 21 => {
            let r = shape_r(2.0) as usize;
            let s = arg(1, 1.0);
            one(square_of(r, &|x, y| {
                let on = if kind == 20 {
                    x == 0 || y == 0
                } else {
                    x == y || x == -y
                };
                if on { s } else { f64::NAN }
            }))
        }
        // Ring / Peaks: inner < d ≤ outer
        22 | 23 => {
            let (mut r1, mut r2) = match (a(0), a(1)) {
                (None, _) => (2.5, 3.5),
                (Some(x), None) => (0.0, x),
                (Some(x), Some(y)) => (x, y),
            };
            if r1 > r2 {
                std::mem::swap(&mut r1, &mut r2);
            }
            let s = if kind == 22 { arg(2, 1.0) } else { 0.0 };
            one(square_of(r2 as usize, &|x, y| {
                let d2 = (x * x + y * y) as f64;
                if kind == 23 && x == 0 && y == 0 {
                    1.0
                } else if d2 > r1 * r1 && d2 <= r2 * r2 {
                    s
                } else {
                    f64::NAN
                }
            }))
        }
        // Edges, Corners, ConvexHull: fixed sets
        24 => Some(
            ["0-1/0-1/0-1", "1-0/1-0/1-0", "111/---/000", "000/---/111"]
                .iter()
                .map(|s| pat(s, None))
                .collect(),
        ),
        25 => Some(
            ["-1-/0-1/00-", "00-/0-1/-1-", "-00/1-0/-1-", "-1-/1-0/-00"]
                .iter()
                .map(|s| pat(s, None))
                .collect(),
        ),
        30 => Some(
            [
                "11-/10-/1-0",
                "111/-01/0--",
                "0-1/-01/-11",
                "--0/10-/111",
                "111/10-/--0",
                "-11/-01/0-1",
                "0--/-01/111",
                "1-0/10-/11-",
            ]
            .iter()
            .map(|s| pat(s, None))
            .collect(),
        ),
        // Diagonals, LineEnds, LineJunctions, Ridges, ThinSE, Skeleton:
        // rho picks a member (0 or none: the whole set), sigma turns one
        26 => typed_set(
            arg(0, 0.0) as i64,
            arg(1, 0.0),
            &[
                "000/0-1/11-",
                "001/0-1/01-",
                "-11/1-0/000",
                "-10/1-0/100",
                "01-/0-1/001",
                "11-/0-1/000",
                "100/1-0/-10",
                "000/1-0/-11",
            ],
            &[(1, "000/0-1/11-"), (2, "001/0-1/01-")],
        ),
        27 => typed_set(
            arg(0, 0.0) as i64,
            arg(1, 0.0),
            &[
                "00-/011/00-",
                "000/010/-1-",
                "-00/110/-00",
                "-1-/010/000",
                "000/010/001",
                "000/010/100",
                "100/010/000",
                "001/010/000",
            ],
            &[
                (1, "00-/011/00-"),
                (2, "000/010/001"),
                (3, "000/011/000"),
                (4, "000/01-/00-"),
            ],
        ),
        28 => typed_set(
            arg(0, 0.0) as i64,
            arg(1, 0.0),
            &[
                "1-1/-1-/-1-",
                "-1-/-11/1--",
                "--1/11-/--1",
                "1--/-11/-1-",
                "-1-/-1-/1-1",
                "--1/11-/-1-",
                "1--/-11/1--",
                "-1-/11-/--1",
                "1--/-1-/1-1",
                "1-1/-1-/1--",
                "1-1/-1-/--1",
                "--1/-1-/1-1",
            ],
            &[
                (1, "1-1/-1-/-1-"),
                (2, "1--/-1-/1-1"),
                (3, "---/111/-1-"),
                (4, "1-1/-1-/1-1"),
                (5, "-1-/111/-1-"),
            ],
        ),
        29 => {
            if arg(0, 1.0) as i64 == 2 {
                let mut v = vec![
                    pat("0110", Some((1, 0))),
                    pat("0/1/1/0", Some((0, 1))),
                    pat("0110", Some((1, 0))),
                    pat("0/1/1/0", Some((0, 1))),
                    pat("011-/-11-/-110", Some((1, 1))),
                    pat("011-/-11-/-110", Some((2, 1))),
                    pat("-110/-11-/011-", Some((1, 1))),
                    pat("-110/-11-/011-", Some((2, 1))),
                    pat("0--/111/111/--0", Some((1, 1))),
                    pat("0--/111/111/--0", Some((1, 2))),
                    pat("--0/111/111/0--", Some((1, 1))),
                    pat("--0/111/111/0--", Some((1, 2))),
                ];
                // the second copies of the straight ridges sit one further
                v[2].x = 2;
                v[3].y = 2;
                Some(v)
            } else {
                Some(vec![pat("010", None), pat("0/1/0", None)])
            }
        }
        31 => {
            let t = arg(0, 1.0) as i64;
            let p = match t {
                41 => "--1/0-1/--1",
                42 => "--1/0-1/-0-",
                43 => "-0-/0-1/--1",
                44 => "-0-/0-1/-0-",
                81 => "-1-/0-1/-1-",
                82 => "-1-/0-1/0--",
                83 => "0--/0-1/-1-",
                84 => "0--/0-1/0--",
                85 => "0-1/0-1/0--",
                86 => "0--/0-1/0-1",
                87 => "-1-/0-1/00-",
                88 => "-1-/0-1/01-",
                89 => "01-/0-1/-1-",
                _ => "0-1/0-1/0-1",
            };
            one(rotate_kernel(&pat(p, None), arg(1, 0.0)))
        }
        32 => {
            let set: &[&str] = match arg(0, 1.0) as i64 {
                2 => &[
                    "0-1/0-1/0-1",
                    "00-/0-1/-1-",
                    "000/---/111",
                    "-00/1-0/-1-",
                    "1-0/1-0/1-0",
                    "-1-/1-0/-00",
                    "111/---/000",
                    "-1-/0-1/00-",
                ],
                3 => &[
                    "--1/0-1/--1",
                    "--1/0-1/-0-",
                    "-0-/0-1/--1",
                    "1--/1-0/1--",
                    "-0-/1-0/1--",
                    "1--/1-0/-0-",
                    "111/---/-0-",
                    "11-/--0/-0-",
                    "-11/0--/-0-",
                    "-0-/---/111",
                    "-0-/0--/-11",
                    "-0-/--0/11-",
                ],
                _ => &[
                    "0-1/0-1/0-1",
                    "00-/0-1/-11",
                    "000/---/111",
                    "-00/1-0/11-",
                    "1-0/1-0/1-0",
                    "11-/1-0/-00",
                    "111/---/000",
                    "-11/0-1/00-",
                ],
            };
            Some(set.iter().map(|s| pat(s, None)).collect())
        }
        // Chebyshev, Manhattan, Octagonal, Euclidean distances
        33..=36 => {
            let r = {
                let d = if kind == 35 { 2.0 } else { 1.0 };
                let r = arg(0, d);
                if r < 1.0 { d } else { r }
            };
            let s = arg(1, 100.0);
            one(square_of(r as usize, &|x, y| {
                let (ax, ay) = (x.abs() as f64, y.abs() as f64);
                let d = match kind {
                    33 => ax.max(ay),
                    34 => ax + ay,
                    35 => {
                        let (lo, hi) = (ax.min(ay), ax.max(ay));
                        hi.max((2.0 * (lo + hi) / 3.0).ceil())
                    }
                    _ => (ax * ax + ay * ay).sqrt(),
                };
                if d == 0.0 { 0.0 } else { d * s }
            }))
        }
        _ => None,
    }
}

/// A member of a fixed set by number, turned by `angle`; type 0 (or one
/// without a member) is the whole set, unturned.
fn typed_set(t: i64, angle: f64, set: &[&str], singles: &[(i64, &str)]) -> Option<Vec<Kernel>> {
    match singles.iter().find(|(n, _)| *n == t) {
        Some((_, p)) => Some(vec![rotate_kernel(&pat(p, None), angle)]),
        None => Some(set.iter().map(|s| pat(s, None)).collect()),
    }
}

/// The half-width for a kernel radius (positive: its integer part, at
/// least 1) or from sigma (1-D or 2-D normalisation); 1 when sigma is 0.
fn kernel_width(radius: f64, sigma: f64, two_d: bool) -> usize {
    if radius > 0.0 {
        return (radius as usize).max(1);
    }
    if sigma <= 0.0 {
        return 1;
    }
    if two_d {
        goro_imagick::convolve::kernel_radius_2d(0.0, sigma)
    } else {
        goro_imagick::convolve::kernel_radius(0.0, sigma)
    }
}

/// The numbers of a kernel geometry: "RxS+X+Y", "a,b,c" or a mix.
fn kernel_args(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<f64>| {
        if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(f64::NAN));
            cur.clear();
        }
    };
    for c in s.chars() {
        match c {
            'x' | 'X' | ',' | ' ' => flush(&mut cur, &mut out),
            '+' | '-' => {
                flush(&mut cur, &mut out);
                if c == '-' {
                    cur.push('-');
                }
            }
            '%' | '!' | '^' | '<' | '>' | '@' => {}
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn from_builtin(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ls", args)?;
    let kind = a[0].long_or(0);
    let shape = String::from_utf8_lossy(a[1].str_or(b"")).into_owned();
    match builtin(kind, &kernel_args(&shape)) {
        Some(k) => Ok(new_kernel_object(vm, k)),
        None => throw_cls(
            vm,
            Cls::Kernel,
            "ImagickKernel::fromBuiltin() is not implemented yet for this kernel type",
        ),
    }
}
fn add_kernel(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let other = match &a[0] {
        Arg::Any(v) => crate::arg_obj(vm, f, 1, v, C_KERNEL, "ImagickKernel")?,
        _ => return Err(()),
    };
    let add = data(&other).list.borrow().clone();
    if add.is_empty() {
        throw(
            vm,
            b"ImagickKernelException",
            "ImagickKernel is empty, cannot be used",
            0,
        );
        return Ok(Value::Null);
    }
    data(o.unwrap()).list.borrow_mut().extend(add);
    Ok(Value::Null)
}

fn separate(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let l = kernels_of(vm, o.unwrap())?;
    let mut a = PhpArray::new();
    for k in l {
        let v = new_kernel_object(vm, vec![k]);
        a.push(v);
    }
    Ok(arr_val(a))
}

fn get_matrix(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let l = kernels_of(vm, o.unwrap())?;
    Ok(arr_val(values_to_array(&l[0])))
}

/// ScaleKernelInfo() as pecl's scale() reaches it (black-box): the
/// percent flag has no effect; NormalizeValue divides by the sum (by the
/// positive sum for zero-sum kernels), CorrelateNormalizeValue scales
/// the positive and negative parts to ±1 apart, then everything is
/// multiplied by the factor. For fromMatrix() kernels the positive sum
/// takes the unused entries along (making it NaN): a NaN divisor leaves
/// a normalised kernel all unused, a NaN positive part of a correlate
/// normalisation unscaled.
pub(crate) fn scale_kernel(k: &mut Kernel, scale: f64, flags: i64) {
    let mut pos = 0.0;
    let mut neg = 0.0;
    for &v in &k.values {
        if v < 0.0 {
            neg += v;
        } else if !v.is_nan() || k.user {
            pos += v;
        }
    }
    let eps = 1e-12;
    if flags & 0x10000 != 0 {
        let ps = if pos.abs() >= eps { 1.0 / pos } else { 1.0 };
        let ns = if neg.abs() >= eps { 1.0 / -neg } else { 1.0 };
        for v in k.values.iter_mut() {
            if *v > 0.0 {
                *v *= ps * scale;
            } else if *v < 0.0 {
                *v *= ns * scale;
            }
        }
        return;
    }
    let mut factor = scale;
    if flags & 0x2000 != 0 {
        let sum = pos + neg;
        let div = if sum.abs() < eps { pos } else { sum };
        if div.is_nan() {
            for v in k.values.iter_mut() {
                *v = f64::NAN;
            }
            return;
        }
        if div.abs() >= eps {
            factor /= div;
        }
    }
    for v in k.values.iter_mut() {
        if !v.is_nan() {
            *v *= factor;
        }
    }
}

fn scale(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d|l!", args)?;
    let s = a[0].double_or(1.0);
    let flags = a[1].long_or(0);
    let o = o.unwrap();
    kernels_of(vm, o)?;
    for k in data(o).list.borrow_mut().iter_mut() {
        scale_kernel(k, s, flags);
    }
    Ok(Value::Null)
}

fn add_unity_kernel(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let s = a[0].double_or(0.0);
    let o = o.unwrap();
    kernels_of(vm, o)?;
    for k in data(o).list.borrow_mut().iter_mut() {
        // an unused origin stays unused
        let i = k.y * k.width + k.x;
        k.values[i] += s;
    }
    Ok(Value::Null)
}

#[allow(dead_code)]
fn key(i: i64) -> ArrayKey {
    ArrayKey::Int(i)
}
