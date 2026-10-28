//! imagetruecolortopalette: median-cut color quantization (Heckbert,
//! "Color Image Quantization for Frame Buffer Display", 1982) on a
//! 5/6/5-bit RGB histogram, with optional Floyd–Steinberg dithering.
//!
//! Behaviour source: the reference image of the ext/gd test
//! imagetruecolortopalette_basic (palette entries are the means of the
//! histogram cell centres of each box: (0,255,255) becomes (4,254,252);
//! the first split cuts the longest box side, weighted 2:3:1 for R:G:B,
//! at its middle).

use crate::image::*;

const RB: u32 = 5;
const GB: u32 = 6;
const BB: u32 = 5;
const RS: u32 = 8 - RB;
const GS: u32 = 8 - GB;
const BS: u32 = 8 - BB;

#[derive(Clone, Copy)]
struct Box3 {
    lo: [i32; 3],
    hi: [i32; 3],
    volume: i64,
    count: u64,
}

struct Hist {
    cells: Vec<u32>,
    alpha: Vec<u64>,
}

impl Hist {
    #[inline]
    fn idx(r: i32, g: i32, b: i32) -> usize {
        ((r as usize) << (GB + BB)) | ((g as usize) << BB) | b as usize
    }
}

fn shrink(h: &Hist, b: &mut Box3) {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    let mut count = 0u64;
    for r in b.lo[0]..=b.hi[0] {
        for g in b.lo[1]..=b.hi[1] {
            for bl in b.lo[2]..=b.hi[2] {
                let n = h.cells[Hist::idx(r, g, bl)];
                if n != 0 {
                    count += n as u64;
                    let v = [r, g, bl];
                    for i in 0..3 {
                        lo[i] = lo[i].min(v[i]);
                        hi[i] = hi[i].max(v[i]);
                    }
                }
            }
        }
    }
    if count == 0 {
        b.count = 0;
        b.volume = 0;
        return;
    }
    b.lo = lo;
    b.hi = hi;
    let d = [
        ((hi[0] - lo[0]) << RS) as i64 * 2,
        ((hi[1] - lo[1]) << GS) as i64 * 3,
        ((hi[2] - lo[2]) << BS) as i64,
    ];
    b.volume = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    b.count = count;
}

fn splittable(b: &Box3) -> bool {
    b.lo != b.hi
}

