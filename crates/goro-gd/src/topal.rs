//! gd_topal.c: truecolor to palette conversion (libjpeg's jquant2 two-pass
//! median-cut quantizer with optional Floyd-Steinberg dithering).

use crate::*;

const MAXJSAMPLE: i32 = 255;
const C0_SCALE: i32 = 2;
const C1_SCALE: i32 = 3;
const C2_SCALE: i32 = 1;
const HIST_C0_BITS: i32 = 5;
const HIST_C1_BITS: i32 = 6;
const HIST_C2_BITS: i32 = 5;
const HIST_C0_ELEMS: usize = 1 << HIST_C0_BITS;
const HIST_C1_ELEMS: usize = 1 << HIST_C1_BITS;
const HIST_C2_ELEMS: usize = 1 << HIST_C2_BITS;
const C0_SHIFT: i32 = 8 - HIST_C0_BITS;
const C1_SHIFT: i32 = 8 - HIST_C1_BITS;
const C2_SHIFT: i32 = 8 - HIST_C2_BITS;
const BOX_C0_LOG: i32 = HIST_C0_BITS - 3;
const BOX_C1_LOG: i32 = HIST_C1_BITS - 3;
const BOX_C2_LOG: i32 = HIST_C2_BITS - 3;
const BOX_C0_ELEMS: usize = 1 << BOX_C0_LOG;
const BOX_C1_ELEMS: usize = 1 << BOX_C1_LOG;
const BOX_C2_ELEMS: usize = 1 << BOX_C2_LOG;
const BOX_C0_SHIFT: i32 = C0_SHIFT + BOX_C0_LOG;
const BOX_C1_SHIFT: i32 = C1_SHIFT + BOX_C1_LOG;
const BOX_C2_SHIFT: i32 = C2_SHIFT + BOX_C2_LOG;
const STEP_C0: i32 = (1 << C0_SHIFT) * C0_SCALE;
const STEP_C1: i32 = (1 << C1_SHIFT) * C1_SCALE;
const STEP_C2: i32 = (1 << C2_SHIFT) * C2_SCALE;

struct Hist(Vec<u16>);

impl Hist {
    #[inline]
    fn idx(c0: i32, c1: i32, c2: i32) -> usize {
        (c0 as usize * HIST_C1_ELEMS + c1 as usize) * HIST_C2_ELEMS + c2 as usize
    }
    #[inline]
    fn get(&self, c0: i32, c1: i32, c2: i32) -> u16 {
        self.0[Self::idx(c0, c1, c2)]
    }
    fn zero(&mut self) {
        self.0.iter_mut().for_each(|c| *c = 0);
    }
}

#[derive(Clone, Copy, Default)]
struct Box3 {
    c0min: i32,
    c0max: i32,
    c1min: i32,
    c1max: i32,
    c2min: i32,
    c2max: i32,
    volume: i32,
    colorcount: i64,
}

fn prescan_quantize(oim: &Image, hist: &mut Hist) {
    for row in &oim.tpixels {
        for &p in row.iter().take(oim.sx as usize) {
            let r = true_color_get_red(p) >> C0_SHIFT;
            let g = true_color_get_green(p) >> C1_SHIFT;
            let b = true_color_get_blue(p) >> C2_SHIFT;
            if oim.transparent >= 0 && p == oim.transparent {
                continue;
            }
            let h = &mut hist.0[Hist::idx(r, g, b)];
            *h = h.wrapping_add(1);
            if *h == 0 {
                *h = h.wrapping_sub(1);
            }
        }
    }
}

fn find_biggest_color_pop(boxes: &[Box3]) -> Option<usize> {
    let mut maxc = 0i64;
    let mut which = None;
    for (i, b) in boxes.iter().enumerate() {
        if b.colorcount > maxc && b.volume > 0 {
            which = Some(i);
            maxc = b.colorcount;
        }
    }
    which
}

fn find_biggest_volume(boxes: &[Box3]) -> Option<usize> {
    let mut maxv = 0i32;
    let mut which = None;
    for (i, b) in boxes.iter().enumerate() {
        if b.volume > maxv {
            which = Some(i);
            maxv = b.volume;
        }
    }
    which
}

