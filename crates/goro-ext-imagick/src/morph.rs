//! MorphologyImage() on kernel lists, as ImageMagick 7.1.2 behaves
//! black-box:
//!
//! - Convolve uses the kernel reflected through its origin, Correlate as
//!   given; neither normalises.
//! - Erode takes the minimum over the elements of at least 0.5 (the
//!   origin pixel always counts); Dilate the maximum over the reflected
//!   elements above 0.5. The intensity forms move whole pixels by their
//!   Rec. 709 luma (first wins on ties), the dilation reflected.
//! - Hit-and-miss: the minimum over the 1s less the maximum over the 0s,
//!   the union (lighten) over a kernel list.
//! - Thinning / thicken subtract / add each kernel's hit-and-miss in turn,
//!   the whole list once per iteration.
//! - Distance runs a forward and a backward raster pass in place per
//!   iteration.
//! - Open/close repeat the erosion then the dilation `iterations` times
//!   each, kernel after kernel; closings use the kernel turned half a turn. The edge and hat forms take the
//!   difference between the operation on the running result and the
//!   original image (Edge: dilation of the running result against the
//!   erosion of the original).
//! - A negative iteration count repeats until nothing changes.

use goro_imagick::filters::channel_mask;
use goro_imagick::image::Image;

use crate::kernel::Kernel;

#[derive(Clone, Copy, PartialEq)]
enum Prim {
    Convolve,
    Correlate,
    Erode,
    Dilate,
    ErodeI,
    DilateI,
}

struct Pad {
    data: Vec<f64>,
    pw: usize,
    px: usize,
    py: usize,
}

fn padded(img: &Image, px: usize, py: usize) -> Pad {
    let (w, h) = (img.columns, img.rows);
    let pw = w + 2 * px;
    let ph = h + 2 * py;
    let mut data = vec![0f64; pw * ph * 4];
    for y in 0..ph {
        for x in 0..pw {
            let p = img.virtual_px(x as i64 - px as i64, y as i64 - py as i64);
            let o = (y * pw + x) * 4;
            for c in 0..4 {
                data[o + c] = p[c] as f64;
            }
        }
    }
    Pad { data, pw, px, py }
}

impl Pad {
    /// Sample index of image pixel (x, y) offset by (dx, dy).
    fn at(&self, x: usize, y: usize, dx: i64, dy: i64) -> usize {
        let xx = (x + self.px) as i64 + dx;
        let yy = (y + self.py) as i64 + dy;
        (yy as usize * self.pw + xx as usize) * 4
    }
}

fn luma(p: &[f64]) -> f64 {
    0.212656 * p[0] + 0.715158 * p[1] + 0.072186 * p[2]
}

fn q(v: f64) -> u16 {
    goro_imagick::color::clamp_to_quantum(v) as u16
}

