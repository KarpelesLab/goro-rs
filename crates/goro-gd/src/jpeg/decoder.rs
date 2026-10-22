//! A JPEG decompressor reproducing libjpeg-turbo's default decoding
//! (jdmarker.c, jdinput.c, jdhuff.c, jdphuff.c, jidctint.c, jdsample.c
//! with fancy upsampling, jdcolor.c), incl. its handling of corrupt and
//! truncated data (warnings, zero-filled data, fake EOI markers).
//!
//! All scans are decoded into a full coefficient buffer before the output
//! pass; for single-scan files this gives the same pixels as libjpeg's
//! one-pass decoding. Not emulated: block smoothing of incomplete
//! progressive files, arithmetic coding, DNL, 12/16-bit and lossless.

use super::tables::*;
use crate::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSpace {
    Unknown,
    Grayscale,
    Rgb,
    YCbCr,
    Cmyk,
    Ycck,
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
    downsampled_width: usize,
    downsampled_height: usize,
    /// blocks across / down of the coefficient buffer (MCU padded)
    bw: usize,
    bh: usize,
    coefs: Vec<[i16; 64]>,
    quant: Option<[u16; 64]>,
    coef_bits: [i32; 64],
    /// the coef_bits before the current scan (cinfo->coef_bits[ci + n])
    prev_coef_bits: [i32; 64],
}

/// d_derived_tbl
struct DTable {
    maxcode: [i64; 18],
    valoffset: [i64; 18],
    vals: Vec<u8>,
}

enum MarkerResult {
    Sos,
    Eoi,
}

type Res<T> = Result<T, String>;

const MIN_GET_BITS: i32 = 57;

pub struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    /// end of the 4096-byte chunk gd's fill_input_buffer() last read
    chunk_end: usize,
    fake: Vec<u8>,
    ignore_warning: bool,
    pub num_warnings: u32,

    pub image_width: u32,
    pub image_height: u32,
    pub density_unit: u8,
    pub x_density: u16,
    pub y_density: u16,
    /// an APP14 "Adobe" marker of at least 12 bytes was seen
    pub saw_adobe: bool,
    adobe_transform: u8,
    saw_jfif: bool,
    saw_soi: bool,
    saw_sof: bool,
    progressive: bool,
    arith: bool,
    precision: u8,
    comps: Vec<Comp>,
    qtables: [Option<[u16; 64]>; 4],
    dc_specs: [Option<HuffSpec>; 4],
    ac_specs: [Option<HuffSpec>; 4],
    restart_interval: u32,
    unread_marker: u8,
    discarded_bytes: u32,
    next_restart_num: u32,
    pub color_space: ColorSpace,
    has_multiple_scans: bool,
    max_h: usize,
    max_v: usize,
    input_scan_number: u32,
    last_good_imcu_row: usize,

    // current scan
    scan: Vec<usize>,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,

    // entropy decoder state
    get_buffer: u64,
    bits_left: i32,
    insufficient: bool,
    restarts_to_go: u32,
    last_dc: [i32; 4],
    eobrun: u32,
    dc_tbls: [Option<DTable>; 4],
    ac_tbls: [Option<DTable>; 4],
    arith_dc_l: [u8; 16],
    arith_dc_u: [u8; 16],
    arith_ac_k: [u8; 16],
    ar: Arith,
}

