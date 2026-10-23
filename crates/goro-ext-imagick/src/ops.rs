//! Imagick image operations: geometry, compositing, layers, colour and
//! tone operations, and the filters (most of them on oxideav-image-filter).

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_imagick::color::{PixelInfo, clamp_to_quantum, to_q16};
use goro_imagick::composite::{ComposeArgs, composite_image};
use goro_imagick::filters::{apply_oxideav, channel_mask};
use goro_imagick::geometry::Rect;
use goro_imagick::image::Image;
use goro_imagick::oxideav_image_filter as of;
use goro_imagick::statistics::{ChannelStats, channel_statistics, composite_statistics};
use goro_imagick::transform;
use goro_imagick::types::*;
use goro_imagick::{MResult, MagickError, Wand};

use crate::imagick::{image_op, not_empty, other_imagick, replace_op, this, with_image};
use crate::zpp::{self, Arg, Func, Obj};
use crate::{
    Cls, Impl, R, arr_val, convert_exception, new_imagick, set, throw_cls, zval_to_opacity,
    zval_to_pixel,
};

pub(crate) const OPS: &[(&str, Impl, bool)] = &[
    ("resizeImage", resize_image, false),
    ("scaleImage", scale_image, false),
    ("sampleImage", sample_image, false),
    ("thumbnailImage", thumbnail_image, false),
    ("adaptiveResizeImage", adaptive_resize_image, false),
    (
        "interpolativeResizeImage",
        interpolative_resize_image,
        false,
    ),
    ("resampleImage", resample_image, false),
    ("magnifyImage", magnify_image, false),
    ("minifyImage", minify_image, false),
    ("cropImage", crop_image, false),
    ("cropThumbnailImage", crop_thumbnail_image, false),
    ("extentImage", extent_image, false),
    ("setImageExtent", set_image_extent, false),
    ("borderImage", border_image, false),
    (
        "borderImageWithComposite",
        border_image_with_composite,
        false,
    ),
    ("frameImage", frame_image, false),
    ("frameImageWithComposite", frame_image_with_composite, false),
    ("rotateImage", rotate_image, false),
    ("flipImage", flip_image, false),
    ("flopImage", flop_image, false),
    ("transposeImage", transpose_image, false),
    ("transverseImage", transverse_image, false),
    ("trimImage", trim_image, false),
    ("rollImage", roll_image, false),
    ("shaveImage", shave_image, false),
    ("chopImage", chop_image, false),
    ("spliceImage", splice_image, false),
    ("autoOrient", auto_orient, false),
    ("autoOrientate", auto_orient, false),
    ("compositeImage", composite_image_m, false),
    ("compositeImageGravity", composite_image_gravity, false),
    ("flattenImages", flatten_images, false),
    ("mergeImageLayers", merge_image_layers, false),
    ("mosaicImages", mosaic_images, false),
    ("appendImages", append_images, false),
    ("coalesceImages", coalesce_images, false),
    ("getImageRegion", get_image_region, false),
    ("negateImage", negate_image, false),
    (
        "transformImageColorspace",
        transform_image_colorspace,
        false,
    ),
    ("separateImageChannel", separate_image_channel, false),
    ("evaluateImage", evaluate_image, false),
    ("evaluateImages", evaluate_images, false),
    ("averageImages", average_images, false),
    ("functionImage", function_image, false),
    ("levelImage", level_image, false),
    ("gammaImage", gamma_image, false),
    ("thresholdImage", threshold_image, false),
    ("blackThresholdImage", black_threshold_image, false),
    ("whiteThresholdImage", white_threshold_image, false),
    ("solarizeImage", solarize_image, false),
    ("posterizeImage", posterize_image, false),
    ("quantizeImage", quantize_image, false),
    ("quantizeImages", quantize_images, false),
    ("colorizeImage", colorize_image, false),
    ("tintImage", tint_image, false),
    ("modulateImage", modulate_image, false),
    ("normalizeImage", normalize_image, false),
    ("equalizeImage", equalize_image, false),
    ("autoLevelImage", auto_level_image, false),
    ("autoGammaImage", auto_gamma_image, false),
    ("contrastStretchImage", contrast_stretch_image, false),
    ("linearStretchImage", linear_stretch_image, false),
    ("sigmoidalContrastImage", sigmoidal_contrast_image, false),
    ("brightnessContrastImage", brightness_contrast_image, false),
    ("contrastImage", contrast_image, false),
    ("sepiaToneImage", sepia_tone_image, false),
    ("blurImage", blur_image, false),
    ("gaussianBlurImage", gaussian_blur_image, false),
    ("adaptiveBlurImage", adaptive_blur_image, false),
    ("motionBlurImage", motion_blur_image, false),
    ("sharpenImage", sharpen_image, false),
    ("adaptiveSharpenImage", adaptive_sharpen_image, false),
    ("unsharpMaskImage", unsharp_mask_image, false),
    ("edgeImage", edge_image, false),
    ("embossImage", emboss_image, false),
    ("charcoalImage", charcoal_image, false),
    ("oilPaintImage", oil_paint_image, false),
    ("oilPaintImageWithSigma", oil_paint_image_with_sigma, false),
    ("swirlImage", swirl_image, false),
    ("swirlImageWithMethod", swirl_image_with_method, false),
    ("waveImage", wave_image, false),
    ("waveImageWithMethod", wave_image_with_method, false),
    ("implodeImage", implode_image, false),
    ("implodeImageWithMethod", implode_image_with_method, false),
    ("spreadImage", spread_image, false),
    ("spreadImageWithMethod", spread_image_with_method, false),
    ("despeckleImage", despeckle_image, false),
    ("enhanceImage", enhance_image, false),
    ("medianFilterImage", median_filter_image, false),
    ("reduceNoiseImage", median_filter_image, false),
    ("statisticImage", statistic_image, false),
    ("vignetteImage", vignette_image, false),
    ("shadeImage", shade_image, false),
    ("blueShiftImage", blue_shift_image, false),
    ("clampImage", clamp_image, false),
    ("addNoiseImage", add_noise_image, false),
    (
        "addNoiseImageWithAttenuate",
        add_noise_image_with_attenuate,
        false,
    ),
    ("raiseImage", raise_image, false),
    ("randomThresholdImage", random_threshold_image, false),
    ("adaptiveThresholdImage", adaptive_threshold_image, false),
    ("transparentPaintImage", transparent_paint_image, false),
    ("opaquePaintImage", opaque_paint_image, false),
    ("clutImage", clut_image, false),
    (
        "clutImageWithInterpolate",
        clut_image_with_interpolate,
        false,
    ),
    ("uniqueImageColors", unique_image_colors, false),
    ("getImageChannelMean", get_image_channel_mean, false),
    ("getImageMean", get_image_mean, false),
    ("getImageChannelRange", get_image_channel_range, false),
    ("getImageRange", get_image_range, false),
    (
        "getImageChannelStatistics",
        get_image_channel_statistics,
        false,
    ),
    ("compareImages", compare_images, false),
    ("compareImageChannels", compare_image_channels, false),
    ("getImageDistortion", get_image_distortion, false),
    (
        "getImageChannelDistortion",
        get_image_channel_distortion,
        false,
    ),
    ("setImageMask", set_image_mask, false),
    ("getImageMask", get_image_mask, false),
    ("colorMatrixImage", color_matrix_image, false),
    ("convolveImage", convolve_image, false),
    ("sketchImage", sketch_image, false),
    ("cycleColormapImage", cycle_colormap_image, false),
    ("drawImage", draw_image, false),
    ("annotateImage", annotate_image, false),
    ("queryFontMetrics", query_font_metrics, false),
];

// ---------------------------------------------------------------------------
// helpers

fn d_(o: Option<&Obj>) -> std::rc::Rc<crate::ImagickData> {
    this(o)
}

/// php_imagick_thumbnail_dimensions()
fn thumbnail_dimensions(
    ow: i64,
    oh: i64,
    bestfit: bool,
    dw: i64,
    dh: i64,
    legacy: bool,
) -> Option<(i64, i64)> {
    let round = |v: f64| -> i64 {
        if v >= 0.0 {
            if v < 1.0 { 1 } else { (v + 0.5).floor() as i64 }
        } else {
            (v - 0.5).ceil() as i64
        }
    };
    if ow == dw && oh == dh {
        return Some((dw, dh));
    }
    if bestfit {
        if dw <= 0 || dh <= 0 {
            return None;
        }
        let rx = dw as f64 / ow as f64;
        let ry = dh as f64 / oh as f64;
        let (mut nw, mut nh);
        if rx == ry {
            nw = dw;
            nh = dh;
        } else if rx < ry {
            nw = dw;
            nh = if legacy {
                (rx * oh as f64) as i64
            } else {
                round(rx * oh as f64)
            };
        } else {
            nh = dh;
            nw = if legacy {
                (ry * ow as f64) as i64
            } else {
                round(ry * ow as f64)
            };
        }
        nw = nw.max(1);
        nh = nh.max(1);
        Some((nw, nh))
    } else {
        if dw <= 0 && dh <= 0 {
            return None;
        }
        if dw <= 0 || dh <= 0 {
            if dw <= 0 {
                let ratio = oh as f64 / dh as f64;
                let nw = if legacy {
                    (ow as f64 / ratio) as i64
                } else {
                    round(ow as f64 / ratio)
                };
                Some((nw, dh))
            } else {
                let ratio = ow as f64 / dw as f64;
                let nh = if legacy {
                    (oh as f64 / ratio) as i64
                } else {
                    round(oh as f64 / ratio)
                };
                Some((dw, nh))
            }
        } else {
            Some((dw, dh))
        }
    }
}

fn dims(vm: &mut Vm, d: &crate::ImagickData) -> Result<(i64, i64), ()> {
    with_image(vm, d, |i| (i.columns as i64, i.rows as i64))
}

fn zero_size() -> MagickError {
    MagickError::with_desc(
        goro_imagick::error::severity::OPTION_ERROR,
        "NegativeOrZeroImageSize",
        "",
        "error/resize.c/ResizeImage",
    )
}

fn color_arg(vm: &mut Vm, a: &Arg) -> Result<PixelInfo, ()> {
    match a {
        Arg::Any(v) => zval_to_pixel(vm, v, Cls::Imagick),
        _ => Err(()),
    }
}

/// All images of the wand through `f`.
fn all_images_op(
    vm: &mut Vm,
    d: &crate::ImagickData,
    msg: &str,
    mut f: impl FnMut(&mut Image) -> MResult<()>,
) -> R {
    not_empty(vm, d)?;
    let r = {
        let mut w = d.wand.borrow_mut();
        let mut r = Ok(());
        for img in w.images.iter_mut() {
            if let Err(e) = f(img) {
                r = Err(e);
                break;
            }
        }
        r
    };
    match r {
        Ok(()) => Ok(Value::True),
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, msg),
    }
}

/// A new Imagick object holding images derived from this one's.
fn new_wand_from(d: &crate::ImagickData, images: Vec<Image>) -> Wand {
    let src = d.wand.borrow();
    let mut w = Wand::new();
    w.settings = src.settings.clone();
    w.images = images;
    w.current = w.images.len().saturating_sub(1);
    w
}

// ---------------------------------------------------------------------------
// resizing