/// One primitive pass of one kernel.
fn prim(img: &Image, k: &Kernel, op: Prim, mask: [bool; 4]) -> Image {
    let (w, h) = (img.columns, img.rows);
    let pad = padded(img, k.width.max(k.x * 2 + 1), k.height.max(k.y * 2 + 1));
    // element offsets relative to the origin, in sampling direction
    let reflect = matches!(op, Prim::Convolve | Prim::Dilate | Prim::DilateI);
    let mut elems: Vec<(i64, i64, f64)> = Vec::new();
    for ky in 0..k.height {
        for kx in 0..k.width {
            let v = k.values[ky * k.width + kx];
            if v.is_nan() {
                continue;
            }
            let keep = match op {
                Prim::Convolve | Prim::Correlate => v != 0.0,
                Prim::Erode | Prim::ErodeI | Prim::DilateI => v >= 0.5,
                Prim::Dilate => v > 0.5,
            };
            if !keep {
                continue;
            }
            let (dx, dy) = (kx as i64 - k.x as i64, ky as i64 - k.y as i64);
            if reflect {
                elems.push((-dx, -dy, v));
            } else {
                elems.push((dx, dy, v));
            }
        }
    }
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            match op {
                Prim::Convolve | Prim::Correlate => {
                    let mut acc = [0f64; 4];
                    for &(dx, dy, v) in &elems {
                        let o = pad.at(x, y, dx, dy);
                        for c in 0..4 {
                            acc[c] += v * pad.data[o + c];
                        }
                    }
                    for c in 0..4 {
                        if mask[c] {
                            out.pixels[idx + c] = q(acc[c]);
                        }
                    }
                }
                Prim::Erode | Prim::Dilate => {
                    let erode = op == Prim::Erode;
                    let mut best = if erode {
                        let o = pad.at(x, y, 0, 0);
                        [
                            pad.data[o],
                            pad.data[o + 1],
                            pad.data[o + 2],
                            pad.data[o + 3],
                        ]
                    } else {
                        [f64::NEG_INFINITY; 4]
                    };
                    for &(dx, dy, _) in &elems {
                        let o = pad.at(x, y, dx, dy);
                        for c in 0..4 {
                            let s = pad.data[o + c];
                            best[c] = if erode {
                                best[c].min(s)
                            } else {
                                best[c].max(s)
                            };
                        }
                    }
                    for c in 0..4 {
                        if mask[c] && best[c].is_finite() {
                            out.pixels[idx + c] = q(best[c]);
                        }
                    }
                }
                Prim::ErodeI | Prim::DilateI => {
                    let erode = op == Prim::ErodeI;
                    let mut best: Option<(f64, usize)> = None;
                    for &(dx, dy, _) in &elems {
                        let o = pad.at(x, y, dx, dy);
                        let l = luma(&pad.data[o..o + 3]);
                        let better = match best {
                            None => true,
                            Some((bl, _)) => {
                                if erode {
                                    l < bl
                                } else {
                                    l > bl
                                }
                            }
                        };
                        if better {
                            best = Some((l, o));
                        }
                    }
                    if let Some((_, o)) = best {
                        for c in 0..4 {
                            if mask[c] {
                                out.pixels[idx + c] = q(pad.data[o + c]);
                            }
                        }
                    }
                }
            }
        }
    }
    out.colors = None;
    out
}

/// The hit-and-miss of one kernel.
fn hit_and_miss(img: &Image, k: &Kernel, mask: [bool; 4]) -> Image {
    let (w, h) = (img.columns, img.rows);
    let pad = padded(img, k.width, k.height);
    let mut fg: Vec<(i64, i64)> = Vec::new();
    let mut bg: Vec<(i64, i64)> = Vec::new();
    for ky in 0..k.height {
        for kx in 0..k.width {
            let v = k.values[ky * k.width + kx];
            if v.is_nan() {
                continue;
            }
            let d = (kx as i64 - k.x as i64, ky as i64 - k.y as i64);
            if v >= 0.5 { fg.push(d) } else { bg.push(d) }
        }
    }
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            for c in 0..4 {
                if !mask[c] {
                    continue;
                }
                let mut lo = 65535f64;
                for &(dx, dy) in &fg {
                    lo = lo.min(pad.data[pad.at(x, y, dx, dy) + c]);
                }
                let mut hi = 0f64;
                for &(dx, dy) in &bg {
                    hi = hi.max(pad.data[pad.at(x, y, dx, dy) + c]);
                }
                out.pixels[idx + c] = q(lo - hi);
            }
        }
    }
    out.colors = None;
    out
}

/// Distance: a forward then a backward raster pass, in place.
fn distance_pass(img: &mut Image, k: &Kernel, mask: [bool; 4]) {
    let (w, h) = (img.columns as i64, img.rows as i64);
    if w == 0 || h == 0 {
        return;
    }
    let mut fwd: Vec<(i64, i64, f64)> = Vec::new();
    let mut bwd: Vec<(i64, i64, f64)> = Vec::new();
    for ky in 0..k.height {
        for kx in 0..k.width {
            let v = k.values[ky * k.width + kx];
            if v.is_nan() {
                continue;
            }
            let (dx, dy) = (kx as i64 - k.x as i64, ky as i64 - k.y as i64);
            if dy < 0 || (dy == 0 && dx <= 0) {
                fwd.push((dx, dy, v));
            }
            if dy > 0 || (dy == 0 && dx >= 0) {
                bwd.push((dx, dy, v));
            }
        }
    }
    let at = |x: i64, y: i64| ((y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) * 4) as usize;
    for c in 0..4 {
        if !mask[c] {
            continue;
        }
        for y in 0..h {
            for x in 0..w {
                let mut m = img.pixels[at(x, y) + c] as f64;
                for &(dx, dy, v) in &fwd {
                    m = m.min(img.pixels[at(x + dx, y + dy) + c] as f64 + v);
                }
                img.pixels[at(x, y) + c] = q(m);
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let mut m = img.pixels[at(x, y) + c] as f64;
                for &(dx, dy, v) in &bwd {
                    m = m.min(img.pixels[at(x + dx, y + dy) + c] as f64 + v);
                }
                img.pixels[at(x, y) + c] = q(m);
            }
        }
    }
    img.colors = None;
}

