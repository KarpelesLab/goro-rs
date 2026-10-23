//! Neighbourhood effects measured against ImageMagick 7.1.2 Q16 (black
//! box): oil paint, despeckle, enhance, selective blur and Canny edges.

use crate::color::clamp_to_quantum;
use crate::convolve::{blur, kernel_radius, kernel_radius_2d};
use crate::image::Image;
use crate::types::{Colorspace, channel};

fn edge_px(img: &Image, x: i64, y: i64) -> [u16; 4] {
    let (w, h) = (img.columns as i64, img.rows as i64);
    img.px(x.clamp(0, w - 1) as usize, y.clamp(0, h - 1) as usize)
}

fn rounded_intensity(p: &[u16]) -> f64 {
    clamp_to_quantum(0.212656 * p[0] as f64 + 0.715158 * p[1] as f64 + 0.072186 * p[2] as f64)
}

/// OilPaintImage(): over a (2w+1)² window (w = ⌈radius⌉, or the 2-D
/// gaussian width of `sigma` for radius 0), the 256-bin intensity
/// histogram's first bin to reach the highest count wins, and the last
/// pixel of that bin in the window is copied.
pub fn oil_paint(img: &mut Image, radius: f64, sigma: f64) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    let r = if radius > 0.0 {
        radius.ceil() as i64
    } else {
        kernel_radius_2d(0.0, sigma) as i64
    };
    let bins: Vec<u8> = img
        .pixels
        .chunks_exact(4)
        .map(|p| ((rounded_intensity(p) as u32 + 128) / 257) as u8)
        .collect();
    let src = img.clone();
    let mut hist = [0u32; 256];
    let mut last = [(0usize, 0usize); 256];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            hist.fill(0);
            let (mut best, mut bc) = (0u32, 0usize);
            for v in -r..=r {
                let yy = (y + v).clamp(0, h as i64 - 1) as usize;
                for u in -r..=r {
                    let xx = (x + u).clamp(0, w as i64 - 1) as usize;
                    let b = bins[yy * w + xx] as usize;
                    hist[b] += 1;
                    last[b] = (xx, yy);
                    if hist[b] > best {
                        best = hist[b];
                        bc = b;
                    }
                }
            }
            let (sx, sy) = last[bc];
            let p = src.px(sx, sy);
            img.set_px(x as usize, y as usize, p);
        }
    }
    img.colors = None;
}

/// One Crimmins hull pass of DespeckleImage() along (dx, dy): samples
/// move by 257 toward a neighbour at least 514 away, then again where
/// both neighbours along the line agree; zero outside the image.
fn hull(f: &mut [i32], w: usize, h: usize, dx: i64, dy: i64, polarity: i32) {
    let at = |v: &[i32], x: i64, y: i64| -> i32 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0
        } else {
            v[y as usize * w + x as usize]
        }
    };
    let (step, thr) = (257, 514);
    let mut g = vec![0i32; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let c = at(f, x, y);
            let n = at(f, x + dx, y + dy);
            let v = if polarity > 0 {
                if n >= c + thr { c + step } else { c }
            } else if n <= c - thr {
                c - step
            } else {
                c
            };
            g[y as usize * w + x as usize] = v.clamp(0, 65535);
        }
    }
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let c = at(&g, x, y);
            let n1 = at(&g, x - dx, y - dy);
            let n2 = at(&g, x + dx, y + dy);
            let v = if polarity > 0 {
                if n1 >= c + thr && n2 > c { c + step } else { c }
            } else if n1 <= c - thr && n2 < c {
                c - step
            } else {
                c
            };
            f[y as usize * w + x as usize] = v.clamp(0, 65535);
        }
    }
}