#[allow(clippy::mut_range_bound)]
fn update_box(hist: &Hist, b: &mut Box3) {
    let (mut c0min, mut c0max) = (b.c0min, b.c0max);
    let (mut c1min, mut c1max) = (b.c1min, b.c1max);
    let (mut c2min, mut c2max) = (b.c2min, b.c2max);

    if c0max > c0min {
        'f: for c0 in c0min..=c0max {
            for c1 in c1min..=c1max {
                for c2 in c2min..=c2max {
                    if hist.get(c0, c1, c2) != 0 {
                        c0min = c0;
                        b.c0min = c0;
                        break 'f;
                    }
                }
            }
        }
    }
    if c0max > c0min {
        'f: for c0 in (c0min..=c0max).rev() {
            for c1 in c1min..=c1max {
                for c2 in c2min..=c2max {
                    if hist.get(c0, c1, c2) != 0 {
                        c0max = c0;
                        b.c0max = c0;
                        break 'f;
                    }
                }
            }
        }
    }
    if c1max > c1min {
        'f: for c1 in c1min..=c1max {
            for c0 in c0min..=c0max {
                for c2 in c2min..=c2max {
                    if hist.get(c0, c1, c2) != 0 {
                        c1min = c1;
                        b.c1min = c1;
                        break 'f;
                    }
                }
            }
        }
    }
    if c1max > c1min {
        'f: for c1 in (c1min..=c1max).rev() {
            for c0 in c0min..=c0max {
                for c2 in c2min..=c2max {
                    if hist.get(c0, c1, c2) != 0 {
                        c1max = c1;
                        b.c1max = c1;
                        break 'f;
                    }
                }
            }
        }
    }
    if c2max > c2min {
        'f: for c2 in c2min..=c2max {
            for c0 in c0min..=c0max {
                for c1 in c1min..=c1max {
                    if hist.get(c0, c1, c2) != 0 {
                        c2min = c2;
                        b.c2min = c2;
                        break 'f;
                    }
                }
            }
        }
    }
    if c2max > c2min {
        'f: for c2 in (c2min..=c2max).rev() {
            for c0 in c0min..=c0max {
                for c1 in c1min..=c1max {
                    if hist.get(c0, c1, c2) != 0 {
                        c2max = c2;
                        b.c2max = c2;
                        break 'f;
                    }
                }
            }
        }
    }
    let dist0 = ((c0max - c0min) << C0_SHIFT) * C0_SCALE;
    let dist1 = ((c1max - c1min) << C1_SHIFT) * C1_SCALE;
    let dist2 = ((c2max - c2min) << C2_SHIFT) * C2_SCALE;
    b.volume = dist0 * dist0 + dist1 * dist1 + dist2 * dist2;
    let mut ccount = 0i64;
    for c0 in c0min..=c0max {
        for c1 in c1min..=c1max {
            for c2 in c2min..=c2max {
                if hist.get(c0, c1, c2) != 0 {
                    ccount += 1;
                }
            }
        }
    }
    b.colorcount = ccount;
}

fn median_cut(hist: &Hist, boxes: &mut Vec<Box3>, desired: i32) {
    while (boxes.len() as i32) < desired {
        let n = boxes.len() as i32;
        let which = if n * 2 <= desired {
            find_biggest_color_pop(boxes)
        } else {
            find_biggest_volume(boxes)
        };
        let Some(i1) = which else {
            break;
        };
        let mut b1 = boxes[i1];
        let mut b2 = b1;
        let c0 = ((b1.c0max - b1.c0min) << C0_SHIFT) * C0_SCALE;
        let c1 = ((b1.c1max - b1.c1min) << C1_SHIFT) * C1_SCALE;
        let c2 = ((b1.c2max - b1.c2min) << C2_SHIFT) * C2_SCALE;
        let mut cmax = c1;
        let mut n = 1;
        if c0 > cmax {
            cmax = c0;
            n = 0;
        }
        if c2 > cmax {
            n = 2;
        }
        match n {
            0 => {
                let lb = (b1.c0max + b1.c0min) / 2;
                b1.c0max = lb;
                b2.c0min = lb + 1;
            }
            1 => {
                let lb = (b1.c1max + b1.c1min) / 2;
                b1.c1max = lb;
                b2.c1min = lb + 1;
            }
            _ => {
                let lb = (b1.c2max + b1.c2min) / 2;
                b1.c2max = lb;
                b2.c2min = lb + 1;
            }
        }
        update_box(hist, &mut b1);
        update_box(hist, &mut b2);
        boxes[i1] = b1;
        boxes.push(b2);
    }
}

