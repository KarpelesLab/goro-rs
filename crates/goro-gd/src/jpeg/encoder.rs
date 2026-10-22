//! A JPEG compressor reproducing libjpeg-turbo with the settings libgd
//! uses: jpeg_set_defaults() (YCbCr, 2x2 luma sampling, islow DCT, the
//! standard Huffman tables), jpeg_set_quality(q, TRUE) and, for
//! interlaced images, jpeg_simple_progression() (which implies optimized
//! Huffman tables).
//!
//! Ported pieces: jccolor.c (rgb_ycc_convert), jcsample.c
//! (fullsize/h2v2 downsampling), jcprepct.c (edge padding), jfdctint.c,
//! jcdctmgr.c (reciprocal quantization), jccoefct.c (dummy blocks),
//! jchuff.c / jcphuff.c (entropy coding, jpeg_gen_optimal_table) and
//! jcmarker.c.

use super::tables::*;

/// What the compressor needs from the caller
pub struct EncodeParams {
    pub width: u32,
    pub height: u32,
    /// -1: library default (75) without calling jpeg_set_quality()
    pub quality: i32,
    pub density_unit: u8,
    pub x_density: u16,
    pub y_density: u16,
    pub progressive: bool,
    /// 4:4:4 instead of jpeg_set_defaults()'s 2x2 luma sampling (newer
    /// libgd versions do this for quality >= 90; the bundled one does not)
    pub sampling_444: bool,
    /// A COM marker written right after the file header
    pub comment: Option<Vec<u8>>,
}

/// jpeg_quality_scaling()
fn quality_scaling(q: i32) -> i64 {
    let q = q.clamp(1, 100);
    if q < 50 {
        5000 / q as i64
    } else {
        200 - q as i64 * 2
    }
}

/// jpeg_add_quant_table() with force_baseline
fn make_quant(basic: &[u32; 64], scale: i64) -> [u16; 64] {
    let mut t = [0u16; 64];
    for i in 0..64 {
        let temp = ((basic[i] as i64 * scale + 50) / 100)
            .clamp(1, 32767)
            .min(255);
        t[i] = temp as u16;
    }
    t
}

/// compute_reciprocal() of jcdctmgr.c (16-bit DCTELEM): (recip, corr, shift)
fn reciprocal(divisor: u32) -> (u64, u64, u32) {
    if divisor == 1 {
        return (1, 0, 0);
    }
    let b = 31 - divisor.leading_zeros(); // flss(divisor) - 1
    let mut r = 16 + b;
    let mut fq = (1u64 << r) / divisor as u64;
    let fr = (1u64 << r) % divisor as u64;
    let mut c = (divisor / 2) as u64;
    if fr == 0 {
        fq >>= 1;
        r -= 1;
    } else if fr <= (divisor / 2) as u64 {
        c += 1;
    } else {
        fq += 1;
    }
    (fq, c, r)
}

struct Divisors {
    d: [(u64, u64, u32); 64],
}

impl Divisors {
    fn new(q: &[u16; 64]) -> Divisors {
        let mut d = [(0, 0, 0); 64];
        for i in 0..64 {
            d[i] = reciprocal((q[i] as u32) << 3);
        }
        Divisors { d }
    }

    fn quantize(&self, ws: &[i32; 64]) -> [i16; 64] {
        let mut out = [0i16; 64];
        for i in 0..64 {
            let (recip, corr, shift) = self.d[i];
            let temp = ws[i];
            let neg = temp < 0;
            let a = temp.unsigned_abs() as u64;
            let p = ((a + corr) * recip) >> shift;
            let v = p as i32;
            out[i] = if neg { -v } else { v } as i16;
        }
        out
    }
}

