//! ImagickDraw (imagickdraw_class.c) on goro-imagick's DrawingWand: the
//! settings, the primitives, paths, clip paths and patterns, and the
//! vector graphics text. Text is recorded but not rendered (no font
//! renderer yet).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::object::NativeData;
use goro_core::value::Value;
use goro_core::vm::{NativeObjectHandlers, Vm};
use goro_imagick::PixelInfo;
use goro_imagick::draw::{Affine, DrawingWand};
use goro_imagick::types::{CompositeOp, Gravity};

use crate::zpp::{self, Arg, Func, Obj};
use crate::{Cls, Impl, R, arr_val, bool_val, new_pixel, set, str_val, throw_cls, zval_to_pixel};

pub(crate) const METHODS: &[(&str, Impl, bool)] = &[
    ("__construct", construct, false),
    ("clear", clear, false),
    ("destroy", clear, false),
    ("clone", clone_method, false),
    ("resetVectorGraphics", reset_vector_graphics, false),
    ("getVectorGraphics", get_vector_graphics, false),
    ("setVectorGraphics", set_vector_graphics, false),
    ("render", render, false),
    ("push", push, false),
    ("pop", pop, false),
    ("setFillColor", set_fill_color, false),
    ("getFillColor", get_fill_color, false),
    ("setFillAlpha", set_fill_alpha, false),
    ("setFillOpacity", set_fill_opacity, false),
    ("getFillOpacity", get_fill_opacity, false),
    ("setFillRule", set_fill_rule, false),
    ("getFillRule", get_fill_rule, false),
    ("setFillPatternUrl", set_fill_pattern_url, false),
    ("setStrokeColor", set_stroke_color, false),
    ("getStrokeColor", get_stroke_color, false),
    ("setStrokeAlpha", set_stroke_alpha, false),
    ("setStrokeOpacity", set_stroke_opacity, false),
    ("getStrokeOpacity", get_stroke_opacity, false),
    ("setStrokeWidth", set_stroke_width, false),
    ("getStrokeWidth", get_stroke_width, false),
    ("setStrokeAntialias", set_stroke_antialias, false),
    ("getStrokeAntialias", get_stroke_antialias, false),
    ("setStrokeDashArray", set_stroke_dash_array, false),
    ("getStrokeDashArray", get_stroke_dash_array, false),
    ("setStrokeDashOffset", set_stroke_dash_offset, false),
    ("getStrokeDashOffset", get_stroke_dash_offset, false),
    ("setStrokeLineCap", set_stroke_line_cap, false),
    ("getStrokeLineCap", get_stroke_line_cap, false),
    ("setStrokeLineJoin", set_stroke_line_join, false),
    ("getStrokeLineJoin", get_stroke_line_join, false),
    ("setStrokeMiterLimit", set_stroke_miter_limit, false),
    ("getStrokeMiterLimit", get_stroke_miter_limit, false),
    ("setStrokePatternUrl", set_stroke_pattern_url, false),
    ("setOpacity", set_opacity, false),
    ("getOpacity", get_opacity, false),
    ("setGravity", set_gravity, false),
    ("getGravity", get_gravity, false),
    ("setClipPath", set_clip_path, false),
    ("getClipPath", get_clip_path, false),
    ("setClipRule", set_clip_rule, false),
    ("getClipRule", get_clip_rule, false),
    ("setClipUnits", set_clip_units, false),
    ("getClipUnits", get_clip_units, false),
    ("setFont", set_font, false),
    ("getFont", get_font, false),
    ("setFontFamily", set_font_family, false),
    ("getFontFamily", get_font_family, false),
    ("setFontSize", set_font_size, false),
    ("getFontSize", get_font_size, false),
    ("setFontStyle", set_font_style, false),
    ("getFontStyle", get_font_style, false),
    ("setFontWeight", set_font_weight, false),
    ("getFontWeight", get_font_weight, false),
    ("setFontStretch", set_font_stretch, false),
    ("getFontStretch", get_font_stretch, false),
    ("setFontResolution", set_font_resolution, false),
    ("getFontResolution", get_font_resolution, false),
    ("setResolution", set_resolution, false),
    ("setDensity", set_density, false),
    ("getDensity", get_density, false),
    ("setTextAlignment", set_text_alignment, false),
    ("getTextAlignment", get_text_alignment, false),
    ("setTextAntialias", set_text_antialias, false),
    ("getTextAntialias", get_text_antialias, false),
    ("setTextDecoration", set_text_decoration, false),
    ("getTextDecoration", get_text_decoration, false),
    ("setTextEncoding", set_text_encoding, false),
    ("getTextEncoding", get_text_encoding, false),
    ("setTextKerning", set_text_kerning, false),
    ("getTextKerning", get_text_kerning, false),
    ("setTextInterlineSpacing", set_text_interline_spacing, false),
    ("getTextInterlineSpacing", get_text_interline_spacing, false),
    ("setTextInterwordSpacing", set_text_interword_spacing, false),
    ("getTextInterwordSpacing", get_text_interword_spacing, false),
    ("setTextDirection", set_text_direction, false),
    ("getTextDirection", get_text_direction, false),
    ("setTextUnderColor", set_text_under_color, false),
    ("getTextUnderColor", get_text_under_color, false),
    ("setBorderColor", set_border_color, false),
    ("getBorderColor", get_border_color, false),
    ("setViewbox", set_viewbox, false),
    ("affine", affine, false),
    ("translate", translate, false),
    ("scale", scale, false),
    ("rotate", rotate, false),
    ("skewX", skew_x, false),
    ("skewY", skew_y, false),
    ("rectangle", rectangle, false),
    ("roundRectangle", round_rectangle, false),
    ("circle", circle, false),
    ("ellipse", ellipse, false),
    ("arc", arc, false),
    ("line", line, false),
    ("point", point, false),
    ("polygon", polygon, false),
    ("polyline", polyline, false),
    ("bezier", bezier, false),
    ("annotation", annotation, false),
    ("composite", composite, false),
    ("color", color, false),
    ("matte", color, false),
    ("alpha", color, false),
    ("comment", comment, false),
    ("pathStart", path_start, false),
    ("pathFinish", path_finish, false),
    ("pathClose", path_close, false),
    ("pathMoveToAbsolute", path_move_to_absolute, false),
    ("pathMoveToRelative", path_move_to_relative, false),
    ("pathLineToAbsolute", path_line_to_absolute, false),
    ("pathLineToRelative", path_line_to_relative, false),
    (
        "pathLineToHorizontalAbsolute",
        path_line_to_horizontal_absolute,
        false,
    ),
    (
        "pathLineToHorizontalRelative",
        path_line_to_horizontal_relative,
        false,
    ),
    (
        "pathLineToVerticalAbsolute",
        path_line_to_vertical_absolute,
        false,
    ),
    (
        "pathLineToVerticalRelative",
        path_line_to_vertical_relative,
        false,
    ),
    ("pathCurveToAbsolute", path_curve_to_absolute, false),
    ("pathCurveToRelative", path_curve_to_relative, false),
    (
        "pathCurveToSmoothAbsolute",
        path_curve_to_smooth_absolute,
        false,
    ),
    (
        "pathCurveToSmoothRelative",
        path_curve_to_smooth_relative,
        false,
    ),
    (
        "pathCurveToQuadraticBezierAbsolute",
        path_quad_absolute,
        false,
    ),
    (
        "pathCurveToQuadraticBezierRelative",
        path_quad_relative,
        false,
    ),
    (
        "pathCurveToQuadraticBezierSmoothAbsolute",
        path_quad_smooth_absolute,
        false,
    ),
    (
        "pathCurveToQuadraticBezierSmoothRelative",
        path_quad_smooth_relative,
        false,
    ),
    ("pathEllipticArcAbsolute", path_arc_absolute, false),
    ("pathEllipticArcRelative", path_arc_relative, false),
    ("pushClipPath", push_clip_path, false),
    ("popClipPath", pop_clip_path, false),
    ("pushPattern", push_pattern, false),
    ("popPattern", pop_pattern, false),
    ("pushDefs", push_defs, false),
    ("popDefs", pop_defs, false),
];

