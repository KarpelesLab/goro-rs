//! Geometry operations: crop, flips and rotations, borders and frames,
//! extent, chop/splice/shave, roll, trim, scaling/sampling/resizing,
//! appending and layer flattening.

use crate::color::{PixelInfo, clamp_to_quantum};
use crate::composite::{ComposeArgs, composite_image};
use crate::error::{MResult, MagickError, severity};
use crate::geometry::Rect;
use crate::image::Image;
use crate::types::*;

/// CropImage(): the part of the image in `r` (in page coordinates).
/// Off the image: a 1x1 transparent image and a warning.
pub fn crop(img: &Image, r: Rect) -> (Image, Option<MagickError>) {
    let px = r.x - img.page.x;
    let py = r.y - img.page.y;
    let (w, h) = (
        if r.width == 0 { img.columns } else { r.width },
        if r.height == 0 { img.rows } else { r.height },
    );
    let x0 = px.max(0);
    let y0 = py.max(0);
    let x1 = (px + w as i64).min(img.columns as i64);
    let y1 = (py + h as i64).min(img.rows as i64);
    if x1 <= x0 || y1 <= y0 {
        let mut out = img.clone_attributes(1, 1);
        out.enable_alpha();
        out.set_px(0, 0, [0, 0, 0, 0]);
        out.page = Rect::new(img.page.width, img.page.height, -1, -1);
        let warn = MagickError::with_desc(
            severity::OPTION_WARNING,
            &format!(
                "geometry does not contain image (\"{}x{}{:+}{:+}\")",
                r.width, r.height, r.x, r.y
            ),
            &img.filename,
            "warning/transform.c/CropImage/599",
        );
        return (out, Some(warn));
    }
    let cr = Rect::new((x1 - x0) as usize, (y1 - y0) as usize, x0, y0);
    let mut out = img.crop_raw(cr);
    out.page = Rect::new(
        if img.page.width > 0 {
            img.page.width
        } else {
            img.columns
        },
        if img.page.height > 0 {
            img.page.height
        } else {
            img.rows
        },
        x0 + img.page.x,
        y0 + img.page.y,
    );
    out.colors = None;
    (out, None)
}

fn remap(img: &Image, w: usize, h: usize, f: impl Fn(usize, usize) -> (usize, usize)) -> Image {
    let mut out = img.clone_attributes(w, h);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = f(x, y);
            out.set_px(x, y, img.px(sx, sy));
        }
    }
    if let (Some(k), Some(ok)) = (&img.black, &mut out.black) {
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = f(x, y);
                ok[y * w + x] = k[sy * img.columns + sx];
            }
        }
    }
    out
}

pub fn flip(img: &Image) -> Image {
    let h = img.rows;
    let mut o = remap(img, img.columns, h, |x, y| (x, h - 1 - y));
    o.page.y = if o.page.height > 0 {
        o.page.height as i64 - h as i64 - img.page.y
    } else {
        o.page.y
    };
    o
}

pub fn flop(img: &Image) -> Image {
    let w = img.columns;
    let mut o = remap(img, w, img.rows, |x, y| (w - 1 - x, y));
    o.page.x = if o.page.width > 0 {
        o.page.width as i64 - w as i64 - img.page.x
    } else {
        o.page.x
    };
    o
}

pub fn transpose(img: &Image) -> Image {
    let mut o = remap(img, img.rows, img.columns, |x, y| (y, x));
    o.page = Rect::new(img.page.height, img.page.width, img.page.y, img.page.x);
    o
}

pub fn transverse(img: &Image) -> Image {
    let (w, h) = (img.columns, img.rows);
    let mut o = remap(img, h, w, |x, y| (w - 1 - y, h - 1 - x));
    o.page = Rect::new(img.page.height, img.page.width, 0, 0);
    o
}

pub(crate) fn rotate90(img: &Image, quarters: i32) -> Image {
    let (w, h) = (img.columns, img.rows);
    let mut o = match quarters.rem_euclid(4) {
        0 => img.clone(),
        1 => remap(img, h, w, |x, y| (y, h - 1 - x)),
        2 => remap(img, w, h, |x, y| (w - 1 - x, h - 1 - y)),
        _ => remap(img, h, w, |x, y| (w - 1 - y, x)),
    };
    let (pw, ph) = (o.columns, o.rows);
    o.page = Rect::new(pw, ph, 0, 0);
    o
}

