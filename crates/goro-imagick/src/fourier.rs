//! ForwardFourierTransformImage() / InverseFourierTransformImage(), as
//! ImageMagick documents them: the image is padded to a square with even
//! sides (the larger dimension), each channel is transformed with the
//! forward transform normalised by 1/(N·N), and the spectrum is shifted so
//! the zero frequency sits at (N/2, N/2). The magnitude / phase pair
//! stores |F| and the phase mapped from [-π, π] onto [0, QuantumRange];
//! the real / imaginary pair stores the components (clamped, as Q16 has
//! no negative samples).

use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    fn mul(self, o: C) -> C {
        C {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
    fn cis(t: f64) -> C {
        C {
            re: t.cos(),
            im: t.sin(),
        }
    }
}

/// In-place radix-2 FFT (`n` a power of two); `inv` flips the sign.
fn fft_pow2(a: &mut [C], inv: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let sign = if inv { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let w = C::cis(sign * 2.0 * PI / len as f64);
        for start in (0..n).step_by(len) {
            let mut wk = C { re: 1.0, im: 0.0 };
            for k in 0..len / 2 {
                let u = a[start + k];
                let v = a[start + k + len / 2].mul(wk);
                a[start + k] = C {
                    re: u.re + v.re,
                    im: u.im + v.im,
                };
                a[start + k + len / 2] = C {
                    re: u.re - v.re,
                    im: u.im - v.im,
                };
                wk = wk.mul(w);
            }
        }
        len <<= 1;
    }
}

/// An unnormalised DFT of any length (Bluestein's algorithm when the
/// length is not a power of two).
fn dft(a: &mut [C], inv: bool) {
    let n = a.len();
    if n <= 1 {
        return;
    }
    if n.is_power_of_two() {
        fft_pow2(a, inv);
        return;
    }
    let sign = if inv { 1.0 } else { -1.0 };
    let m = (2 * n - 1).next_power_of_two();
    // chirp w_k = exp(sign·iπk²/n)
    let chirp: Vec<C> = (0..n)
        .map(|k| C::cis(sign * PI * ((k * k) % (2 * n)) as f64 / n as f64))
        .collect();
    let mut x = vec![C::default(); m];
    for k in 0..n {
        x[k] = a[k].mul(chirp[k]);
    }
    let mut y = vec![C::default(); m];
    let conj = |c: C| C {
        re: c.re,
        im: -c.im,
    };
    y[0] = conj(chirp[0]);
    for k in 1..n {
        y[k] = conj(chirp[k]);
        y[m - k] = conj(chirp[k]);
    }
    fft_pow2(&mut x, false);
    fft_pow2(&mut y, false);
    for k in 0..m {
        x[k] = x[k].mul(y[k]);
    }
    fft_pow2(&mut x, true);
    for k in 0..n {
        let v = C {
            re: x[k].re / m as f64,
            im: x[k].im / m as f64,
        };
        a[k] = v.mul(chirp[k]);
    }
}

/// A 2-D DFT of an n×n row-major plane.
fn dft2(p: &mut [C], n: usize, inv: bool) {
    for row in p.chunks_exact_mut(n) {
        dft(row, inv);
    }
    let mut col = vec![C::default(); n];
    for x in 0..n {
        for y in 0..n {
            col[y] = p[y * n + x];
        }
        dft(&mut col, inv);
        for y in 0..n {
            p[y * n + x] = col[y];
        }
    }
}

/// The square, even side an image is padded to.
fn padded_side(img: &Image) -> usize {
    let s = img.columns.max(img.rows).max(1);
    s + (s & 1)
}

/// The channels transformed: the colour channels (one for gray), and
/// alpha when the image has it.
fn channels(img: &Image) -> Vec<usize> {
    let mut v = if matches!(
        img.colorspace,
        crate::types::Colorspace::Gray | crate::types::Colorspace::LinearGray
    ) {
        vec![0]
    } else {
        vec![0, 1, 2]
    };
    if img.alpha {
        v.push(3);
    }
    v
}

/// ForwardFourierTransformImage(): the (magnitude, phase) or (real,
/// imaginary) pair.
pub fn forward(img: &Image, magnitude: bool) -> MResult<Vec<Image>> {
    let n = padded_side(img);
    let mut a = Image::blank(n, n);
    a.alpha = img.alpha;
    a.colorspace = img.colorspace;
    let mut b = a.clone();
    let norm = (n * n) as f64;
    let half = (n / 2) as isize;
    let chans = channels(img);
    for &c in &chans {
        let mut plane = vec![C::default(); n * n];
        for y in 0..img.rows {
            for x in 0..img.columns {
                plane[y * n + x].re = img.px(x, y)[c] as f64 / 65535.0;
            }
        }
        dft2(&mut plane, n, false);
        for v in 0..n {
            for u in 0..n {
                let f = plane[v * n + u];
                let (f0, f1) = (f.re / norm, f.im / norm);
                // centre the zero frequency
                let x = ((u as isize + half) % n as isize) as usize;
                let y = ((v as isize + half) % n as isize) as usize;
                let (s0, s1) = if magnitude {
                    ((f0 * f0 + f1 * f1).sqrt(), f1.atan2(f0) / (2.0 * PI) + 0.5)
                } else {
                    (f0, f1)
                };
                let q = |s: f64| (s * 65535.0).round().clamp(0.0, 65535.0) as u16;
                let i = (y * n + x) * 4 + c;
                a.pixels[i] = q(s0);
                b.pixels[i] = q(s1);
            }
        }
    }
    if matches!(
        img.colorspace,
        crate::types::Colorspace::Gray | crate::types::Colorspace::LinearGray
    ) {
        for im in [&mut a, &mut b] {
            for p in im.pixels.chunks_exact_mut(4) {
                p[1] = p[0];
                p[2] = p[0];
            }
        }
    }
    for im in [&mut a, &mut b] {
        im.depth = img.depth;
        im.format = img.format.clone();
        im.page = crate::geometry::Rect::new(n, n, 0, 0);
        im.colors = None;
    }
    Ok(vec![a, b])
}

/// InverseFourierTransformImage(): the image from a (magnitude, phase)
/// or (real, imaginary) pair.
pub fn inverse(first: &Image, second: &Image, magnitude: bool) -> MResult<Image> {
    if first.columns != second.columns || first.rows != second.rows {
        return Err(MagickError::with_desc(
            severity::IMAGE_ERROR,
            "ImageSequenceRequired",
            "",
            "error/fourier.c/InverseFourierTransformImage",
        ));
    }
    let n = padded_side(first);
    let half = (n / 2) as isize;
    let mut out = Image::blank(n, n);
    out.alpha = first.alpha;
    out.colorspace = first.colorspace;
    for &c in &channels(first) {
        let mut plane = vec![C::default(); n * n];
        for v in 0..n {
            for u in 0..n {
                let x = ((u as isize + half) % n as isize) as usize;
                let y = ((v as isize + half) % n as isize) as usize;
                let (x, y) = (x.min(first.columns - 1), y.min(first.rows - 1));
                let s0 = first.px(x, y)[c] as f64 / 65535.0;
                let s1 = second.px(x, y)[c] as f64 / 65535.0;
                plane[v * n + u] = if magnitude {
                    let ph = (s1 - 0.5) * 2.0 * PI;
                    C {
                        re: s0 * ph.cos(),
                        im: s0 * ph.sin(),
                    }
                } else {
                    C { re: s0, im: s1 }
                };
            }
        }
        dft2(&mut plane, n, true);
        for y in 0..n {
            for x in 0..n {
                out.pixels[(y * n + x) * 4 + c] =
                    (plane[y * n + x].re * 65535.0).round().clamp(0.0, 65535.0) as u16;
            }
        }
    }
    if matches!(
        first.colorspace,
        crate::types::Colorspace::Gray | crate::types::Colorspace::LinearGray
    ) {
        for p in out.pixels.chunks_exact_mut(4) {
            p[1] = p[0];
            p[2] = p[0];
        }
    }
    out.depth = first.depth;
    out.format = first.format.clone();
    out.page = crate::geometry::Rect::new(n, n, 0, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for (w, h) in [(8, 8), (7, 5), (12, 10)] {
            let mut img = Image::blank(w, h);
            for y in 0..h {
                for x in 0..w {
                    let v = ((x * 7919 + y * 104729) % 65536) as u16;
                    img.set_px(x, y, [v, v / 2, 65535 - v, 65535]);
                }
            }
            for mag in [true, false] {
                let pair = forward(&img, mag).unwrap();
                let back = inverse(&pair[0], &pair[1], mag).unwrap();
                if mag {
                    for y in 0..h {
                        for x in 0..w {
                            let (p, q) = (img.px(x, y), back.px(x, y));
                            for c in 0..3 {
                                assert!((p[c] as i32 - q[c] as i32).abs() < 1200, "{w}x{h}");
                            }
                        }
                    }
                }
            }
        }
    }
}
