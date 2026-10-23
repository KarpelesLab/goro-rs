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
    };
    Ok(new_kernel_object(vm, vec![k]))
}

/// A few of the built-in kernels of ImageMagick's documentation.
fn builtin(kind: i64, args: &[f64]) -> Option<Kernel> {
    let arg = |i: usize, d: f64| args.get(i).copied().filter(|v| !v.is_nan()).unwrap_or(d);
    let square = |r: usize, f: &dyn Fn(i64, i64) -> f64| {
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
        }
    };
    Some(match kind {
        // Unity
        1 => Kernel {
            width: 1,
            height: 1,
            x: 0,
            y: 0,
            values: vec![arg(0, 1.0)],
        },
        // Gaussian / Blur (Blur: a 1-D row)
        2 | 5 => {
            let sigma = arg(1, 1.0).max(1e-6);
            let r = {
                let r = arg(0, 0.0);
                if r >= 1.0 {
                    r as usize
                } else {
                    (3.0 * sigma).ceil() as usize
                }
            };
            let g = |d2: f64| (-d2 / (2.0 * sigma * sigma)).exp();
            let mut k = if kind == 2 {
                square(r, &|x, y| g((x * x + y * y) as f64))
            } else {
                let n = 2 * r + 1;
                Kernel {
                    width: n,
                    height: 1,
                    x: r,
                    y: 0,
                    values: (0..n as i64)
                        .map(|x| g(((x - r as i64) * (x - r as i64)) as f64))
                        .collect(),
                }
            };
            let s: f64 = k.values.iter().sum();
            for v in k.values.iter_mut() {
                *v /= s;
            }
            k
        }
        // Laplacian
        8 => {
            let t = arg(0, 0.0) as i64;
            let v: Vec<f64> = match t {
                1 => vec![0., -1., 0., -1., 4., -1., 0., -1., 0.],
                2 => vec![-2., 1., -2., 1., 4., 1., -2., 1., -2.],
                3 => vec![1., -2., 1., -2., 4., -2., 1., -2., 1.],
                _ => vec![-1., -1., -1., -1., 8., -1., -1., -1., -1.],
            };
            Kernel {
                width: 3,
                height: 3,
                x: 1,
                y: 1,
                values: v,
            }
        }
        // Sobel
        9 => Kernel {
            width: 3,
            height: 3,
            x: 1,
            y: 1,
            values: vec![1., 0., -1., 2., 0., -2., 1., 0., -1.],
        },
        // Roberts
        11 => Kernel {
            width: 3,
            height: 3,
            x: 1,
            y: 1,
            values: vec![0., 0., 0., 1., -1., 0., 0., 0., 0.],
        },
        // Prewitt
        12 => Kernel {
            width: 3,
            height: 3,
            x: 1,
            y: 1,
            values: vec![1., 0., -1., 1., 0., -1., 1., 0., -1.],
        },
        // Diamond
        15 => {
            let r = arg(0, 1.0).max(1.0) as i64;
            let s = arg(1, 1.0);
            square(r as usize, &|x, y| {
                if x.abs() + y.abs() <= r { s } else { f64::NAN }
            })
        }
        // Square
        16 => {
            let r = arg(0, 1.0).max(1.0) as usize;
            let s = arg(1, 1.0);
            square(r, &|_, _| s)
        }
        // Disk
        19 => {
            let rad = arg(0, 3.5).max(0.5);
            let s = arg(1, 1.0);
            square(rad as usize, &|x, y| {
                if ((x * x + y * y) as f64) <= rad * rad {
                    s
                } else {
                    f64::NAN
                }
            })
        }
        // Plus / Cross
        20 | 21 => {
            let r = arg(0, 2.0).max(1.0) as usize;
            let s = arg(1, 1.0);
            square(r, &|x, y| {
                let on = if kind == 20 {
                    x == 0 || y == 0
                } else {
                    x == y || x == -y
                };
                if on { s } else { f64::NAN }
            })
        }
        _ => return None,
    })
}

fn from_builtin(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ls", args)?;
    let kind = a[0].long_or(0);
    let shape = String::from_utf8_lossy(a[1].str_or(b"")).into_owned();
    let nums: Vec<f64> = shape
        .split([',', 'x', 'X', ' '])
        .map(|t| {
            t.trim()
                .trim_end_matches(['%', '!', '^'])
                .parse()
                .unwrap_or(f64::NAN)
        })
        .collect();
    match builtin(kind, &nums) {
        Some(k) => Ok(new_kernel_object(vm, vec![k])),
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

/// ScaleKernelInfo(), as documented: plain scaling, or normalization first
/// (NormalizeValue: by the sum, or the positive sum for zero-sum kernels;
/// CorrelateNormalizeValue: positive and negative parts apart), then the
/// factor (PercentValue: as a percentage).
pub(crate) fn scale_kernel(k: &mut Kernel, scale: f64, flags: i64) {
    let mut scale = scale;
    if flags & 0x1000 != 0 {
        scale /= 100.0;
    }
    let pos: f64 = k.values.iter().filter(|v| **v > 0.0).sum();
    let neg: f64 = -k.values.iter().filter(|v| **v < 0.0).sum::<f64>();
    if flags & 0x10000 != 0 {
        // correlate normalize: positives to +1, negatives to -1
        for v in k.values.iter_mut() {
            if *v > 0.0 && pos > 0.0 {
                *v = *v / pos * scale;
            } else if *v < 0.0 && neg > 0.0 {
                *v = *v / neg * scale;
            }
        }
        return;
    }
    if flags & 0x2000 != 0 {
        let sum = pos - neg;
        let div = if sum.abs() < 1e-12 { pos } else { sum.abs() };
        if div > 0.0 {
            scale /= div;
        }
    }
    for v in k.values.iter_mut() {
        if !v.is_nan() {
            *v *= scale;
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
        let i = k.y * k.width + k.x;
        if k.values[i].is_nan() {
            k.values[i] = s;
        } else {
            k.values[i] += s;
        }
    }
    Ok(Value::Null)
}

#[allow(dead_code)]
fn key(i: i64) -> ArrayKey {
    ArrayKey::Int(i)
}
