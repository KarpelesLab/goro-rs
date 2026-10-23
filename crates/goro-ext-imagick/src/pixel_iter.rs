//! ImagickPixelIterator (imagickpixeliterator_class.c), over the current
//! image of an Imagick object (shared: syncIterator() writes back into it).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::object::NativeData;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};

use crate::zpp::{self, Arg, Func, Obj};
use crate::{
    C_IMAGICK, Cls, ImagickData, Impl, PixelData, R, arg_obj, arr_val, imagick_data, throw_cls,
};

pub(crate) const METHODS: &[(&str, Impl, bool)] = &[
    ("__construct", construct, false),
    ("clear", clear, false),
    ("destroy", clear, false),
    ("getPixelIterator", get_pixel_iterator, true),
    ("getPixelRegionIterator", get_pixel_region_iterator, true),
    ("getCurrentIteratorRow", current_row, false),
    ("current", current_row, false),
    ("getIteratorRow", key, false),
    ("key", key, false),
    ("getNextIteratorRow", next_row, false),
    ("getPreviousIteratorRow", previous_row, false),
    ("next", next, false),
    ("rewind", rewind, false),
    ("resetIterator", reset_iterator, false),
    ("newPixelIterator", new_pixel_iterator, false),
    ("newPixelRegionIterator", new_pixel_region_iterator, false),
    ("setIteratorFirstRow", set_first_row, false),
    ("setIteratorLastRow", set_last_row, false),
    ("setIteratorRow", set_row, false),
    ("syncIterator", sync_iterator, false),
    ("valid", valid, false),
];

#[derive(Default)]
pub struct IterData {
    source: RefCell<Option<Rc<ImagickData>>>,
    image: Cell<usize>,
    /// x, y, columns, rows
    region: Cell<(usize, usize, usize, usize)>,
    y: Cell<i64>,
    row: RefCell<Vec<Rc<PixelData>>>,
    row_y: Cell<i64>,
    initialized: Cell<bool>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(no_clone),
    ..NativeObjectHandlers::EMPTY
};

fn no_clone(vm: &mut Vm, src: &Obj, _dst: &Obj) {
    let m = format!(
        "Trying to clone an uncloneable object of class {}",
        goro_core::value::display_class_name(&src.borrow().class_name)
    );
    crate::throw(vm, b"Error", &m, 0);
}

