//! More Imagick operations: distortions, layer optimisation, morphology,
//! the edge / threshold / statistics family and assorted effects.

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_imagick::color::{PixelInfo, clamp_to_quantum, to_q16};
use goro_imagick::composite::{ComposeArgs, composite_image};
use goro_imagick::filters::apply_oxideav;
use goro_imagick::geometry::Rect;
use goro_imagick::image::Image;
use goro_imagick::oxideav_image_filter as of;
use goro_imagick::types::*;
use goro_imagick::{MResult, MagickError, Wand, transform};

use crate::imagick::{image_op, not_empty, other_imagick, replace_op, this, with_image};
use crate::zpp::{self, Arg, Func, Obj};
use crate::{Cls, Impl, R, arr_val, convert_exception, new_imagick, set, throw_cls, zval_to_pixel};

pub(crate) const OPS2: &[(&str, Impl, bool)] = &[
    ("distortImage", distort_image, false),
    ("deconstructImages", deconstruct_images, false),
    ("compareImageLayers", compare_image_layers, false),
    ("optimizeImageLayers", optimize_image_layers, false),
    (
        "optimizeImageTransparency",
        optimize_image_transparency,
        false,
    ),
    ("deskewImage", deskew_image, false),
    ("complexImages", complex_images, false),
    ("encipherImage", encipher_image, false),
    ("decipherImage", decipher_image, false),
    (
        "forwardFourierTransformImage",
        forward_fourier_transform_image,
        false,
    ),
    (
        "inverseFourierTransformImage",
        inverse_fourier_transform_image,
        false,
    ),
    ("floodfillPaintImage", floodfill_paint_image, false),
    ("haldClutImage", hald_clut_image, false),
    ("roundCornersImage", round_corners_image, false),
    ("roundCorners", round_corners_image, false),
    ("rotationalBlurImage", rotational_blur_image, false),
    ("radialBlurImage", radial_blur_image, false),
    ("selectiveBlurImage", selective_blur_image, false),
    ("segmentImage", segment_image, false),
    ("shadowImage", shadow_image, false),
    ("shearImage", shear_image, false),
    ("sparseColorImage", sparse_color_image, false),
    ("subimageMatch", subimage_match, false),
    ("similarityImage", subimage_match, false),
    ("remapImage", remap_image, false),
    ("liquidRescaleImage", liquid_rescale_image, false),
    ("displayImage", display_image, false),
    ("displayImages", display_image, false),
    ("animateImages", animate_images, false),
    ("clipImage", clip_image, false),
    ("clipPathImage", clip_path_image, false),
    ("clipImagePath", clip_path_image, false),
    ("textureImage", texture_image, false),
    ("localContrastImage", local_contrast_image, false),
    ("colorDecisionListImage", color_decision_list_image, false),
    ("houghLineImage", hough_line_image, false),
    ("cannyEdgeImage", canny_edge_image, false),
    ("waveletDenoiseImage", wavelet_denoise_image, false),
    ("meanShiftImage", mean_shift_image, false),
    ("kmeansImage", kmeans_image, false),
    ("rangeThresholdImage", range_threshold_image, false),
    ("autoThresholdImage", auto_threshold_image, false),
    ("bilateralBlurImage", bilateral_blur_image, false),
    ("claheImage", clahe_image, false),
    ("levelImageColors", level_image_colors, false),
    ("levelizeImage", levelize_image, false),
    ("orderedDitherImage", ordered_dither_image, false),
    ("orderedPosterizeImage", ordered_posterize_image, false),
    ("whiteBalanceImage", white_balance_image, false),
    ("channelFxImage", channel_fx_image, false),
    ("colorThresholdImage", color_threshold_image, false),
    ("getImageKurtosis", get_image_kurtosis, false),
    ("getImageChannelKurtosis", get_image_kurtosis, false),
    ("polynomialImage", polynomial_image, false),
    ("polaroidImage", polaroid_image, false),
    (
        "polaroidWithTextAndMethod",
        polaroid_with_text_and_method,
        false,
    ),
    ("morphology", morphology, false),
    ("fxImage", fx_image, false),
    ("affineTransformImage", affine_transform_image, false),
    ("stereoImage", stereo_image, false),
    ("steganoImage", stegano_image, false),
    ("smushImages", smush_images, false),
    ("combineImages", combine_images, false),
    ("morphImages", morph_images, false),
    ("previewImages", preview_images, false),
    ("getImageDistortions", get_image_distortions, false),
    (
        "getImageChannelDistortions",
        get_image_channel_distortions,
        false,
    ),
    ("getImageColormapColor", get_image_colormap_color, false),
    ("setImageColormapColor", set_image_colormap_color, false),
    ("montageImage", montage_image, false),
];

fn d_(o: Option<&Obj>) -> std::rc::Rc<crate::ImagickData> {
    this(o)
}

fn color_arg(vm: &mut Vm, a: &Arg) -> Result<PixelInfo, ()> {
    match a {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Imagick),
        _ => Err(()),
    }
}

fn doubles(vm: &mut Vm, a: &Arg, msg: &str) -> Result<Vec<f64>, ()> {
    let Arg::Arr(arr) = a else {
        return Err(());
    };
    let mut v = Vec::new();
    for x in arr.borrow().values() {
        match x.deref() {
            Value::Long(n) => v.push(n as f64),
            Value::Double(d) => v.push(d),
            Value::String(_) | Value::True | Value::False | Value::Null => {
                v.push(x.deref().to_double())
            }
            _ => {
                throw_cls(vm, Cls::Imagick, msg)?;
                return Err(());
            }
        }
    }
    Ok(v)
}

fn new_wand_from(d: &crate::ImagickData, images: Vec<Image>) -> Wand {
    let src = d.wand.borrow();
    let mut w = Wand::new();
    w.settings = src.settings.clone();
    w.images = images;
    w.current = w.images.len().saturating_sub(1);
    w
}

// ---------------------------------------------------------------------------

fn distort_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lab", args)?;
    let method = a[0].long_or(0);
    let params = doubles(vm, &a[1], "Can't read argument array")?;
    let bestfit = a[2].bool_or(false);
    replace_op(vm, &d_(o), "Unable to distort the image", |i| {
        goro_imagick::distort::distort(i, method, &params, bestfit)
    })
}