pub struct DrawData {
    pub wand: RefCell<DrawingWand>,
}

static HANDLERS: NativeObjectHandlers = NativeObjectHandlers {
    clone_obj: Some(clone_obj),
    ..NativeObjectHandlers::EMPTY
};

pub(crate) fn data(o: &Obj) -> Rc<DrawData> {
    {
        let b = o.borrow();
        if let Some(n) = b.native.as_ref() {
            if let Ok(d) = n.data.clone().downcast::<DrawData>() {
                return d;
            }
        }
    }
    let d = Rc::new(DrawData {
        wand: RefCell::new(DrawingWand::new()),
    });
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
    let w = data(src).wand.borrow().clone();
    let d = Rc::new(DrawData {
        wand: RefCell::new(w),
    });
    dst.borrow_mut().native = Some(NativeData {
        data: d as Rc<dyn Any>,
        handlers: &HANDLERS,
    });
}

fn d(o: Option<&Obj>) -> Rc<DrawData> {
    data(o.expect("method with $this"))
}

/// The DrawingWand of an ImagickDraw argument.
pub(crate) fn wand_of(o: &Obj) -> DrawingWand {
    data(o).wand.borrow().clone()
}

fn s(a: &Arg) -> String {
    String::from_utf8_lossy(a.str_or(b"")).into_owned()
}