fn resize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llld|bb", args)?;
    let d = d_(o);
    let (ow, oh) = dims(vm, &d)?;
    let Some((nw, nh)) = thumbnail_dimensions(
        ow,
        oh,
        a[4].bool_or(false),
        a[0].long_or(0),
        a[1].long_or(0),
        a[5].bool_or(false),
    ) else {
        return throw_cls(vm, Cls::Imagick, "Invalid image geometry");
    };
    let filter = FilterType(a[2].long_or(0) as i32);
    replace_op(vm, &d, "Unable to resize image", |i| {
        if nw <= 0 || nh <= 0 {
            return Err(zero_size());
        }
        transform::resize(i, nw as usize, nh as usize, filter)
    })
}

fn scale_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll|bb", args)?;
    let d = d_(o);
    let (ow, oh) = dims(vm, &d)?;
    let Some((nw, nh)) = thumbnail_dimensions(
        ow,
        oh,
        a[2].bool_or(false),
        a[0].long_or(0),
        a[1].long_or(0),
        a[3].bool_or(false),
    ) else {
        return throw_cls(vm, Cls::Imagick, "Invalid image geometry");
    };
    replace_op(vm, &d, "Unable to scale image", |i| {
        if nw <= 0 || nh <= 0 {
            return Err(zero_size());
        }
        Ok(transform::scale(i, nw as usize, nh as usize))
    })
}

fn sample_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let (w, h) = (a[0].long_or(0), a[1].long_or(0));
    replace_op(vm, &d_(o), "Unable to sample image", |i| {
        if w <= 0 || h <= 0 {
            return Err(zero_size());
        }
        Ok(transform::sample(i, w as usize, h as usize))
    })
}

/// MagickThumbnailImage(): resize, then strip the profiles and comments.
fn thumbnail_of(i: &Image, w: i64, h: i64) -> MResult<Image> {
    if w <= 0 || h <= 0 {
        return Err(zero_size());
    }
    let mut n = transform::resize(i, w as usize, h as usize, FilterType::Undefined)?;
    n.strip();
    Ok(n)
}

fn thumbnail_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l!l!|bbb", args)?;
    let (w, h) = (a[0].long_or(0), a[1].long_or(0));
    let (bestfit, fill, legacy) = (
        a[2].bool_or(false),
        a[3].bool_or(false),
        a[4].bool_or(false),
    );
    let d = d_(o);
    let (ow, oh) = dims(vm, &d)?;
    if bestfit && fill {
        let Some((nw, nh)) = thumbnail_dimensions(ow, oh, true, w, h, legacy) else {
            return convert_exception(vm, None, Cls::Imagick, "Unable to resize and fill image");
        };
        return replace_op(vm, &d, "Unable to resize and fill image", |i| {
            let mut t = thumbnail_of(i, nw, nh)?;
            let ex = if w > nw { (w - nw) / 2 } else { 0 };
            let ey = if h > nh { (h - nh) / 2 } else { 0 };
            t = transform::extent(&t, Rect::new(w as usize, h as usize, -ex, -ey));
            Ok(t)
        });
    }
    let Some((nw, nh)) = thumbnail_dimensions(ow, oh, bestfit, w, h, legacy) else {
        return throw_cls(vm, Cls::Imagick, "Invalid image geometry");
    };
    replace_op(vm, &d, "Unable to thumbnail image", |i| {
        thumbnail_of(i, nw, nh)
    })
}

fn adaptive_resize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll|bb", args)?;
    let d = d_(o);
    let (ow, oh) = dims(vm, &d)?;
    let Some((nw, nh)) = thumbnail_dimensions(
        ow,
        oh,
        a[2].bool_or(false),
        a[0].long_or(0),
        a[1].long_or(0),
        a[3].bool_or(false),
    ) else {
        return throw_cls(vm, Cls::Imagick, "Invalid image geometry");
    };
    replace_op(vm, &d, "Unable to adaptive resize image", |i| {
        if nw <= 0 || nh <= 0 {
            return Err(zero_size());
        }
        transform::resize(i, nw as usize, nh as usize, FilterType::Mitchell)
    })
}

fn interpolative_resize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lll", args)?;
    let (w, h) = (a[0].long_or(0), a[1].long_or(0));
    let filter = match a[2].long_or(0) {
        10 | 8 => FilterType::Point,
        7 => FilterType::Catrom,
        11 => FilterType::Spline,
        _ => FilterType::Triangle,
    };
    replace_op(vm, &d_(o), "Unable to interpolativeResizeImage", |i| {
        if w <= 0 || h <= 0 {
            return Err(zero_size());
        }
        transform::resize(i, w as usize, h as usize, filter)
    })
}

fn resample_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddld", args)?;
    let (xr, yr) = (a[0].double_or(72.0), a[1].double_or(72.0));
    let filter = FilterType(a[2].long_or(0) as i32);
    replace_op(vm, &d_(o), "Unable to resample image", |i| {
        let (ox, oy) = (
            if i.x_resolution > 0.0 {
                i.x_resolution
            } else {
                72.0
            },
            if i.y_resolution > 0.0 {
                i.y_resolution
            } else {
                72.0
            },
        );
        let w = ((i.columns as f64 * xr / ox) + 0.5).floor().max(1.0) as usize;
        let h = ((i.rows as f64 * yr / oy) + 0.5).floor().max(1.0) as usize;
        let mut n = transform::resize(i, w, h, filter)?;
        n.x_resolution = xr;
        n.y_resolution = yr;
        Ok(n)
    })
}

fn magnify_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to magnify image", |i| {
        transform::resize(i, i.columns * 2, i.rows * 2, FilterType::Spline)
    })
}

fn minify_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to minify image", |i| {
        transform::resize(
            i,
            (i.columns / 2).max(1),
            (i.rows / 2).max(1),
            FilterType::Box,
        )
    })
}

// ---------------------------------------------------------------------------
// cropping, borders

fn crop_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    let d = d_(o);
    let warn = std::cell::Cell::new(None);
    let res = replace_op(vm, &d, "Unable to crop image", |i| {
        let (n, w) = transform::crop(i, r);
        warn.set(w);
        Ok(n)
    });
    if let Some(w) = warn.take() {
        d.wand.borrow_mut().throw(w);
    }
    res
}

fn crop_thumbnail_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll|b", args)?;
    let (dw, dh, legacy) = (a[0].long_or(0), a[1].long_or(0), a[2].bool_or(false));
    replace_op(vm, &d_(o), "Unable to crop-thumbnail image", |i| {
        let (ow, oh) = (i.columns as i64, i.rows as i64);
        if ow == dw && oh == dh {
            let mut n = i.clone();
            n.strip();
            return Ok(n);
        }
        let (nw, nh, ox, oy) = crate::imagick::calc_crop(ow, oh, dw, dh, legacy);
        let mut t = thumbnail_of(i, nw, nh)?;
        if nw == dw && nh == dh {
            return Ok(t);
        }
        t.page = Rect::new(t.columns, t.rows, 0, 0);
        let (mut c, _) = transform::crop(&t, Rect::new(dw as usize, dh as usize, ox, oy));
        c.page = Rect::new(dw as usize, dh as usize, 0, 0);
        Ok(c)
    })
}

fn extent_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    replace_op(vm, &d_(o), "Unable to extent image", |i| {
        if r.width == 0 || r.height == 0 {
            return Err(zero_size());
        }
        Ok(transform::extent(i, r))
    })
}

fn set_image_extent(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let (w, h) = (a[0].long_or(0), a[1].long_or(0));
    replace_op(vm, &d_(o), "Unable to set image extent", |i| {
        if w <= 0 || h <= 0 {
            return Err(zero_size());
        }
        let mut n = i.crop_raw(Rect::new(w as usize, h as usize, 0, 0));
        // SetImageExtent(): new pixels are undefined; keep the overlap
        for y in 0..n.rows {
            for x in 0..n.columns {
                if x >= i.columns || y >= i.rows {
                    n.set_px(x, y, [0, 0, 0, 65535]);
                }
            }
        }
        n.page = Rect::new(w as usize, h as usize, 0, 0);
        Ok(n)
    })
}

fn border_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zll", args)?;
    let c = color_arg(vm, &a[0])?;
    let (w, h) = (
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0).max(0) as usize,
    );
    replace_op(vm, &d_(o), "Unable to border image", |i| {
        let mut n = transform::border(i, w, h, &c, i.compose);
        n.border_color = c.clone();
        Ok(n)
    })
}

fn border_image_with_composite(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zlll", args)?;
    let c = color_arg(vm, &a[0])?;
    let (w, h) = (
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0).max(0) as usize,
    );
    let op = CompositeOp(a[3].long_or(54) as i32);
    replace_op(vm, &d_(o), "Unable to border image", |i| {
        let mut n = transform::border(i, w, h, &c, op);
        n.border_color = c.clone();
        Ok(n)
    })
}

fn frame_common(vm: &mut Vm, o: Option<&Obj>, a: &[Arg], op: Option<CompositeOp>) -> R {
    let c = color_arg(vm, &a[0])?;
    let (w, h) = (
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0).max(0) as usize,
    );
    let (inner, outer) = (
        a[3].long_or(0).max(0) as usize,
        a[4].long_or(0).max(0) as usize,
    );
    replace_op(vm, &d_(o), "Unable to frame image", |i| {
        let op = op.unwrap_or(i.compose);
        let mut n = transform::frame(i, w, h, inner, outer, &c, op);
        n.matte_color = c.clone();
        Ok(n)
    })
}

fn frame_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zllll", args)?;
    frame_common(vm, o, &a, None)
}

fn frame_image_with_composite(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zlllll", args)?;
    let op = CompositeOp(a[5].long_or(54) as i32);
    frame_common(vm, o, &a, Some(op))
}

fn rotate_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zd", args)?;
    let c = color_arg(vm, &a[0])?;
    let deg = a[1].double_or(0.0);
    replace_op(vm, &d_(o), "Unable to rotate image", |i| {
        Ok(transform::rotate(i, deg, &c))
    })
}

fn flip_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to flip image", |i| {
        Ok(transform::flip(i))
    })
}

fn flop_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to flop image", |i| {
        Ok(transform::flop(i))
    })
}

fn transpose_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to transpose image", |i| {
        Ok(transform::transpose(i))
    })
}

fn transverse_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to transverse image", |i| {
        Ok(transform::transverse(i))
    })
}

fn trim_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let fuzz = a[0].double_or(0.0);
    replace_op(vm, &d_(o), "Unable to trim image", |i| {
        Ok(transform::trim(i, fuzz))
    })
}

fn roll_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let (x, y) = (a[0].long_or(0), a[1].long_or(0));
    replace_op(vm, &d_(o), "Unable to roll image", |i| {
        Ok(transform::roll(i, x, y))
    })
}

fn shave_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ll", args)?;
    let (w, h) = (
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
    );
    replace_op(vm, &d_(o), "Unable to shave image", |i| {
        transform::shave(i, w, h)
    })
}

fn chop_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    replace_op(vm, &d_(o), "Unable to chop image", |i| {
        transform::chop(i, r)
    })
}

fn splice_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    replace_op(vm, &d_(o), "Unable to splice image", |i| {
        Ok(transform::splice(i, r, i.gravity))
    })
}

