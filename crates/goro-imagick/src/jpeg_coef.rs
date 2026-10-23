//! A JPEG coefficient decoder with libjpeg's ISLOW integer IDCT, so
//! decoded samples match ImageMagick (libjpeg-turbo defaults) bit for bit.
//! oxideav-mjpeg decodes with a float IDCT and exposes no coefficients,
//! so baseline and progressive Huffman 8-bit frames are decoded here;
//! anything else (arithmetic, lossless, 12-bit, hierarchical) returns
//! None and the caller falls back to oxideav-mjpeg.
//!
//! The IDCT is the Loeffler–Ligtenberg–Moschytz algorithm as libjpeg's
//! `jidctint` documents it: 13-bit constants, two extra bits kept between
//! the passes, and libjpeg's range-limit table (a 10-bit wrap) on output.

/// One decoded component, at its own (downsampled) resolution.
pub struct Component {
    pub id: u8,
    pub h: usize,
    pub v: usize,
    /// width × height of the component's meaningful samples
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub data: Vec<u8>,
}

pub struct Decoded {
    pub width: usize,
    pub height: usize,
    pub components: Vec<Component>,
    /// the Adobe APP14 transform flag, when present
    pub adobe_transform: Option<u8>,
    pub jfif: bool,
}

#[derive(Clone, Default)]
struct Huff {
    // (code length, code) -> symbol via canonical tables
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
    // fast lookup: 9-bit prefix -> (len, symbol), len 0 = slow path
    look: Vec<(u8, u8)>,
}

impl Huff {
    fn new(counts: &[u8; 16], vals: &[u8]) -> Huff {
        let mut h = Huff {
            vals: vals.to_vec(),
            look: vec![(0, 0); 512],
            ..Default::default()
        };
        let mut code = 0i32;
        let mut k = 0i32;
        for l in 1..=16 {
            let n = counts[l - 1] as i32;
            h.valptr[l] = k;
            h.mincode[l] = code;
            code += n;
            k += n;
            h.maxcode[l] = if n > 0 { code - 1 } else { -1 };
            code <<= 1;
        }
        h.maxcode[17] = i32::MAX;
        // fast table
        let mut code = 0u32;
        let mut k = 0usize;
        for l in 1..=16u32 {
            for _ in 0..counts[l as usize - 1] {
                if l <= 9 {
                    let shift = 9 - l;
                    let base = (code << shift) as usize;
                    for j in 0..(1usize << shift) {
                        h.look[base + j] = (l as u8, vals[k]);
                    }
                }
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        h
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
    marker: bool,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], pos: usize) -> Bits<'a> {
        Bits {
            d,
            pos,
            acc: 0,
            n: 0,
            marker: false,
        }
    }
    fn fill(&mut self) {
        while self.n <= 56 {
            let mut b = 0u8;
            if !self.marker && self.pos < self.d.len() {
                b = self.d[self.pos];
                if b == 0xFF {
                    let nx = self.d.get(self.pos + 1).copied().unwrap_or(0xD9);
                    if nx == 0 {
                        self.pos += 2;
                    } else {
                        self.marker = true;
                        b = 0;
                    }
                } else {
                    self.pos += 1;
                }
            }
            self.acc |= (b as u64) << (56 - self.n);
            self.n += 8;
        }
    }
    fn peek(&mut self, k: u32) -> u32 {
        if self.n < k {
            self.fill();
        }
        (self.acc >> (64 - k)) as u32
    }
    fn skip(&mut self, k: u32) {
        self.acc <<= k;
        self.n -= k;
    }
    fn get(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        let v = self.peek(k);
        self.skip(k);
        v
    }
    fn bit(&mut self) -> u32 {
        self.get(1)
    }
    fn decode(&mut self, h: &Huff) -> Option<u8> {
        let p = self.peek(9) as usize;
        let (l, s) = h.look[p];
        if l > 0 {
            self.skip(l as u32);
            return Some(s);
        }
        let mut code = self.get(1) as i32;
        let mut l = 1;
        while l <= 16 && code > h.maxcode[l] {
            code = (code << 1) | self.get(1) as i32;
            l += 1;
        }
        if l > 16 {
            return Some(0);
        }
        let idx = (h.valptr[l] + code - h.mincode[l]) as usize;
        Some(*h.vals.get(idx).unwrap_or(&0))
    }
    /// skip to the next restart marker after the current position
    fn restart(&mut self) {
        self.acc = 0;
        self.n = 0;
        self.marker = false;
        // find FF Dx
        while self.pos + 1 < self.d.len() {
            if self.d[self.pos] == 0xFF {
                let m = self.d[self.pos + 1];
                if (0xD0..=0xD7).contains(&m) {
                    self.pos += 2;
                    return;
                }
                if m != 0 && m != 0xFF {
                    return;
                }
            }
            self.pos += 1;
        }
    }
}