/// The bounding box of the pixels that differ between two frames.
fn diff_box(a: &Image, b: &Image) -> Option<Rect> {
    let (w, h) = (a.columns.min(b.columns), a.rows.min(b.rows));
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    let mut any = false;
    for y in 0..h {
        for x in 0..w {
            if a.px(x, y) != b.px(x, y) {
                any = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    any.then(|| Rect::new(x1 - x0 + 1, y1 - y0 + 1, x0 as i64, y0 as i64))
}

/// CompareImagesLayers(CompareAnyLayer): every frame but the first cut to
/// what changed from the previous (coalesced) frame.
fn deconstruct(images: &[Image]) -> Vec<Image> {
    let mut out = Vec::with_capacity(images.len());
    for (k, img) in images.iter().enumerate() {
        if k == 0 {
            out.push(img.clone());
            continue;
        }
        let prev = &images[k - 1];
        let r = diff_box(prev, img).unwrap_or(Rect::new(1, 1, 0, 0));
        let mut c = img.crop_raw(r);
        c.page = Rect::new(img.columns, img.rows, r.x, r.y);
        out.push(c);
    }
    out
}

fn deconstruct_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = deconstruct(&d.wand.borrow().images);
    let nw = new_wand_from(&d, out);
    Ok(new_imagick(vm, nw))
}

fn compare_image_layers(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "l", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = deconstruct(&d.wand.borrow().images);
    let nw = new_wand_from(&d, out);
    Ok(new_imagick(vm, nw))
}

fn optimize_image_layers(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = deconstruct(&d.wand.borrow().images);
    let nw = new_wand_from(&d, out);
    Ok(new_imagick(vm, nw))
}

fn optimize_image_transparency(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    // pixels unchanged from the previous frame become transparent
    let mut w = d.wand.borrow_mut();
    let prev: Vec<Image> = w.images.clone();
    for (k, img) in w.images.iter_mut().enumerate().skip(1) {
        let p = &prev[k - 1];
        if p.columns != img.columns || p.rows != img.rows {
            continue;
        }
        img.enable_alpha();
        for y in 0..img.rows {
            for x in 0..img.columns {
                if p.px(x, y) == img.px(x, y) {
                    img.set_px(x, y, [0, 0, 0, 0]);
                }
            }
        }
        img.colors = None;
    }
    Ok(Value::True)
}

fn deskew_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let t = a[0].double_or(0.0);
    replace_op(vm, &d_(o), "Unable to deskew image", |i| {
        // the skew angle (a Radon transform), then an affine rotation
        // about the origin onto the best-fit canvas, the uncovered area in
        // the background colour (as DeskewImage() does)
        let angle = goro_imagick::filters::deskew_angle(i, t);
        let (s, c) = angle.to_radians().sin_cos();
        let mut src = i.clone();
        src.virtual_pixel = VirtualPixelMethod::Background;
        let mut out = goro_imagick::distort::distort(
            &src,
            goro_imagick::distort::AFFINE_PROJECTION,
            &[c, s, -s, c, 0.0, 0.0],
            true,
        )?;
        out.virtual_pixel = i.virtual_pixel;
        out.alpha = i.alpha || i.background_color.to_rgba16()[3] < 65535;
        out.artifacts.set("deskew:angle", &format_g20(angle));
        Ok(out)
    })
}

/// printf("%.20g").
fn format_g20(v: f64) -> String {
    let e = format!("{:.19e}", v);
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let strip = |s: String| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if !(-4..20).contains(&exp) {
        format!(
            "{}e{}{:02}",
            strip(mant.to_string()),
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    } else {
        let decimals = (19 - exp).max(0) as usize;
        strip(format!("{:.*}", decimals, v))
    }
}

fn floodfill_paint_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zdzllb|l", args)?;
    let fill = color_arg(vm, &a[0])?;
    let fuzz = a[1].double_or(0.0);
    let border = color_arg(vm, &a[2])?;
    let (sx, sy) = (a[3].long_or(0), a[4].long_or(0));
    let invert = a[5].bool_or(false);
    image_op(vm, &d_(o), "Unable to floodfill paint image", |i| {
        if sx < 0 || sy < 0 || sx as usize >= i.columns || sy as usize >= i.rows {
            return Ok(());
        }
        let target = if border.alpha_trait && border.alpha == 0.0 && !invert {
            i.px(sx as usize, sy as usize)
        } else {
            border.to_rgba16()
        };
        let fill_q = fill.to_rgba16();
        let f = fuzz.max(std::f64::consts::FRAC_1_SQRT_2);
        let f2 = f * f;
        let matches = |p: [u16; 4]| {
            let mut d = 0.0;
            for c in 0..3 {
                let x = p[c] as f64 - target[c] as f64;
                d += x * x;
            }
            (d / 3.0 <= f2) != invert
        };
        let (w, h) = (i.columns, i.rows);
        let mut seen = vec![false; w * h];
        let mut stack = vec![(sx as usize, sy as usize)];
        while let Some((x, y)) = stack.pop() {
            if seen[y * w + x] || !matches(i.px(x, y)) {
                continue;
            }
            seen[y * w + x] = true;
            i.set_px(
                x,
                y,
                [
                    fill_q[0],
                    fill_q[1],
                    fill_q[2],
                    if i.alpha { fill_q[3] } else { 65535 },
                ],
            );
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < w {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < h {
                stack.push((x, y + 1));
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn hald_clut_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|l", args)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let hald = other.wand.borrow().image().unwrap().clone();
    image_op(vm, &d_(o), "Unable to hald clut image", |i| {
        // the hald level from its size (L^3 x L^3 for L^2 levels per channel)
        let side = hald.columns.min(hald.rows);
        let mut level = 2usize;
        while (level * level * level) < side {
            level += 1;
        }
        let cube = level * level;
        if cube < 2 {
            return Ok(());
        }
        // trilinear between the eight surrounding table entries
        let entry = |r: usize, g: usize, b: usize| {
            let n = r + g * cube + b * cube * cube;
            hald.px(n % hald.columns, (n / hald.columns).min(hald.rows - 1))
        };
        for p in i.pixels.chunks_exact_mut(4) {
            let pos = |v: u16| {
                let t = (v as f64 / 65535.0) * (cube - 1) as f64;
                let k = (t.floor() as usize).min(cube - 1);
                (k, (k + 1).min(cube - 1), t - k as f64)
            };
            let (r0, r1, fr) = pos(p[0]);
            let (g0, g1, fg) = pos(p[1]);
            let (b0, b1, fb) = pos(p[2]);
            let mut out = [0f64; 3];
            for (b, wb) in [(b0, 1.0 - fb), (b1, fb)] {
                for (g, wg) in [(g0, 1.0 - fg), (g1, fg)] {
                    for (r, wr) in [(r0, 1.0 - fr), (r1, fr)] {
                        let w = wr * wg * wb;
                        if w == 0.0 {
                            continue;
                        }
                        let q = entry(r, g, b);
                        for c in 0..3 {
                            out[c] += w * q[c] as f64;
                        }
                    }
                }
            }
            for c in 0..3 {
                p[c] = to_q16(out[c]);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn round_corners_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|ddd", args)?;
    let (rx, ry) = (a[0].double_or(0.0), a[1].double_or(0.0));
    image_op(vm, &d_(o), "Unable to round corners on empty image", |i| {
        if i.columns == 0 || i.rows == 0 {
            return Err(MagickError::new(
                goro_imagick::error::severity::IMAGE_ERROR,
                "NegativeOrZeroImageSize",
            ));
        }
        // a rounded rectangle mask on the alpha channel
        i.enable_alpha();
        let (w, h) = (i.columns as f64, i.rows as f64);
        let mut mask = goro_imagick::draw::DrawingWand::new();
        mask.set_fill_color(&PixelInfo::white());
        mask.round_rectangle(0.0, 0.0, w - 1.0, h - 1.0, rx, ry);
        let mut m = Image::new(i.columns, i.rows, &PixelInfo::transparent());
        m.enable_alpha();
        mask.render_onto(&mut m)?;
        for (p, q) in i.pixels.chunks_exact_mut(4).zip(m.pixels.chunks_exact(4)) {
            p[3] = ((p[3] as u32 * q[3] as u32) / 65535) as u16;
        }
        i.colors = None;
        Ok(())
    })
}

fn rotational_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d|l", args)?;
    let ang = a[0].double_or(0.0);
    let ch = a[1].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to rotational blur image", |i| {
        goro_imagick::effects::rotational_blur(i, ang, ch);
        Ok(())
    })
}

fn radial_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    rotational_blur_image(vm, f, o, args)
}

fn selective_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd|l", args)?;
    let (r, s, t) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(0.0),
    );
    let ch = a[3].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to selectiveblurimage", |i| {
        goro_imagick::effects::selective_blur(i, r, s, t, ch);
        Ok(())
    })
}

fn segment_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ldd|b", args)?;
    let cs = Colorspace(a[0].long_or(0) as i32);
    let (ct, st) = (a[1].double_or(1.0), a[2].double_or(1.5));
    image_op(vm, &d_(o), "Unable to segment image", |i| {
        goro_imagick::segment::segment(i, cs, ct, st);
        Ok(())
    })
}

/// ShadowImage(): the alpha shape in the background colour at `op`
/// percent, blurred, with a border for the blur.
fn shadow_of(i: &Image, op: f64, sigma: f64, dx: i64, dy: i64) -> Image {
    let pad = (2.0 * sigma).ceil() as usize;
    let (w, h) = (i.columns + 2 * pad, i.rows + 2 * pad);
    let mut s = i.clone_attributes(w, h);
    s.enable_alpha();
    let bg = i.background_color.to_rgba16();
    for y in 0..h {
        for x in 0..w {
            let a = if x >= pad && y >= pad && x - pad < i.columns && y - pad < i.rows {
                let p = i.px(x - pad, y - pad);
                if i.alpha { p[3] } else { 65535 }
            } else {
                0
            };
            let a = (a as f64 * op / 100.0).round() as u16;
            s.set_px(x, y, [bg[0], bg[1], bg[2], a]);
        }
    }
    if sigma > 0.0 {
        goro_imagick::convolve::blur(&mut s, 0.0, sigma, channel::ALPHA);
    }
    // (ImageMagick grows the page by the offset too)
    let (px, py) = (dx - pad as i64, dy - pad as i64);
    s.page = Rect::new(
        (w as i64 + px).max(0) as usize,
        (h as i64 + py).max(0) as usize,
        px,
        py,
    );
    s.colors = None;
    s
}

fn shadow_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddll", args)?;
    let (op, sigma) = (a[0].double_or(80.0), a[1].double_or(1.0));
    let (dx, dy) = (a[2].long_or(0), a[3].long_or(0));
    replace_op(vm, &d_(o), "Unable to shadow image", |i| {
        Ok(shadow_of(i, op, sigma, dx, dy))
    })
}