/// jpeg_fdct_islow()
fn fdct_islow(data: &mut [i32; 64]) {
    let cb = CONST_BITS;
    let pb = PASS1_BITS;
    for r in 0..8 {
        let d = &mut data[r * 8..r * 8 + 8];
        let tmp0 = (d[0] + d[7]) as i64;
        let tmp7 = (d[0] - d[7]) as i64;
        let tmp1 = (d[1] + d[6]) as i64;
        let tmp6 = (d[1] - d[6]) as i64;
        let tmp2 = (d[2] + d[5]) as i64;
        let tmp5 = (d[2] - d[5]) as i64;
        let tmp3 = (d[3] + d[4]) as i64;
        let tmp4 = (d[3] - d[4]) as i64;
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        d[0] = ((tmp10 + tmp11) << pb) as i32;
        d[4] = ((tmp10 - tmp11) << pb) as i32;
        let z1 = (tmp12 + tmp13) * FIX_0_541196100;
        d[2] = descale(z1 + tmp13 * FIX_0_765366865, cb - pb) as i32;
        d[6] = descale(z1 + tmp12 * (-FIX_1_847759065), cb - pb) as i32;
        let (o7, o5, o3, o1) = odd(tmp4, tmp5, tmp6, tmp7);
        d[7] = descale(o7, cb - pb) as i32;
        d[5] = descale(o5, cb - pb) as i32;
        d[3] = descale(o3, cb - pb) as i32;
        d[1] = descale(o1, cb - pb) as i32;
    }
    for c in 0..8 {
        let g = |i: usize| data[i * 8 + c] as i64;
        let tmp0 = g(0) + g(7);
        let tmp7 = g(0) - g(7);
        let tmp1 = g(1) + g(6);
        let tmp6 = g(1) - g(6);
        let tmp2 = g(2) + g(5);
        let tmp5 = g(2) - g(5);
        let tmp3 = g(3) + g(4);
        let tmp4 = g(3) - g(4);
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        let mut o = [0i64; 8];
        o[0] = descale(tmp10 + tmp11, pb);
        o[4] = descale(tmp10 - tmp11, pb);
        let z1 = (tmp12 + tmp13) * FIX_0_541196100;
        o[2] = descale(z1 + tmp13 * FIX_0_765366865, cb + pb);
        o[6] = descale(z1 + tmp12 * (-FIX_1_847759065), cb + pb);
        let (o7, o5, o3, o1) = odd(tmp4, tmp5, tmp6, tmp7);
        o[7] = descale(o7, cb + pb);
        o[5] = descale(o5, cb + pb);
        o[3] = descale(o3, cb + pb);
        o[1] = descale(o1, cb + pb);
        for i in 0..8 {
            data[i * 8 + c] = o[i] as i32;
        }
    }
}

/// The odd part of jfdctint.c: (out7, out5, out3, out1) before descaling
fn odd(tmp4: i64, tmp5: i64, tmp6: i64, tmp7: i64) -> (i64, i64, i64, i64) {
    let z1 = tmp4 + tmp7;
    let z2 = tmp5 + tmp6;
    let z3 = tmp4 + tmp6;
    let z4 = tmp5 + tmp7;
    let z5 = (z3 + z4) * FIX_1_175875602;
    let tmp4 = tmp4 * FIX_0_298631336;
    let tmp5 = tmp5 * FIX_2_053119869;
    let tmp6 = tmp6 * FIX_3_072711026;
    let tmp7 = tmp7 * FIX_1_501321110;
    let z1 = z1 * -FIX_0_899976223;
    let z2 = z2 * -FIX_2_562915447;
    let z3 = z3 * -FIX_1_961570560 + z5;
    let z4 = z4 * -FIX_0_390180644 + z5;
    (
        tmp4 + z1 + z3,
        tmp5 + z2 + z4,
        tmp6 + z2 + z3,
        tmp7 + z1 + z4,
    )
}