fn data(o: &Obj) -> Rc<IterData> {
    {
        let b = o.borrow();
        if let Some(n) = b.native.as_ref() {
            if let Ok(d) = n.data.clone().downcast::<IterData>() {
                return d;
            }
        }
    }
    let d = Rc::new(IterData::default());
    o.borrow_mut().native = Some(NativeData {
        data: d.clone() as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
    d
}

pub(crate) fn create(_vm: &mut Vm, o: &Obj) {
    data(o);
}

fn this(o: Option<&Obj>) -> Rc<IterData> {
    data(o.expect("method with $this"))
}

fn init_check(vm: &mut Vm, d: &IterData) -> Result<(), ()> {
    if !d.initialized.get() {
        throw_cls(
            vm,
            Cls::Iter,
            "ImagickPixelIterator is not initialized correctly",
        )?;
    }
    Ok(())
}

/// Point the iterator at the current image of `src` (NewPixelIterator /
/// NewPixelRegionIterator).
fn setup(
    vm: &mut Vm,
    d: &IterData,
    src: &Obj,
    region: Option<(i64, i64, i64, i64)>,
) -> Result<(), ()> {
    let im = imagick_data(src);
    let (cur, cols, rows) = {
        let w = im.wand.borrow();
        if w.is_empty() {
            drop(w);
            throw_cls(vm, Cls::Imagick, "Can not process empty Imagick object")?;
            return Err(());
        }
        let img = w.image().unwrap();
        (w.current, img.columns, img.rows)
    };
    let r = match region {
        None => (0, 0, cols, rows),
        Some((x, y, w, h)) => {
            if x < 0
                || y < 0
                || w <= 0
                || h <= 0
                || x as usize + w as usize > cols
                || y as usize + h as usize > rows
            {
                throw_cls(vm, Cls::Iter, "Can not allocate ImagickPixelIterator")?;
                return Err(());
            }
            (x as usize, y as usize, w as usize, h as usize)
        }
    };
    *d.source.borrow_mut() = Some(im);
    d.image.set(cur);
    d.region.set(r);
    d.y.set(0);
    d.row.borrow_mut().clear();
    d.row_y.set(-1);
    d.initialized.set(true);
    Ok(())
}

/// The pixel wands of row `y` (relative to the region), cached.
fn row_wands(d: &IterData, y: i64) -> Option<Vec<Rc<PixelData>>> {
    let (rx, ry, rw, rh) = d.region.get();
    if y < 0 || y as usize >= rh {
        return None;
    }
    if d.row_y.get() == y {
        return Some(d.row.borrow().clone());
    }
    let src = d.source.borrow().clone()?;
    let w = src.wand.borrow();
    let img = w.images.get(d.image.get())?;
    let mut row = Vec::with_capacity(rw);
    for x in 0..rw {
        let (px, py) = (rx + x, ry + y as usize);
        if px >= img.columns || py >= img.rows {
            return None;
        }
        row.push(PixelData::new(img.pixel_info(px, py)));
    }
    *d.row.borrow_mut() = row.clone();
    d.row_y.set(y);
    Some(row)
}

/// s_pixelwands_to_zval()
fn row_value(vm: &mut Vm, row: Vec<Rc<PixelData>>) -> Value {
    let mut a = PhpArray::new();
    for p in row {
        a.push(crate::new_pixel_shared(vm, p));
    }
    arr_val(a)
}

fn construct(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|llll", args)?;
    let src = match &a[0] {
        Arg::Any(v) => arg_obj(vm, f, 1, v, C_IMAGICK, "Imagick")?,
        _ => return Err(()),
    };
    setup(vm, &this(o), &src, None)?;
    Ok(Value::Null)
}

fn clear(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    d.y.set(0);
    d.row.borrow_mut().clear();
    d.row_y.set(-1);
    Ok(Value::True)
}

fn new_iter_object(vm: &mut Vm, src: &Obj, region: Option<(i64, i64, i64, i64)>) -> R {
    let o = crate::new_object(vm, b"ImagickPixelIterator");
    let d = data(&o);
    setup(vm, &d, src, region)?;
    Ok(Value::Object(o))
}

fn get_pixel_iterator(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let src = match &a[0] {
        Arg::Any(v) => arg_obj(vm, f, 1, v, C_IMAGICK, "Imagick")?,
        _ => return Err(()),
    };
    new_iter_object(vm, &src, None)
}

fn get_pixel_region_iterator(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zllll", args)?;
    let src = match &a[0] {
        Arg::Any(v) => arg_obj(vm, f, 1, v, C_IMAGICK, "Imagick")?,
        _ => return Err(()),
    };
    let r = (
        a[1].long_or(0),
        a[2].long_or(0),
        a[3].long_or(0),
        a[4].long_or(0),
    );
    new_iter_object(vm, &src, Some(r))
}

/// The pixel iterator of an Imagick object (Imagick::getPixelIterator()).
pub(crate) fn iterator_for(vm: &mut Vm, src: &Obj, region: Option<(i64, i64, i64, i64)>) -> R {
    new_iter_object(vm, src, region)
}

fn new_pixel_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    vm.emit_deprecated("ImagickPixelIterator::newPixelIterator is deprecated. ImagickPixelIterator::getPixelIterator should be used instead");
    let src = match &a[0] {
        Arg::Any(v) => arg_obj(vm, f, 1, v, C_IMAGICK, "Imagick")?,
        _ => return Err(()),
    };
    setup(vm, &this(o), &src, None)?;
    Ok(Value::True)
}

fn new_pixel_region_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zllll", args)?;
    vm.emit_deprecated("ImagickPixelIterator::newPixelRegionIterator is deprecated. ImagickPixelIterator::getPixelRegionIterator should be used instead");
    let src = match &a[0] {
        Arg::Any(v) => arg_obj(vm, f, 1, v, C_IMAGICK, "Imagick")?,
        _ => return Err(()),
    };
    let r = (
        a[1].long_or(0),
        a[2].long_or(0),
        a[3].long_or(0),
        a[4].long_or(0),
    );
    setup(vm, &this(o), &src, Some(r))?;
    Ok(Value::True)
}