fn shear_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zdd", args)?;
    let bg = color_arg(vm, &a[0])?;
    let (xs, ys) = (a[1].double_or(0.0), a[2].double_or(0.0));
    replace_op(vm, &d_(o), "Unable to shear image", |i| {
        goro_imagick::shear::shear(i, xs, ys, &bg)
    })
}

fn sparse_color_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "la|l", args)?;
    let method = a[0].long_or(0);
    let v = doubles(vm, &a[1], "The map must contain only numeric values")?;
    image_op(vm, &d_(o), "Unable to sparse color image", |i| {
        // x, y, r, g, b per control point (values 0..1)
        let pts: Vec<(f64, f64, [f64; 3])> = v
            .chunks_exact(5)
            .map(|c| (c[0], c[1], [c[2], c[3], c[4]]))
            .collect();
        if pts.is_empty() {
            return Ok(());
        }
        for y in 0..i.rows {
            for x in 0..i.columns {
                let (fx, fy) = (x as f64, y as f64);
                let col = match method {
                    18 => {
                        // voronoi: the nearest point
                        let n = pts
                            .iter()
                            .min_by(|a, b| {
                                let da = (a.0 - fx).powi(2) + (a.1 - fy).powi(2);
                                let db = (b.0 - fx).powi(2) + (b.1 - fy).powi(2);
                                da.partial_cmp(&db).unwrap()
                            })
                            .unwrap();
                        n.2
                    }
                    20 => {
                        // manhattan voronoi
                        let n = pts
                            .iter()
                            .min_by(|a, b| {
                                let da = (a.0 - fx).abs() + (a.1 - fy).abs();
                                let db = (b.0 - fx).abs() + (b.1 - fy).abs();
                                da.partial_cmp(&db).unwrap()
                            })
                            .unwrap();
                        n.2
                    }
                    _ => {
                        // shepards / inverse / barycentric: inverse distance weighting
                        let power = if method == 19 { 1.0 } else { 2.0 };
                        let mut acc = [0.0; 3];
                        let mut ws = 0.0;
                        let mut exact = None;
                        for p in &pts {
                            let d2 = (p.0 - fx).powi(2) + (p.1 - fy).powi(2);
                            if d2 < 1e-9 {
                                exact = Some(p.2);
                                break;
                            }
                            let w = 1.0 / d2.powf(power / 2.0);
                            ws += w;
                            for c in 0..3 {
                                acc[c] += w * p.2[c];
                            }
                        }
                        exact.unwrap_or([acc[0] / ws, acc[1] / ws, acc[2] / ws])
                    }
                };
                let mut p = i.px(x, y);
                for c in 0..3 {
                    p[c] = to_q16(col[c] * 65535.0);
                }
                i.set_px(x, y, p);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn subimage_match(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|zzdl", args)?;
    let metric = a.get(4).map_or(0, |m| m.long_or(0));
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let small = other.wand.borrow().image().unwrap().clone();
    let (best, score, out) = {
        let w = d.wand.borrow();
        let big = w.image().unwrap();
        let mut best = (0usize, 0usize);
        let mut best_err = f64::MAX;
        let step = if big.columns * big.rows > 200_000 {
            2
        } else {
            1
        };
        if small.columns <= big.columns && small.rows <= big.rows {
            for y in (0..=big.rows - small.rows).step_by(step) {
                for x in (0..=big.columns - small.columns).step_by(step) {
                    let mut err = 0.0;
                    'outer: for sy in 0..small.rows {
                        for sx in 0..small.columns {
                            let p = big.px(x + sx, y + sy);
                            let q = small.px(sx, sy);
                            // the transparent parts of the reference
                            // do not count
                            let wq = if small.alpha {
                                q[3] as f64 / 65535.0
                            } else {
                                1.0
                            };
                            for c in 0..3 {
                                let dd = (p[c] as f64 - q[c] as f64) / 65535.0;
                                err += wq * dd * dd;
                            }
                            if err >= best_err {
                                break 'outer;
                            }
                        }
                    }
                    if err < best_err {
                        best_err = err;
                        best = (x, y);
                    }
                }
            }
        }
        let n = (small.columns * small.rows * 3).max(1) as f64;
        let mut score = (best_err / n).sqrt();
        // other metrics than (R)MSE: the least distortion over the offsets
        if !matches!(metric, 0 | 5 | 10) && small.columns <= big.columns && small.rows <= big.rows {
            let mut least = f64::MAX;
            for y in 0..=big.rows - small.rows {
                for x in 0..=big.columns - small.columns {
                    let r = Rect::new(small.columns, small.rows, x as i64, y as i64);
                    let crop = transform::crop(big, r).0;
                    let v = crate::ops::distortion_ch(&crop, &small, metric, channel::DEFAULT);
                    if v < least {
                        least = v;
                        best = (x, y);
                    }
                }
            }
            score = least;
        } else if metric == 5 {
            score *= score;
        }
        let mut sim = big.clone();
        sim.colors = None;
        (best, score, sim)
    };
    if let Some(Arg::Any(r)) = a.get(1) {
        if let Value::Reference(rc) = r {
            let mut off = PhpArray::new();
            set(&mut off, "x", Value::Long(best.0 as i64));
            set(&mut off, "y", Value::Long(best.1 as i64));
            set(&mut off, "width", Value::Long(small.columns as i64));
            set(&mut off, "height", Value::Long(small.rows as i64));
            *rc.borrow_mut() = arr_val(off);
        }
    }
    if let Some(Arg::Any(r)) = a.get(2) {
        if let Value::Reference(rc) = r {
            *rc.borrow_mut() = Value::Double(score);
        }
    }
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

/// What a build without a delegate library (LQR, X11) or without the
/// image's clip path reports.
fn unsupported(
    vm: &mut Vm,
    f: &Func,
    o: Option<&Obj>,
    args: &[Value],
    spec: &str,
    e: MagickError,
    msg: &str,
) -> R {
    zpp::parse(vm, f, spec, args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    convert_exception(vm, Some(e), crate::Cls::Imagick, msg)
}

fn no_delegate(lib: &str, at: &str) -> MagickError {
    MagickError::new(
        goro_imagick::error::severity::MISSING_DELEGATE_ERROR,
        format!("delegate library support not built-in '' ({lib}) @ {at}"),
    )
}

fn liquid_rescale_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    // ImageMagick names the image's file in this one
    let name = d_(o)
        .wand
        .borrow()
        .image()
        .map(|i| i.filename.clone())
        .unwrap_or_default();
    let e = MagickError::new(
        goro_imagick::error::severity::MISSING_DELEGATE_ERROR,
        format!(
            "delegate library support not built-in '{name}' (LQR) @ error/resize.c/LiquidRescaleImage/2079"
        ),
    );
    unsupported(vm, f, o, args, "lldd", e, "Unable to liquid rescale image")
}

fn display_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let e = no_delegate("X11", "error/display.c/DisplayImages/16261");
    unsupported(vm, f, o, args, "s", e, "Unable to display image")
}

fn animate_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let e = no_delegate("X11", "error/animate.c/AnimateImages/1590");
    unsupported(vm, f, o, args, "s", e, "Unable to animate images")
}

fn clip_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    image_op(vm, &d_(o), "Unable to clip image", |i| {
        goro_imagick::clippath::clip_path(i, "#1", true, true)
    })
}

fn clip_path_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "sb", args)?;
    let name = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    let inside = a[1].bool_or(false);
    image_op(vm, &d_(o), "Unable to clip path image", |i| {
        goro_imagick::clippath::clip_path(i, &name, inside, false)
    })
}