impl<'a> Decoder<'a> {
    pub fn new(data: &'a [u8], ignore_warning: bool) -> Decoder<'a> {
        Decoder {
            data,
            pos: 0,
            chunk_end: 0,
            fake: Vec::new(),
            ignore_warning,
            num_warnings: 0,
            image_width: 0,
            image_height: 0,
            density_unit: 0,
            x_density: 1,
            y_density: 1,
            saw_adobe: false,
            adobe_transform: 0,
            saw_jfif: false,
            saw_soi: false,
            saw_sof: false,
            progressive: false,
            arith: false,
            precision: 8,
            comps: Vec::new(),
            qtables: [None; 4],
            dc_specs: [None, None, None, None],
            ac_specs: [None, None, None, None],
            restart_interval: 0,
            unread_marker: 0,
            discarded_bytes: 0,
            next_restart_num: 0,
            color_space: ColorSpace::Unknown,
            has_multiple_scans: false,
            max_h: 1,
            max_v: 1,
            input_scan_number: 0,
            last_good_imcu_row: 0,
            scan: Vec::new(),
            ss: 0,
            se: 0,
            ah: 0,
            al: 0,
            get_buffer: 0,
            bits_left: 0,
            insufficient: false,
            restarts_to_go: 0,
            last_dc: [0; 4],
            eobrun: 0,
            dc_tbls: [None, None, None, None],
            ac_tbls: [None, None, None, None],
            arith_dc_l: [0; 16],
            arith_dc_u: [1; 16],
            arith_ac_k: [5; 16],
            ar: Arith::new(),
        }
    }

    /// emit_message() of gd_jpeg.c for warnings
    fn warn(&mut self, msg: String) {
        if self.num_warnings == 0 && !self.ignore_warning {
            gd_error(format!("gd-jpeg, libjpeg: recoverable error: {}\n", msg));
        }
        self.num_warnings += 1;
    }

    /// The gdIOCtx source manager: a fake EOI after the data
    fn byte(&mut self) -> Res<u8> {
        if self.pos < self.chunk_end {
            let b = self.data[self.pos];
            self.pos += 1;
            return Ok(b);
        }
        if !self.fake.is_empty() {
            return Ok(self.fake.remove(0));
        }
        if self.pos < self.data.len() {
            self.chunk_end = (self.pos + 4096).min(self.data.len());
            let b = self.data[self.pos];
            self.pos += 1;
            return Ok(b);
        }
        if self.data.is_empty() {
            return Err(msg::INPUT_EMPTY.into());
        }
        self.warn(msg::JPEG_EOF.into());
        self.fake = vec![0xd9];
        Ok(0xff)
    }

    fn word(&mut self) -> Res<u32> {
        let a = self.byte()? as u32;
        let b = self.byte()? as u32;
        Ok((a << 8) | b)
    }

    fn skip(&mut self, mut n: i64) -> Res<()> {
        while n > 0 {
            self.byte()?;
            n -= 1;
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // markers (jdmarker.c)

    fn first_marker(&mut self) -> Res<()> {
        let c = self.byte()?;
        let c2 = self.byte()?;
        if c != 0xff || c2 != 0xd8 {
            return Err(format!(
                "Not a JPEG file: starts with 0x{:02x} 0x{:02x}",
                c, c2
            ));
        }
        self.unread_marker = c2;
        Ok(())
    }

    fn next_marker(&mut self) -> Res<()> {
        let mut c;
        loop {
            c = self.byte()?;
            while c != 0xff {
                self.discarded_bytes += 1;
                c = self.byte()?;
            }
            loop {
                c = self.byte()?;
                if c != 0xff {
                    break;
                }
            }
            if c != 0 {
                break;
            }
            self.discarded_bytes += 2;
        }
        if self.discarded_bytes != 0 {
            let m = format!(
                "Corrupt JPEG data: {} extraneous bytes before marker 0x{:02x}",
                self.discarded_bytes, c
            );
            self.warn(m);
            self.discarded_bytes = 0;
        }
        self.unread_marker = c;
        Ok(())
    }

    fn read_markers(&mut self) -> Res<MarkerResult> {
        loop {
            if self.unread_marker == 0 {
                if !self.saw_soi {
                    self.first_marker()?;
                } else {
                    self.next_marker()?;
                }
            }
            let m = self.unread_marker;
            match m {
                0xd8 => self.get_soi()?,
                0xc0 | 0xc1 => self.get_sof(false, false)?,
                0xc2 => self.get_sof(true, false)?,
                0xc9 => self.get_sof(false, true)?,
                0xca => self.get_sof(true, true)?,
                0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc8 | 0xcb | 0xcd | 0xce | 0xcf => {
                    return Err(format!("Unsupported JPEG process: SOF type 0x{:02x}", m));
                }
                0xda => {
                    self.get_sos()?;
                    self.unread_marker = 0;
                    return Ok(MarkerResult::Sos);
                }
                0xd9 => {
                    self.unread_marker = 0;
                    return Ok(MarkerResult::Eoi);
                }
                0xc4 => self.get_dht()?,
                0xdb => self.get_dqt()?,
                0xdd => self.get_dri()?,
                0xe0 | 0xee => self.get_appn(m)?,
                0xcc => self.get_dac()?,
                0xe1..=0xef | 0xfe | 0xdc => self.skip_variable()?,
                0xd0..=0xd7 | 0x01 => {}
                _ => return Err(format!("Unsupported marker type 0x{:02x}", m)),
            }
            self.unread_marker = 0;
        }
    }

    fn get_soi(&mut self) -> Res<()> {
        if self.saw_soi {
            return Err(msg::SOI_DUPLICATE.into());
        }
        self.restart_interval = 0;
        self.arith_dc_l = [0; 16];
        self.arith_dc_u = [1; 16];
        self.arith_ac_k = [5; 16];
        self.saw_jfif = false;
        self.saw_adobe = false;
        self.adobe_transform = 0;
        self.density_unit = 0;
        self.x_density = 1;
        self.y_density = 1;
        self.saw_soi = true;
        Ok(())
    }

    fn get_sof(&mut self, prog: bool, arith: bool) -> Res<()> {
        if self.saw_sof {
            return Err(msg::SOF_DUPLICATE.into());
        }
        self.progressive = prog;
        self.arith = arith;
        let length = self.word()? as i64;
        self.precision = self.byte()?;
        let height = self.word()?;
        let width = self.word()?;
        let n = self.byte()? as usize;
        let length = length - 8;
        if height == 0 || width == 0 || n == 0 {
            return Err(msg::EMPTY_IMAGE.into());
        }
        if length != (n * 3) as i64 {
            return Err(msg::BAD_LENGTH.into());
        }
        self.image_height = height;
        self.image_width = width;
        self.comps.clear();
        for _ in 0..n {
            let id = self.byte()?;
            let c = self.byte()?;
            let tq = self.byte()?;
            self.comps.push(Comp {
                id,
                h: ((c >> 4) & 15) as usize,
                v: (c & 15) as usize,
                tq: tq as usize,
                dc_tbl: 0,
                ac_tbl: 0,
                width_in_blocks: 0,
                height_in_blocks: 0,
                downsampled_width: 0,
                downsampled_height: 0,
                bw: 0,
                bh: 0,
                coefs: Vec::new(),
                quant: None,
                coef_bits: [-1; 64],
                prev_coef_bits: [0; 64],
            });
        }
        self.saw_sof = true;
        Ok(())
    }

    fn get_sos(&mut self) -> Res<()> {
        if !self.saw_sof {
            return Err(msg::SOS_NO_SOF.into());
        }
        let length = self.word()? as usize;
        let n = self.byte()? as usize;
        if length != n * 2 + 6 || !(1..=4).contains(&n) {
            return Err(msg::BAD_LENGTH.into());
        }
        let mut scan = Vec::with_capacity(n);
        for _ in 0..n {
            let cc = self.byte()?;
            let c = self.byte()?;
            let mut found = None;
            for (ci, comp) in self.comps.iter().enumerate().take(4) {
                if comp.id == cc && !scan.contains(&ci) {
                    found = Some(ci);
                    break;
                }
            }
            let Some(ci) = found else {
                return Err(format!("Invalid component ID {} in SOS", cc));
            };
            scan.push(ci);
            self.comps[ci].dc_tbl = ((c >> 4) & 15) as usize;
            self.comps[ci].ac_tbl = (c & 15) as usize;
        }
        self.ss = self.byte()? as usize;
        self.se = self.byte()? as usize;
        let a = self.byte()?;
        self.ah = ((a >> 4) & 15) as u32;
        self.al = (a & 15) as u32;
        self.scan = scan;
        self.next_restart_num = 0;
        self.input_scan_number += 1;
        Ok(())
    }

    fn get_dht(&mut self) -> Res<()> {
        let mut length = self.word()? as i64 - 2;
        while length > 16 {
            let index = self.byte()? as usize;
            let mut bits = [0u8; 17];
            let mut count = 0usize;
            for b in bits.iter_mut().skip(1) {
                *b = self.byte()?;
                count += *b as usize;
            }
            length -= 17;
            if count > 256 || count as i64 > length {
                return Err(msg::BAD_HUFF_TABLE.into());
            }
            let mut vals = Vec::with_capacity(count);
            for _ in 0..count {
                vals.push(self.byte()?);
            }
            length -= count as i64;
            let spec = HuffSpec { bits, vals };
            if index & 0x10 != 0 {
                let i = index - 0x10;
                if i >= 4 {
                    return Err(format!("Bogus DHT index {}", i));
                }
                self.ac_specs[i] = Some(spec);
            } else {
                if index >= 4 {
                    return Err(format!("Bogus DHT index {}", index));
                }
                self.dc_specs[index] = Some(spec);
            }
        }
        if length != 0 {
            return Err(msg::BAD_LENGTH.into());
        }
        Ok(())
    }

    fn get_dqt(&mut self) -> Res<()> {
        let mut length = self.word()? as i64 - 2;
        while length > 0 {
            length -= 1;
            let n = self.byte()?;
            let prec = n >> 4;
            let n = (n & 15) as usize;
            if n >= 4 {
                return Err(format!("Bogus DQT index {}", n));
            }
            let mut t = [0u16; 64];
            for k in 0..64 {
                let v = if prec != 0 {
                    self.word()?
                } else {
                    self.byte()? as u32
                };
                t[NATURAL_ORDER[k]] = v as u16;
            }
            self.qtables[n] = Some(t);
            length -= 64;
            if prec != 0 {
                length -= 64;
            }
        }
        if length != 0 {
            return Err(msg::BAD_LENGTH.into());
        }
        Ok(())
    }

    fn get_dac(&mut self) -> Res<()> {
        let mut length = self.word()? as i64 - 2;
        while length > 0 {
            let index = self.byte()? as usize;
            let val = self.byte()?;
            length -= 2;
            if index >= 32 {
                return Err(format!("Bogus DAC index {}", index));
            }
            if index >= 16 {
                self.arith_ac_k[index - 16] = val;
            } else {
                self.arith_dc_l[index] = val & 0x0f;
                self.arith_dc_u[index] = val >> 4;
                if self.arith_dc_l[index] > self.arith_dc_u[index] {
                    return Err(format!("Bogus DAC value 0x{:x}", val));
                }
            }
        }
        if length != 0 {
            return Err(msg::BAD_LENGTH.into());
        }
        Ok(())
    }

    fn get_dri(&mut self) -> Res<()> {
        let length = self.word()?;
        if length != 4 {
            return Err(msg::BAD_LENGTH.into());
        }
        self.restart_interval = self.word()?;
        Ok(())
    }

    fn skip_variable(&mut self) -> Res<()> {
        let length = self.word()? as i64 - 2;
        self.skip(length)
    }

    /// get_interesting_appn() / save_marker() for APP0 and APP14
    fn get_appn(&mut self, m: u8) -> Res<()> {
        let mut length = self.word()? as i64 - 2;
        let limit: i64 = if m == 0xee { 256 } else { 14 };
        let numtoread = if length >= limit {
            limit
        } else {
            length.max(0)
        };
        let mut b = Vec::with_capacity(numtoread as usize);
        for _ in 0..numtoread {
            b.push(self.byte()?);
        }
        length -= numtoread;
        if m == 0xe0 {
            if b.len() >= 14 && &b[..5] == b"JFIF\0" {
                self.saw_jfif = true;
                self.density_unit = b[7];
                self.x_density = ((b[8] as u16) << 8) | b[9] as u16;
                self.y_density = ((b[10] as u16) << 8) | b[11] as u16;
                if b[5] != 1 {
                    let w = format!("Warning: unknown JFIF revision number {}.{:02}", b[5], b[6]);
                    self.warn(w);
                }
            }
        } else if b.len() >= 12 && &b[..5] == b"Adobe" {
            self.saw_adobe = true;
            self.adobe_transform = b[11];
        }
        self.skip(length)
    }

    // -----------------------------------------------------------------
    // jdinput.c

    /// jpeg_read_header(): up to the first SOS, then initial_setup()
    pub fn read_header(&mut self) -> Res<()> {
        match self.read_markers()? {
            MarkerResult::Sos => {}
            MarkerResult::Eoi => {
                if self.saw_sof {
                    return Err(msg::SOF_NO_SOS.into());
                }
                return Err(msg::NO_IMAGE.into());
            }
        }
        if self.image_height > 65500 || self.image_width > 65500 {
            return Err("Maximum supported image dimension is 65500 pixels".into());
        }
        if self.precision != 8 && self.precision != 12 {
            return Err(format!(
                "Unsupported JPEG data precision {}",
                self.precision
            ));
        }
        if self.comps.len() > 10 {
            return Err(format!(
                "Too many color components: {}, max 10",
                self.comps.len()
            ));
        }
        self.max_h = 1;
        self.max_v = 1;
        for c in &self.comps {
            if !(1..=4).contains(&c.h) || !(1..=4).contains(&c.v) {
                return Err(msg::BAD_SAMPLING.into());
            }
            self.max_h = self.max_h.max(c.h);
            self.max_v = self.max_v.max(c.v);
        }
        let (w, h) = (self.image_width as usize, self.image_height as usize);
        let (mh, mv) = (self.max_h, self.max_v);
        let mcus_across = w.div_ceil(mh * 8);
        let mcus_down = h.div_ceil(mv * 8);
        for c in self.comps.iter_mut() {
            c.width_in_blocks = (w * c.h).div_ceil(mh * 8);
            c.height_in_blocks = (h * c.v).div_ceil(mv * 8);
            c.downsampled_width = (w * c.h).div_ceil(mh);
            c.downsampled_height = (h * c.v).div_ceil(mv);
            c.bw = (mcus_across * c.h).max(c.width_in_blocks);
            c.bh = (mcus_down * c.v).max(c.height_in_blocks);
        }
        self.has_multiple_scans = self.scan.len() < self.comps.len() || self.progressive;
        // default_decompress_parms()
        self.color_space = match self.comps.len() {
            1 => ColorSpace::Grayscale,
            3 => {
                if self.saw_jfif {
                    ColorSpace::YCbCr
                } else if self.saw_adobe {
                    match self.adobe_transform {
                        0 => ColorSpace::Rgb,
                        1 => ColorSpace::YCbCr,
                        t => {
                            self.warn(format!("Unknown Adobe color transform code {}", t));
                            ColorSpace::YCbCr
                        }
                    }
                } else {
                    let ids = (self.comps[0].id, self.comps[1].id, self.comps[2].id);
                    if ids == (82, 71, 66) {
                        ColorSpace::Rgb
                    } else {
                        ColorSpace::YCbCr
                    }
                }
            }
            4 => {
                if self.saw_adobe {
                    match self.adobe_transform {
                        0 => ColorSpace::Cmyk,
                        2 => ColorSpace::Ycck,
                        t => {
                            self.warn(format!("Unknown Adobe color transform code {}", t));
                            ColorSpace::Ycck
                        }
                    }
                } else {
                    ColorSpace::Cmyk
                }
            }
            _ => ColorSpace::Unknown,
        };
        Ok(())
    }

    /// gd asks for CMYK output for CMYK and YCCK files, RGB otherwise
    pub fn is_cmyk(&self) -> bool {
        matches!(self.color_space, ColorSpace::Cmyk | ColorSpace::Ycck)
    }

    fn make_dtable(&self, is_dc: bool, tbl: usize) -> Res<DTable> {
        if tbl >= 4 {
            return Err(format!("Huffman table 0x{:02x} was not defined", tbl));
        }
        let spec = if is_dc {
            &self.dc_specs[tbl]
        } else {
            &self.ac_specs[tbl]
        };
        let spec = match spec {
            Some(s) => s.clone(),
            None => {
                if tbl > 1 {
                    return Err(format!("Huffman table 0x{:02x} was not defined", tbl));
                }
                std_huff(is_dc, tbl)
            }
        };
        let mut huffsize = Vec::new();
        for l in 1..=16 {
            let i = spec.bits[l] as usize;
            if huffsize.len() + i > 256 {
                return Err(msg::BAD_HUFF_TABLE.into());
            }
            for _ in 0..i {
                huffsize.push(l as u32);
            }
        }
        let mut huffcode = Vec::with_capacity(huffsize.len());
        let mut code: i64 = 0;
        let mut p = 0;
        if !huffsize.is_empty() {
            let mut si = huffsize[0];
            while p < huffsize.len() {
                while p < huffsize.len() && huffsize[p] == si {
                    huffcode.push(code);
                    code += 1;
                    p += 1;
                }
                if code >= (1i64 << si) {
                    return Err(msg::BAD_HUFF_TABLE.into());
                }
                code <<= 1;
                si += 1;
            }
        }
        let mut t = DTable {
            maxcode: [0; 18],
            valoffset: [0; 18],
            vals: spec.vals.clone(),
        };
        t.vals.resize(256, 0);
        let mut p = 0usize;
        for l in 1..=16 {
            if spec.bits[l] != 0 {
                t.valoffset[l] = p as i64 - huffcode[p];
                p += spec.bits[l] as usize;
                t.maxcode[l] = huffcode[p - 1];
            } else {
                t.maxcode[l] = -1;
            }
        }
        t.valoffset[17] = 0;
        t.maxcode[17] = 0xfffff;
        if is_dc {
            for &v in &spec.vals[..huffsize.len().min(spec.vals.len())] {
                if v > 15 {
                    return Err(msg::BAD_HUFF_TABLE.into());
                }
            }
        }
        Ok(t)
    }

    /// start_input_pass(): per_scan_setup, latch_quant_tables and the
    /// entropy decoder's start_pass
    fn start_scan(&mut self) -> Res<()> {
        if self.scan.len() > 1 {
            let mut blocks = 0;
            for &ci in &self.scan {
                blocks += self.comps[ci].h * self.comps[ci].v;
            }
            if blocks > 10 {
                return Err(msg::BAD_MCU_SIZE.into());
            }
        }
        for i in 0..self.scan.len() {
            let ci = self.scan[i];
            if self.comps[ci].quant.is_some() {
                continue;
            }
            let tq = self.comps[ci].tq;
            match self.qtables.get(tq).copied().flatten() {
                Some(q) => self.comps[ci].quant = Some(q),
                None => return Err(format!("Quantization table 0x{:02x} was not defined", tq)),
            }
        }
        if self.progressive {
            let is_dc = self.ss == 0;
            let mut bad = false;
            if is_dc {
                if self.se != 0 {
                    bad = true;
                }
            } else {
                if self.ss > self.se || self.se >= 64 {
                    bad = true;
                }
                if self.scan.len() != 1 {
                    bad = true;
                }
            }
            if self.ah != 0 && self.al != self.ah.wrapping_sub(1) {
                bad = true;
            }
            if self.al > 13 {
                bad = true;
            }
            if bad {
                return Err(format!(
                    "Invalid progressive/lossless parameters Ss={} Se={} Ah={} Al={}",
                    self.ss, self.se, self.ah, self.al
                ));
            }
            for i in 0..self.scan.len() {
                let ci = self.scan[i];
                let lo = self.ss.min(1);
                let hi = self.se.clamp(9, 63);
                for k in lo..=hi {
                    self.comps[ci].prev_coef_bits[k] = if self.input_scan_number > 1 {
                        self.comps[ci].coef_bits[k]
                    } else {
                        0
                    };
                }
                if !is_dc && self.comps[ci].coef_bits[0] < 0 {
                    self.warn(format!(
                        "Inconsistent progression sequence for component {} coefficient {}",
                        ci, 0
                    ));
                }
                for k in self.ss..=self.se.min(63) {
                    let cb = self.comps[ci].coef_bits[k];
                    let expected = if cb < 0 { 0 } else { cb };
                    if self.ah as i32 != expected {
                        self.warn(format!(
                            "Inconsistent progression sequence for component {} coefficient {}",
                            ci, k
                        ));
                    }
                    self.comps[ci].coef_bits[k] = self.al as i32;
                }
            }
            for i in 0..self.scan.len() {
                if self.arith {
                    break;
                }
                let ci = self.scan[i];
                if is_dc {
                    if self.ah == 0 {
                        let t = self.comps[ci].dc_tbl;
                        self.dc_tbls[t] = Some(self.make_dtable(true, t)?);
                    }
                } else {
                    let t = self.comps[ci].ac_tbl;
                    self.ac_tbls[t] = Some(self.make_dtable(false, t)?);
                }
            }
        } else {
            if self.ss != 0 || self.se != 63 || self.ah != 0 || self.al != 0 {
                self.warn(msg::NOT_SEQUENTIAL.into());
            }
            for i in 0..self.scan.len() {
                if self.arith {
                    break;
                }
                let ci = self.scan[i];
                let (d, a) = (self.comps[ci].dc_tbl, self.comps[ci].ac_tbl);
                let dt = self.make_dtable(true, d)?;
                self.dc_tbls[d] = Some(dt);
                let at = self.make_dtable(false, a)?;
                self.ac_tbls[a] = Some(at);
            }
        }
        self.bits_left = 0;
        self.get_buffer = 0;
        self.insufficient = false;
        self.last_dc = [0; 4];
        self.eobrun = 0;
        self.restarts_to_go = self.restart_interval;
        if self.arith {
            self.arith_start_pass()?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------
    // bit reading (jdhuff.c)

    fn fill_bit_buffer(&mut self, nbits: i32) -> Res<()> {
        if self.unread_marker == 0 {
            while self.bits_left < MIN_GET_BITS {
                let mut c = self.byte()?;
                if c == 0xff {
                    loop {
                        c = self.byte()?;
                        if c != 0xff {
                            break;
                        }
                    }
                    if c == 0 {
                        c = 0xff;
                    } else {
                        self.unread_marker = c;
                        return self.no_more_bytes(nbits);
                    }
                }
                self.get_buffer = (self.get_buffer << 8) | c as u64;
                self.bits_left += 8;
            }
            Ok(())
        } else {
            self.no_more_bytes(nbits)
        }
    }

    fn no_more_bytes(&mut self, nbits: i32) -> Res<()> {
        if nbits > self.bits_left {
            if !self.insufficient {
                self.warn(msg::HIT_MARKER.into());
                self.insufficient = true;
            }
            self.get_buffer <<= MIN_GET_BITS - self.bits_left;
            self.bits_left = MIN_GET_BITS;
        }
        Ok(())
    }

    fn get_bits(&mut self, n: i32) -> Res<i32> {
        if n == 0 {
            return Ok(0);
        }
        if self.bits_left < n {
            self.fill_bit_buffer(n)?;
        }
        self.bits_left -= n;
        Ok(((self.get_buffer >> self.bits_left) & ((1u64 << n) - 1)) as i32)
    }

    fn table(&self, is_dc: bool, tbl: usize) -> &DTable {
        if is_dc {
            &self.dc_tbls[tbl]
        } else {
            &self.ac_tbls[tbl]
        }
        .as_ref()
        .unwrap()
    }

    /// HUFF_DECODE(): an 8-bit lookahead when at least 8 bits can be
    /// buffered, else jpeg_huff_decode() bit by bit
    fn decode(&mut self, is_dc: bool, tbl: usize) -> Res<i32> {
        if self.bits_left < 8 {
            self.fill_bit_buffer(0)?;
            if self.bits_left < 8 {
                return self.huff_decode_slow(is_dc, tbl, 1);
            }
        }
        let look = ((self.get_buffer >> (self.bits_left - 8)) & 0xff) as i64;
        if let Some((nb, sym)) = lookahead(self.table(is_dc, tbl), look) {
            self.bits_left -= nb;
            return Ok(sym);
        }
        self.huff_decode_slow(is_dc, tbl, 9)
    }

    /// jpeg_huff_decode()
    fn huff_decode_slow(&mut self, is_dc: bool, tbl: usize, min_bits: usize) -> Res<i32> {
        let mut l = min_bits;
        let mut code = self.get_bits(l as i32)? as i64;
        while code > self.table(is_dc, tbl).maxcode[l] {
            code = (code << 1) | self.get_bits(1)? as i64;
            l += 1;
        }
        if l > 16 {
            self.warn(msg::HUFF_BAD_CODE.into());
            return Ok(0);
        }
        let t = self.table(is_dc, tbl);
        Ok(t.vals[((code + t.valoffset[l]) & 0xff) as usize] as i32)
    }

    /// decode_mcu_fast(): used while plenty of data is buffered; it reads
    /// ahead differently and decodes bad codes as 0 without a warning. On
    /// hitting a marker it gives up (false) and the MCU is decoded again
    /// by the slow path.
    fn decode_mcu_fast(&mut self, mcu: &[(usize, usize)], scan_pos: &[usize]) -> bool {
        let data = self.data;
        let mut pos = self.pos;
        let mut gb = self.get_buffer;
        let mut bl = self.bits_left;
        let mut marker = false;
        let mut last_dc = self.last_dc;
        let mut blocks: Vec<[i16; 64]> = mcu
            .iter()
            .map(|&(ci, bi)| self.comps[ci].coefs[bi])
            .collect();
        let fill = |pos: &mut usize, gb: &mut u64, bl: &mut i32, marker: &mut bool| {
            if *bl <= 16 {
                for _ in 0..6 {
                    let c0 = data.get(*pos).copied().unwrap_or(0);
                    *pos += 1;
                    let c1 = data.get(*pos).copied().unwrap_or(0);
                    *gb = (*gb << 8) | c0 as u64;
                    *bl += 8;
                    if c0 == 0xff {
                        *pos += 1;
                        if c1 != 0 {
                            *marker = true;
                            *pos -= 2;
                            *gb &= !0xff;
                        }
                    }
                }
            }
        };
        let get = |gb: u64, bl: &mut i32, n: i32| -> i32 {
            *bl -= n;
            ((gb >> *bl) & ((1u64 << n) - 1)) as i32
        };
        for (bn, &(ci, _)) in mcu.iter().enumerate() {
            let si = scan_pos.iter().position(|&x| x == ci).unwrap();
            let (dct, act) = (self.comps[ci].dc_tbl, self.comps[ci].ac_tbl);
            let huff = |t: &DTable,
                        pos: &mut usize,
                        gb: &mut u64,
                        bl: &mut i32,
                        marker: &mut bool|
             -> i32 {
                fill(pos, gb, bl, marker);
                let look = ((*gb >> (*bl - 8)) & 0xff) as i64;
                if let Some((nb, sym)) = lookahead(t, look) {
                    *bl -= nb;
                    return sym;
                }
                let mut nb = 9usize;
                *bl -= nb as i32;
                let mut s = ((*gb >> *bl) & ((1 << nb) - 1)) as i64;
                while s > t.maxcode[nb] {
                    s = (s << 1) | get(*gb, bl, 1) as i64;
                    nb += 1;
                }
                if nb > 16 {
                    0
                } else {
                    t.vals[((s + t.valoffset[nb]) & 0xff) as usize] as i32
                }
            };
            let mut s = huff(
                self.table(true, dct),
                &mut pos,
                &mut gb,
                &mut bl,
                &mut marker,
            );
            if s != 0 {
                fill(&mut pos, &mut gb, &mut bl, &mut marker);
                let r = get(gb, &mut bl, s);
                s = Self::extend(r, s);
            }
            let dc = last_dc[si].wrapping_add(s);
            last_dc[si] = dc;
            let blk = &mut blocks[bn];
            blk[0] = dc as i16;
            let mut k = 1usize;
            while k < 64 {
                let rs = huff(
                    self.table(false, act),
                    &mut pos,
                    &mut gb,
                    &mut bl,
                    &mut marker,
                );
                let r = (rs >> 4) as usize;
                let s = rs & 15;
                if s != 0 {
                    k += r;
                    fill(&mut pos, &mut gb, &mut bl, &mut marker);
                    let v = get(gb, &mut bl, s);
                    blk[NATURAL_ORDER[k]] = Self::extend(v, s) as i16;
                } else {
                    if r != 15 {
                        break;
                    }
                    k += 15;
                }
                k += 1;
            }
        }
        if marker {
            return false;
        }
        self.pos = pos;
        self.get_buffer = gb;
        self.bits_left = bl;
        self.last_dc = last_dc;
        for (bn, &(ci, bi)) in mcu.iter().enumerate() {
            self.comps[ci].coefs[bi] = blocks[bn];
        }
        true
    }

    fn read_restart_marker(&mut self) -> Res<()> {
        if self.unread_marker == 0 {
            self.next_marker()?;
        }
        if self.unread_marker as u32 == 0xd0 + self.next_restart_num {
            self.unread_marker = 0;
        } else {
            self.resync_to_restart(self.next_restart_num)?;
        }
        self.next_restart_num = (self.next_restart_num + 1) & 7;
        Ok(())
    }

    /// jpeg_resync_to_restart()
    fn resync_to_restart(&mut self, desired: u32) -> Res<()> {
        let mut marker = self.unread_marker as u32;
        self.warn(format!(
            "Corrupt JPEG data: found marker 0x{:02x} instead of RST{}",
            marker, desired
        ));
        loop {
            let action = if marker < 0xc0 {
                2
            } else if !(0xd0..=0xd7).contains(&marker)
                || marker == 0xd0 + ((desired + 1) & 7)
                || marker == 0xd0 + ((desired + 2) & 7)
            {
                // a valid non-restart marker, or one of the next two
                // expected restarts
                3
            } else if marker == 0xd0 + (desired.wrapping_sub(1) & 7)
                || marker == 0xd0 + (desired.wrapping_sub(2) & 7)
            {
                2
            } else {
                1
            };
            match action {
                1 => {
                    self.unread_marker = 0;
                    return Ok(());
                }
                2 => {
                    self.next_marker()?;
                    marker = self.unread_marker as u32;
                }
                _ => return Ok(()),
            }
        }
    }

    fn process_restart(&mut self) -> Res<()> {
        self.discarded_bytes += (self.bits_left / 8) as u32;
        self.bits_left = 0;
        self.read_restart_marker()?;
        self.last_dc = [0; 4];
        self.eobrun = 0;
        self.restarts_to_go = self.restart_interval;
        if self.unread_marker == 0 {
            self.insufficient = false;
        }
        Ok(())
    }

    fn mcu_blocks(&self) -> Vec<Vec<(usize, usize)>> {
        let mut out = Vec::new();
        if self.scan.len() == 1 {
            let ci = self.scan[0];
            let c = &self.comps[ci];
            for br in 0..c.height_in_blocks {
                for bc in 0..c.width_in_blocks {
                    out.push(vec![(ci, br * c.bw + bc)]);
                }
            }
        } else {
            let mcus_across = (self.image_width as usize).div_ceil(self.max_h * 8);
            let mcus_down = (self.image_height as usize).div_ceil(self.max_v * 8);
            for my in 0..mcus_down {
                for mx in 0..mcus_across {
                    let mut m = Vec::new();
                    for &ci in &self.scan {
                        let c = &self.comps[ci];
                        for yi in 0..c.v {
                            for xi in 0..c.h {
                                m.push((ci, (my * c.v + yi) * c.bw + mx * c.h + xi));
                            }
                        }
                    }
                    out.push(m);
                }
            }
        }
        out
    }

    fn decode_scan(&mut self) -> Res<()> {
        let mcus = self.mcu_blocks();
        let scan_pos: Vec<usize> = self.scan.clone();
        // the iMCU row of each MCU
        let (per_row, rows_per_imcu) = if self.scan.len() == 1 {
            let c = &self.comps[self.scan[0]];
            (c.width_in_blocks, c.v)
        } else {
            ((self.image_width as usize).div_ceil(self.max_h * 8), 1)
        };
        for (mi, mcu) in mcus.into_iter().enumerate() {
            if !self.insufficient {
                self.last_good_imcu_row = mi / per_row / rows_per_imcu;
            }
            if self.arith {
                self.arith_decode_mcu(&mcu, &scan_pos)?;
                continue;
            }
            if self.restart_interval != 0 {
                if self.restarts_to_go == 0 {
                    self.process_restart()?;
                }
                self.restarts_to_go -= 1;
            }
            if self.insufficient {
                continue;
            }
            if !self.progressive
                && self.restart_interval == 0
                && self.unread_marker == 0
                && self.fake.is_empty()
                && self.chunk_end - self.pos >= 512 * mcu.len()
                && self.decode_mcu_fast(&mcu, &scan_pos)
            {
                continue;
            }
            for &(ci, bi) in &mcu {
                let si = scan_pos.iter().position(|&x| x == ci).unwrap();
                if self.progressive {
                    self.decode_block_progressive(ci, si, bi)?;
                } else {
                    self.decode_block_sequential(ci, si, bi)?;
                }
            }
        }
        // the bits left in the buffer are dropped silently by the next
        // scan's start_pass (no finish_pass in libjpeg-turbo)
        self.bits_left = 0;
        Ok(())
    }

    fn extend(r: i32, s: i32) -> i32 {
        if r < (1 << (s - 1)) {
            r + ((-1) << s) + 1
        } else {
            r
        }
    }

    fn decode_block_sequential(&mut self, ci: usize, si: usize, bi: usize) -> Res<()> {
        let (dct, act) = (self.comps[ci].dc_tbl, self.comps[ci].ac_tbl);
        let mut s = self.decode(true, dct)?;
        if s != 0 {
            let r = self.get_bits(s)?;
            s = Self::extend(r, s);
        }
        let dc = self.last_dc[si].wrapping_add(s);
        self.last_dc[si] = dc;
        let mut blk = self.comps[ci].coefs[bi];
        blk[0] = dc as i16;
        let mut k = 1usize;
        while k < 64 {
            let rs = self.decode(false, act)?;
            let r = (rs >> 4) as usize;
            let s = rs & 15;
            if s != 0 {
                k += r;
                let v = self.get_bits(s)?;
                let v = Self::extend(v, s);
                blk[NATURAL_ORDER[k]] = v as i16;
            } else {
                if r != 15 {
                    break;
                }
                k += 15;
            }
            k += 1;
        }
        self.comps[ci].coefs[bi] = blk;
        Ok(())
    }

    fn decode_block_progressive(&mut self, ci: usize, si: usize, bi: usize) -> Res<()> {
        let al = self.al;
        if self.ss == 0 {
            if self.ah == 0 {
                let t = self.comps[ci].dc_tbl;
                let mut s = self.decode(true, t)?;
                if s != 0 {
                    let r = self.get_bits(s)?;
                    s = Self::extend(r, s);
                }
                let v = self.last_dc[si].wrapping_add(s);
                self.last_dc[si] = v;
                self.comps[ci].coefs[bi][0] = (v.wrapping_shl(al)) as i16;
            } else if self.get_bits(1)? != 0 {
                self.comps[ci].coefs[bi][0] |= (1i32 << al) as i16;
            }
            return Ok(());
        }
        let t = self.comps[ci].ac_tbl;
        let se = self.se;
        let mut blk = self.comps[ci].coefs[bi];
        if self.ah == 0 {
            if self.eobrun > 0 {
                self.eobrun -= 1;
            } else {
                let mut k = self.ss;
                while k <= se {
                    let rs = self.decode(false, t)?;
                    let r = rs >> 4;
                    let s = rs & 15;
                    if s != 0 {
                        k += r as usize;
                        let v = self.get_bits(s)?;
                        let v = Self::extend(v, s);
                        blk[NATURAL_ORDER[k]] = (v.wrapping_shl(al)) as i16;
                    } else if r == 15 {
                        k += 15;
                    } else {
                        let mut eobrun = 1u32 << r;
                        if r != 0 {
                            eobrun += self.get_bits(r)? as u32;
                        }
                        self.eobrun = eobrun - 1;
                        break;
                    }
                    k += 1;
                }
            }
        } else {
            let p1: i32 = 1 << al;
            let m1: i32 = -1 << al;
            let mut k = self.ss;
            if self.eobrun == 0 {
                while k <= se {
                    let rs = self.decode(false, t)?;
                    let mut r = rs >> 4;
                    let mut s = rs & 15;
                    if s != 0 {
                        if s != 1 {
                            self.warn(msg::HUFF_BAD_CODE.into());
                        }
                        s = if self.get_bits(1)? != 0 { p1 } else { m1 };
                    } else if r != 15 {
                        let mut eobrun = 1u32 << r;
                        if r != 0 {
                            eobrun += self.get_bits(r)? as u32;
                        }
                        self.eobrun = eobrun;
                        break;
                    }
                    loop {
                        let pos = NATURAL_ORDER[k];
                        let coef = blk[pos] as i32;
                        if coef != 0 {
                            if self.get_bits(1)? != 0 && (coef & p1) == 0 {
                                blk[pos] = if coef >= 0 { coef + p1 } else { coef + m1 } as i16;
                            }
                        } else {
                            r -= 1;
                            if r < 0 {
                                break;
                            }
                        }
                        k += 1;
                        if k > se {
                            break;
                        }
                    }
                    if s != 0 {
                        let pos = NATURAL_ORDER[k];
                        blk[pos] = s as i16;
                    }
                    k += 1;
                }
            }
            if self.eobrun > 0 {
                while k <= se {
                    let pos = NATURAL_ORDER[k];
                    let coef = blk[pos] as i32;
                    if coef != 0 && self.get_bits(1)? != 0 && (coef & p1) == 0 {
                        blk[pos] = if coef >= 0 { coef + p1 } else { coef + m1 } as i16;
                    }
                    k += 1;
                }
                self.eobrun -= 1;
            }
        }
        self.comps[ci].coefs[bi] = blk;
        Ok(())
    }

    // -----------------------------------------------------------------
    // output

    /// jpeg_start_decompress() + jpeg_read_scanlines(): the RGB (or CMYK)
    /// rows, `image_width * channels` bytes each
    pub fn decompress(&mut self, cmyk_out: bool) -> Res<Vec<u8>> {
        // jinit_color_deconverter()
        let cs = self.color_space;
        let ok = if cmyk_out {
            matches!(cs, ColorSpace::Cmyk | ColorSpace::Ycck)
        } else {
            matches!(
                cs,
                ColorSpace::Grayscale | ColorSpace::YCbCr | ColorSpace::Rgb
            )
        };
        if !ok {
            return Err(msg::CONVERSION_NOTIMPL.into());
        }
        for c in &self.comps {
            if !self.max_h.is_multiple_of(c.h) || !self.max_v.is_multiple_of(c.v) {
                return Err(msg::FRACT_SAMPLE_NOTIMPL.into());
            }
        }
        for c in self.comps.iter_mut() {
            c.coefs = vec![[0i16; 64]; c.bw * c.bh];
        }
        // absorb the scans
        loop {
            self.start_scan()?;
            self.decode_scan()?;
            match self.read_markers()? {
                MarkerResult::Sos => {
                    if !self.has_multiple_scans {
                        return Err(msg::EOI_EXPECTED.into());
                    }
                }
                MarkerResult::Eoi => break,
            }
        }
        // jpeg_read_scanlines() with the 8-bit API
        if self.precision != 8 {
            return Err(format!(
                "Unsupported JPEG data precision {}",
                self.precision
            ));
        }
        let w = self.image_width as usize;
        let h = self.image_height as usize;
        // IDCT + upsampling into full size planes
        let mut planes: Vec<Vec<u8>> = Vec::with_capacity(self.comps.len());
        let smooth = self.smoothing_ok();
        let total_imcu_rows = h.div_ceil(self.max_v * 8);
        for (ci, c) in self.comps.iter().enumerate() {
            let pw = c.width_in_blocks * 8;
            let ph = c.height_in_blocks * 8;
            let mut plane = vec![0u8; pw * ph];
            let q = c.quant.unwrap_or([0; 64]);
            if let Some(latch) = &smooth {
                let s = Smooth {
                    latch: &latch[ci],
                    total_imcu_rows,
                    last_good_imcu_row: self.last_good_imcu_row,
                };
                smooth_component(c, &q, &s, &mut plane, pw);
            } else {
                for br in 0..c.height_in_blocks {
                    for bc in 0..c.width_in_blocks {
                        idct_islow(&c.coefs[br * c.bw + bc], &q, &mut plane, pw, br * 8, bc * 8);
                    }
                }
            }
            planes.push(upsample(&plane, pw, c, self.max_h, self.max_v, w, h));
        }
        let channels = if cmyk_out { 4 } else { 3 };
        let mut out = vec![0u8; w * h * channels];
        let t = ycc_tables();
        for i in 0..w * h {
            let o = &mut out[i * channels..(i + 1) * channels];
            match cs {
                ColorSpace::Grayscale => {
                    let g = planes[0][i];
                    o[0] = g;
                    o[1] = g;
                    o[2] = g;
                }
                ColorSpace::Rgb | ColorSpace::Cmyk => {
                    for (k, p) in planes.iter().enumerate().take(channels) {
                        o[k] = p[i];
                    }
                }
                ColorSpace::YCbCr => {
                    let (r, g, b) = t.convert(planes[0][i], planes[1][i], planes[2][i]);
                    o[0] = r;
                    o[1] = g;
                    o[2] = b;
                }
                ColorSpace::Ycck => {
                    let y = planes[0][i] as i32;
                    let cb = planes[1][i] as usize;
                    let cr = planes[2][i] as usize;
                    o[0] = clamp(255 - (y + t.cr_r[cr]));
                    o[1] = clamp(255 - (y + ((t.cb_g[cb] + t.cr_g[cr]) >> 16) as i32));
                    o[2] = clamp(255 - (y + t.cb_b[cb]));
                    o[3] = planes[3][i];
                }
                ColorSpace::Unknown => {}
            }
        }
        Ok(out)
    }

    /// smoothing_ok() of jdcoefct.c: the (current, previous) coef_bits
    /// latches per component when block smoothing applies
    fn smoothing_ok(&self) -> Option<Vec<Latch>> {
        if !self.progressive {
            return None;
        }
        let mut useful = false;
        let mut latches = Vec::new();
        for c in &self.comps {
            let q = c.quant?;
            for pos in [0, 1, 8, 16, 9, 2, 3, 10, 17, 24] {
                if q[pos] == 0 {
                    return None;
                }
            }
            if c.coef_bits[0] < 0 {
                return None;
            }
            let mut cur = [0i32; 10];
            let mut prev = [0i32; 10];
            cur[0] = c.coef_bits[0];
            for k in 1..10 {
                prev[k] = if self.input_scan_number > 1 {
                    c.prev_coef_bits[k]
                } else {
                    -1
                };
                cur[k] = c.coef_bits[k];
                if c.coef_bits[k] != 0 {
                    useful = true;
                }
            }
            latches.push((cur, prev));
        }
        if useful { Some(latches) } else { None }
    }

    /// jpeg_finish_decompress(): the scans were absorbed up to EOI already
    pub fn finish(&mut self) -> Res<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------
// arithmetic decoding (jdarith.c)

/// The arithmetic decoder state (arith_entropy_decoder)
pub(super) struct Arith {
    c: i64,
    a: i64,
    ct: i32,
    dc_context: [i32; 4],
    dc_stats: Vec<[u8; 64]>,
    ac_stats: Vec<[u8; 256]>,
    fixed_bin: u8,
}

impl Arith {
    pub(super) fn new() -> Arith {
        Arith {
            c: 0,
            a: 0,
            ct: -16,
            dc_context: [0; 4],
            dc_stats: vec![[0; 64]; 16],
            ac_stats: vec![[0; 256]; 16],
            fixed_bin: 113,
        }
    }
}

/// Which statistics bin arith_decode() works on
#[derive(Clone, Copy)]
enum Bin {
    Dc(usize, usize),
    Ac(usize, usize),
    Fixed,
}

impl Decoder<'_> {
    fn bin(&mut self, b: Bin) -> &mut u8 {
        match b {
            Bin::Dc(t, i) => &mut self.ar.dc_stats[t][i],
            Bin::Ac(t, i) => &mut self.ar.ac_stats[t][i],
            Bin::Fixed => &mut self.ar.fixed_bin,
        }
    }

    /// arith_decode()
    fn arith_decode(&mut self, b: Bin) -> Res<i32> {
        while self.ar.a < 0x8000 {
            self.ar.ct -= 1;
            if self.ar.ct < 0 {
                let data = if self.unread_marker != 0 {
                    0
                } else {
                    let mut d = self.byte()? as i64;
                    if d == 0xff {
                        loop {
                            d = self.byte()? as i64;
                            if d != 0xff {
                                break;
                            }
                        }
                        if d == 0 {
                            d = 0xff;
                        } else {
                            self.unread_marker = d as u8;
                            d = 0;
                        }
                    }
                    d
                };
                self.ar.c = (self.ar.c << 8) | data;
                self.ar.ct += 8;
                if self.ar.ct < 0 {
                    self.ar.ct += 1;
                    if self.ar.ct == 0 {
                        self.ar.a = 0x8000;
                    }
                }
            }
            self.ar.a <<= 1;
        }
        let sv = *self.bin(b) as i32;
        let mut qe = super::aritab::ARITAB[(sv & 0x7f) as usize];
        let nl = (qe & 0xff) as i32;
        qe >>= 8;
        let nm = (qe & 0xff) as i32;
        qe >>= 8;
        let mut temp = self.ar.a - qe;
        self.ar.a = temp;
        temp <<= self.ar.ct;
        let mut sv = sv;
        if self.ar.c >= temp {
            self.ar.c -= temp;
            if self.ar.a < qe {
                self.ar.a = qe;
                *self.bin(b) = ((sv & 0x80) ^ nm) as u8;
            } else {
                self.ar.a = qe;
                *self.bin(b) = ((sv & 0x80) ^ nl) as u8;
                sv ^= 0x80;
            }
        } else if self.ar.a < 0x8000 {
            if self.ar.a < qe {
                *self.bin(b) = ((sv & 0x80) ^ nl) as u8;
                sv ^= 0x80;
            } else {
                *self.bin(b) = ((sv & 0x80) ^ nm) as u8;
            }
        }
        Ok(sv >> 7)
    }

    /// start_pass() of jdarith.c (after the progression bookkeeping)
    fn arith_start_pass(&mut self) -> Res<()> {
        for i in 0..self.scan.len() {
            let ci = self.scan[i];
            if !self.progressive || (self.ss == 0 && self.ah == 0) {
                let t = self.comps[ci].dc_tbl;
                if t >= 16 {
                    return Err(format!("Arithmetic table 0x{:02x} was not defined", t));
                }
                self.ar.dc_stats[t] = [0; 64];
                self.last_dc[i] = 0;
                self.ar.dc_context[i] = 0;
            }
            if !self.progressive || self.ss != 0 {
                let t = self.comps[ci].ac_tbl;
                if t >= 16 {
                    return Err(format!("Arithmetic table 0x{:02x} was not defined", t));
                }
                self.ar.ac_stats[t] = [0; 256];
            }
        }
        self.ar.c = 0;
        self.ar.a = 0;
        self.ar.ct = -16;
        self.insufficient = false;
        self.restarts_to_go = self.restart_interval;
        Ok(())
    }

    fn arith_process_restart(&mut self) -> Res<()> {
        self.read_restart_marker()?;
        for i in 0..self.scan.len() {
            let ci = self.scan[i];
            if !self.progressive || (self.ss == 0 && self.ah == 0) {
                let t = self.comps[ci].dc_tbl;
                self.ar.dc_stats[t] = [0; 64];
                self.last_dc[i] = 0;
                self.ar.dc_context[i] = 0;
            }
            if !self.progressive || self.ss != 0 {
                let t = self.comps[ci].ac_tbl;
                self.ar.ac_stats[t] = [0; 256];
            }
        }
        self.ar.c = 0;
        self.ar.a = 0;
        self.ar.ct = -16;
        self.restarts_to_go = self.restart_interval;
        Ok(())
    }

    fn arith_bad_code(&mut self) {
        self.warn("Corrupt JPEG data: bad arithmetic code".into());
        self.ar.ct = -1;
    }

    /// Figures F.19 - F.24 for a DC difference; None on a bad code
    fn arith_dc_diff(&mut self, si: usize, tbl: usize) -> Res<Option<i32>> {
        let ctx = self.ar.dc_context[si] as usize;
        if self.arith_decode(Bin::Dc(tbl, ctx))? == 0 {
            self.ar.dc_context[si] = 0;
            return Ok(Some(0));
        }
        let sign = self.arith_decode(Bin::Dc(tbl, ctx + 1))?;
        let mut st = ctx + 2 + sign as usize;
        let mut m = self.arith_decode(Bin::Dc(tbl, st))?;
        if m != 0 {
            st = 20;
            while self.arith_decode(Bin::Dc(tbl, st))? != 0 {
                m <<= 1;
                if m == 0x8000 {
                    self.arith_bad_code();
                    return Ok(None);
                }
                st += 1;
            }
        }
        let l = self.arith_dc_l[tbl] as i64;
        let u = self.arith_dc_u[tbl] as i64;
        if (m as i64) < ((1i64 << l) >> 1) {
            self.ar.dc_context[si] = 0;
        } else if (m as i64) > ((1i64 << u) >> 1) {
            self.ar.dc_context[si] = 12 + sign * 4;
        } else {
            self.ar.dc_context[si] = 4 + sign * 4;
        }
        let mut v = m;
        st += 14;
        loop {
            m >>= 1;
            if m == 0 {
                break;
            }
            if self.arith_decode(Bin::Dc(tbl, st))? != 0 {
                v |= m;
            }
        }
        v += 1;
        if sign != 0 {
            v = -v;
        }
        Ok(Some(v))
    }

    /// Figure F.20 for coefficients ss..=se of `blk`; false on a bad code
    fn arith_ac(
        &mut self,
        tbl: usize,
        ss: usize,
        se: usize,
        al: u32,
        blk: &mut [i16; 64],
    ) -> Res<bool> {
        let mut k = ss;
        while k <= se {
            let mut st = 3 * (k - 1);
            if self.arith_decode(Bin::Ac(tbl, st))? != 0 {
                break;
            }
            while self.arith_decode(Bin::Ac(tbl, st + 1))? == 0 {
                st += 3;
                k += 1;
                if k > se {
                    self.arith_bad_code();
                    return Ok(false);
                }
            }
            let sign = self.arith_decode(Bin::Fixed)?;
            st += 2;
            let mut m = self.arith_decode(Bin::Ac(tbl, st))?;
            if m != 0 && self.arith_decode(Bin::Ac(tbl, st))? != 0 {
                m <<= 1;
                st = if k <= self.arith_ac_k[tbl] as usize {
                    189
                } else {
                    217
                };
                while self.arith_decode(Bin::Ac(tbl, st))? != 0 {
                    m <<= 1;
                    if m == 0x8000 {
                        self.arith_bad_code();
                        return Ok(false);
                    }
                    st += 1;
                }
            }
            let mut v = m;
            st += 14;
            loop {
                m >>= 1;
                if m == 0 {
                    break;
                }
                if self.arith_decode(Bin::Ac(tbl, st))? != 0 {
                    v |= m;
                }
            }
            v += 1;
            if sign != 0 {
                v = -v;
            }
            blk[NATURAL_ORDER[k]] = ((v as u32) << al) as i16;
            k += 1;
        }
        Ok(true)
    }

    /// One MCU of an arithmetic-coded scan
    fn arith_decode_mcu(&mut self, mcu: &[(usize, usize)], scan_pos: &[usize]) -> Res<()> {
        if self.restart_interval != 0 {
            if self.restarts_to_go == 0 {
                self.arith_process_restart()?;
            }
            self.restarts_to_go -= 1;
        }
        let refine_dc = self.progressive && self.ss == 0 && self.ah != 0;
        if self.ar.ct == -1 && !refine_dc {
            return Ok(());
        }
        let al = self.al;
        for &(ci, bi) in mcu {
            let si = scan_pos.iter().position(|&x| x == ci).unwrap();
            let mut blk = self.comps[ci].coefs[bi];
            if !self.progressive || self.ss == 0 {
                if refine_dc {
                    if self.arith_decode(Bin::Fixed)? != 0 {
                        blk[0] |= (1i32 << al) as i16;
                    }
                    self.comps[ci].coefs[bi] = blk;
                    continue;
                }
                let tbl = self.comps[ci].dc_tbl;
                let Some(v) = self.arith_dc_diff(si, tbl)? else {
                    return Ok(());
                };
                if v != 0 {
                    self.last_dc[si] = (self.last_dc[si] + v) & 0xffff;
                }
                blk[0] = if self.progressive {
                    (self.last_dc[si] << al) as i16
                } else {
                    self.last_dc[si] as i16
                };
                if self.progressive {
                    self.comps[ci].coefs[bi] = blk;
                    continue;
                }
                let tbl = self.comps[ci].ac_tbl;
                let ok = self.arith_ac(tbl, 1, 63, 0, &mut blk)?;
                self.comps[ci].coefs[bi] = blk;
                if !ok {
                    return Ok(());
                }
                continue;
            }
            let tbl = self.comps[ci].ac_tbl;
            if self.ah == 0 {
                let ok = self.arith_ac(tbl, self.ss, self.se, al, &mut blk)?;
                self.comps[ci].coefs[bi] = blk;
                if !ok {
                    return Ok(());
                }
                continue;
            }
            // decode_mcu_AC_refine
            let p1: i32 = 1 << al;
            let m1: i32 = -1 << al;
            let mut kex = self.se;
            while kex > 0 {
                if blk[NATURAL_ORDER[kex]] != 0 {
                    break;
                }
                kex -= 1;
            }
            let mut k = self.ss;
            'outer: while k <= self.se {
                let mut st = 3 * (k - 1);
                if k > kex && self.arith_decode(Bin::Ac(tbl, st))? != 0 {
                    break;
                }
                loop {
                    let pos = NATURAL_ORDER[k];
                    if blk[pos] != 0 {
                        if self.arith_decode(Bin::Ac(tbl, st + 2))? != 0 {
                            blk[pos] = if blk[pos] < 0 {
                                (blk[pos] as i32 + m1) as i16
                            } else {
                                (blk[pos] as i32 + p1) as i16
                            };
                        }
                        break;
                    }
                    if self.arith_decode(Bin::Ac(tbl, st + 1))? != 0 {
                        blk[pos] = if self.arith_decode(Bin::Fixed)? != 0 {
                            m1 as i16
                        } else {
                            p1 as i16
                        };
                        break;
                    }
                    st += 3;
                    k += 1;
                    if k > self.se {
                        self.arith_bad_code();
                        break 'outer;
                    }
                }
                k += 1;
            }
            self.comps[ci].coefs[bi] = blk;
        }
        Ok(())
    }
}

#[inline]
fn clamp(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

struct YccTables {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i64; 256],
    cb_g: [i64; 256],
}

impl YccTables {
    #[inline]
    fn convert(&self, y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
        let y = y as i32;
        let (cb, cr) = (cb as usize, cr as usize);
        (
            clamp(y + self.cr_r[cr]),
            clamp(y + ((self.cb_g[cb] + self.cr_g[cr]) >> 16) as i32),
            clamp(y + self.cb_b[cb]),
        )
    }
}

/// build_ycc_rgb_table()
fn ycc_tables() -> YccTables {
    let fix = |x: f64| -> i64 { (x * 65536.0 + 0.5) as i64 };
    let one_half: i64 = 1 << 15;
    let mut t = YccTables {
        cr_r: [0; 256],
        cb_b: [0; 256],
        cr_g: [0; 256],
        cb_g: [0; 256],
    };
    for i in 0..256 {
        let x = i as i64 - 128;
        t.cr_r[i] = ((fix(1.40200) * x + one_half) >> 16) as i32;
        t.cb_b[i] = ((fix(1.77200) * x + one_half) >> 16) as i32;
        t.cr_g[i] = -fix(0.71414) * x;
        t.cb_g[i] = -fix(0.34414) * x + one_half;
    }
    t
}

#[inline]
fn w16(v: i32) -> i32 {
    v as i16 as i32
}

#[inline]
fn sat16(v: i32) -> i32 {
    v.clamp(-32768, 32767)
}

/// One DODCT pass of jidctint-avx2.asm over 8 lanes: `inp[k][lane]` are
/// the 16-bit inputs in0..in7, the outputs are packed with signed
/// saturation (vpackssdw) after descaling by `shift`.
fn dodct(inp: &[[i32; 8]; 8], shift: u32) -> [[i32; 8]; 8] {
    const F_0_298: i32 = 2446;
    const F_0_390: i32 = 3196;
    const F_0_541: i32 = 4433;
    const F_0_765: i32 = 6270;
    const F_0_899: i32 = 7373;
    const F_1_175: i32 = 9633;
    const F_1_501: i32 = 12299;
    const F_1_847: i32 = 15137;
    const F_1_961: i32 = 16069;
    const F_2_053: i32 = 16819;
    const F_2_562: i32 = 20995;
    const F_3_072: i32 = 25172;
    let round = 1i32 << (shift - 1);
    let mut out = [[0i32; 8]; 8];
    for l in 0..8 {
        let i = |k: usize| inp[k][l];
        // even part (pmaddwd: exact 32-bit sums of 16x16 products)
        let tmp3 = i(2)
            .wrapping_mul(F_0_541 + F_0_765)
            .wrapping_add(i(6).wrapping_mul(F_0_541));
        let tmp2 = i(6)
            .wrapping_mul(F_0_541 - F_1_847)
            .wrapping_add(i(2).wrapping_mul(F_0_541));
        let tmp0 = w16(i(0) + i(4)) << 13;
        let tmp1 = w16(i(0) - i(4)) << 13;
        let tmp10 = tmp0.wrapping_add(tmp3);
        let tmp13 = tmp0.wrapping_sub(tmp3);
        let tmp11 = tmp1.wrapping_add(tmp2);
        let tmp12 = tmp1.wrapping_sub(tmp2);
        // odd part
        let z3 = w16(i(7) + i(3));
        let z4 = w16(i(5) + i(1));
        let z3p = z3
            .wrapping_mul(F_1_175 - F_1_961)
            .wrapping_add(z4.wrapping_mul(F_1_175));
        let z4p = z4
            .wrapping_mul(F_1_175 - F_0_390)
            .wrapping_add(z3.wrapping_mul(F_1_175));
        let o0 = i(7)
            .wrapping_mul(F_0_298 - F_0_899)
            .wrapping_add(i(1).wrapping_mul(-F_0_899))
            .wrapping_add(z3p);
        let o1 = i(5)
            .wrapping_mul(F_2_053 - F_2_562)
            .wrapping_add(i(3).wrapping_mul(-F_2_562))
            .wrapping_add(z4p);
        let o3 = i(7)
            .wrapping_mul(-F_0_899)
            .wrapping_add(i(1).wrapping_mul(F_1_501 - F_0_899))
            .wrapping_add(z4p);
        let o2 = i(5)
            .wrapping_mul(-F_2_562)
            .wrapping_add(i(3).wrapping_mul(F_3_072 - F_2_562))
            .wrapping_add(z3p);
        let d = |v: i32| sat16(v.wrapping_add(round) >> shift);
        out[0][l] = d(tmp10.wrapping_add(o3));
        out[7][l] = d(tmp10.wrapping_sub(o3));
        out[1][l] = d(tmp11.wrapping_add(o2));
        out[6][l] = d(tmp11.wrapping_sub(o2));
        out[2][l] = d(tmp12.wrapping_add(o1));
        out[5][l] = d(tmp12.wrapping_sub(o1));
        out[3][l] = d(tmp13.wrapping_add(o0));
        out[4][l] = d(tmp13.wrapping_sub(o0));
    }
    out
}

/// jsimd_idct_islow_avx2(): libjpeg-turbo's SIMD islow IDCT, which
/// matches jpeg_idct_islow() for valid data but dequantizes and sums in
/// 16 bits and saturates instead of wrapping on out-of-range input.
fn idct_islow(
    coef: &[i16; 64],
    q: &[u16; 64],
    out: &mut [u8],
    stride: usize,
    oy: usize,
    ox: usize,
) {
    // pass 1: columns; ws[k][c] = output k of column c
    let ws: [[i32; 8]; 8] = if coef[8..].iter().all(|&v| v == 0) {
        let mut ws = [[0i32; 8]; 8];
        for c in 0..8 {
            let dc = w16(w16(coef[c] as i32 * q[c] as i32) << 2);
            for row in ws.iter_mut() {
                row[c] = dc;
            }
        }
        ws
    } else {
        let mut inp = [[0i32; 8]; 8];
        for k in 0..8 {
            for c in 0..8 {
                inp[k][c] = w16(coef[k * 8 + c] as i32 * q[k * 8 + c] as i32);
            }
        }
        dodct(&inp, 11)
    };
    // pass 2: rows; input k of row r is ws[r][k]
    let mut inp = [[0i32; 8]; 8];
    for k in 0..8 {
        for r in 0..8 {
            inp[k][r] = ws[r][k];
        }
    }
    let res = dodct(&inp, 18);
    for r in 0..8 {
        let row = &mut out[(oy + r) * stride + ox..(oy + r) * stride + ox + 8];
        for (x, v) in row.iter_mut().enumerate() {
            *v = (res[x][r].clamp(-128, 127) + 128) as u8;
        }
    }
}

/// The (current, previous) coef_bits latch of a component for
/// coefficients 0..9 (zigzag order)
type Latch = ([i32; 10], [i32; 10]);

struct Smooth<'a> {
    latch: &'a Latch,
    total_imcu_rows: usize,
    last_good_imcu_row: usize,
}

/// One smoothing estimate: `pred` for coefficient `idx` from `num`
fn smooth_pred(al: i32, qq: i64, num: i64, clamp_al: bool) -> i16 {
    let mut pred;
    if num >= 0 {
        pred = ((qq << 7) + num) / (qq << 8);
        if clamp_al && al > 0 && pred >= (1 << al) {
            pred = (1 << al) - 1;
        }
    } else {
        pred = ((qq << 7) - num) / (qq << 8);
        if clamp_al && al > 0 && pred >= (1 << al) {
            pred = (1 << al) - 1;
        }
        pred = -pred;
    }
    pred as i32 as i16
}

/// decompress_smooth_data() of jdcoefct.c for one component: estimate
/// missing low-frequency AC coefficients (and, without any AC data, the
/// DC) from the neighbouring DC values, then IDCT.
fn smooth_component(c: &Comp, q: &[u16; 64], sm: &Smooth, plane: &mut [u8], pw: usize) {
    let v = c.v;
    let total_imcu_rows = sm.total_imcu_rows;
    let last_imcu_row = total_imcu_rows - 1;
    let dc = |row: usize, col: usize| -> i64 { c.coefs[row * c.bw + col][0] as i64 };
    let qv = |pos: usize| q[pos] as i64;
    let q00 = qv(0);
    for imcu in 0..total_imcu_rows {
        let block_rows = if imcu < last_imcu_row {
            v
        } else {
            let r = c.height_in_blocks % v;
            if r == 0 { v } else { r }
        };
        let coef_bits = if imcu > sm.last_good_imcu_row {
            &sm.latch.1
        } else {
            &sm.latch.0
        };
        let change_dc = coef_bits[1..10].iter().all(|&b| b == -1);
        let image_block_rows = block_rows * total_imcu_rows;
        for block_row in 0..block_rows {
            let real = imcu * v + block_row;
            if real >= c.height_in_blocks {
                continue;
            }
            let ibr = imcu * block_rows + block_row;
            let prev = if ibr > 0 { real - 1 } else { real };
            let prev_prev = if ibr > 1 { real - 2 } else { prev };
            let next = if ibr + 1 < image_block_rows {
                real + 1
            } else {
                real
            };
            let next_next = if ibr + 2 < image_block_rows {
                real + 2
            } else {
                next
            };
            let rows = [prev_prev, prev, real, next, next_next];
            // DC01..DC25, row-major 5x5 window
            let mut d = [0i64; 25];
            for (r, &row) in rows.iter().enumerate() {
                for k in 0..5 {
                    d[r * 5 + k] = dc(row, 0);
                }
            }
            let last_block_column = c.width_in_blocks - 1;
            for block_num in 0..c.width_in_blocks {
                let mut ws = c.coefs[real * c.bw + block_num];
                if block_num == 0 && block_num < last_block_column {
                    for (r, &row) in rows.iter().enumerate() {
                        d[r * 5 + 3] = dc(row, 1);
                        d[r * 5 + 4] = dc(row, 1);
                    }
                }
                if block_num + 1 < last_block_column {
                    for (r, &row) in rows.iter().enumerate() {
                        d[r * 5 + 4] = dc(row, block_num + 2);
                    }
                }
                let g = |n: usize| d[n - 1];
                // AC01
                let al = coef_bits[1];
                if al != 0 && ws[1] == 0 {
                    let num = q00
                        * if change_dc {
                            -g(1) - g(2) + g(4) + g(5) - 3 * g(6) + 13 * g(7) - 13 * g(9)
                                + 3 * g(10)
                                - 3 * g(11)
                                + 38 * g(12)
                                - 38 * g(14)
                                + 3 * g(15)
                                - 3 * g(16)
                                + 13 * g(17)
                                - 13 * g(19)
                                + 3 * g(20)
                                - g(21)
                                - g(22)
                                + g(24)
                                + g(25)
                        } else {
                            -7 * g(11) + 50 * g(12) - 50 * g(14) + 7 * g(15)
                        };
                    ws[1] = smooth_pred(al, qv(1), num, true);
                }
                // AC10
                let al = coef_bits[2];
                if al != 0 && ws[8] == 0 {
                    let num = q00
                        * if change_dc {
                            -g(1) - 3 * g(2) - 3 * g(3) - 3 * g(4) - g(5) - g(6)
                                + 13 * g(7)
                                + 38 * g(8)
                                + 13 * g(9)
                                - g(10)
                                + g(16)
                                - 13 * g(17)
                                - 38 * g(18)
                                - 13 * g(19)
                                + g(20)
                                + g(21)
                                + 3 * g(22)
                                + 3 * g(23)
                                + 3 * g(24)
                                + g(25)
                        } else {
                            -7 * g(3) + 50 * g(8) - 50 * g(18) + 7 * g(23)
                        };
                    ws[8] = smooth_pred(al, qv(8), num, true);
                }
                // AC20
                let al = coef_bits[3];
                if al != 0 && ws[16] == 0 {
                    let num = q00
                        * if change_dc {
                            g(3) + 2 * g(7) + 7 * g(8) + 2 * g(9)
                                - 5 * g(12)
                                - 14 * g(13)
                                - 5 * g(14)
                                + 2 * g(17)
                                + 7 * g(18)
                                + 2 * g(19)
                                + g(23)
                        } else {
                            -g(3) + 13 * g(8) - 24 * g(13) + 13 * g(18) - g(23)
                        };
                    ws[16] = smooth_pred(al, qv(16), num, true);
                }
                // AC11
                let al = coef_bits[4];
                if al != 0 && ws[9] == 0 {
                    let num = q00
                        * if change_dc {
                            -g(1) + g(5) + 9 * g(7) - 9 * g(9) - 9 * g(17) + 9 * g(19) + g(21)
                                - g(25)
                        } else {
                            g(10) + g(16) - 10 * g(17) + 10 * g(19) - g(2) - g(20) + g(22) - g(24)
                                + g(4)
                                - g(6)
                                + 10 * g(7)
                                - 10 * g(9)
                        };
                    ws[9] = smooth_pred(al, qv(9), num, true);
                }
                // AC02
                let al = coef_bits[5];
                if al != 0 && ws[2] == 0 {
                    let num = q00
                        * if change_dc {
                            2 * g(7) - 5 * g(8) + 2 * g(9) + g(11) + 7 * g(12) - 14 * g(13)
                                + 7 * g(14)
                                + g(15)
                                + 2 * g(17)
                                - 5 * g(18)
                                + 2 * g(19)
                        } else {
                            -g(11) + 13 * g(12) - 24 * g(13) + 13 * g(14) - g(15)
                        };
                    ws[2] = smooth_pred(al, qv(2), num, true);
                }
                if change_dc {
                    // AC03, AC12, AC21, AC30
                    let al = coef_bits[6];
                    if al != 0 && ws[3] == 0 {
                        let num = q00 * (g(7) - g(9) + 2 * g(12) - 2 * g(14) + g(17) - g(19));
                        ws[3] = smooth_pred(al, qv(3), num, true);
                    }
                    let al = coef_bits[7];
                    if al != 0 && ws[10] == 0 {
                        let num = q00 * (g(7) - 3 * g(8) + g(9) - g(17) + 3 * g(18) - g(19));
                        ws[10] = smooth_pred(al, qv(10), num, true);
                    }
                    let al = coef_bits[8];
                    if al != 0 && ws[17] == 0 {
                        let num = q00 * (g(7) - g(9) - 3 * g(12) + 3 * g(14) + g(17) - g(19));
                        ws[17] = smooth_pred(al, qv(17), num, true);
                    }
                    let al = coef_bits[9];
                    if al != 0 && ws[24] == 0 {
                        let num = q00 * (g(7) + 2 * g(8) + g(9) - g(17) - 2 * g(18) - g(19));
                        ws[24] = smooth_pred(al, qv(24), num, true);
                    }
                    // DC
                    let num = q00
                        * (-2 * g(1) - 6 * g(2) - 8 * g(3) - 6 * g(4) - 2 * g(5) - 6 * g(6)
                            + 6 * g(7)
                            + 42 * g(8)
                            + 6 * g(9)
                            - 6 * g(10)
                            - 8 * g(11)
                            + 42 * g(12)
                            + 152 * g(13)
                            + 42 * g(14)
                            - 8 * g(15)
                            - 6 * g(16)
                            + 6 * g(17)
                            + 42 * g(18)
                            + 6 * g(19)
                            - 6 * g(20)
                            - 2 * g(21)
                            - 6 * g(22)
                            - 8 * g(23)
                            - 6 * g(24)
                            - 2 * g(25));
                    ws[0] = smooth_pred(0, q00, num, false);
                }
                idct_islow(&ws, q, plane, pw, real * 8, block_num * 8);
                for r in 0..5 {
                    for k in 0..4 {
                        d[r * 5 + k] = d[r * 5 + k + 1];
                    }
                }
            }
        }
    }
}

/// The HUFF_LOOKAHEAD table: (code length, symbol) of a code of at most
/// 8 bits that prefixes `look`
fn lookahead(t: &DTable, look: i64) -> Option<(i32, i32)> {
    for l in 1..=8usize {
        let code = look >> (8 - l);
        if code <= t.maxcode[l] {
            return Some((
                l as i32,
                t.vals[((code + t.valoffset[l]) & 0xff) as usize] as i32,
            ));
        }
    }
    None
}

/// jdsample.c: upsample a component's downsampled plane (stored with row
/// stride `pw`) to the full `w` x `h` image
fn upsample(
    plane: &[u8],
    pw: usize,
    c: &Comp,
    max_h: usize,
    max_v: usize,
    w: usize,
    h: usize,
) -> Vec<u8> {
    let dw = c.downsampled_width;
    let dh = c.downsampled_height;
    // sample at (x, y) of the downsampled plane with bottom/top context
    // rows replicated
    let px = |x: usize, y: isize| -> i32 {
        let y = y.clamp(0, dh as isize - 1) as usize;
        plane[y * pw + x] as i32
    };
    let mut out = vec![0u8; w * h];
    let hx = max_h / c.h;
    let vx = max_v / c.v;
    if hx == 1 && vx == 1 {
        for y in 0..h {
            out[y * w..(y + 1) * w].copy_from_slice(&plane[y * pw..y * pw + w]);
        }
    } else if hx == 2 && vx == 1 && dw > 2 {
        // h2v1_fancy_upsample
        for y in 0..h {
            let mut row = Vec::with_capacity(dw * 2);
            let ip = |x: usize| px(x, y as isize);
            let v0 = ip(0);
            row.push(v0 as u8);
            row.push(((v0 * 3 + ip(1) + 2) >> 2) as u8);
            for x in 1..dw - 1 {
                let v = ip(x) * 3;
                row.push(((v + ip(x - 1) + 1) >> 2) as u8);
                row.push(((v + ip(x + 1) + 2) >> 2) as u8);
            }
            let v = ip(dw - 1);
            row.push(((v * 3 + ip(dw - 2) + 1) >> 2) as u8);
            row.push(v as u8);
            out[y * w..(y + 1) * w].copy_from_slice(&row[..w]);
        }
    } else if hx == 1 && vx == 2 {
        // h1v2_fancy_upsample
        for y in 0..h {
            let inrow = (y / 2) as isize;
            let (other, bias) = if y % 2 == 0 {
                (inrow - 1, 1)
            } else {
                (inrow + 1, 2)
            };
            for x in 0..w {
                let s = px(x, inrow) * 3 + px(x, other);
                out[y * w + x] = ((s + bias) >> 2) as u8;
            }
        }
    } else if hx == 2 && vx == 2 && dw > 2 {
        // h2v2_fancy_upsample
        let mut row = vec![0u8; dw * 2];
        for y in 0..h {
            let inrow = (y / 2) as isize;
            let other = if y % 2 == 0 { inrow - 1 } else { inrow + 1 };
            let colsum = |x: usize| px(x, inrow) * 3 + px(x, other);
            let mut this = colsum(0);
            let mut next = colsum(1);
            row[0] = ((this * 4 + 8) >> 4) as u8;
            row[1] = ((this * 3 + next + 7) >> 4) as u8;
            let mut last = this;
            this = next;
            for x in 1..dw - 1 {
                next = colsum(x + 1);
                row[2 * x] = ((this * 3 + last + 8) >> 4) as u8;
                row[2 * x + 1] = ((this * 3 + next + 7) >> 4) as u8;
                last = this;
                this = next;
            }
            row[2 * (dw - 1)] = ((this * 3 + last + 8) >> 4) as u8;
            row[2 * (dw - 1) + 1] = ((this * 4 + 7) >> 4) as u8;
            out[y * w..(y + 1) * w].copy_from_slice(&row[..w]);
        }
    } else {
        // h2v1_upsample / h2v2_upsample / int_upsample: replication
        for y in 0..h {
            let sy = (y / vx) as isize;
            for x in 0..w {
                out[y * w + x] = px(x / hx, sy) as u8;
            }
        }
    }
    out
}