/// A setter: parse, apply, return true.
fn setter(
    vm: &mut Vm,
    f: &Func,
    o: Option<&Obj>,
    args: &[Value],
    spec: &str,
    apply: impl FnOnce(&mut DrawingWand, &[Arg]),
) -> R {
    let a = zpp::parse(vm, f, spec, args)?;
    apply(&mut d(o).wand.borrow_mut(), &a);
    Ok(Value::True)
}

fn getter(
    vm: &mut Vm,
    f: &Func,
    o: Option<&Obj>,
    args: &[Value],
    get: impl FnOnce(&DrawingWand) -> Value,
) -> R {
    zpp::parse(vm, f, "", args)?;
    let v = get(&d(o).wand.borrow());
    Ok(v)
}

fn dbl(a: &Arg) -> f64 {
    a.double_or(0.0)
}

fn construct(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    Ok(Value::Null)
}

fn clear(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    *d(o).wand.borrow_mut() = DrawingWand::new();
    Ok(Value::True)
}

fn clone_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    vm.emit_deprecated("ImagickDraw::clone method is deprecated and it's use should be avoided");
    let w = d(o).wand.borrow().clone();
    let n = crate::new_object(vm, b"ImagickDraw");
    *data(&n).wand.borrow_mut() = w;
    Ok(Value::Object(n))
}

fn reset_vector_graphics(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    w.mvg.clear();
    w.primitives.clear();
    Ok(Value::True)
}

fn get_vector_graphics(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| str_val(&w.vector_graphics()))
}

fn set_vector_graphics(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let text = s(&a[0]);
    // the MVG is kept; parsing it back into primitives is not supported
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    let inner = text
        .find("<vector-graphics>")
        .and_then(|i| {
            text[i + 17..]
                .find("</vector-graphics>")
                .map(|j| text[i + 17..i + 17 + j].to_string())
        })
        .unwrap_or(text);
    w.mvg = inner;
    Ok(Value::True)
}

fn render(vm: &mut Vm, f: &Func, _o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    // DrawRender() renders onto the wand's own (absent) image
    Ok(Value::True)
}

fn push(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    d(o).wand.borrow_mut().push();
    Ok(Value::True)
}

fn pop(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    if !d(o).wand.borrow_mut().pop() {
        let e = goro_imagick::MagickError::with_desc(
            goro_imagick::error::severity::DRAW_ERROR,
            "UnbalancedGraphicContextPushPop",
            "",
            "error/drawing-wand.c/DrawPopGraphicContext",
        );
        return crate::convert_exception(vm, Some(e), Cls::Draw, "Unable to pop");
    }
    Ok(Value::True)
}

fn color_arg(vm: &mut Vm, a: &Arg) -> Result<PixelInfo, ()> {
    match a {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Draw),
        _ => Err(()),
    }
}

fn set_fill_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let c = color_arg(vm, &a[0])?;
    d(o).wand.borrow_mut().set_fill_color(&c);
    Ok(Value::True)
}

fn get_fill_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = d(o).wand.borrow().gc().fill.clone();
    Ok(new_pixel(vm, c))
}