/// RotateImage(): rotate by `degrees` clockwise; the corners show the
/// background colour.
pub fn rotate(img: &Image, degrees: f64, background: &PixelInfo) -> Image {
    let mut deg = degrees % 360.0;
    if deg < 0.0 {
        deg += 360.0;
    }
    if (deg % 90.0).abs() < 1e-9 {
        return rotate90(img, (deg / 90.0).round() as i32);
    }
    crate::shear::rotate(img, deg, background)
}

/// A rotation by any angle through the general path (the canvas grows by
/// two pixels even for 0°, as DeskewImage() shows).
pub fn rotate_free(img: &Image, deg: f64, background: &PixelInfo) -> Image {
    let th = deg.to_radians();
    let (c, s) = (th.cos(), th.sin());
    let (w, h) = (img.columns as f64, img.rows as f64);
    let ow = ((w * c.abs() + h * s.abs()).round() + 2.0) as usize;
    let oh = ((w * s.abs() + h * c.abs()).round() + 2.0) as usize;
    let mut out = img.clone_attributes(ow, oh);
    let bg = background.to_rgba16();
    let bg_alpha = background.alpha_trait && background.alpha < 65535.0;
    if bg_alpha {
        out.alpha = true;
    }
    let mut src = img.clone();
    src.cmyk_to_rgb();
    out.colorspace = src.colorspace;
    out.black = None;
    src.virtual_pixel = VirtualPixelMethod::Transparent;
    let (ocx, ocy) = (ow as f64 / 2.0, oh as f64 / 2.0);
    let (icx, icy) = (w / 2.0, h / 2.0);
    for y in 0..oh {
        for x in 0..ow {
            let dx = x as f64 + 0.5 - ocx;
            let dy = y as f64 + 0.5 - ocy;
            // inverse rotation
            let sx = dx * c + dy * s + icx;
            let sy = -dx * s + dy * c + icy;
            let p = if sx < -1.0 || sy < -1.0 || sx > w + 1.0 || sy > h + 1.0 {
                [0.0, 0.0, 0.0, 0.0]
            } else {
                src.sample_bilinear(sx, sy)
            };
            // over the background
            let a = p[3] / 65535.0;
            let ba = bg[3] as f64 / 65535.0;
            let ao = a + ba * (1.0 - a);
            let mut o = [0u16; 4];
            for ch in 0..3 {
                let v = if ao > 0.0 {
                    (p[ch] * a + bg[ch] as f64 * ba * (1.0 - a)) / ao
                } else {
                    0.0
                };
                o[ch] = clamp_to_quantum(v) as u16;
            }
            o[3] = clamp_to_quantum(ao * 65535.0) as u16;
            out.set_px(x, y, o);
        }
    }
    if src.alpha {
        out.alpha = true;
    }
    out.page = Rect::new(
        ow,
        oh,
        img.page.x - (ow as i64 - img.columns as i64) / 2,
        img.page.y - (oh as i64 - img.rows as i64) / 2,
    );
    out.colors = None;
    out
}

/// RollImage(): offset the image with wrap-around.
pub fn roll(img: &Image, dx: i64, dy: i64) -> Image {
    let (w, h) = (img.columns as i64, img.rows as i64);
    remap(img, img.columns, img.rows, |x, y| {
        (
            (x as i64 - dx).rem_euclid(w) as usize,
            (y as i64 - dy).rem_euclid(h) as usize,
        )
    })
}

/// ExtentImage(): a `w`x`h` canvas of the background colour with the
/// image at (-x, -y).
pub fn extent(img: &Image, r: Rect) -> Image {
    let mut out = img.clone_attributes(r.width, r.height);
    let bg = img.background_color.clone();
    out.fill(&bg);
    if bg.alpha_trait && bg.alpha < 65535.0 {
        out.enable_alpha();
        out.fill(&bg);
    }
    let op = if img.alpha && out.alpha {
        CompositeOp::Copy
    } else {
        CompositeOp::Over
    };
    composite_image(&mut out, img, op, -r.x, -r.y, &ComposeArgs::default());
    out.page = Rect::new(r.width, r.height, 0, 0);
    out.colors = None;
    out
}

