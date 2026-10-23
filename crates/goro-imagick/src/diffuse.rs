//! Error-diffusion dithering onto a set of colours (posterize, remap and
//! quantize with a dither method).

use crate::image::Image;

/// Floyd-Steinberg error diffusion: `nearest` maps a (clamped) colour in
/// quantum units to the colour it is replaced by.
pub fn floyd_steinberg(img: &mut Image, nearest: &dyn Fn([f64; 3]) -> [f64; 3]) {
    let (w, h) = (img.columns, img.rows);
    if w == 0 || h == 0 {
        return;
    }
    let mut cur = vec![[0f64; 3]; w + 2];
    let mut next = vec![[0f64; 3]; w + 2];
    for y in 0..h {
        let ltr = y % 2 == 0;
        for i in 0..w {
            let x = if ltr { i } else { w - 1 - i };
            let p = img.px(x, y);
            let e = cur[x + 1];
            let v = [
                (p[0] as f64 + e[0]).clamp(0.0, 65535.0),
                (p[1] as f64 + e[1]).clamp(0.0, 65535.0),
                (p[2] as f64 + e[2]).clamp(0.0, 65535.0),
            ];
            let c = nearest(v);
            let err = [v[0] - c[0], v[1] - c[1], v[2] - c[2]];
            let fwd = if ltr { x + 2 } else { x };
            let back = if ltr { x } else { x + 2 };
            for k in 0..3 {
                cur[fwd][k] += err[k] * 7.0 / 16.0;
                next[back][k] += err[k] * 3.0 / 16.0;
                next[x + 1][k] += err[k] * 5.0 / 16.0;
                next[fwd][k] += err[k] / 16.0;
            }
            let q = |v: f64| v.round().clamp(0.0, 65535.0) as u16;
            img.set_px(x, y, [q(c[0]), q(c[1]), q(c[2]), p[3]]);
        }
        std::mem::swap(&mut cur, &mut next);
        next.iter_mut().for_each(|e| *e = [0.0; 3]);
    }
    img.colors = None;
}