fn compute_color(hist: &Hist, b: &Box3, nim: &mut Image, icolor: usize) {
    let (mut total, mut c0t, mut c1t, mut c2t) = (0i64, 0i64, 0i64, 0i64);
    for c0 in b.c0min..=b.c0max {
        for c1 in b.c1min..=b.c1max {
            for c2 in b.c2min..=b.c2max {
                let count = hist.get(c0, c1, c2) as i64;
                if count != 0 {
                    total += count;
                    c0t += (((c0 << C0_SHIFT) + ((1 << C0_SHIFT) >> 1)) as i64) * count;
                    c1t += (((c1 << C1_SHIFT) + ((1 << C1_SHIFT) >> 1)) as i64) * count;
                    c2t += (((c2 << C2_SHIFT) + ((1 << C2_SHIFT) >> 1)) as i64) * count;
                }
            }
        }
    }
    if total != 0 {
        nim.red[icolor] = ((c0t + (total >> 1)) / total) as i32;
        nim.green[icolor] = ((c1t + (total >> 1)) / total) as i32;
        nim.blue[icolor] = ((c2t + (total >> 1)) / total) as i32;
    } else {
        nim.red[icolor] = 255;
        nim.green[icolor] = 255;
        nim.blue[icolor] = 255;
    }
    nim.open[icolor] = 0;
}

fn select_colors(oim: &Image, nim: &mut Image, hist: &Hist, desired: i32) {
    let mut boxes = Vec::with_capacity(desired.max(1) as usize);
    let mut b = Box3 {
        c0min: 0,
        c0max: MAXJSAMPLE >> C0_SHIFT,
        c1min: 0,
        c1max: MAXJSAMPLE >> C1_SHIFT,
        c2min: 0,
        c2max: MAXJSAMPLE >> C2_SHIFT,
        ..Default::default()
    };
    update_box(hist, &mut b);
    boxes.push(b);
    median_cut(hist, &mut boxes, desired);
    for (i, b) in boxes.iter().enumerate() {
        compute_color(hist, b, nim, i);
    }
    nim.colors_total = boxes.len() as i32;
    if oim.transparent >= 0 {
        let c = nim.colors_total as usize;
        nim.red[c] = true_color_get_red(oim.transparent);
        nim.green[c] = true_color_get_green(oim.transparent);
        nim.blue[c] = true_color_get_blue(oim.transparent);
        nim.alpha[c] = ALPHA_TRANSPARENT;
        nim.open[c] = 0;
    }
}

#[allow(clippy::needless_range_loop)]
fn find_nearby_colors(
    nim: &Image,
    minc0: i32,
    minc1: i32,
    minc2: i32,
    colorlist: &mut [u8; 256],
) -> usize {
    let numcolors = nim.colors_total as usize;
    let maxc0 = minc0 + ((1 << BOX_C0_SHIFT) - (1 << C0_SHIFT));
    let centerc0 = (minc0 + maxc0) >> 1;
    let maxc1 = minc1 + ((1 << BOX_C1_SHIFT) - (1 << C1_SHIFT));
    let centerc1 = (minc1 + maxc1) >> 1;
    let maxc2 = minc2 + ((1 << BOX_C2_SHIFT) - (1 << C2_SHIFT));
    let centerc2 = (minc2 + maxc2) >> 1;
    let mut mindist = [0i32; 256];
    let mut minmaxdist = 0x7FFFFFFFi32;
    // per axis: (min term, max term)
    let axis = |x: i32, minc: i32, maxc: i32, center: i32, scale: i32| -> (i32, i32) {
        if x < minc {
            let t = (x - minc) * scale;
            let t2 = (x - maxc) * scale;
            (t * t, t2 * t2)
        } else if x > maxc {
            let t = (x - maxc) * scale;
            let t2 = (x - minc) * scale;
            (t * t, t2 * t2)
        } else if x <= center {
            let t = (x - maxc) * scale;
            (0, t * t)
        } else {
            let t = (x - minc) * scale;
            (0, t * t)
        }
    };
    for i in 0..numcolors {
        let (a0, b0) = axis(nim.red[i], minc0, maxc0, centerc0, C0_SCALE);
        let (a1, b1) = axis(nim.green[i], minc1, maxc1, centerc1, C1_SCALE);
        let (a2, b2) = axis(nim.blue[i], minc2, maxc2, centerc2, C2_SCALE);
        let min_dist = a0 + a1 + a2;
        let max_dist = b0 + b1 + b2;
        mindist[i] = min_dist;
        if max_dist < minmaxdist {
            minmaxdist = max_dist;
        }
    }
    let mut ncolors = 0;
    for (i, &md) in mindist.iter().enumerate().take(numcolors) {
        if md <= minmaxdist {
            colorlist[ncolors] = i as u8;
            ncolors += 1;
        }
    }
    ncolors
}