/// One parallel pass of min(p + k) over every element.
fn min_plus(img: &Image, k: &Kernel, mask: [bool; 4]) -> Image {
    let (w, h) = (img.columns, img.rows);
    let pad = padded(img, k.width, k.height);
    let mut elems = Vec::new();
    for ky in 0..k.height {
        for kx in 0..k.width {
            let v = k.values[ky * k.width + kx];
            if !v.is_nan() {
                elems.push((kx as i64 - k.x as i64, ky as i64 - k.y as i64, v));
            }
        }
    }
    let mut out = img.clone();
    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) * 4;
            for c in 0..4 {
                if !mask[c] {
                    continue;
                }
                let mut m = f64::INFINITY;
                for &(dx, dy, v) in &elems {
                    m = m.min(pad.data[pad.at(x, y, dx, dy) + c] + v);
                }
                if m.is_finite() {
                    out.pixels[idx + c] = q(m);
                }
            }
        }
    }
    out
}

/// |a - b| on the selected channels.
fn difference(a: &Image, b: &Image, mask: [bool; 4]) -> Image {
    let mut out = a.clone();
    for (i, (p, q)) in out.pixels.iter_mut().zip(&b.pixels).enumerate() {
        if mask[i % 4] {
            *p = p.abs_diff(*q);
        }
    }
    out
}

/// The kernel turned half a turn about its origin (closings use it).
fn reflected(k: &Kernel) -> Kernel {
    let mut values = k.values.clone();
    values.reverse();
    Kernel {
        width: k.width,
        height: k.height,
        x: k.width - 1 - k.x,
        y: k.height - 1 - k.y,
        values,
        user: false,
    }
}

/// A closing: dilate then erode `n` times each, with the reflected kernel.
fn close(p: &Image, k: &Kernel, er: Prim, di: Prim, n: usize, mask: [bool; 4]) -> Image {
    let r = reflected(k);
    repeat(&repeat(p, &r, di, n, mask), &r, er, n, mask)
}

fn repeat(img: &Image, k: &Kernel, op: Prim, n: usize, mask: [bool; 4]) -> Image {
    let mut p = img.clone();
    for _ in 0..n {
        p = prim(&p, k, op, mask);
    }
    p
}