/// rgb_ycc_convert(): the fixed point tables of jccolor.c
fn rgb_to_ycc(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    const SCALEBITS: i32 = 16;
    const ONE_HALF: i64 = 1 << (SCALEBITS - 1);
    const CBCR_OFFSET: i64 = 128 << SCALEBITS;
    let fix = |x: f64| -> i64 { (x * 65536.0 + 0.5) as i64 };
    let (r, g, b) = (r as i64, g as i64, b as i64);
    let y = (fix(0.29900) * r + fix(0.58700) * g + fix(0.11400) * b + ONE_HALF) >> SCALEBITS;
    let cb = (-fix(0.16874) * r - fix(0.33126) * g + fix(0.5) * b + CBCR_OFFSET + ONE_HALF - 1)
        >> SCALEBITS;
    let cr = (fix(0.5) * r - fix(0.41869) * g - fix(0.08131) * b + CBCR_OFFSET + ONE_HALF - 1)
        >> SCALEBITS;
    (y as u8, cb as u8, cr as u8)
}

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    dc_tbl: usize,
    ac_tbl: usize,
    width_in_blocks: usize,
    height_in_blocks: usize,
    /// blocks across/down including the dummy blocks of partial MCUs
    bw: usize,
    coefs: Vec<[i16; 64]>,
}

/// Canonical code assignment (jpeg_make_c_derived_tbl)
struct CTable {
    code: [u32; 256],
    size: [u8; 256],
}

fn derive(spec: &HuffSpec) -> CTable {
    let mut t = CTable {
        code: [0; 256],
        size: [0; 256],
    };
    let mut code = 0u32;
    let mut p = 0usize;
    for l in 1..=16 {
        for _ in 0..spec.bits[l] {
            let sym = spec.vals[p] as usize;
            t.code[sym] = code;
            t.size[sym] = l as u8;
            code += 1;
            p += 1;
        }
        code <<= 1;
    }
    t
}

/// jpeg_gen_optimal_table() (jchuff.c)
#[allow(clippy::needless_range_loop)]
fn gen_optimal_table(freq_in: &[i64; 257]) -> HuffSpec {
    const MAX_CLEN: usize = 32;
    let mut freq = *freq_in;
    let mut bits = [0u8; MAX_CLEN + 1];
    let mut codesize = [0i32; 257];
    let mut others = [-1i32; 257];
    freq[256] = 1;
    loop {
        let mut c1: i32 = -1;
        let mut v: i64 = 1_000_000_000;
        for i in 0..=256 {
            if freq[i] != 0 && freq[i] <= v {
                v = freq[i];
                c1 = i as i32;
            }
        }
        let mut c2: i32 = -1;
        v = 1_000_000_000;
        for i in 0..=256 {
            if freq[i] != 0 && freq[i] <= v && i as i32 != c1 {
                v = freq[i];
                c2 = i as i32;
            }
        }
        if c2 < 0 {
            break;
        }
        let (mut c1, mut c2) = (c1 as usize, c2 as usize);
        freq[c1] += freq[c2];
        freq[c2] = 0;
        codesize[c1] += 1;
        while others[c1] >= 0 {
            c1 = others[c1] as usize;
            codesize[c1] += 1;
        }
        others[c1] = c2 as i32;
        codesize[c2] += 1;
        while others[c2] >= 0 {
            c2 = others[c2] as usize;
            codesize[c2] += 1;
        }
    }
    for &cs in codesize.iter() {
        if cs != 0 {
            bits[(cs as usize).min(MAX_CLEN)] += 1;
        }
    }
    let mut i = MAX_CLEN;
    while i > 16 {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
        i -= 1;
    }
    while bits[i] == 0 {
        i -= 1;
    }
    bits[i] -= 1;
    let mut spec = HuffSpec {
        bits: [0; 17],
        vals: Vec::new(),
    };
    spec.bits.copy_from_slice(&bits[..17]);
    for i in 1..=MAX_CLEN {
        for j in 0..=255 {
            if codesize[j] == i as i32 {
                spec.vals.push(j as u8);
            }
        }
    }
    spec
}

/// The entropy coder state (jchuff.c / jcphuff.c working_state)
struct BitWriter {
    out: Vec<u8>,
    put_buffer: u32,
    put_bits: u32,
}