/// BorderImage().
pub fn border(img: &Image, bw: usize, bh: usize, color: &PixelInfo, compose: CompositeOp) -> Image {
    let (w, h) = (img.columns + 2 * bw, img.rows + 2 * bh);
    let mut out = img.clone_attributes(w, h);
    if color.alpha_trait && color.alpha < 65535.0 {
        out.enable_alpha();
    }
    out.fill(color);
    // the image area: the image composed over the border colour
    let mut inner = Image::new(img.columns, img.rows, color);
    inner.alpha = out.alpha;
    composite_image(&mut inner, img, compose, 0, 0, &ComposeArgs::default());
    composite_image(
        &mut out,
        &inner,
        CompositeOp::Copy,
        bw as i64,
        bh as i64,
        &ComposeArgs::default(),
    );
    out.page = Rect::new(w, h, 0, 0);
    out.colors = None;
    out
}

fn shade(c: &PixelInfo, f: f64) -> [u16; 4] {
    let (r, g, b) = c.rgb_q();
    let q = |v: f64| {
        if f >= 0.0 {
            clamp_to_quantum(v + (65535.0 - v) * f) as u16
        } else {
            clamp_to_quantum(v * (1.0 + f)) as u16
        }
    };
    [q(r), q(g), q(b), crate::color::to_q16(c.alpha)]
}

/// FrameImage(): a bevelled frame of the matte colour.
pub fn frame(
    img: &Image,
    fw: usize,
    fh: usize,
    inner: usize,
    outer: usize,
    matte: &PixelInfo,
    compose: CompositeOp,
) -> Image {
    let (w, h) = (img.columns + 2 * fw, img.rows + 2 * fh);
    let mut out = img.clone_attributes(w, h);
    let flat = matte.to_rgba16();
    let highlight = shade(matte, 0.5);
    let shadow = shade(matte, -0.5);
    let accent = shade(matte, 0.25);
    for y in 0..h {
        for x in 0..w {
            out.set_px(x, y, flat);
        }
    }
    let wi = w as i64;
    let hi = h as i64;
    // outer bevel: highlight top/left, shadow bottom/right
    for y in 0..hi {
        for x in 0..wi {
            let d_left = x;
            let d_top = y;
            let d_right = wi - 1 - x;
            let d_bottom = hi - 1 - y;
            let o = outer as i64;
            let mut c = None;
            if d_top < o || d_left < o {
                c = Some(
                    if d_top < o && d_right < d_top || d_left < o && d_bottom < d_left {
                        shadow
                    } else {
                        highlight
                    },
                );
            }
            if d_bottom < o || d_right < o {
                c = Some(
                    if d_bottom < o && d_left < d_bottom || d_right < o && d_top < d_right {
                        highlight
                    } else {
                        shadow
                    },
                );
            }
            // inner bevel: around the image, reversed
            let ix0 = fw as i64 - inner as i64;
            let iy0 = fh as i64 - inner as i64;
            let ix1 = fw as i64 + img.columns as i64 + inner as i64;
            let iy1 = fh as i64 + img.rows as i64 + inner as i64;
            if inner > 0 && x >= ix0 && x < ix1 && y >= iy0 && y < iy1 {
                let in_img = x >= fw as i64
                    && x < fw as i64 + img.columns as i64
                    && y >= fh as i64
                    && y < fh as i64 + img.rows as i64;
                if !in_img {
                    let top = y < fh as i64;
                    let left = x < fw as i64;
                    c = Some(if top || left { shadow } else { accent });
                }
            }
            if let Some(c) = c {
                out.set_px(x as usize, y as usize, c);
            }
        }
    }
    let mut inner_img = Image::new(img.columns, img.rows, matte);
    composite_image(&mut inner_img, img, compose, 0, 0, &ComposeArgs::default());
    composite_image(
        &mut out,
        &inner_img,
        CompositeOp::Copy,
        fw as i64,
        fh as i64,
        &ComposeArgs::default(),
    );
    out.page = Rect::new(w, h, 0, 0);
    out.colors = None;
    out
}

/// ShaveImage().
pub fn shave(img: &Image, sw: usize, sh: usize) -> MResult<Image> {
    if 2 * sw >= img.columns || 2 * sh >= img.rows {
        return Err(MagickError::with_desc(
            severity::OPTION_WARNING,
            "geometry does not contain image",
            &img.filename,
            "warning/transform.c/ShaveImage",
        ));
    }
    let r = Rect::new(
        img.columns - 2 * sw,
        img.rows - 2 * sh,
        sw as i64,
        sh as i64,
    );
    let mut out = img.crop_raw(r);
    out.page = Rect::new(r.width, r.height, 0, 0);
    Ok(out)
}