fn set_fill_alpha(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    vm.emit_deprecated(
        "ImagickDraw::setFillAlpha method is deprecated and it's use should be avoided",
    );
    setter(vm, f, o, args, "d", |w, a| w.set_fill_opacity(dbl(&a[0])))
}

fn set_fill_opacity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.set_fill_opacity(dbl(&a[0])))
}

fn get_fill_opacity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Double(w.gc().fill_opacity))
}

fn set_fill_rule(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        w.set_fill_rule(a[0].long_or(0) as i32)
    })
}

fn get_fill_rule(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().fill_rule as i64))
}

fn set_fill_pattern_url(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let url = s(&a[0]);
    if !d(o).wand.borrow_mut().set_fill_pattern(&url) {
        let e = goro_imagick::MagickError::with_desc(
            goro_imagick::error::severity::DRAW_ERROR,
            "URLNotFound",
            &url,
            "error/drawing-wand.c/DrawSetFillPatternURL",
        );
        return crate::convert_exception(vm, Some(e), Cls::Draw, "Unable to set fill pattern URL");
    }
    Ok(Value::True)
}

fn set_stroke_pattern_url(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let url = s(&a[0]);
    if !d(o).wand.borrow_mut().set_stroke_pattern(&url) {
        let e = goro_imagick::MagickError::with_desc(
            goro_imagick::error::severity::DRAW_ERROR,
            "URLNotFound",
            &url,
            "error/drawing-wand.c/DrawSetStrokePatternURL",
        );
        return crate::convert_exception(
            vm,
            Some(e),
            Cls::Draw,
            "Unable to set stroke pattern URL",
        );
    }
    Ok(Value::True)
}

fn set_stroke_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let c = color_arg(vm, &a[0])?;
    d(o).wand.borrow_mut().set_stroke_color(&c);
    Ok(Value::True)
}

fn get_stroke_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = d(o).wand.borrow().gc().stroke.clone();
    Ok(new_pixel(vm, c))
}

fn set_stroke_alpha(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    vm.emit_deprecated(
        "ImagickDraw::setStrokeAlpha method is deprecated and it's use should be avoided",
    );
    setter(vm, f, o, args, "d", |w, a| w.set_stroke_opacity(dbl(&a[0])))
}

fn set_stroke_opacity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.set_stroke_opacity(dbl(&a[0])))
}

fn get_stroke_opacity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| {
        Value::Double(w.gc().stroke.alpha / 65535.0)
    })
}

fn set_stroke_width(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.set_stroke_width(dbl(&a[0])))
}

fn get_stroke_width(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Double(w.gc().stroke_width))
}

fn set_stroke_antialias(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "b", |w, a| {
        let v = a[0].bool_or(true);
        w.print(&format!("stroke-antialias {}", v as u8));
        w.gc_mut().stroke_antialias = v;
    })
}

fn get_stroke_antialias(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| bool_val(w.gc().stroke_antialias))
}

fn set_stroke_dash_array(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a!", args)?;
    let mut v = Vec::new();
    if let Arg::Arr(arr) = &a[0] {
        for x in arr.borrow().values() {
            match x.deref() {
                Value::Long(n) => v.push(n as f64),
                Value::Double(dd) => v.push(dd),
                Value::String(st) => match String::from_utf8_lossy(st.as_bytes())
                    .trim()
                    .parse::<f64>()
                {
                    Ok(n) => v.push(n),
                    Err(_) => {
                        return throw_cls(vm, Cls::Draw, "Cannot read stroke dash array parameter");
                    }
                },
                _ => return throw_cls(vm, Cls::Draw, "Cannot read stroke dash array parameter"),
            }
        }
    }
    d(o).wand.borrow_mut().set_stroke_dash_array(v);
    Ok(Value::True)
}

fn get_stroke_dash_array(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| {
        let mut a = PhpArray::new();
        for x in &w.gc().dash {
            a.push(Value::Double(*x));
        }
        arr_val(a)
    })
}

fn set_stroke_dash_offset(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| {
        w.set_stroke_dash_offset(dbl(&a[0]))
    })
}

fn get_stroke_dash_offset(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Double(w.gc().dash_offset))
}

fn set_stroke_line_cap(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        w.set_stroke_line_cap(a[0].long_or(0) as i32)
    })
}