/// AutoOrientImage(): undo the EXIF orientation.
fn orient(i: &Image) -> Image {
    let mut n = match i.orientation {
        OrientationType::TopRight => transform::flop(i),
        OrientationType::BottomRight => transform::rotate(i, 180.0, &PixelInfo::black()),
        OrientationType::BottomLeft => transform::flip(i),
        OrientationType::LeftTop => transform::transpose(i),
        OrientationType::RightTop => transform::rotate(i, 90.0, &PixelInfo::black()),
        OrientationType::RightBottom => transform::transverse(i),
        OrientationType::LeftBottom => transform::rotate(i, 270.0, &PixelInfo::black()),
        _ => i.clone(),
    };
    n.orientation = OrientationType::TopLeft;
    if n.properties.get("exif:Orientation").is_some() {
        n.properties.set("exif:Orientation", "1");
    }
    n
}

fn auto_orient(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = d_(o);
    replace_op(vm, &d, "Unable autoOrient image", |i| Ok(orient(i)))?;
    Ok(Value::Null)
}

// ---------------------------------------------------------------------------
// compositing, layers

fn composite_image_m(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zlll|l", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let src_d = other_imagick(vm, f, 1, &a[0])?;
    let src = src_d.wand.borrow().image().unwrap().clone();
    let op = CompositeOp(a[1].long_or(0) as i32);
    let (x, y) = (a[2].long_or(0), a[3].long_or(0));
    let mut w = d.wand.borrow_mut();
    let img = w.image_mut().unwrap();
    let args_s = img
        .artifacts
        .get("compose:args")
        .map(ComposeArgs::parse)
        .unwrap_or_default();
    composite_image(img, &src, op, x, y, &args_s);
    img.colors = None;
    Ok(Value::True)
}

fn composite_image_gravity(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zll", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let src_d = other_imagick(vm, f, 1, &a[0])?;
    let src = src_d.wand.borrow().image().unwrap().clone();
    let op = CompositeOp(a[1].long_or(0) as i32);
    let g = Gravity(a[2].long_or(0) as i32);
    image_op(vm, &d, "Unable to compositeImageGravity", |img| {
        let (x, y) = goro_imagick::geometry::gravity_adjust(
            img.columns,
            img.rows,
            g,
            src.columns,
            src.rows,
            0,
            0,
        );
        let args_s = img
            .artifacts
            .get("compose:args")
            .map(ComposeArgs::parse)
            .unwrap_or_default();
        composite_image(img, &src, op, x, y, &args_s);
        img.colors = None;
        Ok(())
    })
}

fn merge_layers(vm: &mut Vm, d: &crate::ImagickData, method: i32, msg: &str) -> R {
    not_empty(vm, d)?;
    let merged = {
        let mut w = d.wand.borrow_mut();
        w.set_first_iterator();
        let bg = w.images[0].background_color.clone();
        let refs: Vec<&Image> = w.images.iter().collect();
        transform::merge_layers(&refs, method, &bg)
    };
    let _ = msg;
    let nw = new_wand_from(d, vec![merged]);
    Ok(new_imagick(vm, nw))
}

fn flatten_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    merge_layers(vm, &d_(o), 14, "Flatten images failed")
}

fn mosaic_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    vm.emit_deprecated("Imagick::mosaicImages method is deprecated and it's use should be avoided");
    merge_layers(vm, &d_(o), 15, "Mosaic image failed")
}

fn merge_image_layers(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let m = a[0].long_or(0) as i32;
    let d = d_(o);
    match m {
        13..=16 => merge_layers(vm, &d, m, "Unable to merge image layers"),
        1 => coalesce(vm, &d),
        _ => {
            // other layer methods: the images unchanged
            not_empty(vm, &d)?;
            let imgs = d.wand.borrow().images.clone();
            let nw = new_wand_from(&d, imgs);
            Ok(new_imagick(vm, nw))
        }
    }
}

fn append_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|b", args)?;
    let stack = a[0].bool_or(false);
    let d = d_(o);
    not_empty(vm, &d)?;
    let out = {
        let w = d.wand.borrow();
        let first = &w.images[0];
        let bg = first.background_color.clone();
        let refs: Vec<&Image> = w.images.iter().collect();
        transform::append(&refs, stack, &bg, w.settings.gravity)
    };
    let nw = new_wand_from(&d, vec![out]);
    Ok(new_imagick(vm, nw))
}

fn coalesce(vm: &mut Vm, d: &crate::ImagickData) -> R {
    not_empty(vm, d)?;
    let out = {
        let w = d.wand.borrow();
        let first = &w.images[0];
        let (cw, ch) = (
            if first.page.width > 0 {
                first.page.width
            } else {
                first.columns
            },
            if first.page.height > 0 {
                first.page.height
            } else {
                first.rows
            },
        );
        let mut canvas = first.clone_attributes(cw, ch);
        canvas.enable_alpha();
        canvas.fill(&PixelInfo::transparent());
        let mut out = Vec::new();
        let mut previous = canvas.clone();
        for img in w.images.iter() {
            let before = canvas.clone();
            composite_image(
                &mut canvas,
                img,
                CompositeOp::Over,
                img.page.x,
                img.page.y,
                &ComposeArgs::default(),
            );
            let mut frame = canvas.clone();
            frame.delay = img.delay;
            frame.ticks_per_second = img.ticks_per_second;
            frame.iterations = img.iterations;
            frame.dispose = img.dispose;
            frame.page = Rect::new(cw, ch, 0, 0);
            out.push(frame);
            match img.dispose {
                DisposeType::Background => {
                    for y in 0..img.rows {
                        for x in 0..img.columns {
                            let tx = x as i64 + img.page.x;
                            let ty = y as i64 + img.page.y;
                            if tx >= 0 && ty >= 0 && (tx as usize) < cw && (ty as usize) < ch {
                                canvas.set_px(tx as usize, ty as usize, [0, 0, 0, 0]);
                            }
                        }
                    }
                }
                DisposeType::Previous => canvas = previous.clone(),
                _ => {}
            }
            previous = before;
        }
        out
    };
    let nw = new_wand_from(d, out);
    Ok(new_imagick(vm, nw))
}

fn coalesce_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    coalesce(vm, &d_(o))
}

fn get_image_region(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llll", args)?;
    let r = Rect::new(
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
        a[2].long_or(0),
        a[3].long_or(0),
    );
    let d = d_(o);
    let img = with_image(vm, &d, |i| transform::crop(i, r).0)?;
    let nw = new_wand_from(&d, vec![img]);
    Ok(new_imagick(vm, nw))
}

// ---------------------------------------------------------------------------
// point operations (on the Q16 samples)

fn negate_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "b|l", args)?;
    let gray = a[0].bool_or(false);
    let ch = a[1].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to negate image", |i| {
        goro_imagick::filters::negate(i, gray, ch);
        Ok(())
    })
}

/// TransformImageColorspace().
pub(crate) fn transform_colorspace(i: &mut Image, cs: Colorspace) {
    goro_imagick::colorspace::transform(i, cs);
}

fn transform_image_colorspace(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let cs = Colorspace(a[0].long_or(0) as i32);
    image_op(vm, &d_(o), "Unable to transform image colorspace", |i| {
        transform_colorspace(i, cs);
        Ok(())
    })
}

fn separate_image_channel(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let ch = a[0].long_or(0);
    image_op(vm, &d_(o), "Unable to separate image channel", |i| {
        let k = i.black.clone();
        for (n, p) in i.pixels.chunks_exact_mut(4).enumerate() {
            let v = if ch & channel::RED != 0 {
                p[0]
            } else if ch & channel::GREEN != 0 {
                p[1]
            } else if ch & channel::BLUE != 0 {
                p[2]
            } else if ch & channel::ALPHA != 0 {
                p[3]
            } else if ch & channel::BLACK != 0 {
                k.as_ref().map_or(0, |k| k[n])
            } else {
                p[0]
            };
            p.copy_from_slice(&[v, v, v, 65535]);
        }
        i.alpha = false;
        i.black = None;
        i.colorspace = Colorspace::Gray;
        i.colors = None;
        Ok(())
    })
}

/// The evaluate operators on a sample in quantum scale.
fn evaluate(op: i64, v: f64, x: f64, rng: &mut goro_imagick::pseudo::Rng) -> f64 {
    let q = 65535.0;
    match op {
        1 => (v).abs(),                                                        // Abs
        2 => v + x,                                                            // Add
        3 => (v + x).rem_euclid(q + 1.0),                                      // AddModulus
        4 => ((v as u64) & (x as u64)) as f64,                                 // And
        5 => q * (0.5 * (2.0 * std::f64::consts::PI * x * v / q).cos() + 0.5), // Cosine
        6 => {
            if x == 0.0 {
                q
            } else {
                v / x
            }
        } // Divide
        7 => q * (x * v / q).exp(),                                            // Exponential
        11 => ((v as u64) << (x as u32).min(63)) as f64,                       // LeftShift
        12 => {
            // Log
            if x == 0.0 {
                v
            } else {
                q * (x * v / q + 1.0).ln() / (x + 1.0).ln()
            }
        }
        13 => v.max(x),                                                         // Max
        14 => v,                                         // Mean (of one image)
        15 => v,                                         // Median
        16 => v.min(x),                                  // Min
        18 => v * x,                                     // Multiply
        19 => ((v as u64) | (x as u64)) as f64,          // Or
        21 => q * (v / q).powf(x),                       // Pow
        22 => ((v as u64) >> (x as u32).min(63)) as f64, // RightShift
        23 => v,                                         // RootMeanSquare
        24 => x,                                         // Set
        25 => q * (0.5 * (2.0 * std::f64::consts::PI * x * v / q).sin() + 0.5), // Sine
        26 => v - x,                                     // Subtract
        27 => v,                                         // Sum
        28 => {
            if v < x {
                0.0
            } else {
                v
            }
        } // ThresholdBlack
        29 => {
            if v <= x {
                0.0
            } else {
                q
            }
        } // Threshold
        30 => {
            if v > x {
                q
            } else {
                v
            }
        } // ThresholdWhite
        31 | 8 | 9 | 10 | 17 | 20 => {
            // the noise operators: a little uniform noise scaled by x
            v + (rng.next_f64() - 0.5) * x
        }
        32 => ((v as u64) ^ (x as u64)) as f64, // Xor
        33 => {
            // InverseLog
            if x == 0.0 {
                v
            } else {
                q * ((x + 1.0).powf(v / q) - 1.0) / x
            }
        }
        _ => v,
    }
}