fn extend(v: u32, s: u32) -> i32 {
    if s == 0 {
        return 0;
    }
    let v = v as i32;
    if v < (1 << (s - 1)) {
        v - (1 << s) + 1
    } else {
        v
    }
}

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

struct FrameComp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    bw: usize, // blocks per row (MCU-aligned)
    bh: usize,
    w: usize, // component sample width
    hgt: usize,
    coef: Vec<[i32; 64]>,
}

/// Decode `data`; None for streams this decoder does not handle.
pub fn decode(data: &[u8]) -> Option<Decoded> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut qt = [[0i32; 64]; 4];
    let mut dc: Vec<Huff> = vec![Huff::default(); 4];
    let mut ac: Vec<Huff> = vec![Huff::default(); 4];
    let mut comps: Vec<FrameComp> = Vec::new();
    let (mut width, mut height) = (0usize, 0usize);
    let mut progressive = false;
    let mut restart = 0usize;
    let mut adobe = None;
    let mut jfif = false;
    let (mut mcux, mut mcuy) = (0usize, 0usize);
    let mut pos = 2;
    loop {
        // next marker
        while pos < data.len() && data[pos] != 0xFF {
            pos += 1;
        }
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }
        if pos >= data.len() {
            break;
        }
        let m = data[pos];
        pos += 1;
        if m == 0xD9 {
            break;
        }
        if (0xD0..=0xD7).contains(&m) || m == 0x01 {
            continue;
        }
        if pos + 2 > data.len() {
            return None;
        }
        let len = ((data[pos] as usize) << 8) | data[pos + 1] as usize;
        if len < 2 || pos + len > data.len() {
            return None;
        }
        let seg = &data[pos + 2..pos + len];
        let seg_end = pos + len;
        match m {
            0xC0..=0xC2 => {
                progressive = m == 0xC2;
                if seg.len() < 6 || seg[0] != 8 {
                    return None;
                }
                height = ((seg[1] as usize) << 8) | seg[2] as usize;
                width = ((seg[3] as usize) << 8) | seg[4] as usize;
                let n = seg[5] as usize;
                if height == 0 || width == 0 || n == 0 || seg.len() < 6 + 3 * n {
                    return None;
                }
                comps.clear();
                for i in 0..n {
                    let c = &seg[6 + 3 * i..];
                    let (h, v) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
                    if h == 0 || v == 0 || h > 4 || v > 4 {
                        return None;
                    }
                    comps.push(FrameComp {
                        id: c[0],
                        h,
                        v,
                        tq: (c[2] & 3) as usize,
                        bw: 0,
                        bh: 0,
                        w: 0,
                        hgt: 0,
                        coef: Vec::new(),
                    });
                }
                let hmax = comps.iter().map(|c| c.h).max().unwrap();
                let vmax = comps.iter().map(|c| c.v).max().unwrap();
                mcux = width.div_ceil(8 * hmax);
                mcuy = height.div_ceil(8 * vmax);
                for c in comps.iter_mut() {
                    c.w = (width * c.h).div_ceil(hmax);
                    c.hgt = (height * c.v).div_ceil(vmax);
                    c.bw = mcux * c.h;
                    c.bh = mcuy * c.v;
                    c.coef = vec![[0i32; 64]; c.bw * c.bh];
                }
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => return None,
            0xC4 => {
                let mut p = 0;
                while p + 17 <= seg.len() {
                    let tc = seg[p] >> 4;
                    let th = (seg[p] & 3) as usize;
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&seg[p + 1..p + 17]);
                    let tot: usize = counts.iter().map(|&c| c as usize).sum();
                    if p + 17 + tot > seg.len() {
                        return None;
                    }
                    let h = Huff::new(&counts, &seg[p + 17..p + 17 + tot]);
                    if tc == 0 {
                        dc[th] = h;
                    } else {
                        ac[th] = h;
                    }
                    p += 17 + tot;
                }
            }
            0xDB => {
                let mut p = 0;
                while p < seg.len() {
                    let pq = seg[p] >> 4;
                    let tq = (seg[p] & 3) as usize;
                    p += 1;
                    for k in 0..64 {
                        let v = if pq == 0 {
                            *seg.get(p + k)? as i32
                        } else {
                            ((*seg.get(p + 2 * k)? as i32) << 8) | *seg.get(p + 2 * k + 1)? as i32
                        };
                        qt[tq][ZIGZAG[k]] = v;
                    }
                    p += if pq == 0 { 64 } else { 128 };
                }
            }
            0xDD => {
                if seg.len() >= 2 {
                    restart = ((seg[0] as usize) << 8) | seg[1] as usize;
                }
            }
            0xE0 => {
                if seg.starts_with(b"JFIF\0") {
                    jfif = true;
                }
            }
            0xEE => {
                if seg.len() >= 12 && seg.starts_with(b"Adobe") {
                    adobe = Some(seg[11]);
                }
            }
            0xDA => {
                if comps.is_empty() {
                    return None;
                }
                let ns = *seg.first()? as usize;
                if seg.len() < 1 + 2 * ns + 3 {
                    return None;
                }
                let mut sc = Vec::new();
                for i in 0..ns {
                    let cid = seg[1 + 2 * i];
                    let t = seg[2 + 2 * i];
                    let ci = comps.iter().position(|c| c.id == cid)?;
                    sc.push((ci, (t >> 4) as usize & 3, (t & 3) as usize));
                }
                let ss = seg[1 + 2 * ns] as usize;
                let se = seg[2 + 2 * ns] as usize;
                let ah = (seg[3 + 2 * ns] >> 4) as u32;
                let al = (seg[3 + 2 * ns] & 15) as u32;
                let mut br = Bits::new(data, seg_end);
                scan(
                    &mut br,
                    &mut comps,
                    &sc,
                    &dc,
                    &ac,
                    progressive,
                    (ss, se, ah, al),
                    restart,
                    (mcux, mcuy),
                );
                // continue after the entropy-coded data
                pos = br.pos;
                continue;
            }
            _ => {}
        }
        pos = seg_end;
    }
    if comps.is_empty() || comps[0].coef.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for c in &comps {
        let stride = c.bw * 8;
        let mut px = vec![0u8; stride * c.bh * 8];
        let q = &qt[c.tq];
        for by in 0..c.bh {
            for bx in 0..c.bw {
                let blk = &c.coef[by * c.bw + bx];
                let mut o = [0u8; 64];
                idct_islow(blk, q, &mut o);
                for y in 0..8 {
                    let row = (by * 8 + y) * stride + bx * 8;
                    px[row..row + 8].copy_from_slice(&o[y * 8..y * 8 + 8]);
                }
            }
        }
        out.push(Component {
            id: c.id,
            h: c.h,
            v: c.v,
            width: c.w,
            height: c.hgt,
            stride,
            data: px,
        });
    }
    Some(Decoded {
        width,
        height,
        components: out,
        adobe_transform: adobe,
        jfif,
    })
}