/// Builds the palette (r, g, b, a) for `want` colors.
fn median_cut(h: &Hist, want: usize) -> Vec<(i32, i32, i32, i32)> {
    let mut boxes = vec![Box3 {
        lo: [0, 0, 0],
        hi: [(1 << RB) - 1, (1 << GB) - 1, (1 << BB) - 1],
        volume: 0,
        count: 0,
    }];
    shrink(h, &mut boxes[0]);
    if boxes[0].count == 0 {
        return vec![(0, 0, 0, 0)];
    }
    while boxes.len() < want {
        // Most populous box for the first half of the splits, then the
        // largest one.
        let pick = if boxes.len() * 2 <= want {
            boxes
                .iter()
                .enumerate()
                .filter(|(_, b)| splittable(b))
                .max_by(|a, b| a.1.count.cmp(&b.1.count).then(b.0.cmp(&a.0)))
                .map(|(i, _)| i)
        } else {
            boxes
                .iter()
                .enumerate()
                .filter(|(_, b)| splittable(b))
                .max_by(|a, b| a.1.volume.cmp(&b.1.volume).then(b.0.cmp(&a.0)))
                .map(|(i, _)| i)
        };
        let Some(i) = pick else {
            break;
        };
        let b = boxes[i];
        let d = [
            ((b.hi[0] - b.lo[0]) << RS) as i64 * 2,
            ((b.hi[1] - b.lo[1]) << GS) as i64 * 3,
            ((b.hi[2] - b.lo[2]) << BS) as i64,
        ];
        // Longest side; ties prefer green, then red, then blue.
        let mut axis = 1;
        let mut best = d[1];
        if d[0] > best {
            best = d[0];
            axis = 0;
        }
        if d[2] > best {
            axis = 2;
        }
        let mid = (b.lo[axis] + b.hi[axis]) / 2;
        let mut b1 = b;
        let mut b2 = b;
        b1.hi[axis] = mid;
        b2.lo[axis] = mid + 1;
        shrink(h, &mut b1);
        shrink(h, &mut b2);
        boxes[i] = b1;
        boxes.push(b2);
    }
    boxes
        .iter()
        .map(|b| {
            let (mut tr, mut tg, mut tb, mut ta, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for r in b.lo[0]..=b.hi[0] {
                for g in b.lo[1]..=b.hi[1] {
                    for bl in b.lo[2]..=b.hi[2] {
                        let k = Hist::idx(r, g, bl);
                        let c = h.cells[k] as u64;
                        if c == 0 {
                            continue;
                        }
                        n += c;
                        tr += c * (((r as u64) << RS) + (1 << (RS - 1)));
                        tg += c * (((g as u64) << GS) + (1 << (GS - 1)));
                        tb += c * (((bl as u64) << BS) + (1 << (BS - 1)));
                        ta += h.alpha[k];
                    }
                }
            }
            let n = n.max(1);
            (
                ((tr + n / 2) / n) as i32,
                ((tg + n / 2) / n) as i32,
                ((tb + n / 2) / n) as i32,
                ((ta + n / 2) / n) as i32,
            )
        })
        .collect()
}

/// Converts a truecolor image to a palette of at most `colors` colors.
pub fn truecolor_to_palette(im: &mut Image, dither: bool, colors: usize) {
    if !im.truecolor {
        return;
    }
    let colors = colors.clamp(1, MAX_COLORS);
    let (w, h) = (im.sx as usize, im.sy as usize);
    let n = 1usize << (RB + GB + BB);
    let mut hist = Hist {
        cells: vec![0; n],
        alpha: vec![0; n],
    };
    for &p in &im.tpixels {
        let k = Hist::idx(tc_r(p) >> RS, tc_g(p) >> GS, tc_b(p) >> BS);
        hist.cells[k] = hist.cells[k].saturating_add(1);
        hist.alpha[k] += tc_a(p) as u64;
    }
    let pal = median_cut(&hist, colors);
    let nearest = |r: i32, g: i32, b: i32| -> usize {
        let mut best = 0;
        let mut bd = i64::MAX;
        for (i, &(pr, pg, pb, _)) in pal.iter().enumerate() {
            let d = ((pr - r) as i64).pow(2) + ((pg - g) as i64).pow(2) + ((pb - b) as i64).pow(2);
            if d < bd {
                bd = d;
                best = i;
            }
        }
        best
    };
    let mut cache = std::collections::HashMap::new();
    let mut out = vec![0u8; w * h];
    if dither {
        // Floyd–Steinberg with the error limited to ±16 per channel
        // step (so flat areas don't speckle).
        let limit = |e: i32| -> i32 { e.clamp(-16, 16) };
        let mut err_cur = vec![[0i32; 3]; w + 2];
        let mut err_next = vec![[0i32; 3]; w + 2];
        for y in 0..h {
            let ltr = y % 2 == 0;
            for e in err_next.iter_mut() {
                *e = [0; 3];
            }
            let xs: Vec<usize> = if ltr {
                (0..w).collect()
            } else {
                (0..w).rev().collect()
            };
            for x in xs {
                let p = im.tpixels[y * w + x];
                let e = err_cur[x + 1];
                let r = (tc_r(p) + limit((e[0] + 8) >> 4)).clamp(0, 255);
                let g = (tc_g(p) + limit((e[1] + 8) >> 4)).clamp(0, 255);
                let b = (tc_b(p) + limit((e[2] + 8) >> 4)).clamp(0, 255);
                let i = *cache.entry((r, g, b)).or_insert_with(|| nearest(r, g, b));
                out[y * w + x] = i as u8;
                let (pr, pg, pb, _) = pal[i];
                let d = [r - pr, g - pg, b - pb];
                let (fwd, back) = if ltr {
                    (x + 2, x)
                } else {
                    (x, x + 2)
                };
                for c in 0..3 {
                    err_cur[fwd][c] += d[c] * 7;
                    err_next[back][c] += d[c] * 3;
                    err_next[x + 1][c] += d[c] * 5;
                    err_next[fwd][c] += d[c];
                }
            }
            std::mem::swap(&mut err_cur, &mut err_next);
        }
    } else {
        for (i, &p) in im.tpixels.iter().enumerate() {
            let k = (tc_r(p), tc_g(p), tc_b(p));
            let c = *cache.entry(k).or_insert_with(|| nearest(k.0, k.1, k.2));
            out[i] = c as u8;
        }
    }
    let had_transparent = im.transparent;
    im.pixels = out;
    im.tpixels = Vec::new();
    im.truecolor = false;
    im.open = [true; MAX_COLORS];
    for (i, &(r, g, b, a)) in pal.iter().enumerate() {
        im.red[i] = r;
        im.green[i] = g;
        im.blue[i] = b;
        im.alpha[i] = a;
        im.open[i] = false;
    }
    im.colors_total = pal.len() as i32;
    im.transparent = -1;
    if had_transparent >= 0 {
        let t = im.color_exact(
            tc_r(had_transparent),
            tc_g(had_transparent),
            tc_b(had_transparent),
            tc_a(had_transparent),
        );
        if t >= 0 {
            im.transparent = t;
        }
    }
    im.alpha_blending = EFFECT_REPLACE;
}