/// RemapImage(): every pixel takes a colour of the reference image,
/// dithered as asked (Undefined dithers as Floyd-Steinberg).
fn remap_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let reference = other.wand.borrow().image().unwrap().clone();
    let method = match a[1].long_or(0) {
        1 => goro_imagick::quantize::Dither::None,
        2 => goro_imagick::quantize::Dither::Riemersma,
        _ => goro_imagick::quantize::Dither::FloydSteinberg,
    };
    image_op(vm, &d, "Unable to remap image", |i| {
        goro_imagick::quantize::remap(i, &reference, method);
        Ok(())
    })
}

fn texture_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let tile = other.wand.borrow().image().unwrap().clone();
    let out = with_image(vm, &d, |i| {
        let mut out = i.clone();
        let (tw, th) = (tile.columns.max(1), tile.rows.max(1));
        let mut y = 0;
        while y < out.rows {
            let mut x = 0;
            while x < out.columns {
                composite_image(
                    &mut out,
                    &tile,
                    CompositeOp::Over,
                    x as i64,
                    y as i64,
                    &ComposeArgs::default(),
                );
                x += tw;
            }
            y += th;
        }
        out.colors = None;
        out
    })?;
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn local_contrast_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let (r, s) = (a[0].double_or(10.0), a[1].double_or(12.5));
    image_op(vm, &d_(o), "Failed to localContrastImage", |i| {
        apply_oxideav(
            i,
            &of::Unsharp::new(
                (r.max(1.0)) as u32,
                (r / 3.0).max(0.5) as f32,
                (s / 100.0) as f32,
                0,
            ),
        )
    })
}

fn color_decision_list_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let xml = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    let triple = |tag: &str, d: f64| -> [f64; 3] {
        let Some(st) = xml.find(&format!("<{}>", tag)) else {
            return [d; 3];
        };
        let rest = &xml[st + tag.len() + 2..];
        let end = rest.find('<').unwrap_or(rest.len());
        let v: Vec<f64> = rest[..end]
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        [
            v.first().copied().unwrap_or(d),
            v.get(1).copied().unwrap_or(d),
            v.get(2).copied().unwrap_or(d),
        ]
    };
    let slope = triple("Slope", 1.0);
    let offset = triple("Offset", 0.0);
    let power = triple("Power", 1.0);
    let sat = triple("Saturation", 1.0)[0];
    image_op(vm, &d_(o), "Unable to colorDecisionListImage", |i| {
        for p in i.pixels.chunks_exact_mut(4) {
            // the saturation pivots on the luma of the original pixel, the
            // slope/offset/power result clamped to [0, 1]
            let luma = Image::intensity(p) / 65535.0;
            let mut v = [0f64; 3];
            for c in 0..3 {
                let x = p[c] as f64 / 65535.0 * slope[c] + offset[c];
                v[c] = x.clamp(0.0, 1.0).powf(power[c]);
            }
            for c in 0..3 {
                p[c] = to_q16((luma + sat * (v[c] - luma)) * 65535.0);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn hough_line_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lld", args)?;
    let t = a[2].double_or(40.0);
    image_op(vm, &d_(o), "Unable to Hough line image", |i| {
        apply_oxideav(i, &of::HoughLines::new(128, t.max(1.0) as u32))
    })
}

fn canny_edge_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dddd", args)?;
    let (r, s, lo, hi) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(0.1),
        a[3].double_or(0.3),
    );
    image_op(vm, &d_(o), "Unable to cannyEdgeImage", |i| {
        goro_imagick::effects::canny(i, r, s, lo, hi);
        Ok(())
    })
}

fn wavelet_denoise_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let (t, soft) = (a[0].double_or(0.0), a[1].double_or(0.0));
    image_op(vm, &d_(o), "Unable to waveletDenoiseImage", |i| {
        goro_imagick::effects::wavelet_denoise(i, t, soft);
        Ok(())
    })
}

fn mean_shift_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lld", args)?;
    let (w, h, dist) = (a[0].long_or(3), a[1].long_or(3), a[2].double_or(0.1));
    image_op(vm, &d_(o), "Unable to meanShiftImage", |i| {
        let r = (w.max(h).max(1) / 2).max(1) as u32;
        apply_oxideav(
            i,
            &of::BilateralBlur::new(r, r as f32, (dist * 255.0).max(1.0) as f32),
        )
    })
}