fn evaluate_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ld|l", args)?;
    let op = a[0].long_or(0);
    let x = a[1].double_or(0.0);
    let ch = a[2].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to evaluate image", |i| {
        let mut rng = goro_imagick::pseudo::Rng::new(1);
        let mask = channel_mask(ch, i.alpha);
        for p in i.pixels.chunks_exact_mut(4) {
            for c in 0..4 {
                if mask[c] {
                    p[c] = clamp_to_quantum(evaluate(op, p[c] as f64, x, &mut rng)) as u16;
                }
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn evaluate_list(d: &crate::ImagickData, op: i64) -> MResult<Image> {
    let w = d.wand.borrow();
    let first = &w.images[0];
    let mut out = first.clone();
    let n = w.images.len() as f64;
    for y in 0..out.rows {
        for x in 0..out.columns {
            let mut acc = [0f64; 4];
            let mut vals: Vec<[f64; 4]> = Vec::new();
            for img in w.images.iter() {
                let p = img.virtual_px(x as i64, y as i64);
                let pf = [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64];
                vals.push(pf);
            }
            for c in 0..4 {
                let col: Vec<f64> = vals.iter().map(|v| v[c]).collect();
                acc[c] = match op {
                    13 => col.iter().cloned().fold(0.0, f64::max),
                    16 => col.iter().cloned().fold(f64::MAX, f64::min),
                    15 => {
                        let mut s = col.clone();
                        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        s[s.len() / 2]
                    }
                    27 | 2 => col.iter().sum(),
                    26 => col[0] - col[1..].iter().sum::<f64>(),
                    18 => col.iter().map(|v| v / 65535.0).product::<f64>() * 65535.0,
                    23 => (col.iter().map(|v| v * v).sum::<f64>() / n).sqrt(),
                    _ => col.iter().sum::<f64>() / n,
                };
            }
            out.set_px(
                x,
                y,
                [
                    to_q16(acc[0]),
                    to_q16(acc[1]),
                    to_q16(acc[2]),
                    to_q16(acc[3]),
                ],
            );
        }
    }
    out.colors = None;
    Ok(out)
}

fn evaluate_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    d.wand.borrow_mut().set_iterator_index(0);
    match evaluate_list(&d, a[0].long_or(0)) {
        Ok(img) => {
            let nw = new_wand_from(&d, vec![img]);
            Ok(new_imagick(vm, nw))
        }
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, "Evaluating images failed"),
    }
}

fn average_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    match evaluate_list(&d, 14) {
        Ok(img) => {
            let nw = new_wand_from(&d, vec![img]);
            Ok(new_imagick(vm, nw))
        }
        Err(e) => convert_exception(vm, Some(e), Cls::Imagick, "Averaging images failed"),
    }
}

fn function_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "la|l", args)?;
    let func = a[0].long_or(0);
    let Arg::Arr(arr) = &a[1] else {
        return Err(());
    };
    let mut params = Vec::new();
    for v in arr.borrow().values() {
        match v.deref() {
            Value::Long(n) => params.push(n as f64),
            Value::Double(d) => params.push(d),
            _ => {
                return throw_cls(
                    vm,
                    Cls::Imagick,
                    "The arguments array contains disallowed characters or is empty.",
                );
            }
        }
    }
    if params.is_empty() {
        return throw_cls(
            vm,
            Cls::Imagick,
            "The arguments array contains disallowed characters or is empty.",
        );
    }
    let ch = a[2].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to execute function on the image", |i| {
        let pi = std::f64::consts::PI;
        let fv = |x: f64| -> f64 {
            let p = |k: usize, d: f64| params.get(k).copied().unwrap_or(d);
            match func {
                3 => params.iter().fold(0.0, |acc, c| acc * x + c),
                4 => {
                    let (freq, phase, amp, bias) = (p(0, 1.0), p(1, 0.0), p(2, 0.5), p(3, 0.5));
                    amp * (2.0 * pi * (freq * x + phase / 360.0)).sin() + bias
                }
                1 => {
                    let (width, center, range, bias) = (p(0, 1.0), p(1, 0.5), p(2, 1.0), p(3, 0.5));
                    let r = 2.0 / width * (x - center);
                    if r <= -1.0 {
                        bias - range / 2.0
                    } else if r >= 1.0 {
                        bias + range / 2.0
                    } else {
                        range / pi * r.asin() + bias
                    }
                }
                2 => {
                    let (slope, center, range, bias) = (p(0, 1.0), p(1, 0.5), p(2, 1.0), p(3, 0.5));
                    range / pi * (pi * slope * (x - center)).atan() + bias
                }
                _ => x,
            }
        };
        let mask = channel_mask(ch, i.alpha);
        for px in i.pixels.chunks_exact_mut(4) {
            for c in 0..4 {
                if mask[c] {
                    px[c] = to_q16(fv(px[c] as f64 / 65535.0) * 65535.0);
                }
            }
        }
        i.colors = None;
        Ok(())
    })
}

/// LevelImage(): black/white points in quantum scale, gamma.
fn level_of(i: &mut Image, black: f64, white: f64, gamma: f64, ch: i64) {
    let mask = channel_mask(ch, i.alpha);
    let range = (white - black).abs().max(1e-12);
    for p in i.pixels.chunks_exact_mut(4) {
        for c in 0..4 {
            if mask[c] {
                let v = (p[c] as f64 - black) / range;
                let v = if v <= 0.0 {
                    0.0
                } else {
                    v.powf(1.0 / gamma.max(1e-12))
                };
                p[c] = to_q16(v * 65535.0);
            }
        }
    }
    i.colors = None;
}

fn level_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd|l", args)?;
    let (b, g, w) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(65535.0),
    );
    let ch = a[3].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to level image", |i| {
        level_of(i, b, w, g, ch);
        Ok(())
    })
}

fn gamma_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d|l", args)?;
    let g = a[0].double_or(1.0);
    let ch = a[1].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to gamma image", |i| {
        if g > 0.0 {
            goro_imagick::filters::map_channels(i, ch, |v| 65535.0 * (v / 65535.0).powf(1.0 / g));
        }
        Ok(())
    })
}

fn threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d|l", args)?;
    let t = a[0].double_or(0.0);
    let ch = a[1].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to threshold image", |i| {
        let all = ch == channel::DEFAULT;
        let mask = channel_mask(ch, i.alpha);
        for p in i.pixels.chunks_exact_mut(4) {
            if all {
                // BilevelImage(): on the pixel intensity
                let v = if Image::intensity(p) <= t { 0 } else { 65535 };
                p[0] = v;
                p[1] = v;
                p[2] = v;
            } else {
                for c in 0..4 {
                    if mask[c] {
                        p[c] = if (p[c] as f64) <= t { 0 } else { 65535 };
                    }
                }
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn threshold_color(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value], white: bool) -> R {
    let a = zpp::parse(vm, f, "z", args)?;
    let c = color_arg(vm, &a[0])?;
    let t = c.to_rgba16();
    let msg = if white {
        "Unable to white threshold image"
    } else {
        "Unable to black threshold image"
    };
    image_op(vm, &d_(o), msg, |i| {
        for p in i.pixels.chunks_exact_mut(4) {
            for ch in 0..3 {
                if white {
                    if p[ch] > t[ch] {
                        p[ch] = 65535;
                    }
                } else if p[ch] < t[ch] {
                    p[ch] = 0;
                }
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn black_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    threshold_color(vm, f, o, args, false)
}

fn white_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    threshold_color(vm, f, o, args, true)
}

fn solarize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let t = a[0].long_or(0) as f64;
    image_op(vm, &d_(o), "Unable to solarize image", |i| {
        goro_imagick::filters::map_channels(i, channel::DEFAULT, |v| {
            if v > t { 65535.0 - v } else { v }
        });
        Ok(())
    })
}

fn posterize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lb", args)?;
    let levels = a[0].long_or(2).max(2) as f64;
    image_op(vm, &d_(o), "Unable to posterize image", |i| {
        goro_imagick::filters::map_channels(i, channel::DEFAULT, |v| {
            ((v / 65535.0 * (levels - 1.0)).round() / (levels - 1.0)) * 65535.0
        });
        Ok(())
    })
}

/// QuantizeImage(): reduce to `n` colours (oxideav's uniform quantizer),
/// gray when the colourspace asks for it.
fn quantize_of(i: &mut Image, n: i64, cs: i64) -> MResult<()> {
    if cs == Colorspace::Gray.0 as i64 {
        transform_colorspace(i, Colorspace::Gray);
        let levels = n.clamp(2, 65536) as f64;
        for p in i.pixels.chunks_exact_mut(4) {
            let v = ((p[0] as f64 / 65535.0 * (levels - 1.0)).round() / (levels - 1.0) * 65535.0)
                as u16;
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
        i.colors = Some(n.max(1) as usize);
        return Ok(());
    }
    if i.number_colors() as i64 <= n {
        i.colors = Some(i.number_colors());
        return Ok(());
    }
    apply_oxideav(i, &of::Quantize::new(n.clamp(2, 1 << 24) as u32))?;
    i.colors = Some(n.max(1) as usize);
    Ok(())
}

fn quantize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lllbb", args)?;
    let (n, cs) = (a[0].long_or(256), a[1].long_or(0));
    image_op(vm, &d_(o), "Unable to quantize image", |i| {
        quantize_of(i, n, cs)
    })
}

fn quantize_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lllbb", args)?;
    let (n, cs) = (a[0].long_or(256), a[1].long_or(0));
    all_images_op(vm, &d_(o), "Unable to quantize images", |i| {
        quantize_of(i, n, cs)
    })
}

fn blend_toward(i: &mut Image, c: &PixelInfo, amount: [f64; 3]) {
    let (r, g, b) = c.rgb_q();
    let t = [r, g, b];
    for p in i.pixels.chunks_exact_mut(4) {
        for ch in 0..3 {
            p[ch] = to_q16(p[ch] as f64 * (1.0 - amount[ch]) + t[ch] * amount[ch]);
        }
    }
    i.colors = None;
}

fn colorize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zz|b", args)?;
    let c = color_arg(vm, &a[0])?;
    let op = match &a[1] {
        Arg::Any(v) => zval_to_opacity(vm, v, Cls::Imagick)?,
        _ => return Err(()),
    };
    image_op(vm, &d_(o), "Unable to colorize image", |i| {
        // the blend is per channel from the "opacity" colour (or its alpha)
        let amt = if op.alpha_trait || op.red != 0.0 || op.green != 0.0 || op.blue != 0.0 {
            [op.red / 65535.0, op.green / 65535.0, op.blue / 65535.0]
        } else {
            let a = op.alpha / 65535.0;
            [a, a, a]
        };
        let amt = if amt == [0.0, 0.0, 0.0] {
            let a = op.alpha / 65535.0;
            [a, a, a]
        } else {
            amt
        };
        blend_toward(i, &c, amt);
        Ok(())
    })
}