impl BitWriter {
    fn emit_bits(&mut self, code: u32, size: u32) {
        if size == 0 {
            return;
        }
        let mut pb = code & ((1u32 << size) - 1);
        self.put_bits += size;
        pb <<= 24 - self.put_bits;
        pb |= self.put_buffer;
        while self.put_bits >= 8 {
            let c = ((pb >> 16) & 0xff) as u8;
            self.out.push(c);
            if c == 0xff {
                self.out.push(0);
            }
            pb <<= 8;
            self.put_bits -= 8;
        }
        self.put_buffer = pb & 0xffffff;
    }

    /// flush_bits(): pad with 1 bits
    fn flush(&mut self) {
        self.emit_bits(0x7f, 7);
        self.put_buffer = 0;
        self.put_bits = 0;
    }
}

fn nbits(mut v: u32) -> u32 {
    let mut n = 0;
    while v != 0 {
        n += 1;
        v >>= 1;
    }
    n
}

/// Either counts symbols (optimization pass) or writes them
enum Sink<'a> {
    Count(&'a mut [[i64; 257]; 4]),
    Write(&'a mut BitWriter, &'a [Option<CTable>; 4]),
}

impl Sink<'_> {
    fn symbol(&mut self, tbl: usize, sym: u32) {
        match self {
            Sink::Count(c) => c[tbl][sym as usize] += 1,
            Sink::Write(w, t) => {
                let t = t[tbl].as_ref().unwrap();
                let s = sym as usize;
                w.emit_bits(t.code[s], t.size[s] as u32);
            }
        }
    }
    fn bits(&mut self, code: u32, size: u32) {
        if let Sink::Write(w, _) = self {
            w.emit_bits(code, size);
        }
    }
    fn flush(&mut self) {
        if let Sink::Write(w, _) = self {
            w.flush();
        }
    }
}

/// One component of a scan, its blocks visited in MCU order
fn scan_blocks(
    comps: &[Comp],
    scan: &[usize],
    mcus_across: usize,
    mcus_down: usize,
) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    if scan.len() == 1 {
        let c = &comps[scan[0]];
        for br in 0..c.height_in_blocks {
            for bc in 0..c.width_in_blocks {
                v.push((scan[0], br * c.bw + bc));
            }
        }
    } else {
        for my in 0..mcus_down {
            for mx in 0..mcus_across {
                for &ci in scan {
                    let c = &comps[ci];
                    for yi in 0..c.v {
                        for xi in 0..c.h {
                            v.push((ci, (my * c.v + yi) * c.bw + mx * c.h + xi));
                        }
                    }
                }
            }
        }
    }
    v
}

struct ScanDef {
    comps: Vec<usize>,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
}

/// The progressive entropy encoder of jcphuff.c for one scan
struct Phuff {
    eobrun: u32,
    be: Vec<u8>,
    ac_tbl: usize,
}

const MAX_CORR_BITS: usize = 1000;

impl Phuff {
    fn emit_eobrun(&mut self, sink: &mut Sink) {
        if self.eobrun > 0 {
            let nb = nbits(self.eobrun) - 1;
            sink.symbol(self.ac_tbl, nb << 4);
            if nb > 0 {
                sink.bits(self.eobrun, nb);
            }
            self.eobrun = 0;
            for &b in &self.be {
                sink.bits(b as u32, 1);
            }
            self.be.clear();
        }
    }
}