fn get_stroke_line_cap(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().line_cap as i64))
}

fn set_stroke_line_join(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        w.set_stroke_line_join(a[0].long_or(0) as i32)
    })
}

fn get_stroke_line_join(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().line_join as i64))
}

fn set_stroke_miter_limit(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        w.set_stroke_miter_limit(a[0].long_or(10) as f64)
    })
}

fn get_stroke_miter_limit(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().miter_limit as i64))
}

fn set_opacity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.set_opacity(dbl(&a[0])))
}

fn get_opacity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Double(w.gc().opacity))
}

fn set_gravity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        w.set_gravity(Gravity(a[0].long_or(0) as i32))
    })
}

fn get_gravity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().gravity.0 as i64))
}

fn set_clip_path(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "s", |w, a| w.set_clip_path(&s(&a[0])))
}

fn get_clip_path(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| match &w.gc().clip_path {
        Some(c) => str_val(c),
        None => Value::False,
    })
}

fn set_clip_rule(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        let r = a[0].long_or(0) as i32;
        w.print(&format!(
            "clip-rule '{}'",
            if r == 2 { "nonzero" } else { "evenodd" }
        ));
        w.gc_mut().clip_rule = r;
    })
}

fn get_clip_rule(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().clip_rule as i64))
}

fn set_clip_units(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        let u = a[0].long_or(0) as i32;
        let name = [
            "Undefined",
            "userSpace",
            "userSpaceOnUse",
            "objectBoundingBox",
        ]
        .get(u as usize)
        .copied()
        .unwrap_or("Undefined");
        w.print(&format!("clip-units '{}'", name));
        w.gc_mut().clip_units = u;
    })
}

fn get_clip_units(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().clip_units as i64))
}

fn set_font(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let font = s(&a[0]);
    if font.is_empty() {
        return throw_cls(vm, Cls::Draw, "Can not set empty font");
    }
    // php_imagick_check_font(): a configured font name, or a font file
    if goro_imagick::fonts::resolve(&font).is_none() {
        let abs = if font.starts_with('/') {
            font.clone()
        } else {
            std::env::current_dir()
                .map(|c| c.join(&font).to_string_lossy().into_owned())
                .unwrap_or(font.clone())
        };
        crate::throw(
            vm,
            b"ImagickDrawException",
            &format!("The path does not exist: {}", abs),
            1,
        );
        return Err(());
    }
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    w.print(&format!("font '{}'", font));
    w.gc_mut().font = Some(font);
    Ok(Value::True)
}

fn get_font(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| match &w.gc().font {
        Some(x) => str_val(x),
        None => Value::False,
    })
}

fn set_font_family(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let fam = s(&a[0]);
    if fam.is_empty() {
        return throw_cls(vm, Cls::Draw, "Can not set empty font family");
    }
    // no font configuration: the family names are accepted as given
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    w.print(&format!("font-family '{}'", fam));
    w.gc_mut().family = Some(fam);
    Ok(Value::True)
}

fn get_font_family(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| match &w.gc().family {
        Some(x) => str_val(x),
        None => Value::False,
    })
}

fn set_font_size(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| {
        let v = dbl(&a[0]);
        w.print(&format!("font-size {}", goro_imagick::draw::num(v)));
        w.gc_mut().font_size = v;
    })
}

fn get_font_size(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Double(w.gc().font_size))
}

fn set_font_style(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        let v = a[0].long_or(0) as i32;
        let name = ["Undefined", "normal", "italic", "oblique", "all", "bold"]
            .get(v as usize)
            .copied()
            .unwrap_or("undefined");
        w.print(&format!("font-style '{}'", name));
        w.gc_mut().font_style = v;
    })
}

fn get_font_style(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().font_style as i64))
}

fn set_font_weight(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let v = a[0].long_or(0);
    if !(100..=900).contains(&v) {
        return throw_cls(vm, Cls::Draw, "Font weight valid range is 100-900");
    }
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    w.print(&format!("font-weight {}", v));
    w.gc_mut().font_weight = v;
    Ok(Value::True)
}

fn get_font_weight(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().font_weight))
}

fn set_font_stretch(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "l", |w, a| {
        let v = a[0].long_or(0) as i32;
        w.print(&format!("font-stretch '{}'", v));
        w.gc_mut().font_stretch = v;
    })
}