/// KmeansImage(): seeds from the "kmeans:seed-colors" artifact (a
/// ';'-separated colour list), the rest from colour quantization.
fn kmeans_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lld", args)?;
    let n = a[0].long_or(8).max(1) as usize;
    let iterations = a[1].long_or(300).max(0) as usize;
    let tolerance = a[2].double_or(0.0001);
    let d = d_(o);
    image_op(vm, &d, "Unable to kmeansImage", |i| {
        if iterations == 0 {
            return Err(MagickError::new(
                goro_imagick::error::severity::OPTION_ERROR,
                String::new(),
            ));
        }
        // an image artifact (a wand option does not reach it)
        let spec = i.artifacts.get("kmeans:seed-colors").map(str::to_string);
        let seeds = spec.map(|s| {
            s.split(';')
                .filter_map(|c| PixelInfo::parse(c.trim()))
                .map(|c| {
                    let p = c.to_rgba16();
                    [p[0], p[1], p[2]]
                })
                .collect::<Vec<_>>()
        });
        goro_imagick::quantize::kmeans(i, n, iterations, tolerance, seeds);
        Ok(())
    })
}

fn range_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dddd", args)?;
    let (lb, lw, hw, hb) = (
        a[0].double_or(0.0),
        a[1].double_or(0.0),
        a[2].double_or(0.0),
        a[3].double_or(0.0),
    );
    image_op(vm, &d_(o), "Unable to rangeThresholdImage", |i| {
        // RangeThresholdImage(): every channel from the pixel intensity
        for p in i.pixels.chunks_exact_mut(4) {
            let v = Image::intensity(p);
            let o = if v < lb || v > hb {
                0.0
            } else if v >= lw && v <= hw {
                65535.0
            } else if v < lw {
                (v - lb) / (lw - lb).max(1e-9) * 65535.0
            } else {
                (hb - v) / (hb - hw).max(1e-9) * 65535.0
            };
            let o = clamp_to_quantum(o) as u16;
            p[0] = o;
            p[1] = o;
            p[2] = o;
        }
        i.colors = None;
        Ok(())
    })
}

fn auto_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let method = a[0].long_or(2);
    image_op(vm, &d_(o), "Unable to autoThresholdImage", |i| {
        goro_imagick::enhance::auto_threshold(i, method);
        Ok(())
    })
}

fn bilateral_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dddd", args)?;
    let (r, s, ss) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(1.0),
    );
    image_op(vm, &d_(o), "Unable to bilateralBlurImage", |i| {
        let radius = if r >= 1.0 { r as u32 } else { 3 };
        apply_oxideav(
            i,
            &of::BilateralBlur::new(radius, ss.max(0.1) as f32, (s * 2.55).max(1.0) as f32),
        )
    })
}

fn clahe_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "llld", args)?;
    image_op(vm, &d_(o), "Unable to claheImage", |i| {
        apply_oxideav(i, &of::Equalize::new())
    })
}

fn level_image_colors(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zzb", args)?;
    let black = color_arg(vm, &a[0])?;
    let white = color_arg(vm, &a[1])?;
    let invert = a[2].bool_or(false);
    image_op(vm, &d_(o), "Unable to levelImageColors", |i| {
        let b = black.to_rgba16();
        let w = white.to_rgba16();
        for p in i.pixels.chunks_exact_mut(4) {
            for c in 0..3 {
                let (lo, hi) = (b[c] as f64, w[c] as f64);
                let v = p[c] as f64;
                p[c] = to_q16(if invert {
                    // map 0..range onto lo..hi
                    lo + v / 65535.0 * (hi - lo)
                } else if (hi - lo).abs() < 1e-9 {
                    v
                } else {
                    (v - lo) / (hi - lo) * 65535.0
                });
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn levelize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd", args)?;
    let (b, g, w) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(65535.0),
    );
    image_op(vm, &d_(o), "Unable to levelizeImage", |i| {
        goro_imagick::filters::map_channels(i, channel::DEFAULT, |v| {
            // LevelizeImage() raises to the gamma itself (not its inverse)
            let x = (v / 65535.0).powf(g);
            b + x * (w - b)
        });
        Ok(())
    })
}

fn ordered_dither_of(i: &mut Image, map: &str) -> MResult<()> {
    goro_imagick::dither::ordered_dither(i, map)
}

fn ordered_dither_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let map = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    image_op(vm, &d_(o), "Unable to orderedDitherImage", |i| {
        ordered_dither_of(i, &map)
    })
}

fn ordered_posterize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s|l", args)?;
    let map = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    image_op(vm, &d_(o), "Unable to posterize image", |i| {
        ordered_dither_of(i, &map)
    })
}

fn white_balance_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    image_op(vm, &d_(o), "Unable to whiteBalanceImage", |i| {
        // gray world: scale each channel so their means are equal
        let n = (i.columns * i.rows).max(1) as f64;
        let mut mean = [0f64; 3];
        for p in i.pixels.chunks_exact(4) {
            for c in 0..3 {
                mean[c] += p[c] as f64;
            }
        }
        for m in mean.iter_mut() {
            *m /= n;
        }
        let avg = (mean[0] + mean[1] + mean[2]) / 3.0;
        for p in i.pixels.chunks_exact_mut(4) {
            for c in 0..3 {
                if mean[c] > 0.0 {
                    p[c] = to_q16(p[c] as f64 * avg / mean[c]);
                }
            }
        }
        i.colors = None;
        Ok(())
    })?;
    Ok(Value::True)
}

fn channel_fx_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let expr = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    let d = d_(o);
    let out = with_image(vm, &d, |i| {
        // "red=>blue; green<=>red; ..." channel moves and swaps
        let mut out = i.clone();
        let ch = |s: &str| -> Option<usize> {
            match s.trim().to_ascii_lowercase().as_str() {
                "red" | "r" | "cyan" | "c" | "0" => Some(0),
                "green" | "g" | "magenta" | "m" | "1" => Some(1),
                "blue" | "b" | "yellow" | "y" | "2" => Some(2),
                "alpha" | "a" | "3" => Some(3),
                _ => None,
            }
        };
        for part in expr.split([';', '|']) {
            if let Some((l, r)) = part.split_once("<=>") {
                if let (Some(a), Some(b)) = (ch(l), ch(r)) {
                    for p in out.pixels.chunks_exact_mut(4) {
                        p.swap(a, b);
                    }
                }
            } else if let Some((l, r)) = part.split_once("=>") {
                if let (Some(a), Some(b)) = (ch(l), ch(r)) {
                    for p in out.pixels.chunks_exact_mut(4) {
                        p[b] = p[a];
                    }
                }
            }
        }
        out.colors = None;
        out
    })?;
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn color_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zz", args)?;
    let start = color_arg(vm, &a[0])?;
    let stop = color_arg(vm, &a[1])?;
    image_op(vm, &d_(o), "Unable to colorThresholdImage", |i| {
        let (s, e) = (start.to_rgba16(), stop.to_rgba16());
        for p in i.pixels.chunks_exact_mut(4) {
            let inside = (0..3).all(|c| p[c] >= s[c].min(e[c]) && p[c] <= s[c].max(e[c]));
            let v = if inside { 65535 } else { 0 };
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
        i.colors = None;
        Ok(())
    })
}

fn get_image_kurtosis(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|l", args)?;
    let ch = a[0].long_or(channel::DEFAULT);
    let s = with_image(vm, &d_(o), |i| {
        goro_imagick::statistics::composite_statistics(i, ch)
    })?;
    let mut arr = PhpArray::new();
    set(&mut arr, "kurtosis", Value::Double(s.kurtosis));
    set(&mut arr, "skewness", Value::Double(s.skewness));
    Ok(arr_val(arr))
}