/// ChopImage(): remove the rows and columns of a region.
pub fn chop(img: &Image, r: Rect) -> MResult<Image> {
    let x0 = r.x.max(0) as usize;
    let y0 = r.y.max(0) as usize;
    if x0 >= img.columns || y0 >= img.rows {
        return Err(MagickError::with_desc(
            severity::OPTION_WARNING,
            "geometry does not contain image",
            &img.filename,
            "warning/transform.c/ChopImage",
        ));
    }
    let cw = r.width.min(img.columns - x0);
    let ch = r.height.min(img.rows - y0);
    let (w, h) = (img.columns - cw, img.rows - ch);
    let mut out = remap(img, w, h, |x, y| {
        (
            if x < x0 { x } else { x + cw },
            if y < y0 { y } else { y + ch },
        )
    });
    out.page = Rect::new(w, h, 0, 0);
    Ok(out)
}

/// SpliceImage(): insert rows and columns of the background colour.
pub fn splice(img: &Image, r: Rect, gravity: Gravity) -> Image {
    let (w, h) = (img.columns + r.width, img.rows + r.height);
    let (gx, gy) = crate::geometry::gravity_adjust(
        img.columns,
        img.rows,
        gravity,
        r.width,
        r.height,
        r.x,
        r.y,
    );
    let x0 = gx.clamp(0, img.columns as i64) as usize;
    let y0 = gy.clamp(0, img.rows as i64) as usize;
    let mut out = img.clone_attributes(w, h);
    let bg = img.background_color.to_rgba16();
    for y in 0..h {
        for x in 0..w {
            let inx = x < x0 || x >= x0 + r.width;
            let iny = y < y0 || y >= y0 + r.height;
            let p = if inx && iny {
                let sx = if x < x0 { x } else { x - r.width };
                let sy = if y < y0 { y } else { y - r.height };
                img.px(sx, sy)
            } else {
                bg
            };
            out.set_px(x, y, p);
        }
    }
    out.page = Rect::new(w, h, 0, 0);
    out.colors = None;
    out
}

fn color_dist2(a: [u16; 4], b: [u16; 4], alpha: bool) -> f64 {
    let mut d = 0.0;
    for c in 0..3 {
        let x = a[c] as f64 - b[c] as f64;
        d += x * x;
    }
    if alpha {
        let x = a[3] as f64 - b[3] as f64;
        d += x * x;
    }
    d
}