fn get_font_stretch(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| Value::Long(w.gc().font_stretch as i64))
}

fn set_font_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.font_resolution = (dbl(&a[0]), dbl(&a[1]));
    })
}

fn get_font_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| {
        let mut a = PhpArray::new();
        set(&mut a, "x", Value::Double(w.font_resolution.0));
        set(&mut a, "y", Value::Double(w.font_resolution.1));
        arr_val(a)
    })
}

fn set_resolution(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        let (x, y) = (dbl(&a[0]), dbl(&a[1]));
        w.density = Some(format!(
            "{}x{}",
            goro_imagick::draw::num(x),
            goro_imagick::draw::num(y)
        ));
        w.font_resolution = (x, y);
    })
}

fn set_density(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "s", |w, a| {
        let dens = s(&a[0]);
        w.print(&format!("density '{}'", dens));
        w.density = Some(dens);
    })
}

fn get_density(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| match &w.density {
        Some(x) => str_val(x),
        None => Value::Null,
    })
}

macro_rules! int_setting {
    ($set:ident, $get:ident, $field:ident, $mvg:expr) => {
        fn $set(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            setter(vm, f, o, args, "l", |w, a| {
                let v = a[0].long_or(0) as i32;
                w.print(&format!("{} {}", $mvg, v));
                w.gc_mut().$field = v;
            })
        }
        fn $get(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            getter(vm, f, o, args, |w| Value::Long(w.gc().$field as i64))
        }
    };
}

int_setting!(
    set_text_alignment,
    get_text_alignment,
    text_align,
    "text-align"
);
int_setting!(
    set_text_decoration,
    get_text_decoration,
    decoration,
    "decorate"
);
int_setting!(
    set_text_direction,
    get_text_direction,
    direction,
    "direction"
);

macro_rules! dbl_setting {
    ($set:ident, $get:ident, $field:ident, $mvg:expr) => {
        fn $set(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            setter(vm, f, o, args, "d", |w, a| {
                let v = dbl(&a[0]);
                w.print(&format!("{} {}", $mvg, goro_imagick::draw::num(v)));
                w.gc_mut().$field = v;
            })
        }
        fn $get(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
            getter(vm, f, o, args, |w| Value::Double(w.gc().$field))
        }
    };
}

dbl_setting!(set_text_kerning, get_text_kerning, kerning, "kerning");
dbl_setting!(
    set_text_interline_spacing,
    get_text_interline_spacing,
    interline,
    "interline-spacing"
);
dbl_setting!(
    set_text_interword_spacing,
    get_text_interword_spacing,
    interword,
    "interword-spacing"
);

fn set_text_antialias(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "b", |w, a| {
        let v = a[0].bool_or(true);
        w.print(&format!("text-antialias {}", v as u8));
        w.gc_mut().text_antialias = v;
    })
}

fn get_text_antialias(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| bool_val(w.gc().text_antialias))
}

fn set_text_encoding(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "s", |w, a| {
        let e = s(&a[0]);
        w.print(&format!("encoding '{}'", e));
        w.gc_mut().encoding = Some(e);
    })
}

fn get_text_encoding(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    getter(vm, f, o, args, |w| match &w.gc().encoding {
        Some(e) => str_val(e),
        None => Value::False,
    })
}

fn set_text_under_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let c = color_arg(vm, &a[0])?;
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    w.print(&format!("text-undercolor '{}'", c.to_color_string()));
    w.gc_mut().under_color = c;
    Ok(Value::True)
}

fn get_text_under_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = d(o).wand.borrow().gc().under_color.clone();
    Ok(new_pixel(vm, c))
}

fn set_border_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let c = color_arg(vm, &a[0])?;
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    w.print(&format!("border-color '{}'", c.to_color_string()));
    w.gc_mut().border_color = c;
    Ok(Value::True)
}

fn get_border_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let c = d(o).wand.borrow().gc().border_color.clone();
    Ok(new_pixel(vm, c))
}

fn set_viewbox(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "llll", |w, a| {
        let v = (
            a[0].long_or(0),
            a[1].long_or(0),
            a[2].long_or(0),
            a[3].long_or(0),
        );
        w.print(&format!("viewbox {} {} {} {}", v.0, v.1, v.2, v.3));
        w.gc_mut().viewbox = Some(v);
    })
}