fn tint_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zz|b", args)?;
    let c = color_arg(vm, &a[0])?;
    let op = match &a[1] {
        Arg::Any(v) => zval_to_opacity(vm, v, Cls::Imagick)?,
        _ => return Err(()),
    };
    image_op(vm, &d_(o), "Unable tint image", |i| {
        // TintImage(): mid-tones move toward the colour, by the amount
        let (r, g, b) = c.rgb_q();
        let amt = [op.red / 65535.0, op.green / 65535.0, op.blue / 65535.0];
        let amt = if amt == [0.0, 0.0, 0.0] {
            let a = op.alpha / 65535.0;
            [a, a, a]
        } else {
            amt
        };
        let t = [r, g, b];
        for p in i.pixels.chunks_exact_mut(4) {
            let lum = Image::intensity(p) / 65535.0;
            let weight = 1.0 - 4.0 * (lum - 0.5) * (lum - 0.5);
            for ch in 0..3 {
                let delta = (t[ch] - 32767.5) * amt[ch] * weight;
                p[ch] = to_q16(p[ch] as f64 + delta);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn modulate_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd", args)?;
    let (b, s, h) = (
        a[0].double_or(100.0),
        a[1].double_or(100.0),
        a[2].double_or(100.0),
    );
    image_op(vm, &d_(o), "Unable to modulate image", |i| {
        // ModulateImage() in HSL: brightness/saturation in percent, hue as
        // a percentage where 100 is no rotation, 0/200 a half turn
        for p in i.pixels.chunks_exact_mut(4) {
            let (hh, ss, ll) = goro_imagick::color::rgb_to_hsl(
                p[0] as f64 / 65535.0,
                p[1] as f64 / 65535.0,
                p[2] as f64 / 65535.0,
            );
            let mut hh = hh + (h - 100.0) / 200.0;
            hh -= hh.floor();
            let ss = (ss * s / 100.0).clamp(0.0, 1.0);
            let ll = (ll * b / 100.0).clamp(0.0, 1.0);
            let (r, g, bb) = goro_imagick::color::hsl_to_rgb(hh, ss, ll);
            p[0] = to_q16(r * 65535.0);
            p[1] = to_q16(g * 65535.0);
            p[2] = to_q16(bb * 65535.0);
        }
        i.colors = None;
        Ok(())
    })
}

/// The histogram-based stretch of normalize/auto-level/contrast-stretch.
fn stretch(i: &mut Image, ch: i64, black_frac: f64, white_frac: f64, per_channel: bool) {
    let mask = channel_mask(ch, i.alpha);
    let n = (i.columns * i.rows) as f64;
    let ranges: Vec<(f64, f64)> = (0..3)
        .map(|c| {
            let mut v: Vec<u16> = i.pixels.chunks_exact(4).map(|p| p[c]).collect();
            v.sort_unstable();
            let lo = ((black_frac * n) as usize).min(v.len().saturating_sub(1));
            let hi = ((n - white_frac * n) as usize)
                .saturating_sub(1)
                .min(v.len().saturating_sub(1));
            (v[lo] as f64, v[hi.max(lo)] as f64)
        })
        .collect();
    let all = (
        ranges.iter().map(|r| r.0).fold(f64::MAX, f64::min),
        ranges.iter().map(|r| r.1).fold(0.0, f64::max),
    );
    for p in i.pixels.chunks_exact_mut(4) {
        for c in 0..3 {
            if !mask[c] {
                continue;
            }
            let (lo, hi) = if per_channel { ranges[c] } else { all };
            if hi > lo {
                p[c] = to_q16((p[c] as f64 - lo) / (hi - lo) * 65535.0);
            }
        }
    }
    i.colors = None;
}

fn normalize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|l", args)?;
    let ch = a[0].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to normalize image", |i| {
        // NormalizeImage(): a 2% / 1% contrast stretch
        stretch(i, ch, 0.02, 0.01, false);
        Ok(())
    })
}

fn equalize_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    image_op(vm, &d_(o), "Unable to equalize image", |i| {
        let n = (i.columns * i.rows) as f64;
        for c in 0..3 {
            let mut hist = vec![0u32; 65536];
            for p in i.pixels.chunks_exact(4) {
                hist[p[c] as usize] += 1;
            }
            let mut map = vec![0u16; 65536];
            let mut acc = 0f64;
            for (v, &h) in hist.iter().enumerate() {
                acc += h as f64;
                map[v] = to_q16(acc / n * 65535.0);
            }
            for p in i.pixels.chunks_exact_mut(4) {
                p[c] = map[p[c] as usize];
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn auto_level_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|l", args)?;
    let ch = a[0].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to autolevel image", |i| {
        stretch(i, ch, 0.0, 0.0, false);
        Ok(())
    })
}

fn auto_gamma_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|l", args)?;
    let ch = a[0].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "autoGammaImage", |i| {
        // gamma so that the mean goes to mid-gray
        let n = (i.columns * i.rows) as f64;
        let mean: f64 = i
            .pixels
            .chunks_exact(4)
            .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
            .sum::<f64>()
            / n
            / 65535.0;
        if mean > 0.0 && mean < 1.0 {
            let g = (0.5f64).ln() / mean.ln();
            goro_imagick::filters::map_channels(i, ch, |v| 65535.0 * (v / 65535.0).powf(g));
        }
        Ok(())
    })
}

fn contrast_stretch_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    let (b, w) = (a[0].double_or(0.0), a[1].double_or(0.0));
    let ch = a[2].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to contrast strech image", |i| {
        let n = (i.columns * i.rows) as f64;
        stretch(i, ch, b / n, w / n, false);
        Ok(())
    })
}

fn linear_stretch_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let (b, w) = (a[0].double_or(0.0), a[1].double_or(0.0));
    image_op(vm, &d_(o), "Unable to linear strech image", |i| {
        let n = (i.columns * i.rows) as f64;
        stretch(i, channel::DEFAULT, b / n, w / n, false);
        Ok(())
    })
}

fn sigmoidal_contrast_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "bdd|l", args)?;
    let sharpen = a[0].bool_or(true);
    let alpha = a[1].double_or(3.0);
    let beta = a[2].double_or(32768.0) / 65535.0;
    let ch = a[3].long_or(channel::DEFAULT);
    image_op(vm, &d_(o), "Unable to sigmoidal contrast image", |i| {
        if alpha.abs() < 1e-9 {
            return Ok(());
        }
        let sig = |x: f64| 1.0 / (1.0 + (alpha * (beta - x)).exp());
        let (s0, s1) = (sig(0.0), sig(1.0));
        goro_imagick::filters::map_channels(i, ch, |v| {
            let x = v / 65535.0;
            let y = if sharpen {
                (sig(x) - s0) / (s1 - s0)
            } else {
                let t = x * (s1 - s0) + s0;
                let t = t.clamp(1e-12, 1.0 - 1e-12);
                beta - (1.0 / t - 1.0).ln() / alpha
            };
            y * 65535.0
        });
        Ok(())
    })
}

fn brightness_contrast_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    let (b, c) = (a[0].double_or(0.0), a[1].double_or(0.0));
    let ch = a[2].long_or(channel::DEFAULT);
    let _ = ch;
    image_op(vm, &d_(o), "Unable to brightnesscontrastimage", |i| {
        apply_oxideav(i, &of::BrightnessContrast::new(b as f32, c as f32))
    })
}

fn contrast_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "b", args)?;
    let sharpen = a[0].bool_or(true);
    image_op(vm, &d_(o), "Unable to contrast image", |i| {
        // ContrastImage(): a sigmoid push away from (or toward) mid-gray
        for p in i.pixels.chunks_exact_mut(4) {
            for c in 0..3 {
                let x = p[c] as f64 / 65535.0;
                let s = 0.5 * (1.0 + ((std::f64::consts::PI * (x - 0.5)).sin()));
                let y = if sharpen {
                    x + 0.5 * (s - x)
                } else {
                    x - 0.5 * (s - x)
                };
                p[c] = to_q16(y * 65535.0);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn sepia_tone_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let t = a[0].double_or(80.0);
    image_op(vm, &d_(o), "Unable to sepia tone image", |i| {
        // SepiaToneImage(): a threshold in quantum scale
        let thr = (t / 65535.0).clamp(0.0, 1.0);
        for p in i.pixels.chunks_exact_mut(4) {
            let int = Image::intensity(p) / 65535.0;
            let r = if int > thr { 1.0 } else { int + (1.0 - thr) };
            let g = if int > 7.0 * thr / 6.0 {
                1.0
            } else {
                int + (1.0 - 7.0 * thr / 6.0)
            };
            let b = if int < thr / 6.0 {
                0.0
            } else {
                int - thr / 6.0
            };
            p[0] = to_q16(r.min(1.0) * 65535.0);
            p[1] = to_q16(g.clamp(0.0, 1.0) * 65535.0);
            p[2] = to_q16(b.clamp(0.0, 1.0) * 65535.0);
        }
        i.colors = None;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// filters on oxideav-image-filter

fn radius_of(radius: f64, sigma: f64) -> u32 {
    if radius >= 1.0 {
        radius.round() as u32
    } else {
        (3.0 * sigma.abs()).ceil().max(1.0) as u32
    }
}

fn blur_common(vm: &mut Vm, o: Option<&Obj>, radius: f64, sigma: f64, msg: &str) -> R {
    image_op(vm, &d_(o), msg, |i| {
        let r = radius_of(radius, sigma);
        apply_oxideav(i, &of::Blur::new(r).with_sigma(sigma.max(0.01) as f32))
    })
}

fn blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    blur_common(
        vm,
        o,
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        "Unable to blur image",
    )
}

fn gaussian_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    blur_common(
        vm,
        o,
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        "Unable to gaussian blur image",
    )
}

fn adaptive_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    blur_common(
        vm,
        o,
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        "Unable to adaptive blur image",
    )
}

fn motion_blur_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd|l", args)?;
    let (r, s, ang) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(0.0),
    );
    image_op(vm, &d_(o), "Unable to motion blur image", |i| {
        apply_oxideav(
            i,
            &of::MotionBlur::new(radius_of(r, s), s.max(0.01) as f32, ang as f32),
        )
    })
}

fn sharpen_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    let (r, s) = (a[0].double_or(0.0), a[1].double_or(1.0));
    image_op(vm, &d_(o), "Unable to sharpen image", |i| {
        apply_oxideav(i, &of::Sharpen::new(radius_of(r, s), s.max(0.01) as f32))
    })
}

fn adaptive_sharpen_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    let (r, s) = (a[0].double_or(0.0), a[1].double_or(1.0));
    image_op(vm, &d_(o), "Unable to adaptive sharpen image", |i| {
        apply_oxideav(i, &of::Sharpen::new(radius_of(r, s), s.max(0.01) as f32))
    })
}

fn unsharp_mask_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dddd|l", args)?;
    let (r, s, amount, thr) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(1.0),
        a[3].double_or(0.05),
    );
    image_op(vm, &d_(o), "Unable to unsharp mask image", |i| {
        apply_oxideav(
            i,
            &of::Unsharp::new(
                radius_of(r, s),
                s.max(0.01) as f32,
                amount as f32,
                (thr.clamp(0.0, 1.0) * 255.0) as u8,
            ),
        )
    })
}

fn edge_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "d", args)?;
    image_op(vm, &d_(o), "Unable to edge image", |i| {
        apply_oxideav(i, &of::Edge::new())
    })
}

fn emboss_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let r = radius_of(a[0].double_or(0.0), a[1].double_or(1.0));
    image_op(vm, &d_(o), "Unable to emboss image", |i| {
        apply_oxideav(i, &of::Emboss::new(r))
    })
}

fn charcoal_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let (r, s) = (a[0].double_or(0.0), a[1].double_or(1.0));
    image_op(vm, &d_(o), "Unable to charcoal image", |i| {
        apply_oxideav(i, &of::Charcoal::new(1.0).with_radius(radius_of(r, s)))
    })
}

fn sketch_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddd", args)?;
    let (r, s, ang) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].double_or(0.0),
    );
    image_op(vm, &d_(o), "Unable to sketch image", |i| {
        apply_oxideav(
            i,
            &of::MotionBlur::new(radius_of(r, s), s.max(0.01) as f32, ang as f32),
        )?;
        apply_oxideav(i, &of::Charcoal::new(1.0))
    })
}