fn polynomial_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a", args)?;
    let terms = doubles(vm, &a[0], "Unable to polynomialImage")?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let r = {
        let mut w = d.wand.borrow_mut();
        w.set_iterator_index(0);
        // sum of weight * image^power over the images, pairs of terms
        let images = w.images.clone();
        let first = &images[0];
        let mut out = first.clone();
        for y in 0..out.rows {
            for x in 0..out.columns {
                let mut acc = [0f64; 3];
                for (k, img) in images.iter().enumerate() {
                    let (wt, pw) = (
                        terms.get(2 * k).copied().unwrap_or(0.0),
                        terms.get(2 * k + 1).copied().unwrap_or(1.0),
                    );
                    let p = img.virtual_px(x as i64, y as i64);
                    for c in 0..3 {
                        acc[c] += wt * (p[c] as f64 / 65535.0).powf(pw);
                    }
                }
                let mut p = out.px(x, y);
                for c in 0..3 {
                    p[c] = to_q16(acc[c] * 65535.0);
                }
                out.set_px(x, y, p);
            }
        }
        out.colors = None;
        out
    };
    let nw = new_wand_from(&d, vec![r]);
    Ok(new_imagick(vm, nw))
}

fn polaroid_of(i: &Image, angle: f64) -> MResult<Image> {
    // PolaroidImage(), black-box: a #DFDFDF border of max(10, max(w,h)/25),
    // the picture curled by a wave across it (on the image turned a
    // quarter: amplitude 1% of its width, wavelength twice its height), an
    // 80% sigma-2 shadow of the flat bordered picture under it, rotated
    let q = (i.columns.max(i.rows) / 25).max(10);
    let border_color = PixelInfo::parse("#DFDFDF").unwrap_or_else(PixelInfo::white);
    let flat = transform::border(i, q, q, &border_color, CompositeOp::Over);
    let mut turned = transform::rotate(&flat, 90.0, &PixelInfo::transparent());
    turned.background_color = PixelInfo::transparent();
    let amp = 0.01 * turned.rows as f64;
    let len = 2.0 * turned.columns as f64;
    let waved = goro_imagick::warp::wave(&turned, amp, len, PixelInterpolateMethod::Undefined);
    let mut pic = transform::rotate(&waved, 270.0, &PixelInfo::transparent());
    pic.page = Rect::new(pic.columns, pic.rows, 0, 0);
    let shadow = shadow_of(&flat, 80.0, 2.0, 0, 0);
    let pad = 4;
    let mut canvas = i.clone_attributes(pic.columns + 2 * pad, pic.rows + 2 * pad);
    canvas.enable_alpha();
    canvas.fill(&PixelInfo::transparent());
    let ca = goro_imagick::composite::ComposeArgs::default();
    goro_imagick::composite::composite_image(&mut canvas, &shadow, CompositeOp::Over, 0, 0, &ca);
    goro_imagick::composite::composite_image(&mut canvas, &pic, CompositeOp::Over, 0, 0, &ca);
    canvas.page = Rect::new(canvas.columns, canvas.rows, 0, 0);
    canvas.background_color = i.background_color.clone();
    if angle == 0.0 {
        return Ok(canvas);
    }
    let mut out = transform::rotate(&canvas, angle, &PixelInfo::transparent());
    out.background_color = i.background_color.clone();
    Ok(out)
}
fn polaroid_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zd", args)?;
    let _ = crate::draw::wand_of(&crate::arg_obj(
        vm,
        f,
        1,
        match &a[0] {
            Arg::Any(v) => v,
            _ => return Err(()),
        },
        crate::C_DRAW,
        "ImagickDraw",
    )?);
    let angle = a[1].double_or(0.0);
    replace_op(vm, &d_(o), "Unable to polaroid image", |i| {
        polaroid_of(i, angle)
    })
}

fn polaroid_with_text_and_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zdsl", args)?;
    let angle = a[1].double_or(0.0);
    replace_op(
        vm,
        &d_(o),
        "Unable to polaroidWithTextAndMethod image",
        |i| polaroid_of(i, angle),
    )
}

/// MorphologyImage() with a kernel list.
fn morphology_of(i: &mut Image, method: i64, iterations: i64, kernels: &[crate::kernel::Kernel]) {
    let erode_dilate = |img: &Image, k: &crate::kernel::Kernel, dilate: bool| -> Image {
        let mut out = img.clone();
        for y in 0..img.rows {
            for x in 0..img.columns {
                let mut best = [if dilate { 0u16 } else { 65535 }; 3];
                for ky in 0..k.height {
                    for kx in 0..k.width {
                        let w = k.values[ky * k.width + kx];
                        if w.is_nan() || w <= 0.0 {
                            continue;
                        }
                        let (sx, sy) = if dilate {
                            (
                                x as i64 - (kx as i64 - k.x as i64),
                                y as i64 - (ky as i64 - k.y as i64),
                            )
                        } else {
                            (
                                x as i64 + kx as i64 - k.x as i64,
                                y as i64 + ky as i64 - k.y as i64,
                            )
                        };
                        let p = img.virtual_px(sx, sy);
                        for c in 0..3 {
                            best[c] = if dilate {
                                best[c].max(p[c])
                            } else {
                                best[c].min(p[c])
                            };
                        }
                    }
                }
                let mut p = img.px(x, y);
                p[..3].copy_from_slice(&best);
                out.set_px(x, y, p);
            }
        }
        out
    };
    let convolve = |img: &Image, k: &crate::kernel::Kernel| -> Image {
        let mut out = img.clone();
        for y in 0..img.rows {
            for x in 0..img.columns {
                let mut acc = [0f64; 3];
                for ky in 0..k.height {
                    for kx in 0..k.width {
                        let w = k.values[ky * k.width + kx];
                        if w.is_nan() || w == 0.0 {
                            continue;
                        }
                        let p = img.virtual_px(
                            x as i64 + kx as i64 - k.x as i64,
                            y as i64 + ky as i64 - k.y as i64,
                        );
                        for c in 0..3 {
                            acc[c] += w * p[c] as f64;
                        }
                    }
                }
                let mut p = img.px(x, y);
                for c in 0..3 {
                    p[c] = to_q16(acc[c]);
                }
                out.set_px(x, y, p);
            }
        }
        out
    };
    let n = if iterations < 0 { 1 } else { iterations.max(1) } as usize;
    for k in kernels {
        for _ in 0..n {
            let src = i.clone();
            let r = match method {
                1 | 2 => convolve(&src, k),
                3 | 5 => erode_dilate(&src, k, false),
                4 | 6 => erode_dilate(&src, k, true),
                8 | 10 => erode_dilate(&erode_dilate(&src, k, false), k, true),
                9 | 11 => erode_dilate(&erode_dilate(&src, k, true), k, false),
                12 => {
                    let o = erode_dilate(&erode_dilate(&src, k, false), k, true);
                    erode_dilate(&erode_dilate(&o, k, true), k, false)
                }
                13..=17 => {
                    let e = erode_dilate(&src, k, false);
                    let dl = erode_dilate(&src, k, true);
                    let mut out = src.clone();
                    for (idx, p) in out.pixels.chunks_exact_mut(4).enumerate() {
                        for c in 0..3 {
                            let s = src.pixels[idx * 4 + c] as i32;
                            let ev = e.pixels[idx * 4 + c] as i32;
                            let dv = dl.pixels[idx * 4 + c] as i32;
                            let v = match method {
                                13 => s - ev,
                                14 => dv - s,
                                15 => dv - ev,
                                16 => s - ev.max(0),
                                _ => dv - s,
                            };
                            p[c] = v.clamp(0, 65535) as u16;
                        }
                    }
                    out
                }
                _ => convolve(&src, k),
            };
            *i = r;
        }
    }
    i.colors = None;
}