fn affine(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a", args)?;
    let Arg::Arr(arr) = &a[0] else {
        return Err(());
    };
    let arr = arr.borrow();
    let mut v = [0f64; 6];
    for (i, k) in ["sx", "rx", "ry", "sy", "tx", "ty"].iter().enumerate() {
        match arr.get_str(k.as_bytes()) {
            Some(x) => v[i] = x.deref().to_double(),
            None => {
                return throw_cls(
                    vm,
                    Cls::Draw,
                    "AffineMatrix must contain keys: sx, rx, ry, sy, tx and ty",
                );
            }
        }
    }
    drop(arr);
    d(o).wand.borrow_mut().affine(Affine {
        sx: v[0],
        rx: v[1],
        ry: v[2],
        sy: v[3],
        tx: v[4],
        ty: v[5],
    });
    Ok(Value::True)
}

fn translate(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.translate(dbl(&a[0]), dbl(&a[1]))
    })
}

fn scale(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| w.scale(dbl(&a[0]), dbl(&a[1])))
}

fn rotate(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.rotate(dbl(&a[0])))
}

fn skew_x(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.skew_x(dbl(&a[0])))
}

fn skew_y(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| w.skew_y(dbl(&a[0])))
}

fn rectangle(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.rectangle(dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn round_rectangle(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddddd", |w, a| {
        w.round_rectangle(
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            dbl(&a[3]),
            dbl(&a[4]),
            dbl(&a[5]),
        )
    })
}

fn circle(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.circle(dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn ellipse(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddddd", |w, a| {
        w.ellipse(
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            dbl(&a[3]),
            dbl(&a[4]),
            dbl(&a[5]),
        )
    })
}

fn arc(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddddd", |w, a| {
        w.arc(
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            dbl(&a[3]),
            dbl(&a[4]),
            dbl(&a[5]),
        )
    })
}

fn line(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.line(dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn point(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| w.point(dbl(&a[0]), dbl(&a[1])))
}

/// php_imagick_zval_to_pointinfo_array()
fn points(arr: &Arg) -> Option<Vec<(f64, f64)>> {
    let Arg::Arr(arr) = arr else {
        return None;
    };
    let arr = arr.borrow();
    if arr.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for v in arr.values() {
        let Value::Array(p) = v.deref() else {
            return None;
        };
        let p = p.borrow();
        if p.len() != 2 {
            return None;
        }
        let x = p.get_str(b"x")?.deref().to_double();
        let y = p.get_str(b"y")?.deref().to_double();
        out.push((x, y));
    }
    Some(out)
}

fn poly(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value], kind: u8) -> R {
    let a = zpp::parse(vm, f, "a", args)?;
    let Some(pts) = points(&a[0]) else {
        return throw_cls(vm, Cls::Draw, "Unable to read coordinate array");
    };
    let dd = d(o);
    let mut w = dd.wand.borrow_mut();
    match kind {
        0 => w.polygon(&pts),
        1 => w.polyline(&pts),
        _ => w.bezier(&pts),
    }
    Ok(Value::True)
}

fn polygon(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    poly(vm, f, o, args, 0)
}

fn polyline(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    poly(vm, f, o, args, 1)
}

fn bezier(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    poly(vm, f, o, args, 2)
}

fn annotation(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dds", |w, a| {
        w.annotation(dbl(&a[0]), dbl(&a[1]), &s(&a[2]))
    })
}

fn composite(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lddddz", args)?;
    let other = crate::imagick::other_imagick(vm, f, 6, &a[5])?;
    let img = other.wand.borrow().image().unwrap().clone();
    d(o).wand.borrow_mut().composite(
        CompositeOp(a[0].long_or(54) as i32),
        dbl(&a[1]),
        dbl(&a[2]),
        dbl(&a[3]),
        dbl(&a[4]),
        &img,
    );
    Ok(Value::True)
}