fn oil_paint_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let r = a[0].double_or(3.0).max(1.0).round() as u32;
    image_op(vm, &d_(o), "Unable to oilpaint image", |i| {
        apply_oxideav(i, &of::Paint::new(r))
    })
}

fn oil_paint_image_with_sigma(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let r = radius_of(a[0].double_or(0.0), a[1].double_or(1.0));
    image_op(vm, &d_(o), "Unable to oilPaintImageWithSigma image", |i| {
        apply_oxideav(i, &of::Paint::new(r))
    })
}

fn swirl_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let deg = a[0].double_or(0.0);
    image_op(vm, &d_(o), "Unable to swirl image", |i| {
        apply_oxideav(i, &of::Swirl::new(deg as f32))
    })
}

fn swirl_image_with_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dl", args)?;
    let deg = a[0].double_or(0.0);
    image_op(vm, &d_(o), "Unable to swirl image", |i| {
        apply_oxideav(i, &of::Swirl::new(deg as f32))
    })
}

/// WaveImage(): the image grows by twice the amplitude in height.
fn wave_of(i: &Image, amp: f64, len: f64) -> MResult<Image> {
    let extra = (2.0 * amp.abs()).ceil() as usize;
    let mut out = i.clone_attributes(i.columns, i.rows + extra);
    let bg = i.background_color.to_rgba16();
    let len = if len.abs() < 1e-9 { 1.0 } else { len };
    for x in 0..out.columns {
        let off = amp * (2.0 * std::f64::consts::PI * x as f64 / len).sin();
        for y in 0..out.rows {
            let sy = y as f64 - amp.abs() - off;
            let p = if sy < -0.5 || sy > i.rows as f64 - 0.5 {
                bg
            } else {
                let v = i.sample_bilinear(x as f64 + 0.5, sy + 0.5);
                [to_q16(v[0]), to_q16(v[1]), to_q16(v[2]), to_q16(v[3])]
            };
            out.set_px(x, y, p);
        }
    }
    out.page = Rect::new(out.columns, out.rows, 0, 0);
    out.colors = None;
    Ok(out)
}

fn wave_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd", args)?;
    let (amp, len) = (a[0].double_or(0.0), a[1].double_or(1.0));
    replace_op(vm, &d_(o), "Unable to wave image", |i| wave_of(i, amp, len))
}

fn wave_image_with_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddl", args)?;
    let (amp, len) = (a[0].double_or(0.0), a[1].double_or(1.0));
    replace_op(vm, &d_(o), "Unable to wave image", |i| wave_of(i, amp, len))
}

fn implode_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let r = a[0].double_or(0.0);
    image_op(vm, &d_(o), "Unable to implode image", |i| {
        apply_oxideav(i, &of::Implode::new(r as f32))
    })
}

fn implode_image_with_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dl", args)?;
    let r = a[0].double_or(0.0);
    image_op(vm, &d_(o), "Unable to implode image", |i| {
        apply_oxideav(i, &of::Implode::new(r as f32))
    })
}

fn spread_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    let r = a[0].double_or(1.0).max(0.0).round() as u32;
    image_op(vm, &d_(o), "Unable to spread image", |i| {
        apply_oxideav(i, &of::Spread::new(r.max(1)))
    })
}

fn spread_image_with_method(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dl", args)?;
    let r = a[0].double_or(1.0).max(0.0).round() as u32;
    image_op(vm, &d_(o), "Unable to spread image", |i| {
        apply_oxideav(i, &of::Spread::new(r.max(1)))
    })
}

fn despeckle_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    image_op(vm, &d_(o), "Unable to despeckle image", |i| {
        apply_oxideav(i, &of::Despeckle::new(1))
    })
}

fn enhance_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    image_op(vm, &d_(o), "Unable to enhance image", |i| {
        apply_oxideav(i, &of::Despeckle::new(1))
    })
}

fn median_filter_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "d", args)?;
    vm.emit_deprecated(&format!(
        "{} method is deprecated and it's use should be avoided",
        f.name
    ));
    let r = a[0].double_or(1.0).max(0.0).round() as u32;
    let n = 2 * r.max(1) + 1;
    image_op(vm, &d_(o), "Unable to median filter image", |i| {
        apply_oxideav(i, &of::Statistic::new(of::StatisticOp::Median, n, n))
    })
}

fn statistic_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lll|l", args)?;
    let op = match a[0].long_or(0) {
        2 => of::StatisticOp::Max,
        3 => of::StatisticOp::Mean,
        5 => of::StatisticOp::Min,
        _ => of::StatisticOp::Median,
    };
    let (w, h) = (a[1].long_or(1).max(1) as u32, a[2].long_or(1).max(1) as u32);
    image_op(vm, &d_(o), "Unable to statisticImage", |i| {
        apply_oxideav(i, &of::Statistic::new(op, w, h))
    })
}

fn vignette_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ddll", args)?;
    let (r, s, x, y) = (
        a[0].double_or(0.0),
        a[1].double_or(1.0),
        a[2].long_or(0),
        a[3].long_or(0),
    );
    let _ = r;
    image_op(vm, &d_(o), "Unable to apply vignette filter", |i| {
        vignette(i, s, x as f64, y as f64);
        Ok(())
    })
}

/// -vignette: a centred ellipse inset by (x, y), blurred by `sigma`,
/// blends the image with its background colour.
fn vignette(i: &mut Image, sigma: f64, x: f64, y: f64) {
    let (w, h) = (i.columns, i.rows);
    if w == 0 || h == 0 {
        return;
    }
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let (rx, ry) = ((cx - x).max(0.0), (cy - y).max(0.0));
    let mut mask = vec![0f64; w * h];
    if rx > 0.0 && ry > 0.0 {
        for py in 0..h {
            for px in 0..w {
                let dx = (px as f64 + 0.5 - cx) / rx;
                let dy = (py as f64 + 0.5 - cy) / ry;
                if dx * dx + dy * dy <= 1.0 {
                    mask[py * w + px] = 1.0;
                }
            }
        }
    }
    if sigma > 0.0 {
        let rad = (3.0 * sigma).ceil() as i64;
        let k: Vec<f64> = (-rad..=rad)
            .map(|t| (-(t * t) as f64 / (2.0 * sigma * sigma)).exp())
            .collect();
        let ks: f64 = k.iter().sum();
        let blur = |src: &[f64], horiz: bool| -> Vec<f64> {
            let mut out = vec![0f64; w * h];
            for py in 0..h {
                for px in 0..w {
                    let mut acc = 0.0;
                    for (j, kv) in k.iter().enumerate() {
                        let o = j as i64 - rad;
                        let (sx, sy) = if horiz {
                            ((px as i64 + o).clamp(0, w as i64 - 1) as usize, py)
                        } else {
                            (px, (py as i64 + o).clamp(0, h as i64 - 1) as usize)
                        };
                        acc += kv * src[sy * w + sx];
                    }
                    out[py * w + px] = acc / ks;
                }
            }
            out
        };
        mask = blur(&blur(&mask, true), false);
    }
    i.enable_alpha();
    let bg = i.background_color.to_rgba16();
    for (p, m) in i.pixels.chunks_exact_mut(4).zip(mask) {
        for c in 0..4 {
            p[c] = (p[c] as f64 * m + bg[c] as f64 * (1.0 - m))
                .round()
                .clamp(0.0, 65535.0) as u16;
        }
    }
    i.colors = None;
}

fn shade_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "bdd", args)?;
    let (gray, az, el) = (
        a[0].bool_or(true),
        a[1].double_or(30.0),
        a[2].double_or(30.0),
    );
    image_op(vm, &d_(o), "Unable to shade image", |i| {
        apply_oxideav(i, &of::Shade::new(az as f32, el as f32).with_color(!gray))
    })
}

fn blue_shift_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "|d", args)?;
    let fct = a[0].double_or(1.5);
    image_op(vm, &d_(o), "Unable to blue shift image", |i| {
        apply_oxideav(i, &of::BlueShift::new(fct as f32))
    })
}

fn clamp_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "|l", args)?;
    // Q16 samples are always within range
    image_op(vm, &d_(o), "Unable to clamp image", |_| Ok(()))
}

fn add_noise_of(i: &mut Image, kind: i64, attenuate: f64) {
    let mut rng = goro_imagick::pseudo::Rng::new(0x5eed);
    let scale = 65535.0 * 0.1 * attenuate;
    for p in i.pixels.chunks_exact_mut(4) {
        for c in 0..3 {
            let v = p[c] as f64;
            let u = rng.next_f64();
            let n = match kind {
                2 | 3 => {
                    // gaussian (Box-Muller)
                    let u2 = rng.next_f64().max(1e-12);
                    (-2.0 * u2.ln()).sqrt() * (2.0 * std::f64::consts::PI * u).cos() * scale * 0.5
                }
                4 => {
                    if u < 0.05 {
                        -v
                    } else if u > 0.95 {
                        65535.0 - v
                    } else {
                        0.0
                    }
                }
                5 => {
                    let s = if u < 0.5 { -1.0 } else { 1.0 };
                    s * (1.0 - 2.0 * (u - 0.5).abs()).max(1e-12).ln().abs() * scale * 0.3
                }
                _ => (u - 0.5) * scale,
            };
            p[c] = to_q16(v + n);
        }
    }
    i.colors = None;
}

fn add_noise_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l|l", args)?;
    let kind = a[0].long_or(1);
    image_op(vm, &d_(o), "Unable to add image noise", |i| {
        add_noise_of(i, kind, 1.0);
        Ok(())
    })
}

fn add_noise_image_with_attenuate(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "ld|l", args)?;
    let kind = a[0].long_or(1);
    let att = a[1].double_or(1.0);
    image_op(vm, &d_(o), "Unable to add image noise", |i| {
        add_noise_of(i, kind, att);
        Ok(())
    })
}

fn raise_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "llllb", args)?;
    let (w, h) = (
        a[0].long_or(0).max(0) as usize,
        a[1].long_or(0).max(0) as usize,
    );
    let raise = a[4].bool_or(true);
    image_op(vm, &d_(o), "Unable to raise image", |i| {
        // RaiseImage(): lighten the top/left edges, darken bottom/right
        let (cw, ch) = (i.columns, i.rows);
        for y in 0..ch {
            for x in 0..cw {
                let top = y < h && x < cw - y.min(cw);
                let left = x < w && y < ch - x.min(ch);
                let bottom = y >= ch.saturating_sub(h);
                let right = x >= cw.saturating_sub(w);
                let lighten = (top || left) == raise && (top || left);
                let darken = (bottom || right) && !(top || left);
                let mut p = i.px(x, y);
                for v in p.iter_mut().take(3) {
                    let f = *v as f64;
                    *v = if lighten {
                        to_q16(f + (65535.0 - f) * 0.4)
                    } else if (darken == raise && darken) || ((top || left) && !raise) {
                        to_q16(f * 0.6)
                    } else if darken && !raise {
                        to_q16(f + (65535.0 - f) * 0.4)
                    } else {
                        *v
                    };
                }
                i.set_px(x, y, p);
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn random_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "dd|l", args)?;
    let (lo, hi) = (a[0].double_or(0.0), a[1].double_or(65535.0));
    image_op(vm, &d_(o), "Unable to random threshold image", |i| {
        let mut rng = goro_imagick::pseudo::Rng::new(7);
        for p in i.pixels.chunks_exact_mut(4) {
            for c in 0..3 {
                let v = p[c] as f64;
                let t = lo + rng.next_f64() * (hi - lo);
                p[c] = if v < lo {
                    0
                } else if v > hi {
                    65535
                } else if v <= t {
                    0
                } else {
                    65535
                };
            }
        }
        i.colors = None;
        Ok(())
    })
}

fn adaptive_threshold_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "lll", args)?;
    let (w, h, off) = (a[0].long_or(3), a[1].long_or(3), a[2].long_or(0));
    image_op(vm, &d_(o), "Unable to adaptive threshold image", |i| {
        let r = (w.max(h).max(1) / 2).max(1) as u32;
        apply_oxideav(i, &of::AdaptiveThreshold::new(r, -(off / 257) as i32))
    })
}