fn current_row(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    match row_wands(&d, d.y.get()) {
        Some(row) => Ok(row_value(vm, row)),
        None => Ok(Value::Null),
    }
}

fn key(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    Ok(Value::Long(d.y.get()))
}

fn next_row(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    d.y.set(d.y.get() + 1);
    match row_wands(&d, d.y.get()) {
        Some(row) => Ok(row_value(vm, row)),
        None => Ok(Value::Null),
    }
}

fn previous_row(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    d.y.set(d.y.get() - 1);
    match row_wands(&d, d.y.get()) {
        Some(row) => Ok(row_value(vm, row)),
        None => Ok(Value::Null),
    }
}

fn next(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    d.y.set(d.y.get() + 1);
    Ok(Value::Null)
}

fn rewind(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    d.y.set(0);
    Ok(Value::Null)
}

fn reset_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    rewind(vm, f, o, args)?;
    Ok(Value::True)
}

fn set_first_row(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    rewind(vm, f, o, args)?;
    Ok(Value::True)
}

fn set_last_row(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    let rows = d.region.get().3 as i64;
    d.y.set(rows - 1);
    Ok(Value::True)
}

fn set_row(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    let r = a[0].long_or(0);
    let rows = d.region.get().3 as i64;
    if r < 0 || r >= rows {
        let e = goro_imagick::MagickError::new(0, "");
        return crate::convert_exception(vm, Some(e), Cls::Iter, "Unable to set iterator row");
    }
    d.y.set(r);
    Ok(Value::True)
}

fn sync_iterator(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    let y = d.row_y.get();
    if y < 0 {
        return Ok(Value::True);
    }
    let (rx, ry, _, _) = d.region.get();
    let row = d.row.borrow().clone();
    let Some(src) = d.source.borrow().clone() else {
        return Ok(Value::True);
    };
    let mut w = src.wand.borrow_mut();
    if let Some(img) = w.images.get_mut(d.image.get()) {
        let py = ry + y as usize;
        if py < img.rows {
            for (x, p) in row.iter().enumerate() {
                let px = rx + x;
                if px >= img.columns {
                    break;
                }
                if let Some(c) = p.color.borrow().as_ref() {
                    let mut c = c.clone();
                    if img.colorspace != goro_imagick::types::Colorspace::CMYK
                        && c.colorspace == goro_imagick::types::Colorspace::CMYK
                    {
                        let (r, g, b) = c.rgb_q();
                        c.red = r;
                        c.green = g;
                        c.blue = b;
                        c.colorspace = goro_imagick::types::Colorspace::SRGB;
                    }
                    let q = c.to_rgba16_raw();
                    let a = if img.alpha { q[3] } else { 65535 };
                    if c.alpha_trait && q[3] < 65535 && !img.alpha {
                        img.enable_alpha();
                    }
                    img.set_px(px, py, [q[0], q[1], q[2], if img.alpha { q[3] } else { a }]);
                    if let Some(k) = &mut img.black {
                        k[py * img.columns + px] = goro_imagick::color::to_q16(c.black);
                    }
                }
            }
            img.colors = None;
        }
    }
    Ok(Value::True)
}

fn valid(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = this(o);
    init_check(vm, &d)?;
    let y = d.y.get();
    Ok(crate::bool_val(y >= 0 && (y as usize) < d.region.get().3))
}