/// DespeckleImage(): Crimmins' complementary hulling on each colour
/// channel, in four directions.
pub fn despeckle(img: &mut Image) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    for ch in 0..3 {
        let mut f: Vec<i32> = img.pixels.chunks_exact(4).map(|p| p[ch] as i32).collect();
        for (dx, dy) in [(0, 1), (1, 0), (1, 1), (-1, 1)] {
            hull(&mut f, w, h, dx, dy, 1);
            hull(&mut f, w, h, -dx, -dy, 1);
            hull(&mut f, w, h, -dx, -dy, -1);
            hull(&mut f, w, h, dx, dy, -1);
        }
        for (p, v) in img.pixels.chunks_exact_mut(4).zip(f) {
            p[ch] = v as u16;
        }
    }
    img.colors = None;
}

/// EnhanceImage(): a weighted 5×5 mean over the neighbours whose
/// red-mean colour distance is under QuantumRange²/25.
pub fn enhance(img: &mut Image) {
    const WT: [[f64; 5]; 5] = [
        [5.0, 8.0, 10.0, 8.0, 5.0],
        [8.0, 20.0, 40.0, 20.0, 8.0],
        [10.0, 40.0, 80.0, 40.0, 10.0],
        [8.0, 20.0, 40.0, 20.0, 8.0],
        [5.0, 8.0, 10.0, 8.0, 5.0],
    ];
    let (w, h) = (img.columns, img.rows);
    let src = img.clone();
    let q = 65535.0f64;
    let thr = q * q / 25.0;
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let c = src.px(x as usize, y as usize);
            let mut acc = [0f64; 3];
            let mut tot = 0f64;
            for (j, row) in WT.iter().enumerate() {
                for (i, wt) in row.iter().enumerate() {
                    let n = edge_px(&src, x + i as i64 - 2, y + j as i64 - 2);
                    let mean = (c[0] as f64 + n[0] as f64) / 2.0 / q;
                    let dr = c[0] as f64 - n[0] as f64;
                    let dg = c[1] as f64 - n[1] as f64;
                    let db = c[2] as f64 - n[2] as f64;
                    let d = (2.0 + mean) * dr * dr + 4.0 * dg * dg + (3.0 - mean) * db * db;
                    if d < thr {
                        for k in 0..3 {
                            acc[k] += wt * n[k] as f64;
                        }
                        tot += wt;
                    }
                }
            }
            let idx = (y as usize * w + x as usize) * 4;
            for k in 0..3 {
                img.pixels[idx + k] = (acc[k] / tot).ceil().clamp(0.0, q) as u16;
            }
        }
    }
    img.colors = None;
}

/// SelectiveBlurImage(): a 2-D gaussian (1-D width rule) over the
/// neighbours whose intensity is within `threshold` of the centre's,
/// renormalised over them.
pub fn selective_blur(img: &mut Image, radius: f64, sigma: f64, threshold: f64, ch: i64) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    let r = kernel_radius(radius, sigma) as i64;
    let s = sigma.abs().max(1e-6);
    let n = (2 * r + 1) as usize;
    let mut k = vec![0f64; n * n];
    for v in 0..n {
        for u in 0..n {
            let (du, dv) = (u as f64 - r as f64, v as f64 - r as f64);
            k[v * n + u] = (-(du * du + dv * dv) / (2.0 * s * s)).exp();
        }
    }
    let sum: f64 = k.iter().sum();
    k.iter_mut().for_each(|x| *x /= sum);
    let src = img.clone();
    let int: Vec<f64> = src
        .pixels
        .chunks_exact(4)
        .map(|p| 0.212656 * p[0] as f64 + 0.715158 * p[1] as f64 + 0.072186 * p[2] as f64)
        .collect();
    let mask = crate::filters::channel_mask(ch, img.alpha);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let ci = int[y as usize * w + x as usize];
            let mut acc = [0f64; 4];
            let mut tot = 0f64;
            for v in 0..n as i64 {
                let yy = (y + v - r).clamp(0, h as i64 - 1) as usize;
                for u in 0..n as i64 {
                    let xx = (x + u - r).clamp(0, w as i64 - 1) as usize;
                    if (int[yy * w + xx] - ci).abs() < threshold {
                        let kv = k[v as usize * n + u as usize];
                        let p = src.px(xx, yy);
                        for c in 0..4 {
                            acc[c] += kv * p[c] as f64;
                        }
                        tot += kv;
                    }
                }
            }
            let idx = (y as usize * w + x as usize) * 4;
            for c in 0..4 {
                if mask[c] && tot > 0.0 {
                    img.pixels[idx + c] = clamp_to_quantum(acc[c] / tot) as u16;
                }
            }
        }
    }
    img.colors = None;
}