fn paint_common(
    i: &mut Image,
    target: &PixelInfo,
    fuzz: f64,
    invert: bool,
    mut set: impl FnMut(&mut [u16]),
) {
    let t = target.to_rgba16();
    let f2 = fuzz * fuzz;
    for p in i.pixels.chunks_exact_mut(4) {
        let mut dist = 0.0;
        for c in 0..3 {
            let d = p[c] as f64 - t[c] as f64;
            dist += d * d;
        }
        let matches = dist <= f2.max(0.0);
        if matches != invert {
            set(p);
        }
    }
    i.colors = None;
}

fn transparent_paint_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zddb", args)?;
    let c = color_arg(vm, &a[0])?;
    let alpha = to_q16(a[1].double_or(0.0) * 65535.0);
    let fuzz = a[2].double_or(0.0);
    let invert = a[3].bool_or(false);
    image_op(vm, &d_(o), "Unable to paint transparent image", |i| {
        i.enable_alpha();
        paint_common(i, &c, fuzz, invert, |p| p[3] = alpha);
        Ok(())
    })
}

fn opaque_paint_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zzdb|l", args)?;
    let target = color_arg(vm, &a[0])?;
    let fill = color_arg(vm, &a[1])?;
    let fuzz = a[2].double_or(0.0);
    let invert = a[3].bool_or(false);
    image_op(vm, &d_(o), "Unable to opaque paint image", |i| {
        let q = fill.to_rgba16();
        let alpha = i.alpha;
        paint_common(i, &target, fuzz, invert, |p| {
            p[0] = q[0];
            p[1] = q[1];
            p[2] = q[2];
            if alpha {
                p[3] = q[3];
            }
        });
        Ok(())
    })
}

#[allow(clippy::manual_memcpy)] // each channel looks up its own entry
fn clut_of(i: &mut Image, clut: &Image) {
    let n = (clut.columns * clut.rows).max(1);
    for p in i.pixels.chunks_exact_mut(4) {
        for c in 0..3 {
            let idx = ((p[c] as f64 / 65535.0) * (n - 1) as f64).round() as usize;
            let cp = clut.px(
                idx % clut.columns.max(1),
                (idx / clut.columns.max(1)).min(clut.rows - 1),
            );
            p[c] = cp[c];
        }
    }
    i.colors = None;
}

fn clut_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|l", args)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let clut = other.wand.borrow().image().unwrap().clone();
    image_op(
        vm,
        &d_(o),
        "Unable to replace colors in the image from a color lookup table",
        |i| {
            clut_of(i, &clut);
            Ok(())
        },
    )
}

fn clut_image_with_interpolate(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let clut = other.wand.borrow().image().unwrap().clone();
    image_op(
        vm,
        &d_(o),
        "Unable to replace colors in the image from a color lookup table",
        |i| {
            clut_of(i, &clut);
            Ok(())
        },
    )
}

fn unique_image_colors(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    replace_op(vm, &d_(o), "Unable to get unique image colors", |i| {
        let mut colors: Vec<[u16; 4]> = i
            .pixels
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], if i.alpha { p[3] } else { 65535 }])
            .collect();
        colors.sort_unstable();
        colors.dedup();
        let mut out = i.clone_attributes(colors.len(), 1);
        for (x, c) in colors.iter().enumerate() {
            out.set_px(x, 0, *c);
        }
        out.page = Rect::new(colors.len(), 1, 0, 0);
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// statistics, comparison

fn get_image_channel_mean(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let ch = a[0].long_or(channel::DEFAULT);
    let s = with_image(vm, &d_(o), |i| composite_statistics(i, ch))?;
    let mut arr = PhpArray::new();
    set(&mut arr, "mean", Value::Double(s.mean));
    set(
        &mut arr,
        "standardDeviation",
        Value::Double(s.standard_deviation),
    );
    Ok(arr_val(arr))
}

fn get_image_mean(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let s = with_image(vm, &d_(o), |i| composite_statistics(i, channel::DEFAULT))?;
    let mut arr = PhpArray::new();
    set(&mut arr, "mean", Value::Double(s.mean));
    set(
        &mut arr,
        "standard_deviation",
        Value::Double(s.standard_deviation),
    );
    Ok(arr_val(arr))
}

fn get_image_channel_range(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let ch = a[0].long_or(channel::DEFAULT);
    let s = with_image(vm, &d_(o), |i| composite_statistics(i, ch))?;
    let mut arr = PhpArray::new();
    set(&mut arr, "minima", Value::Double(s.minima));
    set(&mut arr, "maxima", Value::Double(s.maxima));
    Ok(arr_val(arr))
}

fn get_image_range(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    let s = with_image(vm, &d_(o), |i| composite_statistics(i, channel::DEFAULT))?;
    let mut arr = PhpArray::new();
    set(&mut arr, "minima", Value::Double(s.minima));
    set(&mut arr, "maxima", Value::Double(s.maxima));
    Ok(arr_val(arr))
}

fn get_image_channel_statistics(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    zpp::parse(vm, f, "", args)?;
    // indexed by PixelChannel (0 red, 1 green, 2 blue, 3 black, 4 alpha,
    // 5 index...); channels the image lacks report empty statistics
    let list = with_image(vm, &d_(o), |i| channel_statistics(i))?;
    let mut arr = PhpArray::new();
    for (k, s) in list.into_iter().enumerate() {
        let s = s.unwrap_or_else(ChannelStats::absent);
        let mut t = PhpArray::new();
        set(&mut t, "mean", Value::Double(s.mean));
        set(&mut t, "minima", Value::Double(s.minima));
        set(&mut t, "maxima", Value::Double(s.maxima));
        set(
            &mut t,
            "standardDeviation",
            Value::Double(s.standard_deviation),
        );
        set(&mut t, "depth", Value::Long(s.depth as i64));
        arr.set(goro_core::array::ArrayKey::Int(k as i64), arr_val(t));
    }
    Ok(arr_val(arr))
}

/// A distortion metric between two images (quantum-scale samples).
fn distortion(a: &Image, b: &Image, metric: i64) -> f64 {
    distortion_ch(a, b, metric, channel::DEFAULT)
}

/// A metric over the colour channels `ch` selects.
pub(crate) fn distortion_ch(a: &Image, b: &Image, metric: i64, ch: i64) -> f64 {
    let mask = channel_mask(ch, false);
    let nch = (0..3).filter(|&c| mask[c]).count().max(1) as f64;
    let (w, h) = (a.columns.min(b.columns), a.rows.min(b.rows));
    let n = (w * h).max(1) as f64;
    let mut abs_err = 0usize;
    let mut sum_abs = 0.0;
    let mut sum_sq = 0.0;
    let mut peak: f64 = 0.0;
    for y in 0..h {
        for x in 0..w {
            let (p, q) = (a.px(x, y), b.px(x, y));
            let mut diff_any = false;
            let mut pix_sq = 0.0;
            for c in (0..3).filter(|&c| mask[c]) {
                let d = (p[c] as f64 - q[c] as f64) / 65535.0;
                if d != 0.0 {
                    diff_any = true;
                }
                sum_abs += d.abs();
                pix_sq += d * d;
                peak = peak.max(d.abs());
            }
            sum_sq += pix_sq;
            if diff_any {
                abs_err += 1;
            }
        }
    }
    let mse = sum_sq / (n * nch);
    // ImageMagick 7.1.2 reports normalised metrics: AE as the fraction
    // of differing pixels, MEPP as the mean error, PSNR over 120 dB
    match metric {
        1 => abs_err as f64 / n,
        3 | 4 => sum_abs / (n * nch),
        5 => mse,
        7 => peak,
        8 => {
            if mse == 0.0 {
                1.0
            } else {
                (-10.0 * mse.log10() / 120.0).min(1.0)
            }
        }
        10 => mse.sqrt(),
        2 => mse.sqrt(),
        _ => mse,
    }
}

fn compare_images(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    compare_of(vm, f, o, &a[0], None, a[1].long_or(0))
}

fn compare_image_channels(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zll", args)?;
    compare_of(vm, f, o, &a[0], Some(a[1].long_or(0)), a[2].long_or(0))
}

fn compare_of(
    vm: &mut Vm,
    f: &Func,
    o: Option<&Obj>,
    other: &Arg,
    ch: Option<i64>,
    metric: i64,
) -> R {
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, other)?;
    let b = other.wand.borrow().image().unwrap().clone();
    let (dist, diff) = {
        let w = d.wand.borrow();
        let img = w.image().unwrap();
        let dist = distortion_ch(img, &b, metric, ch.unwrap_or(channel::DEFAULT));
        // the difference image: red where pixels differ, a faded copy elsewhere
        let mut diff = img.clone();
        for y in 0..diff.rows {
            for x in 0..diff.columns {
                let p = img.px(x, y);
                let same = x < b.columns && y < b.rows && b.px(x, y)[..3] == p[..3];
                let np = if same {
                    [
                        (p[0] as f64 * 0.2 + 52428.0) as u16,
                        (p[1] as f64 * 0.2 + 52428.0) as u16,
                        (p[2] as f64 * 0.2 + 52428.0) as u16,
                        65535,
                    ]
                } else {
                    [61166, 0, 0, 65535]
                };
                diff.set_px(x, y, np);
            }
        }
        diff.colors = None;
        (dist, diff)
    };
    let nw = new_wand_from(&d, vec![diff]);
    let mut arr = PhpArray::new();
    arr.push(new_imagick(vm, nw));
    arr.push(Value::Double(dist));
    Ok(arr_val(arr))
}

fn get_image_distortion(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let b = other.wand.borrow().image().unwrap().clone();
    let m = a[1].long_or(0);
    let v = with_image(vm, &d, |i| distortion(i, &b, m))?;
    Ok(Value::Double(v))
}

fn get_image_channel_distortion(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zll", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let b = other.wand.borrow().image().unwrap().clone();
    let m = a[2].long_or(0);
    let v = with_image(vm, &d, |i| distortion_ch(i, &b, m, a[1].long_or(0)))?;
    Ok(Value::Double(v))
}

fn set_image_mask(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zl", args)?;
    let other = other_imagick(vm, f, 1, &a[0])?;
    let m = other.wand.borrow().image().unwrap().clone();
    let kind = a[1].long_or(1);
    image_op(vm, &d_(o), "Unable to set image mask", |i| {
        if kind == 2 {
            i.write_mask = Some(Box::new(m));
        } else {
            i.read_mask = Some(Box::new(m));
        }
        Ok(())
    })?;
    Ok(Value::Null)
}