fn progressive_scan(sink: &mut Sink, comps: &[Comp], s: &ScanDef, blocks: &[(usize, usize)]) {
    let al = s.al;
    let ac_tbl = comps[s.comps[0]].ac_tbl;
    let mut st = Phuff {
        eobrun: 0,
        be: Vec::new(),
        ac_tbl,
    };
    let mut last_dc = [0i32; 4];
    for &(ci, bi) in blocks {
        let c = &comps[ci];
        let blk = &c.coefs[bi];
        if s.ss == 0 {
            if s.ah == 0 {
                let temp2 = (blk[0] as i32) >> al;
                let mut temp = temp2 - last_dc[ci];
                last_dc[ci] = temp2;
                let mut t2 = temp;
                if temp < 0 {
                    temp = -temp;
                    t2 -= 1;
                }
                let nb = nbits(temp as u32);
                sink.symbol(c.dc_tbl, nb);
                if nb > 0 {
                    sink.bits(t2 as u32, nb);
                }
            } else {
                sink.bits(((blk[0] as i32) >> al) as u32, 1);
            }
            continue;
        }
        if s.ah == 0 {
            // encode_mcu_AC_first
            let mut r = 0u32;
            for k in s.ss..=s.se {
                let mut temp = blk[NATURAL_ORDER[k]] as i32;
                if temp == 0 {
                    r += 1;
                    continue;
                }
                let temp2 = if temp < 0 {
                    temp = -temp;
                    temp >>= al;
                    !temp
                } else {
                    temp >>= al;
                    temp
                };
                if temp == 0 {
                    r += 1;
                    continue;
                }
                if st.eobrun > 0 {
                    st.emit_eobrun(sink);
                }
                while r > 15 {
                    sink.symbol(ac_tbl, 0xf0);
                    r -= 16;
                }
                let nb = nbits(temp as u32);
                sink.symbol(ac_tbl, (r << 4) + nb);
                sink.bits(temp2 as u32, nb);
                r = 0;
            }
            if r > 0 {
                st.eobrun += 1;
                if st.eobrun == 0x7fff {
                    st.emit_eobrun(sink);
                }
            }
        } else {
            // encode_mcu_AC_refine
            let mut absvalues = [0u32; 64];
            let mut eob = 0usize;
            for k in s.ss..=s.se {
                let temp = (blk[NATURAL_ORDER[k]] as i32).unsigned_abs() >> al;
                absvalues[k] = temp;
                if temp == 1 {
                    eob = k;
                }
            }
            let mut r = 0u32;
            let mut br: Vec<u8> = Vec::new();
            for k in s.ss..=s.se {
                let temp = absvalues[k];
                if temp == 0 {
                    r += 1;
                    continue;
                }
                while r > 15 && k <= eob {
                    st.emit_eobrun(sink);
                    sink.symbol(ac_tbl, 0xf0);
                    r -= 16;
                    for &b in &br {
                        sink.bits(b as u32, 1);
                    }
                    br.clear();
                }
                if temp > 1 {
                    br.push((temp & 1) as u8);
                    continue;
                }
                st.emit_eobrun(sink);
                sink.symbol(ac_tbl, (r << 4) + 1);
                let t = if blk[NATURAL_ORDER[k]] < 0 { 0 } else { 1 };
                sink.bits(t, 1);
                for &b in &br {
                    sink.bits(b as u32, 1);
                }
                br.clear();
                r = 0;
            }
            if r > 0 || !br.is_empty() {
                st.eobrun += 1;
                st.be.extend_from_slice(&br);
                if st.eobrun == 0x7fff || st.be.len() > MAX_CORR_BITS - 64 + 1 {
                    st.emit_eobrun(sink);
                }
            }
        }
    }
    st.emit_eobrun(sink);
    sink.flush();
}

fn marker(out: &mut Vec<u8>, m: u8, data: &[u8]) {
    out.push(0xff);
    out.push(m);
    let len = data.len() + 2;
    out.push((len >> 8) as u8);
    out.push(len as u8);
    out.extend_from_slice(data);
}

fn dht(out: &mut Vec<u8>, index: u8, spec: &HuffSpec) {
    let mut d = vec![index];
    d.extend_from_slice(&spec.bits[1..17]);
    d.extend_from_slice(&spec.vals);
    marker(out, 0xc4, &d);
}