const BOX_ELEMS: usize = BOX_C0_ELEMS * BOX_C1_ELEMS * BOX_C2_ELEMS;

fn find_best_colors(
    nim: &Image,
    minc0: i32,
    minc1: i32,
    minc2: i32,
    colorlist: &[u8],
    bestcolor: &mut [u8; BOX_ELEMS],
) {
    let mut bestdist = [0x7FFFFFFFi32; BOX_ELEMS];
    for &icolor in colorlist {
        let ic = icolor as usize;
        let (r, g, b) = (nim.red[ic], nim.green[ic], nim.blue[ic]);
        let mut inc0 = (minc0 - r) * C0_SCALE;
        let mut dist0 = inc0 * inc0;
        let mut inc1 = (minc1 - g) * C1_SCALE;
        dist0 += inc1 * inc1;
        let mut inc2 = (minc2 - b) * C2_SCALE;
        dist0 += inc2 * inc2;
        inc0 = inc0 * (2 * STEP_C0) + STEP_C0 * STEP_C0;
        inc1 = inc1 * (2 * STEP_C1) + STEP_C1 * STEP_C1;
        inc2 = inc2 * (2 * STEP_C2) + STEP_C2 * STEP_C2;
        let mut p = 0usize;
        let mut xx0 = inc0;
        for _ in 0..BOX_C0_ELEMS {
            let mut dist1 = dist0;
            let mut xx1 = inc1;
            for _ in 0..BOX_C1_ELEMS {
                let mut dist2 = dist1;
                let mut xx2 = inc2;
                for _ in 0..BOX_C2_ELEMS {
                    if dist2 < bestdist[p] {
                        bestdist[p] = dist2;
                        bestcolor[p] = icolor;
                    }
                    dist2 += xx2;
                    xx2 += 2 * STEP_C2 * STEP_C2;
                    p += 1;
                }
                dist1 += xx1;
                xx1 += 2 * STEP_C1 * STEP_C1;
            }
            dist0 += xx0;
            xx0 += 2 * STEP_C0 * STEP_C0;
        }
    }
}

fn fill_inverse_cmap(nim: &Image, hist: &mut Hist, c0: i32, c1: i32, c2: i32) {
    let c0 = c0 >> BOX_C0_LOG;
    let c1 = c1 >> BOX_C1_LOG;
    let c2 = c2 >> BOX_C2_LOG;
    let minc0 = (c0 << BOX_C0_SHIFT) + ((1 << C0_SHIFT) >> 1);
    let minc1 = (c1 << BOX_C1_SHIFT) + ((1 << C1_SHIFT) >> 1);
    let minc2 = (c2 << BOX_C2_SHIFT) + ((1 << C2_SHIFT) >> 1);
    let mut colorlist = [0u8; 256];
    let n = find_nearby_colors(nim, minc0, minc1, minc2, &mut colorlist);
    let mut bestcolor = [0u8; BOX_ELEMS];
    find_best_colors(nim, minc0, minc1, minc2, &colorlist[..n], &mut bestcolor);
    let c0 = c0 << BOX_C0_LOG;
    let c1 = c1 << BOX_C1_LOG;
    let c2 = c2 << BOX_C2_LOG;
    let mut p = 0;
    for ic0 in 0..BOX_C0_ELEMS as i32 {
        for ic1 in 0..BOX_C1_ELEMS as i32 {
            for ic2 in 0..BOX_C2_ELEMS as i32 {
                hist.0[Hist::idx(c0 + ic0, c1 + ic1, c2 + ic2)] = bestcolor[p] as u16 + 1;
                p += 1;
            }
        }
    }
}