fn morphology(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llz|l", args)?;
    let method = a[0].long_or(1);
    let iterations = a[1].long_or(1);
    let kobj = match &a[2] {
        Arg::Any(v) => crate::arg_obj(vm, f, 3, v, crate::C_KERNEL, "ImagickKernel")?,
        _ => return Err(()),
    };
    let ks = crate::kernel::kernels_of(vm, &kobj)?;
    image_op(vm, &d_(o), "Unable to morphology image", |i| {
        morphology_of(i, method, iterations, &ks);
        Ok(())
    })
}

fn fx_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s|l", args)?;
    let expr = String::from_utf8_lossy(a[0].str_or(b"")).into_owned();
    let d = d_(o);
    not_empty(vm, &d)?;
    let r = {
        let w = d.wand.borrow();
        goro_imagick::fx::fx(&w.images, w.current, &expr)
    };
    match r {
        Ok(img) => {
            let nw = new_wand_from(&d, vec![img]);
            Ok(new_imagick(vm, nw))
        }
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, "Fx image failed"),
    }
}

fn affine_transform_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let dw = match &a[0] {
        Arg::Any(v) => {
            crate::draw::wand_of(&crate::arg_obj(vm, f, 1, v, crate::C_DRAW, "ImagickDraw")?)
        }
        _ => return Err(()),
    };
    let m = dw.gc().affine;
    replace_op(vm, &d_(o), "Unable to affine transform image", |i| {
        goro_imagick::distort::distort(i, 2, &[m.sx, m.rx, m.ry, m.sy, m.tx, m.ty], true)
    })
}

fn stereo_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let right = other.wand.borrow().image().unwrap().clone();
    let out = with_image(vm, &d, |left| {
        // red from the left image, green and blue from the right
        let mut out = left.clone();
        for y in 0..out.rows.min(right.rows) {
            for x in 0..out.columns.min(right.columns) {
                let mut p = left.px(x, y);
                let q = right.px(x, y);
                p[1] = q[1];
                p[2] = q[2];
                out.set_px(x, y, p);
            }
        }
        out.colors = None;
        out
    })?;
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn stegano_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let wm = other.wand.borrow().image().unwrap().clone();
    let out = with_image(vm, &d, |i| {
        // hide the watermark's intensity in the low bit of the pixels
        let mut out = i.clone();
        for y in 0..out.rows {
            for x in 0..out.columns {
                let bit = if x < wm.columns && y < wm.rows {
                    (Image::intensity(&wm.px(x, y)) > 32767.0) as u16
                } else {
                    0
                };
                let mut p = out.px(x, y);
                p[2] = (p[2] & !1) | bit;
                out.set_px(x, y, p);
            }
        }
        out.colors = None;
        out
    })?;
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn smush_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "bl", args)?;
    let stack = a[0].bool_or(false);
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = {
        let w = d.wand.borrow();
        let bg = w.images[0].background_color.clone();
        let refs: Vec<&Image> = w.images.iter().collect();
        transform::append(&refs, stack, &bg, w.settings.gravity)
    };
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn combine_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "l", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = {
        let w = d.wand.borrow();
        let first = &w.images[0];
        let mut out = first.clone();
        for y in 0..out.rows {
            for x in 0..out.columns {
                let mut p = [0u16, 0, 0, 65535];
                for (c, img) in w.images.iter().take(4).enumerate() {
                    let q = img.virtual_px(x as i64, y as i64);
                    p[c] = to_q16(Image::intensity(&q));
                }
                out.set_px(x, y, p);
            }
        }
        out.alpha = w.images.len() > 3;
        out.colorspace = Colorspace::SRGB;
        out.colors = None;
        out
    };
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

/// ComplexImages(): (real, imaginary) image pairs; a binary operator
/// takes the second pair (or the first again when there is none).
fn complex_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let op = a[0].long_or(0);
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = {
        let w = d.wand.borrow();
        if w.images.len() < 2 {
            drop(w);
            let e = MagickError::with_desc(
                goro_imagick::error::severity::IMAGE_ERROR,
                "ImageSequenceRequired",
                "",
                "error/fourier.c/ComplexImages",
            );
            return convert_exception(vm, Some(e), crate::Cls::Imagick, "Unable to complex image");
        }
        let (ar, ai) = (&w.images[0], &w.images[1]);
        let (br, bi) = if w.images.len() >= 4 {
            (&w.images[2], &w.images[3])
        } else {
            (ar, ai)
        };
        let mut re = ar.clone();
        let mut im = ar.clone();
        let q = |img: &Image, x: usize, y: usize, c: usize| {
            img.virtual_px(x as i64, y as i64)[c] as f64 / 65535.0
        };
        let snr = 1e-12;
        for y in 0..ar.rows {
            for x in 0..ar.columns {
                let mut pr = re.px(x, y);
                let mut pi = pr;
                for c in 0..3 {
                    let (xr, xi, yr, yi) = (
                        q(ar, x, y, c),
                        q(ai, x, y, c),
                        q(br, x, y, c),
                        q(bi, x, y, c),
                    );
                    let (r, i) = match op {
                        1 => (xr + yr, xi + yi),
                        2 => (xr, -xi),
                        3 => {
                            let g = yr * yr + yi * yi + snr;
                            ((xr * yr + xi * yi) / g, (xi * yr - xr * yi) / g)
                        }
                        4 => (
                            (xr * xr + xi * xi).sqrt(),
                            xi.atan2(xr) / (2.0 * std::f64::consts::PI) + 0.5,
                        ),
                        5 => (xr * yr - xi * yi, xr * yi + xi * yr),
                        6 => {
                            let t = 2.0 * std::f64::consts::PI * (xi - 0.5);
                            (xr * t.cos(), xr * t.sin())
                        }
                        7 => (xr - yr, xi - yi),
                        _ => (xr, xi),
                    };
                    pr[c] = to_q16(r * 65535.0);
                    pi[c] = to_q16(i * 65535.0);
                }
                re.set_px(x, y, pr);
                im.set_px(x, y, pi);
            }
        }
        re.colors = None;
        im.colors = None;
        vec![re, im]
    };
    // both wands are left at their first image
    d.wand.borrow_mut().set_first_iterator();
    let mut nw = new_wand_from(&d, out);
    nw.set_first_iterator();
    Ok(new_imagick(vm, nw))
}