fn get_image_mask(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let kind = a[0].long_or(1);
    let d = d_(o);
    let m = with_image(vm, &d, |i| {
        if kind == 2 {
            i.write_mask.clone()
        } else {
            i.read_mask.clone()
        }
    })?;
    match m {
        Some(m) => {
            let nw = new_wand_from(&d, vec![*m]);
            Ok(new_imagick(vm, nw))
        }
        None => Ok(Value::Null),
    }
}

fn color_matrix_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "a", args)?;
    let Arg::Arr(arr) = &a[0] else {
        return Err(());
    };
    let mut v = Vec::new();
    for x in arr.borrow().values() {
        match x.deref() {
            Value::Long(n) => v.push(n as f64),
            Value::Double(d) => v.push(d),
            _ => return throw_cls(vm, Cls::Imagick, "Unable to read color matrix array"),
        }
    }
    let order = match v.len() {
        25 => 5,
        36 => 6,
        _ => return throw_cls(vm, Cls::Imagick, "Color matrix array must be 5x5 or 6x6"),
    };
    image_op(vm, &d_(o), "Unable to colormatriximage", |i| {
        for p in i.pixels.chunks_exact_mut(4) {
            let src = [
                p[0] as f64 / 65535.0,
                p[1] as f64 / 65535.0,
                p[2] as f64 / 65535.0,
                p[3] as f64 / 65535.0,
            ];
            let mut out = [0f64; 4];
            for (r, o) in out.iter_mut().enumerate().take(3) {
                let row = &v[r * order..];
                let mut s = 0.0;
                for c in 0..4.min(order) {
                    s += row[c] * src[c];
                }
                // the last column is the offset
                s += row[order - 1];
                *o = s;
            }
            p[0] = to_q16(out[0] * 65535.0);
            p[1] = to_q16(out[1] * 65535.0);
            p[2] = to_q16(out[2] * 65535.0);
        }
        i.colors = None;
        Ok(())
    })
}

/// ConvolveImage() with an ImagickKernel (Q16, edge virtual pixels).
fn convolve_of(i: &mut Image, k: &crate::kernel::Kernel) {
    let src = i.clone();
    for y in 0..i.rows {
        for x in 0..i.columns {
            let mut acc = [0f64; 3];
            for ky in 0..k.height {
                for kx in 0..k.width {
                    let w = k.values[ky * k.width + kx];
                    if w.is_nan() || w == 0.0 {
                        continue;
                    }
                    let sx = x as i64 + kx as i64 - k.x as i64;
                    let sy = y as i64 + ky as i64 - k.y as i64;
                    let p = src.virtual_px(sx, sy);
                    for c in 0..3 {
                        acc[c] += w * p[c] as f64;
                    }
                }
            }
            let mut p = src.px(x, y);
            for c in 0..3 {
                p[c] = to_q16(acc[c]);
            }
            i.set_px(x, y, p);
        }
    }
    i.colors = None;
}

fn convolve_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "z|l", args)?;
    let kobj = match &a[0] {
        Arg::Any(v) => crate::arg_obj(vm, f, 1, v, crate::C_KERNEL, "ImagickKernel")?,
        _ => return Err(()),
    };
    let ks = crate::kernel::kernels_of(vm, &kobj)?;
    image_op(vm, &d_(o), "Failed to filter image", |i| {
        for k in &ks {
            convolve_of(i, k);
        }
        Ok(())
    })
}

fn cycle_colormap_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "l", args)?;
    let n = a[0].long_or(0);
    image_op(vm, &d_(o), "Unable to cycle image colormap", |i| {
        apply_oxideav(i, &of::Cycle::new(n as i32))
    })
}

fn draw_arg(
    vm: &mut Vm,
    f: &Func,
    n: usize,
    a: &Arg,
) -> Result<goro_imagick::draw::DrawingWand, ()> {
    match a {
        Arg::Any(v) => {
            let o = crate::arg_obj(vm, f, n, v, crate::C_DRAW, "ImagickDraw")?;
            Ok(crate::draw::wand_of(&o))
        }
        _ => Err(()),
    }
}

fn draw_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let d = d_(o);
    not_empty(vm, &d)?;
    let a = zpp::parse(vm, f, "z", args)?;
    let dw = draw_arg(vm, f, 1, &a[0])?;
    image_op(vm, &d, "Unable to draw image", |i| dw.render_onto(i))
}

fn annotate_image(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zddds", args)?;
    let d = d_(o);
    not_empty(vm, &d)?;
    let mut dw = draw_arg(vm, f, 1, &a[0])?;
    let (x, y) = (a[1].double_or(0.0), a[2].double_or(0.0));
    let text = String::from_utf8_lossy(a[4].str_or(b"")).into_owned();
    let angle = a[3].double_or(0.0);
    dw.primitives.clear();
    if angle != 0.0 {
        dw.translate(x, y);
        dw.rotate(angle);
        dw.annotation(0.0, 0.0, &text);
    } else {
        dw.annotation(x, y, &text);
    }
    image_op(vm, &d, "Unable to annotate image", |i| dw.render_onto(i))
}

fn query_font_metrics(vm: &mut Vm, f: &Func, o: Option<&Obj>, args: &[Value]) -> R {
    let a = zpp::parse(vm, f, "zs|z!", args)?;
    let dw = draw_arg(vm, f, 1, &a[0])?;
    let text = String::from_utf8_lossy(a[1].str_or(b"")).into_owned();
    let multiline = match &a[2] {
        Arg::Any(v) if !matches!(v.deref(), Value::Null) => v.deref().to_bool(),
        _ => text.contains('\n'),
    };
    let _ = d_(o);
    let text = if multiline {
        text
    } else {
        text.replace('\n', " ")
    };
    let Some(m) = dw.font_metrics(&text) else {
        return convert_exception(vm, None, Cls::Imagick, "Failed to query the font metrics");
    };
    let mut arr = PhpArray::new();
    set(&mut arr, "characterWidth", Value::Double(m.character_width));
    set(
        &mut arr,
        "characterHeight",
        Value::Double(m.character_height),
    );
    set(&mut arr, "ascender", Value::Double(m.ascender));
    set(&mut arr, "descender", Value::Double(m.descender));
    set(&mut arr, "textWidth", Value::Double(m.text_width));
    set(&mut arr, "textHeight", Value::Double(m.text_height));
    set(
        &mut arr,
        "maxHorizontalAdvance",
        Value::Double(m.max_horizontal_advance),
    );
    let mut b = PhpArray::new();
    set(&mut b, "x1", Value::Double(m.bounds.0));
    set(&mut b, "y1", Value::Double(m.bounds.1));
    set(&mut b, "x2", Value::Double(m.bounds.2));
    set(&mut b, "y2", Value::Double(m.bounds.3));
    set(&mut arr, "boundingBox", arr_val(b));
    set(&mut arr, "originX", Value::Double(m.origin.0));
    set(&mut arr, "originY", Value::Double(m.origin.1));
    Ok(arr_val(arr))
}

// ---------------------------------------------------------------------------
// shared with imagick.rs

/// SetImageType(): convert the image to the type.
pub(crate) fn set_type_of(i: &mut Image, t: ImageType) {
    match t {
        ImageType::Bilevel => {
            transform_colorspace(i, Colorspace::Gray);
            for p in i.pixels.chunks_exact_mut(4) {
                let v = if p[0] < 32768 { 0 } else { 65535 };
                p[0] = v;
                p[1] = v;
                p[2] = v;
            }
            i.disable_alpha();
            i.depth = 1;
        }
        ImageType::Grayscale => {
            transform_colorspace(i, Colorspace::Gray);
            i.disable_alpha();
        }
        ImageType::GrayscaleAlpha => {
            transform_colorspace(i, Colorspace::Gray);
            i.enable_alpha();
        }
        ImageType::Palette => {
            let _ = quantize_of(i, 256, 0);
            i.disable_alpha();
        }
        ImageType::PaletteAlpha | ImageType::PaletteBilevelAlpha => {
            i.enable_alpha();
            let _ = quantize_of(i, 256, 0);
        }
        ImageType::TrueColor => {
            if i.colorspace != Colorspace::SRGB {
                transform_colorspace(i, Colorspace::SRGB);
            }
            i.disable_alpha();
            i.colors = None;
        }
        ImageType::TrueColorAlpha => {
            if i.colorspace != Colorspace::SRGB {
                transform_colorspace(i, Colorspace::SRGB);
            }
            i.enable_alpha();
            i.colors = None;
        }
        ImageType::ColorSeparation => {
            transform_colorspace(i, Colorspace::CMYK);
            i.disable_alpha();
        }
        ImageType::ColorSeparationAlpha => {
            transform_colorspace(i, Colorspace::CMYK);
            i.enable_alpha();
        }
        _ => {}
    }
    i.image_type = t;
}

/// SetImageAlphaChannel().
pub(crate) fn alpha_channel(i: &mut Image, op: AlphaChannelOption) {
    match op {
        AlphaChannelOption::Activate | AlphaChannelOption::On => {
            if !i.alpha {
                i.enable_alpha();
            }
        }
        AlphaChannelOption::Deactivate | AlphaChannelOption::Off => {
            i.alpha = false;
        }
        AlphaChannelOption::Set | AlphaChannelOption::Opaque => {
            i.enable_alpha();
            for p in i.pixels.chunks_exact_mut(4) {
                p[3] = 65535;
            }
        }
        AlphaChannelOption::Transparent => {
            i.enable_alpha();
            for p in i.pixels.chunks_exact_mut(4) {
                p[3] = 0;
            }
        }
        AlphaChannelOption::Extract => {
            let alpha = i.alpha;
            for p in i.pixels.chunks_exact_mut(4) {
                let a = if alpha { p[3] } else { 65535 };
                p.copy_from_slice(&[a, a, a, 65535]);
            }
            i.alpha = false;
            i.colorspace = Colorspace::Gray;
        }
        AlphaChannelOption::Copy => {
            for p in i.pixels.chunks_exact_mut(4) {
                p[3] = to_q16(Image::intensity(p));
            }
            i.alpha = true;
        }
        AlphaChannelOption::Shape => {
            let bg = i.background_color.to_rgba16();
            for p in i.pixels.chunks_exact_mut(4) {
                let a = to_q16(Image::intensity(p));
                p.copy_from_slice(&[bg[0], bg[1], bg[2], a]);
            }
            i.alpha = true;
        }
        AlphaChannelOption::Remove | AlphaChannelOption::Background => {
            goro_imagick::composite::flatten_alpha(i);
        }
        AlphaChannelOption::Associate => {
            for p in i.pixels.chunks_exact_mut(4) {
                let a = p[3] as f64 / 65535.0;
                for c in 0..3 {
                    p[c] = to_q16(p[c] as f64 * a);
                }
            }
        }
        AlphaChannelOption::Disassociate => {
            for p in i.pixels.chunks_exact_mut(4) {
                let a = p[3] as f64 / 65535.0;
                if a > 0.0 {
                    for c in 0..3 {
                        p[c] = to_q16(p[c] as f64 / a);
                    }
                }
            }
        }
        AlphaChannelOption::OffIfOpaque => {
            if i.is_opaque() {
                i.alpha = false;
            }
        }
        _ => {}
    }
    i.colors = None;
}