#[allow(clippy::too_many_arguments)]
fn scan(
    br: &mut Bits,
    comps: &mut [FrameComp],
    sc: &[(usize, usize, usize)],
    dc: &[Huff],
    ac: &[Huff],
    progressive: bool,
    (ss, se, ah, al): (usize, usize, u32, u32),
    restart: usize,
    (mcux, mcuy): (usize, usize),
) {
    let mut pred = vec![0i32; comps.len()];
    let mut eobrun = 0u32;
    // the block positions a scan visits
    let single = sc.len() == 1;
    let units: Vec<(usize, usize, usize)> = if single {
        let ci = sc[0].0;
        let c = &comps[ci];
        let bw = c.w.div_ceil(8);
        let bh = c.hgt.div_ceil(8);
        let mut u = Vec::with_capacity(bw * bh);
        for by in 0..bh {
            for bx in 0..bw {
                u.push((0, bx, by));
            }
        }
        u
    } else {
        let mut u = Vec::new();
        for my in 0..mcuy {
            for mx in 0..mcux {
                for (k, &(ci, _, _)) in sc.iter().enumerate() {
                    let c = &comps[ci];
                    for v in 0..c.v {
                        for h in 0..c.h {
                            u.push((k, mx * c.h + h, my * c.v + v));
                        }
                    }
                }
            }
        }
        u
    };
    // units per restart interval are counted in MCUs
    let per_mcu = if single {
        1
    } else {
        sc.iter().map(|&(ci, _, _)| comps[ci].h * comps[ci].v).sum()
    };
    let mut mcu_count = 0usize;
    for (n, &(k, bx, by)) in units.iter().enumerate() {
        if n % per_mcu == 0 {
            if restart > 0 && mcu_count > 0 && mcu_count.is_multiple_of(restart) {
                br.restart();
                pred.iter_mut().for_each(|p| *p = 0);
                eobrun = 0;
            }
            mcu_count += 1;
        }
        let (ci, td, ta) = sc[k];
        let bw = comps[ci].bw;
        let blk = &mut comps[ci].coef[by * bw + bx];
        if !progressive {
            let t = br.decode(&dc[td]).unwrap_or(0) as u32;
            let diff = extend(br.get(t), t);
            pred[ci] += diff;
            blk[0] = pred[ci];
            let mut i = 1;
            while i < 64 {
                let rs = br.decode(&ac[ta]).unwrap_or(0) as u32;
                let (r, s) = (rs >> 4, rs & 15);
                if s == 0 {
                    if r == 15 {
                        i += 16;
                        continue;
                    }
                    break;
                }
                i += r as usize;
                if i >= 64 {
                    break;
                }
                blk[ZIGZAG[i]] = extend(br.get(s), s);
                i += 1;
            }
        } else if ss == 0 {
            // DC scan
            if ah == 0 {
                let t = br.decode(&dc[td]).unwrap_or(0) as u32;
                let diff = extend(br.get(t), t);
                pred[ci] += diff;
                blk[0] = pred[ci] << al;
            } else if br.bit() != 0 {
                blk[0] |= 1 << al;
            }
        } else if ah == 0 {
            // AC first
            if eobrun > 0 {
                eobrun -= 1;
                continue;
            }
            let mut i = ss;
            while i <= se {
                let rs = br.decode(&ac[ta]).unwrap_or(0) as u32;
                let (r, s) = (rs >> 4, rs & 15);
                if s == 0 {
                    if r < 15 {
                        eobrun = (1 << r) - 1;
                        if r > 0 {
                            eobrun += br.get(r);
                        }
                        break;
                    }
                    i += 16;
                    continue;
                }
                i += r as usize;
                if i > 63 {
                    break;
                }
                blk[ZIGZAG[i]] = extend(br.get(s), s) * (1 << al);
                i += 1;
            }
        } else {
            // AC refinement
            let p1 = 1i32 << al;
            let m1 = -1i32 << al;
            let mut i = ss;
            if eobrun == 0 {
                while i <= se {
                    let rs = br.decode(&ac[ta]).unwrap_or(0) as u32;
                    let (mut r, s) = ((rs >> 4) as i32, rs & 15);
                    let mut val = 0i32;
                    if s == 0 {
                        if r < 15 {
                            eobrun = 1 << r;
                            if r > 0 {
                                eobrun += br.get(r as u32);
                            }
                            break;
                        }
                    } else {
                        val = if br.bit() != 0 { p1 } else { m1 };
                    }
                    while i <= se {
                        let z = ZIGZAG[i];
                        if blk[z] != 0 {
                            if br.bit() != 0 && (blk[z] & p1) == 0 {
                                blk[z] += if blk[z] >= 0 { p1 } else { m1 };
                            }
                        } else {
                            if r == 0 {
                                if val != 0 {
                                    blk[z] = val;
                                }
                                i += 1;
                                break;
                            }
                            r -= 1;
                        }
                        i += 1;
                    }
                }
            }
            if eobrun > 0 {
                while i <= se {
                    let z = ZIGZAG[i];
                    if blk[z] != 0 && br.bit() != 0 && (blk[z] & p1) == 0 {
                        blk[z] += if blk[z] >= 0 { p1 } else { m1 };
                    }
                    i += 1;
                }
                eobrun -= 1;
            }
        }
    }
    // position the reader at the marker that ends the scan's data
    while br.pos + 1 < br.d.len()
        && !(br.d[br.pos] == 0xFF
            && br.d[br.pos + 1] != 0
            && !(0xD0..=0xD7).contains(&br.d[br.pos + 1]))
    {
        br.pos += 1;
    }
}