/// GetImageBoundingBox(): the box of the pixels that differ (more than
/// `fuzz`, in quantum units) from the corner colour.
pub fn bounding_box(img: &Image, fuzz: f64) -> Option<Rect> {
    if img.columns == 0 || img.rows == 0 {
        return None;
    }
    let target = img.px(0, 0);
    let f2 = (fuzz * fuzz).max(0.0);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    let mut any = false;
    for y in 0..img.rows {
        for x in 0..img.columns {
            if color_dist2(img.px(x, y), target, img.alpha) > f2 {
                any = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if !any {
        return None;
    }
    Some(Rect::new(x1 - x0 + 1, y1 - y0 + 1, x0 as i64, y0 as i64))
}

/// TrimImage().
pub fn trim(img: &Image, fuzz: f64) -> Image {
    match bounding_box(img, fuzz) {
        Some(bb) => {
            let mut out = img.crop_raw(bb);
            out.page = Rect::new(
                if img.page.width > 0 {
                    img.page.width
                } else {
                    img.columns
                },
                if img.page.height > 0 {
                    img.page.height
                } else {
                    img.rows
                },
                bb.x + img.page.x,
                bb.y + img.page.y,
            );
            out.colors = None;
            out
        }
        None => {
            let mut out = img.clone_attributes(1, 1);
            out.enable_alpha();
            out.set_px(0, 0, [0, 0, 0, 0]);
            out.page = Rect::new(img.columns, img.rows, -1, -1);
            out
        }
    }
}

/// SampleImage(): point sampling at pixel centres.
pub fn sample(img: &Image, w: usize, h: usize) -> Image {
    let (sw, sh) = (img.columns, img.rows);
    let mut out = remap(img, w, h, |x, y| {
        (
            (((x as f64 + 0.5) * sw as f64 / w as f64) as usize).min(sw - 1),
            (((y as f64 + 0.5) * sh as f64 / h as f64) as usize).min(sh - 1),
        )
    });
    out.page = Rect::new(w, h, 0, 0);
    out.colors = None;
    out
}

/// ScaleImage(): box averaging (shrink) / replication (enlarge).
pub fn scale(img: &Image, w: usize, h: usize) -> Image {
    let (sw, sh) = (img.columns as f64, img.rows as f64);
    let fx = sw / w as f64;
    let fy = sh / h as f64;
    let mut out = img.clone_attributes(w, h);
    for y in 0..h {
        let y0 = y as f64 * fy;
        let y1 = (y as f64 + 1.0) * fy;
        for x in 0..w {
            let x0 = x as f64 * fx;
            let x1 = (x as f64 + 1.0) * fx;
            let mut acc = [0f64; 4];
            let mut wsum = 0.0;
            let mut asum = 0.0;
            let mut yy = y0.floor() as usize;
            while (yy as f64) < y1 && yy < img.rows {
                let wy = (y1.min(yy as f64 + 1.0) - y0.max(yy as f64)).max(0.0);
                let mut xx = x0.floor() as usize;
                while (xx as f64) < x1 && xx < img.columns {
                    let wx = (x1.min(xx as f64 + 1.0) - x0.max(xx as f64)).max(0.0);
                    let wt = wx * wy;
                    let p = img.px(xx, yy);
                    let a = if img.alpha {
                        p[3] as f64 / 65535.0
                    } else {
                        1.0
                    };
                    for c in 0..3 {
                        acc[c] += wt * a * p[c] as f64;
                    }
                    acc[3] += wt * p[3] as f64;
                    asum += wt * a;
                    wsum += wt;
                    xx += 1;
                }
                yy += 1;
            }
            let mut o = [0u16; 4];
            for c in 0..3 {
                o[c] = if asum > 0.0 {
                    clamp_to_quantum(acc[c] / asum) as u16
                } else {
                    0
                };
            }
            o[3] = if wsum > 0.0 {
                clamp_to_quantum(acc[3] / wsum) as u16
            } else {
                65535
            };
            out.set_px(x, y, o);
        }
    }
    out.black = None;
    if img.colorspace == crate::types::Colorspace::CMYK {
        out.black = Some(vec![0; w * h]);
        if let (Some(k), Some(ok)) = (&img.black, &mut out.black) {
            for y in 0..h {
                for x in 0..w {
                    let sx = ((x as f64 + 0.5) * fx) as usize;
                    let sy = ((y as f64 + 0.5) * fy) as usize;
                    ok[y * w + x] = k[sy.min(img.rows - 1) * img.columns + sx.min(img.columns - 1)];
                }
            }
        }
    }
    out.page = Rect::new(w, h, 0, 0);
    out.colors = None;
    out
}

/// ResizeImage() with ImageMagick's filters (see resize.rs).
pub fn resize(img: &Image, w: usize, h: usize, filter: FilterType) -> MResult<Image> {
    if w == img.columns && h == img.rows {
        return Ok(img.clone());
    }
    if w == 0 || h == 0 {
        return Err(MagickError::with_desc(
            severity::OPTION_ERROR,
            "NegativeOrZeroImageSize",
            "",
            "error/resize.c/ResizeImage",
        ));
    }
    let shrink = w * h < img.columns * img.rows;
    let f = crate::resize::Filter::new(filter, shrink && !img.alpha);
    let mut out = img.clone_attributes(w, h);
    out.pixels = crate::resize::resize_pixels(img, w, h, &f);
    out.alpha = img.alpha;
    if let Some(k) = &img.black {
        // the black channel through the same filter
        let mut kimg = Image::blank(img.columns, img.rows);
        for (p, &v) in kimg.pixels.chunks_exact_mut(4).zip(k.iter()) {
            p[0] = v;
        }
        let kp = crate::resize::resize_pixels(&kimg, w, h, &f);
        out.black = Some(kp.chunks_exact(4).map(|p| p[0]).collect());
    } else {
        out.black = None;
    }
    let sx = w as f64 / img.columns as f64;
    let sy = h as f64 / img.rows as f64;
    out.page = Rect::new(
        (img.page.width as f64 * sx).round() as usize,
        (img.page.height as f64 * sy).round() as usize,
        (img.page.x as f64 * sx).round() as i64,
        (img.page.y as f64 * sy).round() as i64,
    );
    if out.page.width == 0 {
        out.page = Rect::new(w, h, 0, 0);
    }
    out.colors = None;
    Ok(out)
}

/// AppendImages(): side by side (or stacked when `stack`).
pub fn append(images: &[&Image], stack: bool, background: &PixelInfo, gravity: Gravity) -> Image {
    let (w, h) = if stack {
        (
            images.iter().map(|i| i.columns).max().unwrap_or(0),
            images.iter().map(|i| i.rows).sum(),
        )
    } else {
        (
            images.iter().map(|i| i.columns).sum(),
            images.iter().map(|i| i.rows).max().unwrap_or(0),
        )
    };
    let mut out = images[0].clone_attributes(w, h);
    out.alpha = images.iter().any(|i| i.alpha);
    out.fill(background);
    if out.alpha && !(background.alpha_trait && background.alpha < 65535.0) {
        for p in out.pixels.chunks_exact_mut(4) {
            p[3] = crate::color::to_q16(background.alpha);
        }
    }
    let mut off = 0i64;
    for img in images {
        let (x, y) = if stack {
            let x = match gravity {
                Gravity::North | Gravity::Center | Gravity::South => (w - img.columns) as i64 / 2,
                Gravity::NorthEast | Gravity::East | Gravity::SouthEast => (w - img.columns) as i64,
                _ => 0,
            };
            (x, off)
        } else {
            let y = match gravity {
                Gravity::West | Gravity::Center | Gravity::East => (h - img.rows) as i64 / 2,
                Gravity::SouthWest | Gravity::South | Gravity::SouthEast => (h - img.rows) as i64,
                _ => 0,
            };
            (off, y)
        };
        composite_image(
            &mut out,
            img,
            CompositeOp::Copy,
            x,
            y,
            &ComposeArgs::default(),
        );
        off += if stack { img.rows } else { img.columns } as i64;
    }
    out.page = Rect::new(w, h, 0, 0);
    out.colors = None;
    out
}

/// MergeImageLayers(): flatten / mosaic / merge onto a canvas.
pub fn merge_layers(images: &[&Image], method: i32, background: &PixelInfo) -> Image {
    // 13 merge, 14 flatten, 15 mosaic, 16 trim-bounds
    let first = images[0];
    let (mut x0, mut y0, mut x1, mut y1);
    let (pw, ph) = (
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
    match method {
        14 => {
            x0 = 0;
            y0 = 0;
            x1 = pw as i64;
            y1 = ph as i64;
        }
        15 => {
            x0 = 0;
            y0 = 0;
            x1 = pw as i64;
            y1 = ph as i64;
            for i in images {
                x1 = x1.max(i.page.x + i.columns as i64);
                y1 = y1.max(i.page.y + i.rows as i64);
            }
        }
        _ => {
            x0 = i64::MAX;
            y0 = i64::MAX;
            x1 = i64::MIN;
            y1 = i64::MIN;
            for i in images {
                x0 = x0.min(i.page.x);
                y0 = y0.min(i.page.y);
                x1 = x1.max(i.page.x + i.columns as i64);
                y1 = y1.max(i.page.y + i.rows as i64);
            }
        }
    }
    let (w, h) = ((x1 - x0).max(1) as usize, (y1 - y0).max(1) as usize);
    let mut out = first.clone_attributes(w, h);
    out.alpha = images.iter().any(|i| i.alpha) || background.alpha_trait;
    out.fill(background);
    if !out.alpha {
        for p in out.pixels.chunks_exact_mut(4) {
            p[3] = 65535;
        }
    }
    for img in images {
        let op = if img.compose == CompositeOp::Undefined {
            CompositeOp::Over
        } else {
            img.compose
        };
        composite_image(
            &mut out,
            img,
            op,
            img.page.x - x0,
            img.page.y - y0,
            &ComposeArgs::default(),
        );
    }
    out.page = if method == 13 || method == 16 {
        Rect::new(pw, ph, x0, y0)
    } else {
        Rect::new(w, h, 0, 0)
    };
    out.colors = None;
    out
}