fn pass2_no_dither(oim: &Image, nim: &mut Image, hist: &mut Hist) {
    let width = oim.sx as usize;
    for row in 0..oim.sy as usize {
        for col in 0..width {
            let p = oim.tpixels[row][col];
            if oim.transparent >= 0 && oim.transparent == p {
                nim.pixels[row][col] = nim.colors_total as u8;
                continue;
            }
            let c0 = true_color_get_red(p) >> C0_SHIFT;
            let c1 = true_color_get_green(p) >> C1_SHIFT;
            let c2 = true_color_get_blue(p) >> C2_SHIFT;
            if hist.get(c0, c1, c2) == 0 {
                fill_inverse_cmap(nim, hist, c0, c1, c2);
            }
            nim.pixels[row][col] = (hist.get(c0, c1, c2) as i32 - 1) as u8;
        }
    }
}

/// init_error_limit(): indexable by -255..=255 (offset 255)
fn init_error_limit() -> Vec<i32> {
    let mut table = vec![0i32; (MAXJSAMPLE * 2 + 1) as usize];
    let m = MAXJSAMPLE as usize;
    let stepsize = (MAXJSAMPLE + 1) / 16;
    let mut out = 0;
    let mut inp = 0;
    while inp < stepsize {
        table[m + inp as usize] = out;
        table[m - inp as usize] = -out;
        inp += 1;
        out += 1;
    }
    while inp < stepsize * 3 {
        table[m + inp as usize] = out;
        table[m - inp as usize] = -out;
        inp += 1;
        out += if inp & 1 != 0 { 0 } else { 1 };
    }
    while inp <= MAXJSAMPLE {
        table[m + inp as usize] = out;
        table[m - inp as usize] = -out;
        inp += 1;
    }
    table
}

fn pass2_fs_dither(
    oim: &Image,
    nim: &mut Image,
    hist: &mut Hist,
    error_limit: &[i32],
    fserrors: &mut [i16],
) {
    let width = oim.sx as usize;
    let lim = |v: i32| -> i32 { error_limit[(v + MAXJSAMPLE) as usize] };
    // on_odd_row is never toggled by libgd: always left to right
    for row in 0..oim.sy as usize {
        let mut e = 0usize; // errorptr (index into fserrors, 3 per column)
        let (mut cur0, mut cur1, mut cur2) = (0i32, 0i32, 0i32);
        let (mut belowerr0, mut belowerr1, mut belowerr2) = (0i32, 0i32, 0i32);
        let (mut bpreverr0, mut bpreverr1, mut bpreverr2) = (0i32, 0i32, 0i32);
        for col in 0..width {
            let p = oim.tpixels[row][col];
            if oim.transparent >= 0 && p == oim.transparent {
                nim.pixels[row][col] = nim.colors_total as u8;
                fserrors[e] = 0;
                fserrors[e + 1] = 0;
                fserrors[e + 2] = 0;
                fserrors[e + 3] = 0;
                e += 3;
                continue;
            }
            cur0 = (cur0 + fserrors[e + 3] as i32 + 8) >> 4;
            cur1 = (cur1 + fserrors[e + 4] as i32 + 8) >> 4;
            cur2 = (cur2 + fserrors[e + 5] as i32 + 8) >> 4;
            cur0 = lim(cur0);
            cur1 = lim(cur1);
            cur2 = lim(cur2);
            cur0 += true_color_get_red(p);
            cur1 += true_color_get_green(p);
            cur2 += true_color_get_blue(p);
            cur0 = cur0.clamp(0, 255);
            cur1 = cur1.clamp(0, 255);
            cur2 = cur2.clamp(0, 255);
            let (h0, h1, h2) = (cur0 >> C0_SHIFT, cur1 >> C1_SHIFT, cur2 >> C2_SHIFT);
            if hist.get(h0, h1, h2) == 0 {
                fill_inverse_cmap(nim, hist, h0, h1, h2);
            }
            let pixcode = hist.get(h0, h1, h2) as i32 - 1;
            nim.pixels[row][col] = pixcode as u8;
            let pc = (pixcode as usize) & 0xff;
            cur0 -= nim.red[pc];
            cur1 -= nim.green[pc];
            cur2 -= nim.blue[pc];

            let mut bnexterr = cur0;
            let mut delta = cur0 * 2;
            cur0 += delta;
            fserrors[e] = (bpreverr0 + cur0) as i16;
            cur0 += delta;
            bpreverr0 = belowerr0 + cur0;
            belowerr0 = bnexterr;
            cur0 += delta;

            bnexterr = cur1;
            delta = cur1 * 2;
            cur1 += delta;
            fserrors[e + 1] = (bpreverr1 + cur1) as i16;
            cur1 += delta;
            bpreverr1 = belowerr1 + cur1;
            belowerr1 = bnexterr;
            cur1 += delta;

            bnexterr = cur2;
            delta = cur2 * 2;
            cur2 += delta;
            fserrors[e + 2] = (bpreverr2 + cur2) as i16;
            cur2 += delta;
            bpreverr2 = belowerr2 + cur2;
            belowerr2 = bnexterr;
            cur2 += delta;

            e += 3;
        }
        fserrors[e] = bpreverr0 as i16;
        fserrors[e + 1] = bpreverr1 as i16;
        fserrors[e + 2] = bpreverr2 as i16;
    }
}