const CONST_BITS: i32 = 13;
const PASS1_BITS: i32 = 2;
const F_0_298631336: i64 = 2446;
const F_0_390180644: i64 = 3196;
const F_0_541196100: i64 = 4433;
const F_0_765366865: i64 = 6270;
const F_0_899976223: i64 = 7373;
const F_1_175875602: i64 = 9633;
const F_1_501321110: i64 = 12299;
const F_1_847759065: i64 = 15137;
const F_1_961570560: i64 = 16069;
const F_2_053119869: i64 = 16819;
const F_2_562915447: i64 = 20995;
const F_3_072711026: i64 = 25172;

#[inline]
fn descale(x: i64, n: i32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// libjpeg's output range limit: the centred value wrapped to 10 bits,
/// then clamped (so overflow beyond ±512 wraps, as libjpeg's table does).
#[inline]
fn range_limit(x: i64) -> u8 {
    let t = (x & 1023) as i32;
    if t < 128 {
        (t + 128) as u8
    } else if t < 512 {
        255
    } else if t < 896 {
        0
    } else {
        (t - 896) as u8
    }
}

/// The ISLOW IDCT of one dequantized block (natural order coefficients).
pub fn idct_islow(coef: &[i32; 64], q: &[i32; 64], out: &mut [u8; 64]) {
    let mut ws = [0i64; 64];
    for col in 0..8 {
        let c = |r: usize| (coef[r * 8 + col] as i64) * (q[r * 8 + col] as i64);
        if (1..8).all(|r| coef[r * 8 + col] == 0) {
            let dcval = c(0) << PASS1_BITS;
            for r in 0..8 {
                ws[r * 8 + col] = dcval;
            }
            continue;
        }
        let (z2, z3) = (c(2), c(6));
        let z1 = (z2 + z3) * F_0_541196100;
        let tmp2 = z1 + z3 * -F_1_847759065;
        let tmp3 = z1 + z2 * F_0_765366865;
        let (z2, z3) = (c(0), c(4));
        let tmp0 = (z2 + z3) << CONST_BITS;
        let tmp1 = (z2 - z3) << CONST_BITS;
        let (tmp10, tmp13, tmp11, tmp12) = (tmp0 + tmp3, tmp0 - tmp3, tmp1 + tmp2, tmp1 - tmp2);
        let (mut t0, mut t1, mut t2, mut t3) = (c(7), c(5), c(3), c(1));
        let (mut z1, mut z2, mut z3, mut z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * F_1_175875602;
        t0 *= F_0_298631336;
        t1 *= F_2_053119869;
        t2 *= F_3_072711026;
        t3 *= F_1_501321110;
        z1 *= -F_0_899976223;
        z2 *= -F_2_562915447;
        z3 *= -F_1_961570560;
        z4 *= -F_0_390180644;
        z3 += z5;
        z4 += z5;
        t0 += z1 + z3;
        t1 += z2 + z4;
        t2 += z2 + z3;
        t3 += z1 + z4;
        let n = CONST_BITS - PASS1_BITS;
        ws[col] = descale(tmp10 + t3, n);
        ws[7 * 8 + col] = descale(tmp10 - t3, n);
        ws[8 + col] = descale(tmp11 + t2, n);
        ws[6 * 8 + col] = descale(tmp11 - t2, n);
        ws[2 * 8 + col] = descale(tmp12 + t1, n);
        ws[5 * 8 + col] = descale(tmp12 - t1, n);
        ws[3 * 8 + col] = descale(tmp13 + t0, n);
        ws[4 * 8 + col] = descale(tmp13 - t0, n);
    }
    for row in 0..8 {
        let w = &ws[row * 8..row * 8 + 8];
        let o = &mut out[row * 8..row * 8 + 8];
        if w[1..].iter().all(|&v| v == 0) {
            let v = range_limit(descale(w[0], PASS1_BITS + 3));
            o.iter_mut().for_each(|x| *x = v);
            continue;
        }
        let (z2, z3) = (w[2], w[6]);
        let z1 = (z2 + z3) * F_0_541196100;
        let tmp2 = z1 + z3 * -F_1_847759065;
        let tmp3 = z1 + z2 * F_0_765366865;
        let tmp0 = (w[0] + w[4]) << CONST_BITS;
        let tmp1 = (w[0] - w[4]) << CONST_BITS;
        let (tmp10, tmp13, tmp11, tmp12) = (tmp0 + tmp3, tmp0 - tmp3, tmp1 + tmp2, tmp1 - tmp2);
        let (mut t0, mut t1, mut t2, mut t3) = (w[7], w[5], w[3], w[1]);
        let (mut z1, mut z2, mut z3, mut z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * F_1_175875602;
        t0 *= F_0_298631336;
        t1 *= F_2_053119869;
        t2 *= F_3_072711026;
        t3 *= F_1_501321110;
        z1 *= -F_0_899976223;
        z2 *= -F_2_562915447;
        z3 *= -F_1_961570560;
        z4 *= -F_0_390180644;
        z3 += z5;
        z4 += z5;
        t0 += z1 + z3;
        t1 += z2 + z4;
        t2 += z2 + z3;
        t3 += z1 + z4;
        let n = CONST_BITS + PASS1_BITS + 3;
        o[0] = range_limit(descale(tmp10 + t3, n));
        o[7] = range_limit(descale(tmp10 - t3, n));
        o[1] = range_limit(descale(tmp11 + t2, n));
        o[6] = range_limit(descale(tmp11 - t2, n));
        o[2] = range_limit(descale(tmp12 + t1, n));
        o[5] = range_limit(descale(tmp12 - t1, n));
        o[3] = range_limit(descale(tmp13 + t0, n));
        o[4] = range_limit(descale(tmp13 - t0, n));
    }
}