/// Compress `rgb` (width * height * 3 bytes)
pub fn encode(p: &EncodeParams, rgb: &[u8]) -> Vec<u8> {
    let w = p.width as usize;
    let h = p.height as usize;
    let scale = quality_scaling(if p.quality < 0 { 75 } else { p.quality });
    let qt = [
        make_quant(&STD_LUMINANCE_QUANT, scale),
        make_quant(&STD_CHROMINANCE_QUANT, scale),
    ];
    let divs = [Divisors::new(&qt[0]), Divisors::new(&qt[1])];

    // Color conversion into full-size planes
    let mut planes = vec![vec![0u8; w * h]; 3];
    for i in 0..w * h {
        let (y, cb, cr) = rgb_to_ycc(rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]);
        planes[0][i] = y;
        planes[1][i] = cb;
        planes[2][i] = cr;
    }
    let samp = if p.sampling_444 {
        [(1usize, 1usize), (1, 1), (1, 1)]
    } else {
        [(2usize, 2usize), (1, 1), (1, 1)]
    };
    let (max_h, max_v) = (samp[0].0, samp[0].1);
    let mcus_across = w.div_ceil(max_h * 8);
    let mcus_down = h.div_ceil(max_v * 8);
    let mut comps: Vec<Comp> = Vec::new();
    for ci in 0..3 {
        let (hs, vs) = samp[ci];
        let wib = (w * hs).div_ceil(max_h * 8);
        let hib = (h * vs).div_ceil(max_v * 8);
        let bw = mcus_across * hs;
        let bh = mcus_down * vs;
        // the downsampled plane, padded as jcprepct.c/jcsample.c do
        let out_cols = wib * 8;
        let real_rows = h.div_ceil(max_v) * vs;
        let total_rows = mcus_down * vs * 8;
        let src = &planes[ci];
        let px = |x: usize, y: usize| -> i32 { src[y.min(h - 1) * w + x.min(w - 1)] as i32 };
        let mut d = vec![0u8; out_cols * total_rows];
        for r in 0..real_rows {
            if hs == max_h && vs == max_v {
                for c in 0..out_cols {
                    d[r * out_cols + c] = px(c, r) as u8;
                }
            } else {
                // h2v2_downsample with its 1,2,1,2 bias
                let mut bias = 1;
                for c in 0..out_cols {
                    let s = px(2 * c, 2 * r)
                        + px(2 * c + 1, 2 * r)
                        + px(2 * c, 2 * r + 1)
                        + px(2 * c + 1, 2 * r + 1);
                    d[r * out_cols + c] = ((s + bias) >> 2) as u8;
                    bias ^= 3;
                }
            }
        }
        for r in real_rows..total_rows {
            let (a, b) = d.split_at_mut(r * out_cols);
            b[..out_cols].copy_from_slice(&a[(real_rows - 1) * out_cols..real_rows * out_cols]);
        }
        let tq = if ci == 0 { 0 } else { 1 };
        let mut coefs = vec![[0i16; 64]; bw * bh];
        for br in 0..bh {
            for bc in 0..bw {
                let idx = br * bw + bc;
                if br < hib && bc < wib {
                    let mut ws = [0i32; 64];
                    for y in 0..8 {
                        for x in 0..8 {
                            ws[y * 8 + x] = d[(br * 8 + y) * out_cols + bc * 8 + x] as i32 - 128;
                        }
                    }
                    fdct_islow(&mut ws);
                    coefs[idx] = divs[tq].quantize(&ws);
                } else if br < hib {
                    coefs[idx][0] = coefs[idx - 1][0];
                } else {
                    let mcu_last = (bc / hs) * hs + hs - 1;
                    coefs[idx][0] = coefs[(br - 1) * bw + mcu_last][0];
                }
            }
        }
        comps.push(Comp {
            id: ci as u8 + 1,
            h: hs,
            v: vs,
            tq,
            dc_tbl: tq,
            ac_tbl: tq,
            width_in_blocks: wib,
            height_in_blocks: hib,
            bw,
            coefs,
        });
    }

    let mut out = vec![0xff, 0xd8];
    // JFIF APP0
    let mut app0 = b"JFIF\0".to_vec();
    app0.extend_from_slice(&[1, 1, p.density_unit]);
    app0.extend_from_slice(&p.x_density.to_be_bytes());
    app0.extend_from_slice(&p.y_density.to_be_bytes());
    app0.extend_from_slice(&[0, 0]);
    marker(&mut out, 0xe0, &app0);
    if let Some(c) = &p.comment {
        marker(&mut out, 0xfe, c);
    }
    // DQT (zigzag order, 8-bit precision)
    for (n, t) in qt.iter().enumerate() {
        let mut d = vec![n as u8];
        for k in 0..64 {
            d.push(t[NATURAL_ORDER[k]] as u8);
        }
        marker(&mut out, 0xdb, &d);
    }
    // SOF
    let mut sof = vec![8u8];
    sof.extend_from_slice(&(h as u16).to_be_bytes());
    sof.extend_from_slice(&(w as u16).to_be_bytes());
    sof.push(3);
    for c in &comps {
        sof.push(c.id);
        sof.push(((c.h << 4) | c.v) as u8);
        sof.push(c.tq as u8);
    }
    marker(&mut out, if p.progressive { 0xc2 } else { 0xc0 }, &sof);

    let progressive = p.progressive;
    let sos = |out: &mut Vec<u8>, s: &ScanDef, comps: &[Comp]| {
        let mut d = vec![s.comps.len() as u8];
        for &ci in &s.comps {
            d.push(comps[ci].id);
            let (mut td, mut ta) = (comps[ci].dc_tbl, comps[ci].ac_tbl);
            if progressive {
                // only DC or only AC tables are used in one scan; 0 for
                // the unused field(s)
                if s.ss == 0 {
                    ta = 0;
                    if s.ah != 0 {
                        td = 0;
                    }
                } else {
                    td = 0;
                }
            }
            d.push(((td << 4) | ta) as u8);
        }
        d.push(s.ss as u8);
        d.push(s.se as u8);
        d.push(((s.ah << 4) | s.al) as u8);
        marker(out, 0xda, &d);
    };

    if !p.progressive {
        let specs = [
            std_huff(true, 0),
            std_huff(true, 1),
            std_huff(false, 0),
            std_huff(false, 1),
        ];
        dht(&mut out, 0x00, &specs[0]);
        dht(&mut out, 0x10, &specs[2]);
        dht(&mut out, 0x01, &specs[1]);
        dht(&mut out, 0x11, &specs[3]);
        let s = ScanDef {
            comps: vec![0, 1, 2],
            ss: 0,
            se: 63,
            ah: 0,
            al: 0,
        };
        sos(&mut out, &s, &comps);
        // tables: 0,1 = DC, 4,5 = AC
        let tables: [Option<CTable>; 4] =
            [Some(derive(&specs[0])), Some(derive(&specs[1])), None, None];
        let actables: [Option<CTable>; 4] =
            [Some(derive(&specs[2])), Some(derive(&specs[3])), None, None];
        let blocks = scan_blocks(&comps, &s.comps, mcus_across, mcus_down);
        let mut bw = BitWriter {
            out: Vec::new(),
            put_buffer: 0,
            put_bits: 0,
        };
        // DC and AC tables are addressed as 0..3 (DC) and 4..7 (AC)
        seq_write(&mut bw, &comps, &blocks, &tables, &actables);
        out.extend_from_slice(&bw.out);
    } else {
        let scans = [
            ScanDef {
                comps: vec![0, 1, 2],
                ss: 0,
                se: 0,
                ah: 0,
                al: 1,
            },
            ScanDef {
                comps: vec![0],
                ss: 1,
                se: 5,
                ah: 0,
                al: 2,
            },
            ScanDef {
                comps: vec![2],
                ss: 1,
                se: 63,
                ah: 0,
                al: 1,
            },
            ScanDef {
                comps: vec![1],
                ss: 1,
                se: 63,
                ah: 0,
                al: 1,
            },
            ScanDef {
                comps: vec![0],
                ss: 6,
                se: 63,
                ah: 0,
                al: 2,
            },
            ScanDef {
                comps: vec![0],
                ss: 1,
                se: 63,
                ah: 2,
                al: 1,
            },
            ScanDef {
                comps: vec![0, 1, 2],
                ss: 0,
                se: 0,
                ah: 1,
                al: 0,
            },
            ScanDef {
                comps: vec![2],
                ss: 1,
                se: 63,
                ah: 1,
                al: 0,
            },
            ScanDef {
                comps: vec![1],
                ss: 1,
                se: 63,
                ah: 1,
                al: 0,
            },
            ScanDef {
                comps: vec![0],
                ss: 1,
                se: 63,
                ah: 1,
                al: 0,
            },
        ];
        for s in &scans {
            let blocks = scan_blocks(&comps, &s.comps, mcus_across, mcus_down);
            let is_dc = s.ss == 0;
            let mut tables: [Option<CTable>; 4] = [None, None, None, None];
            if !(is_dc && s.ah != 0) {
                // gather pass
                let mut counts = [[0i64; 257]; 4];
                {
                    let mut sink = Sink::Count(&mut counts);
                    progressive_scan(&mut sink, &comps, s, &blocks);
                }
                let mut did = [false; 4];
                for &ci in &s.comps {
                    let tbl = if is_dc {
                        comps[ci].dc_tbl
                    } else {
                        comps[ci].ac_tbl
                    };
                    if !did[tbl] {
                        let spec = gen_optimal_table(&counts[tbl]);
                        dht(
                            &mut out,
                            if is_dc { tbl as u8 } else { 0x10 | tbl as u8 },
                            &spec,
                        );
                        tables[tbl] = Some(derive(&spec));
                        did[tbl] = true;
                    }
                }
            }
            sos(&mut out, s, &comps);
            let mut bw = BitWriter {
                out: Vec::new(),
                put_buffer: 0,
                put_bits: 0,
            };
            {
                let mut sink = Sink::Write(&mut bw, &tables);
                progressive_scan(&mut sink, &comps, s, &blocks);
            }
            out.extend_from_slice(&bw.out);
        }
    }
    out.push(0xff);
    out.push(0xd9);
    out
}