fn morph_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let n = a[0].long_or(1).max(0) as usize;
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = {
        let w = d.wand.borrow();
        let mut out = Vec::new();
        for pair in w.images.windows(2) {
            out.push(pair[0].clone());
            for k in 1..=n {
                let t = k as f64 / (n + 1) as f64;
                let mut m = pair[0].clone();
                for y in 0..m.rows {
                    for x in 0..m.columns {
                        let p = pair[0].px(x, y);
                        let q = pair[1].virtual_px(x as i64, y as i64);
                        let mut r = [0u16; 4];
                        for c in 0..4 {
                            r[c] = to_q16(p[c] as f64 * (1.0 - t) + q[c] as f64 * t);
                        }
                        m.set_px(x, y, r);
                    }
                }
                m.colors = None;
                out.push(m);
            }
        }
        out.push(w.images.last().unwrap().clone());
        out
    };
    let nw = new_wand_from(&d, out);
    Ok(new_imagick(vm, nw))
}

fn preview_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "l", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let img = d.wand.borrow().image().unwrap().clone();
    let nw = new_wand_from(&d, vec![img]);
    Ok(new_imagick(vm, nw))
}

fn get_image_distortions(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    let _ = a;
    let d = d_(o);
    not_empty(vm, &d)?;
    let _ = f;
    throw_cls(
        vm,
        Cls::Imagick,
        "Imagick::getImageDistortions() is not implemented yet",
    )
}

fn get_image_channel_distortions(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl|l", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let b = other.wand.borrow().image().unwrap().clone();
    let m = a[1].long_or(0);
    let v = with_image(vm, &d, |i| {
        let (w, h) = (i.columns.min(b.columns), i.rows.min(b.rows));
        let mut sq = 0.0;
        for y in 0..h {
            for x in 0..w {
                let (p, q) = (i.px(x, y), b.px(x, y));
                for c in 0..3 {
                    let dd = (p[c] as f64 - q[c] as f64) / 65535.0;
                    sq += dd * dd;
                }
            }
        }
        let mse = sq / ((w * h * 3).max(1) as f64);
        match m {
            8 => {
                if mse == 0.0 {
                    f64::INFINITY
                } else {
                    -10.0 * mse.log10()
                }
            }
            10 | 2 => mse.sqrt(),
            _ => mse,
        }
    })?;
    Ok(Value::Double(v))
}

fn get_image_colormap_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let idx = a[0].long_or(0);
    let d = d_(o);
    let c = with_image(vm, &d, |i| {
        // the palette in order of first appearance
        let mut seen: Vec<[u16; 4]> = Vec::new();
        for p in i.pixels.chunks_exact(4) {
            let q = [p[0], p[1], p[2], p[3]];
            if !seen.contains(&q) {
                seen.push(q);
                if seen.len() > 256 {
                    break;
                }
            }
        }
        seen.get(idx.max(0) as usize).copied()
    })?;
    match c {
        Some(q) if idx >= 0 => Ok(crate::new_pixel(
            vm,
            PixelInfo::from_rgba_q(q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64),
        )),
        _ => convert_exception(vm, None, Cls::Imagick, "Unable to get image colormap color"),
    }
}

fn set_image_colormap_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lz", args)?;
    let idx = a[0].long_or(0);
    let c = color_arg(vm, &a[1])?;
    image_op(vm, &d_(o), "Unable to set image color map color", |i| {
        let mut seen: Vec<[u16; 4]> = Vec::new();
        for p in i.pixels.chunks_exact(4) {
            let q = [p[0], p[1], p[2], p[3]];
            if !seen.contains(&q) {
                seen.push(q);
            }
        }
        let Some(old) = seen.get(idx.max(0) as usize).copied() else {
            return Err(MagickError::new(0, ""));
        };
        let new = c.to_rgba16();
        for p in i.pixels.chunks_exact_mut(4) {
            if p == old {
                p.copy_from_slice(&new);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn montage_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zssls", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let tile = String::from_utf8_lossy(a[1].str_or(b"")).into_owned();
    let geom = String::from_utf8_lossy(a[2].str_or(b"")).into_owned();
    let out = {
        let w = d.wand.borrow();
        let n = w.images.len();
        let (cols, _rows) = match goro_imagick::Geometry::parse(&tile) {
            Some(g) => (
                g.width.unwrap_or(n as f64).max(1.0) as usize,
                g.height.unwrap_or(1.0) as usize,
            ),
            None => (n.clamp(1, 4), n.div_ceil(4)),
        };
        let (tw, th) = match goro_imagick::Geometry::parse(&geom) {
            Some(g) if g.width.is_some() => (
                g.width.unwrap_or(120.0) as usize,
                g.height.or(g.width).unwrap_or(120.0) as usize,
            ),
            _ => (
                w.images.iter().map(|i| i.columns).max().unwrap_or(1),
                w.images.iter().map(|i| i.rows).max().unwrap_or(1),
            ),
        };
        let rows = n.div_ceil(cols);
        let mut canvas = Image::new(cols * tw, rows * th, &PixelInfo::white());
        for (k, img) in w.images.iter().enumerate() {
            let (cx, cy) = ((k % cols) * tw, (k / cols) * th);
            let g = goro_imagick::Geometry {
                width: Some(tw as f64),
                height: Some(th as f64),
                ..Default::default()
            };
            let (nw, nh) = g.meta_size(img.columns, img.rows);
            let t = transform::resize(img, nw, nh, FilterType::Undefined)
                .unwrap_or_else(|_| img.clone());
            composite_image(
                &mut canvas,
                &t,
                CompositeOp::Over,
                (cx + (tw - nw.min(tw)) / 2) as i64,
                (cy + (th - nh.min(th)) / 2) as i64,
                &ComposeArgs::default(),
            );
        }
        canvas.format = w.images[0].format.clone();
        canvas
    };
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn encipher_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let pass = a[0].str_or(b"").to_vec();
    image_op(vm, &d_(o), "Unable to encipher image", |i| {
        goro_imagick::cipher::encipher(i, &pass)
    })
}

fn decipher_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "s", args)?;
    let pass = a[0].str_or(b"").to_vec();
    image_op(vm, &d_(o), "Unable to decipher image", |i| {
        goro_imagick::cipher::decipher(i, &pass)
    })
}

/// MagickForwardFourierTransformImage(): the wand's images become the
/// (magnitude, phase) or (real, imaginary) pair of the current image.
fn forward_fourier_transform_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "b", args)?;
    let magnitude = a[0].bool_or(true);
    let d = d_(o);
    not_empty(vm, &d)?;
    let r = {
        let w = d.wand.borrow();
        goro_imagick::fourier::forward(w.image().unwrap(), magnitude)
    };
    match r {
        Ok(pair) => {
            let mut w = d.wand.borrow_mut();
            w.clear();
            w.insert_images(pair);
            Ok(Value::True)
        }
        Err(e) => convert_exception(
            vm,
            Some(e),
            Cls::Imagick,
            "Unable to forward fourier transform image",
        ),
    }
}

/// MagickInverseFourierTransformImage(): this wand's current image and
/// the other wand's make the spatial image, which replaces this wand's.
fn inverse_fourier_transform_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zb", args)?;
    let magnitude = a[1].bool_or(true);
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let second = other.wand.borrow().image().unwrap().clone();
    let r = {
        let w = d.wand.borrow();
        goro_imagick::fourier::inverse(w.image().unwrap(), &second, magnitude)
    };
    match r {
        Ok(img) => {
            let mut w = d.wand.borrow_mut();
            w.clear();
            w.insert_images(vec![img]);
            Ok(Value::True)
        }
        Err(e) => convert_exception(
            vm,
            Some(e),
            Cls::Imagick,
            "Unable to inverse fourier transform image",
        ),
    }
}