/// Quantize the truecolor `oim` into the palette image `nim`.
fn quantize(oim: &Image, nim: &mut Image, dither: bool, mut colors_wanted: i32) {
    let mut max_colors = MAX_COLORS as i32;
    if oim.transparent >= 0 {
        max_colors -= 1;
    }
    if colors_wanted > max_colors {
        colors_wanted = max_colors;
    }
    let error_limit = init_error_limit();
    let mut fserrors = vec![0i16; (nim.sx as usize + 2) * 3];
    let mut hist = Hist(vec![0u16; HIST_C0_ELEMS * HIST_C1_ELEMS * HIST_C2_ELEMS]);
    hist.zero();
    prescan_quantize(oim, &mut hist);
    select_colors(oim, nim, &hist, colors_wanted);
    hist.zero();
    if dither {
        pass2_fs_dither(oim, nim, &mut hist, &error_limit, &mut fserrors);
    } else {
        pass2_no_dither(oim, nim, &mut hist);
    }
    if oim.transparent >= 0 {
        nim.transparent = nim.colors_total;
        nim.colors_total += 1;
    }
}

/// gdImageCreatePaletteFromTrueColor()
pub fn create_palette_from_true_color(
    im: &Image,
    dither: bool,
    colors_wanted: i32,
) -> Option<Image> {
    let mut nim = Image::create(im.sx, im.sy)?;
    if !im.true_color {
        crate::gd::copy(&mut nim, im, false, 0, 0, 0, 0, im.sx, im.sy);
        return Some(nim);
    }
    quantize(im, &mut nim, dither, colors_wanted);
    Some(nim)
}

/// gdImageTrueColorToPalette()
pub fn true_color_to_palette(im: &mut Image, dither: bool, colors_wanted: i32) -> bool {
    if !im.true_color {
        return true;
    }
    // In place: the palette fields of the image itself are the output.
    let mut nim = Image::create(im.sx, im.sy).unwrap_or_else(|| im.clone());
    nim.pixels = vec![vec![0u8; im.sx.max(0) as usize]; im.sy.max(0) as usize];
    nim.red = im.red;
    nim.green = im.green;
    nim.blue = im.blue;
    nim.alpha = im.alpha;
    nim.open = im.open;
    nim.colors_total = im.colors_total;
    nim.transparent = im.transparent;
    nim.sx = im.sx;
    nim.sy = im.sy;
    quantize(im, &mut nim, dither, colors_wanted);
    im.pixels = nim.pixels;
    im.red = nim.red;
    im.green = nim.green;
    im.blue = nim.blue;
    im.alpha = nim.alpha;
    im.open = nim.open;
    im.colors_total = nim.colors_total;
    im.transparent = nim.transparent;
    im.true_color = false;
    im.tpixels = Vec::new();
    true
}