fn seq_write(
    bw: &mut BitWriter,
    comps: &[Comp],
    blocks: &[(usize, usize)],
    dc: &[Option<CTable>; 4],
    ac: &[Option<CTable>; 4],
) {
    // A sink that maps tables 0..3 to DC and 4..7 to AC
    struct SeqSink<'a> {
        w: &'a mut BitWriter,
        dc: &'a [Option<CTable>; 4],
        ac: &'a [Option<CTable>; 4],
    }
    impl SeqSink<'_> {
        fn symbol(&mut self, tbl: usize, sym: u32) {
            let t = if tbl >= 4 {
                self.ac[tbl - 4].as_ref()
            } else {
                self.dc[tbl].as_ref()
            }
            .unwrap();
            self.w
                .emit_bits(t.code[sym as usize], t.size[sym as usize] as u32);
        }
    }
    let mut s = SeqSink { w: bw, dc, ac };
    let mut last_dc = [0i32; 4];
    for &(ci, bi) in blocks {
        let c = &comps[ci];
        let blk = &c.coefs[bi];
        let mut temp = blk[0] as i32 - last_dc[ci];
        last_dc[ci] = blk[0] as i32;
        let mut temp2 = temp;
        if temp < 0 {
            temp = -temp;
            temp2 -= 1;
        }
        let nb = nbits(temp as u32);
        s.symbol(c.dc_tbl, nb);
        if nb > 0 {
            s.w.emit_bits(temp2 as u32, nb);
        }
        let mut r = 0u32;
        for k in 1..64 {
            let mut temp = blk[NATURAL_ORDER[k]] as i32;
            if temp == 0 {
                r += 1;
                continue;
            }
            while r > 15 {
                s.symbol(4 + c.ac_tbl, 0xf0);
                r -= 16;
            }
            let mut temp2 = temp;
            if temp < 0 {
                temp = -temp;
                temp2 -= 1;
            }
            let nb = nbits(temp as u32);
            s.symbol(4 + c.ac_tbl, (r << 4) + nb);
            s.w.emit_bits(temp2 as u32, nb);
            r = 0;
        }
        if r > 0 {
            s.symbol(4 + c.ac_tbl, 0);
        }
    }
    s.w.flush();
}