/// CannyEdgeImage(): BlurImage(radius, sigma), a 2×2 forward-difference
/// gradient of the intensity, four-direction non-maximum suppression,
/// and 8-connected hysteresis between `lower` and `upper` times the
/// strongest kept magnitude. A Gray 0/QuantumRange image results.
pub fn canny(img: &mut Image, radius: f64, sigma: f64, lower: f64, upper: f64) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    let mut b = img.clone();
    blur(&mut b, radius, sigma, channel::DEFAULT);
    let int: Vec<f64> = b
        .pixels
        .chunks_exact(4)
        .map(|p| 0.212656 * p[0] as f64 + 0.715158 * p[1] as f64 + 0.072186 * p[2] as f64)
        .collect();
    let at = |x: i64, y: i64| {
        int[(y.clamp(0, h as i64 - 1) as usize) * w + x.clamp(0, w as i64 - 1) as usize]
    };
    let mut mag = vec![0f64; w * h];
    let mut dir = vec![0u8; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let gx = at(x + 1, y) + at(x + 1, y + 1) - at(x, y) - at(x, y + 1);
            let gy = at(x, y + 1) + at(x + 1, y + 1) - at(x, y) - at(x + 1, y);
            let i = y as usize * w + x as usize;
            mag[i] = gx.hypot(gy);
            let a = (gy.atan2(gx).to_degrees() + 180.0) % 180.0;
            dir[i] = if !(22.5..157.5).contains(&a) {
                0
            } else if a < 67.5 {
                1
            } else if a < 112.5 {
                2
            } else {
                3
            };
        }
    }
    let m = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.0
        } else {
            mag[y as usize * w + x as usize]
        }
    };
    let mut s = vec![0f64; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let i = y as usize * w + x as usize;
            let ((ax, ay), (bx, by)) = match dir[i] {
                0 => ((-1, 0), (1, 0)),
                1 => ((-1, -1), (1, 1)),
                2 => ((0, -1), (0, 1)),
                _ => ((1, -1), (-1, 1)),
            };
            if mag[i] > m(x + ax, y + ay) && mag[i] >= m(x + bx, y + by) {
                s[i] = mag[i];
            }
        }
    }
    let mx = s.iter().cloned().fold(0.0, f64::max);
    let (t1, t2) = (lower * mx, upper * mx);
    let mut out = vec![false; w * h];
    let mut stack = Vec::new();
    for i in 0..w * h {
        if s[i] >= t2 && !out[i] {
            out[i] = true;
            stack.push(i);
            while let Some(j) = stack.pop() {
                let (x, y) = ((j % w) as i64, (j / w) as i64);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (xx, yy) = (x + dx, y + dy);
                        if xx < 0 || yy < 0 || xx >= w as i64 || yy >= h as i64 {
                            continue;
                        }
                        let k = yy as usize * w + xx as usize;
                        if !out[k] && s[k] >= t1 {
                            out[k] = true;
                            stack.push(k);
                        }
                    }
                }
            }
        }
    }
    for (p, e) in img.pixels.chunks_exact_mut(4).zip(out) {
        let v = if e { 65535 } else { 0 };
        p[0] = v;
        p[1] = v;
        p[2] = v;
    }
    img.colorspace = Colorspace::Gray;
    img.colors = None;
}