/// MorphologyImage(method, iterations, kernels) on `img`.
pub(crate) fn morphology(img: &mut Image, method: i64, iterations: i64, ks: &[Kernel], ch: i64) {
    if img.columns == 0 || img.rows == 0 || ks.is_empty() {
        return;
    }
    let mask = channel_mask(ch, img.alpha);
    let until_stable = iterations < 0;
    let limit = if until_stable {
        (img.columns + img.rows).max(1)
    } else {
        iterations as usize
    };
    if limit == 0 {
        return;
    }
    let n = if until_stable { 1 } else { limit };
    let orig = img.clone();
    let intensity = matches!(method, 5 | 6 | 10 | 11);
    let (er, di) = if intensity {
        (Prim::ErodeI, Prim::DilateI)
    } else {
        (Prim::Erode, Prim::Dilate)
    };
    // iterate a whole-list step until stable or `limit` times
    let iterate = |p: &mut Image, step: &dyn Fn(&Image) -> Image| {
        for _ in 0..limit {
            let next = step(p);
            let same = next.pixels == p.pixels;
            *p = next;
            if until_stable && same {
                break;
            }
        }
    };
    let mut p = img.clone();
    match method {
        1..=6 => {
            let op = match method {
                1 => Prim::Convolve,
                2 => Prim::Correlate,
                3 => Prim::Erode,
                4 => Prim::Dilate,
                5 => Prim::ErodeI,
                _ => Prim::DilateI,
            };
            // each kernel its iterations in turn
            for k in ks {
                iterate(&mut p, &|src: &Image| prim(src, k, op, mask));
            }
        }
        8 | 10 => {
            for k in ks {
                p = repeat(&repeat(&p, k, er, n, mask), k, di, n, mask);
            }
        }
        9 | 11 => {
            for k in ks {
                p = close(&p, k, er, di, n, mask);
            }
        }
        12 => {
            for k in ks {
                let o = repeat(&repeat(&p, k, er, n, mask), k, di, n, mask);
                p = close(&o, k, er, di, n, mask);
            }
        }
        13 => {
            for k in ks {
                p = difference(&orig, &repeat(&p, k, er, n, mask), mask);
            }
        }
        14 => {
            for k in ks {
                p = difference(&repeat(&p, k, di, n, mask), &orig, mask);
            }
        }
        15 => {
            for k in ks {
                let d = repeat(&p, k, di, n, mask);
                let e = repeat(&orig, k, er, n, mask);
                p = difference(&d, &e, mask);
            }
        }
        16 => {
            for k in ks {
                let o = repeat(&repeat(&p, k, er, n, mask), k, di, n, mask);
                p = difference(&orig, &o, mask);
            }
        }
        17 => {
            for k in ks {
                let c = close(&p, k, er, di, n, mask);
                p = difference(&c, &orig, mask);
            }
        }
        18 => {
            let mut acc: Option<Image> = None;
            for k in ks {
                let h = hit_and_miss(&p, k, mask);
                acc = Some(match acc {
                    None => h,
                    Some(mut a) => {
                        for (x, y) in a.pixels.iter_mut().zip(&h.pixels) {
                            *x = (*x).max(*y);
                        }
                        a
                    }
                });
            }
            if let Some(a) = acc {
                p = a;
            }
        }
        19 | 20 => {
            let thicken = method == 20;
            iterate(&mut p, &|src: &Image| {
                let mut s = src.clone();
                for k in ks {
                    let h = hit_and_miss(&s, k, mask);
                    for (i, (x, y)) in s.pixels.iter_mut().zip(&h.pixels).enumerate() {
                        if mask[i % 4] {
                            *x = if thicken {
                                x.saturating_add(*y)
                            } else {
                                x.saturating_sub(*y)
                            };
                        }
                    }
                }
                s
            });
        }
        21 => {
            iterate(&mut p, &|src: &Image| {
                let mut s = src.clone();
                for k in ks {
                    distance_pass(&mut s, k, mask);
                }
                s
            });
        }
        // ImageMagick 7.1.2's Voronoi (as pecl reaches it) leaves the
        // colour channels at (row + 1) times the kernel's unit distance
        22 => {
            let unit = ks[0]
                .values
                .iter()
                .copied()
                .filter(|v| *v > 0.0)
                .fold(f64::INFINITY, f64::min);
            let unit = if unit.is_finite() { unit } else { 0.0 };
            let w = p.columns;
            for (i, px) in p.pixels.chunks_exact_mut(4).enumerate() {
                let v = q(((i / w) as f64 + 1.0) * unit);
                for c in 0..3 {
                    if mask[c] {
                        px[c] = v;
                    }
                }
            }
        }
        // IterativeDistance: every element at once, min(p + k), repeated
        7 => {
            iterate(&mut p, &|src: &Image| {
                let mut s = src.clone();
                for k in ks {
                    s = min_plus(&s, k, mask);
                }
                s
            });
        }
        _ => {
            iterate(&mut p, &|src: &Image| {
                let mut s = src.clone();
                for k in ks {
                    s = prim(&s, k, Prim::Convolve, mask);
                }
                s
            });
        }
    }
    *img = p;
    img.colors = None;
}

/// ConvolveImage(): each kernel of the list in turn, reflected as the
/// Convolve method uses it.
pub(crate) fn convolve(img: &mut Image, ks: &[Kernel], ch: i64) {
    let mask = channel_mask(ch, img.alpha);
    for k in ks {
        *img = prim(img, k, Prim::Convolve, mask);
    }
}