fn color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    // color / matte / alpha: flood-fill style painting, recorded only
    let a = zpp::parse(vm, f, "ddl", args)?;
    let name = f
        .name
        .rsplit("::")
        .next()
        .unwrap_or("color")
        .to_ascii_lowercase();
    let method = ["", "point", "replace", "floodfill", "filltoborder", "reset"]
        .get(a[2].long_or(0) as usize)
        .copied()
        .unwrap_or("point");
    d(o).wand.borrow_mut().print(&format!(
        "{} {} {} {}",
        name,
        goro_imagick::draw::num(dbl(&a[0])),
        goro_imagick::draw::num(dbl(&a[1])),
        method
    ));
    Ok(Value::True)
}

fn comment(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "s", |w, a| {
        w.print(&format!("#{}", s(&a[0])))
    })
}

fn path_start(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "", |w, _| w.path_start())
}

fn path_finish(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "", |w, _| w.path_finish())
}

fn path_close(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "", |w, _| w.path_close())
}

fn path_move_to_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.path_move_to(false, dbl(&a[0]), dbl(&a[1]))
    })
}

fn path_move_to_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.path_move_to(true, dbl(&a[0]), dbl(&a[1]))
    })
}

fn path_line_to_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.path_line_to(false, dbl(&a[0]), dbl(&a[1]))
    })
}

fn path_line_to_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.path_line_to(true, dbl(&a[0]), dbl(&a[1]))
    })
}

fn path_line_to_horizontal_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| {
        w.path_line_to_h(false, dbl(&a[0]))
    })
}

fn path_line_to_horizontal_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| {
        w.path_line_to_h(true, dbl(&a[0]))
    })
}

fn path_line_to_vertical_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| {
        w.path_line_to_v(false, dbl(&a[0]))
    })
}

fn path_line_to_vertical_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "d", |w, a| {
        w.path_line_to_v(true, dbl(&a[0]))
    })
}

fn path_curve_to_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddddd", |w, a| {
        w.path_curve_to(
            false,
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            dbl(&a[3]),
            dbl(&a[4]),
            dbl(&a[5]),
        )
    })
}

fn path_curve_to_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddddd", |w, a| {
        w.path_curve_to(
            true,
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            dbl(&a[3]),
            dbl(&a[4]),
            dbl(&a[5]),
        )
    })
}

fn path_curve_to_smooth_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.path_curve_to_smooth(false, dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn path_curve_to_smooth_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.path_curve_to_smooth(true, dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn path_quad_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.path_quad_to(false, dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn path_quad_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddd", |w, a| {
        w.path_quad_to(true, dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
    })
}

fn path_quad_smooth_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.path_quad_to_smooth(false, dbl(&a[0]), dbl(&a[1]))
    })
}

fn path_quad_smooth_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dd", |w, a| {
        w.path_quad_to_smooth(true, dbl(&a[0]), dbl(&a[1]))
    })
}

fn path_arc_absolute(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddbbdd", |w, a| {
        w.path_arc(
            false,
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            a[3].bool_or(false),
            a[4].bool_or(false),
            dbl(&a[5]),
            dbl(&a[6]),
        )
    })
}

fn path_arc_relative(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "dddbbdd", |w, a| {
        w.path_arc(
            true,
            dbl(&a[0]),
            dbl(&a[1]),
            dbl(&a[2]),
            a[3].bool_or(false),
            a[4].bool_or(false),
            dbl(&a[5]),
            dbl(&a[6]),
        )
    })
}

fn push_clip_path(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "s", |w, a| w.push_clip_path(&s(&a[0])))
}

fn pop_clip_path(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "", |w, _| w.pop_clip_path())
}

fn push_pattern(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "sdddd", |w, a| {
        w.push_pattern(&s(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]), dbl(&a[4]))
    })
}

fn pop_pattern(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    if !d(o).wand.borrow_mut().pop_pattern() {
        let e = goro_imagick::MagickError::with_desc(
            goro_imagick::error::severity::DRAW_ERROR,
            "NotCurrentlyPushingPatternDefinition",
            "",
            "error/drawing-wand.c/DrawPopPattern",
        );
        return crate::convert_exception(vm, Some(e), Cls::Draw, "Unable to pop pattern");
    }
    Ok(Value::True)
}

fn push_defs(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "", |w, _| w.push_defs())
}

fn pop_defs(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    setter(vm, f, o, args, "", |w, _| w.pop_defs())
}
