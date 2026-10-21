//! Port of the `convert.*` and `dechunk` stream filters from php-src
//! `ext/standard/filters.c`.
//!
//! The converters (`php_conv_*`) are ported state-for-state, including the
//! way they react to a full output buffer (`PHP_CONV_ERR_TOO_BIG`): the C code
//! keeps some state in locals (e.g. the quoted-printable encoder's `trail_ws`)
//! which is lost when the converter returns early, so the output-buffer
//! growth loop of `strfilter_convert_append_bucket` is reproduced exactly
//! (initial size = input bucket length, or 64 for the flush call, doubled on
//! every `TOO_BIG`).

use goro_core::array::ArrayKey;
use goro_core::string::PhpString;
use goro_core::value::Value;

/// Return value of a filter call, mirroring php_stream_filter_status_t.
pub const PSFS_ERR_FATAL: i32 = 0;
pub const PSFS_FEED_ME: i32 = 1;
pub const PSFS_PASS_ON: i32 = 2;

/// PSFS_FLAG_NORMAL
const PSFS_FLAG_NORMAL: i32 = 0;

pub struct FilterOutput {
    /// PSFS_* status the C filter function would return.
    pub status: i32,
    /// Buckets the C code would append to buckets_out, in order.
    pub out: Vec<Vec<u8>>,
    /// Value the C code stores in *bytes_consumed (sum of consumed input).
    /// On PSFS_ERR_FATAL the C code leaves *bytes_consumed untouched; this is
    /// then 0.
    pub consumed: usize,
    /// Warnings raised with php_error_docref(NULL, E_WARNING, ...), without
    /// any "func(): " prefix.
    pub warnings: Vec<String>,
}

impl FilterOutput {
    fn new() -> Self {
        FilterOutput {
            status: PSFS_PASS_ON,
            out: Vec::new(),
            consumed: 0,
            warnings: Vec::new(),
        }
    }
}

/* ------------------------------------------------------------------------ */
/* php_conv                                                                  */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConvErr {
    Success,
    Unknown,
    TooBig,
    InvalidSeq,
    UnexpectedEos,
}

/// Output cursor: the C code's `pd` / `ocnt` pair. Bytes are appended to
/// `buf`; `ocnt` is the room left in the (simulated) output buffer.
struct Out<'a> {
    buf: &'a mut Vec<u8>,
    ocnt: usize,
}

impl Out<'_> {
    #[inline]
    fn put(&mut self, b: u8) {
        self.buf.push(b);
        self.ocnt -= 1;
    }
    #[inline]
    fn put_all(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
        self.ocnt -= b.len();
    }
}

/// Line-break characters as the C code sees them: a NUL-terminated copy, so
/// indexing at `len` yields 0 (the terminator) like `inst->lbchars[len]`.
#[derive(Clone)]
struct LbChars(Vec<u8>);

impl LbChars {
    #[inline]
    fn at(&self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }
    #[inline]
    fn len(&self) -> usize {
        self.0.len()
    }
}

static B64_TBL_ENC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[inline]
fn b64e(v: u32) -> u8 {
    // The C table has 256 entries repeating the 64-char alphabet.
    B64_TBL_ENC[(v & 0x3f) as usize]
}

#[inline]
fn b64_tbl_dec(c: u8) -> u32 {
    match c {
        b'A'..=b'Z' => (c - b'A') as u32,
        b'a'..=b'z' => (c - b'a') as u32 + 26,
        b'0'..=b'9' => (c - b'0') as u32 + 52,
        b'+' => 62,
        b'/' => 63,
        b'=' => 128,
        _ => 64,
    }
}

/* {{{ php_conv_base64_encode */
struct Base64Encode {
    lbchars: Option<LbChars>,
    erem_len: usize,
    line_ccnt: u32,
    line_len: u32,
    erem: [u8; 3],
}

impl Base64Encode {
    fn new(line_len: u32, lbchars: Option<Vec<u8>>) -> Self {
        Base64Encode {
            lbchars: lbchars.map(LbChars),
            erem_len: 0,
            line_ccnt: line_len,
            line_len,
            erem: [0; 3],
        }
    }

    fn flush(&mut self, out: &mut Out) -> ConvErr {
        let mut err = ConvErr::Success;
        let mut line_ccnt = self.line_ccnt;
        match self.erem_len {
            0 => {}
            1 | 2 => 'out: {
                if line_ccnt < 4 {
                    if let Some(lb) = &self.lbchars {
                        if out.ocnt < lb.len() {
                            return ConvErr::TooBig;
                        }
                        out.put_all(&lb.0);
                        line_ccnt = self.line_len;
                    }
                }
                if out.ocnt < 4 {
                    err = ConvErr::TooBig;
                    break 'out;
                }
                let e0 = self.erem[0] as u32;
                out.put(b64e(e0 >> 2));
                if self.erem_len == 1 {
                    out.put(b64e((e0 << 4) & 0xff));
                    out.put(b'=');
                } else {
                    let e1 = self.erem[1] as u32;
                    out.put(b64e(((e0 << 4) & 0xff) | (e1 >> 4)));
                    out.put(b64e((e1 << 2) & 0xff));
                }
                out.put(b'=');
                self.erem_len = 0;
                line_ccnt = line_ccnt.wrapping_sub(4);
            }
            _ => err = ConvErr::Unknown,
        }
        self.line_ccnt = line_ccnt;
        err
    }

    fn convert(&mut self, input: Option<(&[u8], &mut usize)>, out: &mut Out) -> ConvErr {
        let Some((inp, pos)) = input else {
            return self.flush(out);
        };
        let mut err = ConvErr::Success;
        let mut ps = *pos;
        let mut icnt = inp.len() - ps;
        let mut line_ccnt = self.line_ccnt;

        'out: {
            /* consume the remainder first */
            match self.erem_len {
                1 => {
                    if icnt >= 2 {
                        if line_ccnt < 4 {
                            if let Some(lb) = &self.lbchars {
                                if out.ocnt < lb.len() {
                                    return ConvErr::TooBig;
                                }
                                out.put_all(&lb.0);
                                line_ccnt = self.line_len;
                            }
                        }
                        if out.ocnt < 4 {
                            err = ConvErr::TooBig;
                            break 'out;
                        }
                        let e0 = self.erem[0] as u32;
                        let p0 = inp[ps] as u32;
                        let p1 = inp[ps + 1] as u32;
                        out.put(b64e(e0 >> 2));
                        out.put(b64e(((e0 << 4) & 0xff) | (p0 >> 4)));
                        out.put(b64e(((p0 << 2) & 0xff) | (p1 >> 6)));
                        out.put(b64e(p1));
                        ps += 2;
                        icnt -= 2;
                        self.erem_len = 0;
                        line_ccnt = line_ccnt.wrapping_sub(4);
                    }
                }
                2 => {
                    if icnt >= 1 {
                        if self.line_ccnt < 4 {
                            if let Some(lb) = &self.lbchars {
                                if out.ocnt < lb.len() {
                                    return ConvErr::TooBig;
                                }
                                out.put_all(&lb.0);
                                line_ccnt = self.line_len;
                            }
                        }
                        if out.ocnt < 4 {
                            err = ConvErr::TooBig;
                            break 'out;
                        }
                        let e0 = self.erem[0] as u32;
                        let e1 = self.erem[1] as u32;
                        let p0 = inp[ps] as u32;
                        out.put(b64e(e0 >> 2));
                        out.put(b64e(((e0 << 4) & 0xff) | (e1 >> 4)));
                        out.put(b64e(((e1 << 2) & 0xff) | (p0 >> 6)));
                        out.put(b64e(p0));
                        ps += 1;
                        icnt -= 1;
                        self.erem_len = 0;
                        line_ccnt = line_ccnt.wrapping_sub(4);
                    }
                }
                _ => {}
            }

            while icnt >= 3 {
                if line_ccnt < 4 {
                    if let Some(lb) = &self.lbchars {
                        if out.ocnt < lb.len() {
                            err = ConvErr::TooBig;
                            break 'out;
                        }
                        out.put_all(&lb.0);
                        line_ccnt = self.line_len;
                    }
                }
                if out.ocnt < 4 {
                    err = ConvErr::TooBig;
                    break 'out;
                }
                let p0 = inp[ps] as u32;
                let p1 = inp[ps + 1] as u32;
                let p2 = inp[ps + 2] as u32;
                out.put(b64e(p0 >> 2));
                out.put(b64e(((p0 << 4) & 0xff) | (p1 >> 4)));
                out.put(b64e(((p1 << 2) & 0xff) | (p2 >> 6)));
                out.put(b64e(p2));
                ps += 3;
                icnt -= 3;
                line_ccnt = line_ccnt.wrapping_sub(4);
            }
            while icnt > 0 {
                self.erem[self.erem_len] = inp[ps];
                self.erem_len += 1;
                ps += 1;
                icnt -= 1;
            }
        }

        *pos = ps;
        self.line_ccnt = line_ccnt;
        err
    }
}
/* }}} */

/* {{{ php_conv_base64_decode */
struct Base64Decode {
    urem: u32,
    urem_nbits: u32,
    ustat: u32,
    eos: bool,
}

#[inline]
fn bmask(a: u32) -> u32 {
    0xffff >> (16 - a)
}

impl Base64Decode {
    fn new() -> Self {
        Base64Decode {
            urem: 0,
            urem_nbits: 0,
            ustat: 0,
            eos: false,
        }
    }

    fn convert(&mut self, input: Option<(&[u8], &mut usize)>, out: &mut Out) -> ConvErr {
        const NBITSOF_PACK: u32 = 8;
        let Some((inp, pos)) = input else {
            if self.eos || self.urem_nbits == 0 {
                return ConvErr::Success;
            }
            return ConvErr::UnexpectedEos;
        };

        let mut err = ConvErr::Success;
        let mut ps = *pos;
        let mut icnt = inp.len() - ps;

        let mut urem = self.urem;
        let mut urem_nbits = self.urem_nbits;
        let mut ustat = self.ustat;

        let mut pack: u32 = 0;
        let mut pack_bcnt: u32 = NBITSOF_PACK;

        loop {
            if pack_bcnt >= urem_nbits {
                pack_bcnt -= urem_nbits;
                pack |= urem << pack_bcnt;
                urem_nbits = 0;
            } else {
                urem_nbits -= pack_bcnt;
                pack |= urem >> urem_nbits;
                urem &= bmask(urem_nbits);
                pack_bcnt = 0;
            }
            if pack_bcnt > 0 {
                if icnt < 1 {
                    break;
                }
                let i = b64_tbl_dec(inp[ps]);
                ps += 1;
                icnt -= 1;
                ustat |= i & 0x80;

                if i & 0xc0 == 0 {
                    if ustat != 0 {
                        err = ConvErr::InvalidSeq;
                        break;
                    }
                    if 6 <= pack_bcnt {
                        pack_bcnt -= 6;
                        pack |= i << pack_bcnt;
                        urem = 0;
                    } else {
                        urem_nbits = 6 - pack_bcnt;
                        pack |= i >> urem_nbits;
                        urem = i & bmask(urem_nbits);
                        pack_bcnt = 0;
                    }
                } else if ustat != 0 {
                    if pack_bcnt == 8 || pack_bcnt == 2 {
                        err = ConvErr::InvalidSeq;
                        break;
                    }
                    self.eos = true;
                }
            }
            if (pack_bcnt | ustat) == 0 {
                if out.ocnt < 1 {
                    err = ConvErr::TooBig;
                    break;
                }
                out.put(pack as u8);
                pack = 0;
                pack_bcnt = NBITSOF_PACK;
            }
        }

        if urem_nbits >= pack_bcnt {
            urem |= pack << (urem_nbits - pack_bcnt);
        } else {
            urem |= pack >> (pack_bcnt - urem_nbits);
        }
        urem_nbits += NBITSOF_PACK - pack_bcnt;

        self.urem = urem;
        self.urem_nbits = urem_nbits;
        self.ustat = ustat;
        *pos = ps;
        err
    }
}
/* }}} */

/* {{{ php_conv_qprint_encode */
const PHP_CONV_QPRINT_OPT_BINARY: i32 = 0x0000_0001;
const PHP_CONV_QPRINT_OPT_FORCE_ENCODE_FIRST: i32 = 0x0000_0002;

struct QprintEncode {
    lbchars: Option<LbChars>,
    opts: i32,
    line_ccnt: u32,
    line_len: u32,
    lb_ptr: u32,
    lb_cnt: u32,
}

static QP_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

impl QprintEncode {
    fn new(line_len: u32, lbchars: Option<Vec<u8>>, opts: i32) -> Result<Self, ConvErr> {
        if line_len < 4 && lbchars.is_some() {
            return Err(ConvErr::TooBig);
        }
        Ok(QprintEncode {
            lbchars: lbchars.map(LbChars),
            opts,
            line_ccnt: line_len,
            line_len,
            lb_ptr: 0,
            lb_cnt: 0,
        })
    }

    fn convert(&mut self, input: Option<(&[u8], &mut usize)>, out: &mut Out) -> ConvErr {
        let mut err = ConvErr::Success;
        let mut line_ccnt = self.line_ccnt;
        let opts = self.opts;
        let mut lb_ptr = self.lb_ptr;
        let mut lb_cnt = self.lb_cnt;

        let Some((inp, pos)) = input else {
            return ConvErr::Success;
        };

        let mut ps = *pos;
        let mut icnt = inp.len() - ps;
        let mut trail_ws: u32 = 0;
        let lbchars = self.lbchars.as_ref();
        let binary = opts & PHP_CONV_QPRINT_OPT_BINARY != 0;

        macro_rules! consume_char {
            () => {
                if lb_ptr < lb_cnt {
                    lb_ptr += 1;
                } else {
                    lb_cnt = 0;
                    lb_ptr = 0;
                    icnt -= 1;
                    ps += 1;
                }
            };
        }

        loop {
            if !binary {
                if let Some(lb) = lbchars {
                    if lb.len() > 0 {
                        /* look ahead for the line break chars to make a right
                         * decision how to consume incoming characters */
                        if icnt > 0 && inp[ps] == lb.at(lb_cnt as usize) {
                            lb_cnt += 1;
                            if lb_cnt as usize >= lb.len() {
                                if out.ocnt < lb_cnt as usize {
                                    lb_cnt -= 1;
                                    err = ConvErr::TooBig;
                                    break;
                                }
                                for i in 0..lb_cnt as usize {
                                    out.put(lb.at(i));
                                }
                                line_ccnt = self.line_len;
                                lb_ptr = 0;
                                lb_cnt = 0;
                            }
                            ps += 1;
                            icnt -= 1;
                            continue;
                        }
                    }
                }
            }

            if lb_ptr >= lb_cnt && icnt == 0 {
                break;
            }

            let c: u8 = if lb_ptr < lb_cnt {
                lbchars.map_or(0, |lb| lb.at(lb_ptr as usize))
            } else {
                inp[ps]
            };

            if !binary && trail_ws == 0 && (c == b'\t' || c == b' ') {
                if let Some(lb) = lbchars.filter(|_| line_ccnt < 2) {
                    if out.ocnt < lb.len() + 1 {
                        err = ConvErr::TooBig;
                        break;
                    }
                    out.put(b'=');
                    out.put_all(&lb.0);
                    line_ccnt = self.line_len;
                } else {
                    if out.ocnt < 1 {
                        err = ConvErr::TooBig;
                        break;
                    }

                    /* Check to see if this is EOL whitespace. */
                    if let Some(lb) = lbchars {
                        let mut lb_cnt2: usize = 0;
                        let mut ps2 = ps;
                        trail_ws = 1;

                        // C: for (j = icnt - 1; j > 0; j--, ps2++) — with
                        // icnt == 0 that would wrap and read out of bounds;
                        // stop at the end of the input instead.
                        let mut j = icnt.wrapping_sub(1);
                        while j > 0 && ps2 < inp.len() {
                            let b = inp[ps2];
                            if b == lb.at(lb_cnt2) {
                                lb_cnt2 += 1;
                                if lb_cnt2 >= lb.len() {
                                    /* Found trailing ws. */
                                    break;
                                }
                            } else if lb_cnt2 != 0 || (b != b'\t' && b != b' ') {
                                /* At least one non-EOL character following, so
                                 * don't need to encode ws. */
                                trail_ws = 0;
                                break;
                            } else {
                                trail_ws += 1;
                            }
                            j -= 1;
                            ps2 += 1;
                        }
                    }

                    if trail_ws == 0 {
                        out.put(c);
                        line_ccnt = line_ccnt.wrapping_sub(1);
                        consume_char!();
                    }
                }
            } else if (opts & PHP_CONV_QPRINT_OPT_FORCE_ENCODE_FIRST == 0
                || line_ccnt < self.line_len)
                && ((33..=60).contains(&c) || (62..=126).contains(&c))
            {
                if line_ccnt < 2 {
                    if let Some(lb) = lbchars {
                        if out.ocnt < lb.len() + 1 {
                            err = ConvErr::TooBig;
                            break;
                        }
                        out.put(b'=');
                        out.put_all(&lb.0);
                        line_ccnt = self.line_len;
                    }
                }
                if out.ocnt < 1 {
                    err = ConvErr::TooBig;
                    break;
                }
                out.put(c);
                line_ccnt = line_ccnt.wrapping_sub(1);
                consume_char!();
            } else {
                if line_ccnt < 4 {
                    if let Some(lb) = lbchars {
                        if out.ocnt < lb.len() + 1 {
                            err = ConvErr::TooBig;
                            break;
                        }
                        out.put(b'=');
                        out.put_all(&lb.0);
                        line_ccnt = self.line_len;
                    }
                }
                if out.ocnt < 3 {
                    err = ConvErr::TooBig;
                    break;
                }
                out.put(b'=');
                out.put(QP_DIGITS[(c >> 4) as usize]);
                out.put(QP_DIGITS[(c & 0x0f) as usize]);
                line_ccnt = line_ccnt.wrapping_sub(3);
                trail_ws = trail_ws.saturating_sub(1);
                consume_char!();
            }
        }

        *pos = ps;
        self.line_ccnt = line_ccnt;
        self.lb_ptr = lb_ptr;
        self.lb_cnt = lb_cnt;
        err
    }
}
/* }}} */

/* {{{ php_conv_qprint_decode */
struct QprintDecode {
    lbchars: Option<LbChars>,
    scan_stat: u32,
    next_char: u32,
    lb_ptr: u32,
    lb_cnt: u32,
}

impl QprintDecode {
    fn new(lbchars: Option<Vec<u8>>) -> Self {
        QprintDecode {
            lbchars: lbchars.map(LbChars),
            scan_stat: 0,
            next_char: 0,
            lb_ptr: 0,
            lb_cnt: 0,
        }
    }

    fn convert(&mut self, input: Option<(&[u8], &mut usize)>, out: &mut Out) -> ConvErr {
        let mut err = ConvErr::Success;
        let mut lb_ptr = self.lb_ptr;
        let mut lb_cnt = self.lb_cnt;

        let Some((inp, pos)) = input else {
            if self.scan_stat != 0 {
                return ConvErr::UnexpectedEos;
            }
            return ConvErr::Success;
        };

        let mut ps = *pos;
        let mut icnt = inp.len() - ps;
        let mut scan_stat = self.scan_stat;
        let mut next_char = self.next_char;
        let has_lb = self.lbchars.is_some();
        let lbchars_len = self.lbchars.as_ref().map_or(0, |l| l.len());
        let lbc = |i: u32| self.lbchars.as_ref().map_or(0, |l| l.at(i as usize));

        'out: loop {
            // `label` emulates the C switch's case labels and fallthroughs;
            // `break` out of the inner loop is the C `break` out of the switch.
            let mut label = scan_stat;
            loop {
                match label {
                    0 => {
                        if icnt == 0 {
                            break 'out;
                        }
                        if inp[ps] == b'=' {
                            scan_stat = 1;
                        } else {
                            if out.ocnt < 1 {
                                err = ConvErr::TooBig;
                                break 'out;
                            }
                            out.put(inp[ps]);
                        }
                        ps += 1;
                        icnt -= 1;
                        break;
                    }
                    1 => {
                        if icnt == 0 {
                            break 'out;
                        }
                        let b = inp[ps];
                        if b == b' ' || b == b'\t' {
                            scan_stat = 4;
                            ps += 1;
                            icnt -= 1;
                            break;
                        } else if !has_lb && lb_cnt == 0 && b == b'\r' {
                            /* auto-detect line endings, looks like network line ending \r\n (could be mac \r) */
                            lb_cnt += 1;
                            scan_stat = 5;
                            ps += 1;
                            icnt -= 1;
                            break;
                        } else if !has_lb && lb_cnt == 0 && b == b'\n' {
                            /* auto-detect line endings, looks like unix-lineendings */
                            lb_cnt = 0;
                            lb_ptr = 0;
                            scan_stat = 0;
                            ps += 1;
                            icnt -= 1;
                            break;
                        } else if (lb_cnt as usize) < lbchars_len && b == lbc(lb_cnt) {
                            lb_cnt += 1;
                            scan_stat = 5;
                            ps += 1;
                            icnt -= 1;
                            break;
                        }
                        label = 2;
                    }
                    2 => {
                        if icnt == 0 {
                            break 'out;
                        }
                        let b = inp[ps];
                        if !b.is_ascii_hexdigit() {
                            err = ConvErr::InvalidSeq;
                            break 'out;
                        }
                        let d = if b >= b'A' {
                            (b - 0x37) as u32
                        } else {
                            (b - 0x30) as u32
                        };
                        next_char = (next_char << 4) | d;
                        scan_stat += 1;
                        ps += 1;
                        icnt -= 1;
                        if scan_stat != 3 {
                            break;
                        }
                        label = 3;
                    }
                    3 => {
                        if out.ocnt < 1 {
                            err = ConvErr::TooBig;
                            break 'out;
                        }
                        out.put(next_char as u8);
                        scan_stat = 0;
                        break;
                    }
                    4 => {
                        if icnt == 0 {
                            break 'out;
                        }
                        let b = inp[ps];
                        if (lb_cnt as usize) < lbchars_len && b == lbc(lb_cnt) {
                            lb_cnt += 1;
                            scan_stat = 5;
                        } else if b != b'\t' && b != b' ' {
                            err = ConvErr::InvalidSeq;
                            break 'out;
                        }
                        ps += 1;
                        icnt -= 1;
                        break;
                    }
                    5 => {
                        if icnt == 0 {
                            break 'out;
                        }
                        let b = inp[ps];
                        if !has_lb && lb_cnt == 1 && b == b'\n' {
                            /* auto-detect soft line breaks, found network line break */
                            lb_cnt = 0;
                            lb_ptr = 0;
                            scan_stat = 0;
                            ps += 1;
                            icnt -= 1;
                        } else if !has_lb && lb_cnt > 0 {
                            /* auto-detect soft line breaks, found mac line break */
                            lb_cnt = 0;
                            lb_ptr = 0;
                            scan_stat = 0;
                        } else if lb_cnt as usize >= lbchars_len {
                            /* soft line break */
                            lb_cnt = 0;
                            lb_ptr = 0;
                            scan_stat = 0;
                        } else if b == lbc(lb_cnt) {
                            lb_cnt += 1;
                            ps += 1;
                            icnt -= 1;
                        } else {
                            scan_stat = 6; /* no break for short-cut */
                        }
                        break;
                    }
                    6 => {
                        if lb_ptr < lb_cnt {
                            if out.ocnt < 1 {
                                err = ConvErr::TooBig;
                                break 'out;
                            }
                            out.put(lbc(lb_ptr));
                            lb_ptr += 1;
                        } else {
                            scan_stat = 0;
                            lb_cnt = 0;
                            lb_ptr = 0;
                        }
                        break;
                    }
                    _ => break,
                }
            }
        }

        *pos = ps;
        self.scan_stat = scan_stat;
        self.lb_ptr = lb_ptr;
        self.lb_cnt = lb_cnt;
        self.next_char = next_char;
        err
    }
}
/* }}} */

enum Conv {
    Base64Encode(Base64Encode),
    Base64Decode(Base64Decode),
    QprintEncode(QprintEncode),
    QprintDecode(QprintDecode),
}

impl Conv {
    fn convert(&mut self, input: Option<(&[u8], &mut usize)>, out: &mut Out) -> ConvErr {
        match self {
            Conv::Base64Encode(c) => c.convert(input, out),
            Conv::Base64Decode(c) => c.convert(input, out),
            Conv::QprintEncode(c) => c.convert(input, out),
            Conv::QprintDecode(c) => c.convert(input, out),
        }
    }
}

/* ------------------------------------------------------------------------ */
/* option helpers (php_conv_get_*_prop_ex)                                  */
/* ------------------------------------------------------------------------ */

fn find_prop(ht: &Value, name: &[u8]) -> Option<Value> {
    match ht {
        Value::Array(rc) => rc
            .borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(name)))
            .map(|v| v.deref()),
        _ => None,
    }
}

/// php_conv_get_string_prop_ex (zval_get_tmp_string conversion).
fn get_string_prop(ht: &Value, name: &[u8]) -> Option<Vec<u8>> {
    find_prop(ht, name).map(|v| v.to_php_string().as_bytes().to_vec())
}

/// php_conv_get_uint_prop_ex: zval_get_long, negative -> 0, truncated to
/// unsigned int. Not found -> 0.
fn get_uint_prop(ht: &Value, name: &[u8]) -> u32 {
    match find_prop(ht, name) {
        Some(v) => {
            let l = v.to_long();
            if l < 0 { 0 } else { l as u64 as u32 }
        }
        None => 0,
    }
}

/// php_conv_get_bool_prop_ex (zend_is_true). Not found -> false.
fn get_bool_prop(ht: &Value, name: &[u8]) -> bool {
    find_prop(ht, name).is_some_and(|v| v.is_truthy())
}

const PHP_CONV_BASE64_ENCODE: i32 = 1;
const PHP_CONV_BASE64_DECODE: i32 = 2;
const PHP_CONV_QPRINT_ENCODE: i32 = 3;
const PHP_CONV_QPRINT_DECODE: i32 = 4;

/// Reads "line-break-chars" / "line-length" as the base64 / qprint encoders
/// do: below 4 the line-break chars are dropped, otherwise they default to
/// "\r\n".
fn get_line_opts(options: &Value) -> (u32, Option<Vec<u8>>) {
    let lbchars = get_string_prop(options, b"line-break-chars");
    let line_len = get_uint_prop(options, b"line-length");
    if line_len < 4 {
        (line_len, None)
    } else {
        (line_len, Some(lbchars.unwrap_or_else(|| b"\r\n".to_vec())))
    }
}

fn php_conv_open(conv_mode: i32, options: Option<&Value>) -> Option<Conv> {
    match conv_mode {
        PHP_CONV_BASE64_ENCODE => {
            let (line_len, lbchars) = options.map_or((0, None), get_line_opts);
            Some(Conv::Base64Encode(match lbchars {
                Some(lb) => Base64Encode::new(line_len, Some(lb)),
                None => Base64Encode::new(0, None),
            }))
        }
        PHP_CONV_BASE64_DECODE => Some(Conv::Base64Decode(Base64Decode::new())),
        PHP_CONV_QPRINT_ENCODE => {
            let mut line_len = 0;
            let mut lbchars = None;
            let mut opts = 0;
            if let Some(options) = options {
                let lbc = get_string_prop(options, b"line-break-chars");
                line_len = get_uint_prop(options, b"line-length");
                let opt_binary = get_bool_prop(options, b"binary");
                let opt_force_encode_first = get_bool_prop(options, b"force-encode-first");
                if line_len >= 4 {
                    lbchars = Some(lbc.unwrap_or_else(|| b"\r\n".to_vec()));
                }
                if opt_binary {
                    opts |= PHP_CONV_QPRINT_OPT_BINARY;
                }
                if opt_force_encode_first {
                    opts |= PHP_CONV_QPRINT_OPT_FORCE_ENCODE_FIRST;
                }
            }
            let r = if lbchars.is_some() {
                QprintEncode::new(line_len, lbchars, opts)
            } else {
                QprintEncode::new(0, None, opts)
            };
            r.ok().map(Conv::QprintEncode)
        }
        PHP_CONV_QPRINT_DECODE => {
            /* If line-break-chars are not specified, filter will attempt to
             * detect line endings (\r, \n, or \r\n) */
            let lbchars = options.and_then(|o| get_string_prop(o, b"line-break-chars"));
            Some(Conv::QprintDecode(QprintDecode::new(lbchars)))
        }
        _ => None,
    }
}

/* ------------------------------------------------------------------------ */
/* strfilter_convert                                                        */
/* ------------------------------------------------------------------------ */

/// strfilter_convert (convert.base64-encode/decode,
/// convert.quoted-printable-encode/decode)
pub struct ConvertFilter {
    cd: Conv,
    filtername: String,
}

impl ConvertFilter {
    /// strfilter_convert_create(filtername, filterparams, persistent=0).
    /// `Err(warnings)` when the C factory returns NULL. Note that an explicit
    /// `null` filter parameter is `Some(&Value::Null)` (a non-array zval) and
    /// fails with "invalid filter parameter", like in PHP.
    pub fn create(filtername: &[u8], params: Option<&Value>) -> Result<ConvertFilter, Vec<String>> {
        let name = String::from_utf8_lossy(filtername).into_owned();
        let params = params.map(|p| p.deref());
        if let Some(p) = &params {
            if !matches!(p, Value::Array(_)) {
                return Err(vec![format!(
                    "Stream filter ({}): invalid filter parameter",
                    name
                )]);
            }
        }
        let Some(dot) = filtername.iter().position(|&b| b == b'.') else {
            return Err(Vec::new());
        };
        let rest = &filtername[dot + 1..];
        let conv_mode = if rest.eq_ignore_ascii_case(b"base64-encode") {
            PHP_CONV_BASE64_ENCODE
        } else if rest.eq_ignore_ascii_case(b"base64-decode") {
            PHP_CONV_BASE64_DECODE
        } else if rest.eq_ignore_ascii_case(b"quoted-printable-encode") {
            PHP_CONV_QPRINT_ENCODE
        } else if rest.eq_ignore_ascii_case(b"quoted-printable-decode") {
            PHP_CONV_QPRINT_DECODE
        } else {
            0
        };
        match php_conv_open(conv_mode, params.as_ref()) {
            Some(cd) => Ok(ConvertFilter {
                cd,
                filtername: name,
            }),
            None => Err(Vec::new()),
        }
    }

    /// strfilter_convert_append_bucket. `ps == None` is the flush call.
    fn append_bucket(
        &mut self,
        ps: Option<&[u8]>,
        buckets_out: &mut Vec<Vec<u8>>,
        consumed: &mut usize,
    ) -> Result<(), String> {
        let (initial_out_buf_size, mut icnt, buf_len) = match ps {
            None => (64usize, 1usize, 0usize),
            Some(b) => (b.len(), b.len(), b.len()),
        };
        let mut out_buf_size = initial_out_buf_size;
        let mut buf = Vec::new();
        let mut ocnt = initial_out_buf_size;
        let mut pos = 0usize;

        // (inst->stub is only filled on PHP_CONV_ERR_MORE, which no converter
        // returns, so the stub handling is dead code and omitted.)
        while icnt > 0 {
            let mut out = Out {
                buf: &mut buf,
                ocnt,
            };
            let err = match ps {
                None => self.cd.convert(None, &mut out),
                Some(b) => {
                    let e = self.cd.convert(Some((b, &mut pos)), &mut out);
                    icnt = b.len() - pos;
                    e
                }
            };
            ocnt = out.ocnt;
            match err {
                ConvErr::InvalidSeq => {
                    return Err(format!(
                        "Stream filter ({}): invalid byte sequence",
                        self.filtername
                    ));
                }
                ConvErr::TooBig => {
                    // new_out_buf_size = out_buf_size << 1 never overflows here.
                    ocnt += out_buf_size;
                    out_buf_size <<= 1;
                }
                ConvErr::Unknown => {
                    return Err(format!(
                        "Stream filter ({}): unknown error",
                        self.filtername
                    ));
                }
                // UNEXPECTED_EOS is not handled in this loop by the C code
                // (only in the dead stub loop): it is silently ignored.
                ConvErr::Success | ConvErr::UnexpectedEos => {
                    if ps.is_none() {
                        icnt = 0;
                    }
                }
            }
        }

        if out_buf_size > ocnt {
            buckets_out.push(buf);
        }
        *consumed += buf_len - icnt;
        Ok(())
    }

    /// strfilter_convert_filter().
    pub fn filter(&mut self, input: &[Vec<u8>], flags: i32) -> FilterOutput {
        let mut res = FilterOutput::new();
        let mut consumed = 0usize;
        for bucket in input {
            if let Err(w) = self.append_bucket(Some(bucket), &mut res.out, &mut consumed) {
                res.warnings.push(w);
                res.status = PSFS_ERR_FATAL;
                return res;
            }
        }
        if flags != PSFS_FLAG_NORMAL {
            if let Err(w) = self.append_bucket(None, &mut res.out, &mut consumed) {
                res.warnings.push(w);
                res.status = PSFS_ERR_FATAL;
                return res;
            }
        }
        res.consumed = consumed;
        res
    }
}

/* ------------------------------------------------------------------------ */
/* dechunk                                                                  */
/* ------------------------------------------------------------------------ */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkState {
    SizeStart,
    Size,
    SizeExt,
    SizeCr,
    SizeLf,
    Body,
    BodyCr,
    BodyLf,
    Trailer,
    Error,
}

/// The "dechunk" filter (php_chunked_filter_*).
pub struct DechunkFilter {
    chunk_size: usize,
    state: ChunkState,
}

impl DechunkFilter {
    /// chunked_filter_create: only the exact (case-insensitive) name
    /// "dechunk" is accepted; parameters are ignored.
    pub fn create(
        filtername: &[u8],
        _params: Option<&Value>,
    ) -> Result<DechunkFilter, Vec<String>> {
        if !filtername.eq_ignore_ascii_case(b"dechunk") {
            return Err(Vec::new());
        }
        Ok(DechunkFilter {
            chunk_size: 0,
            state: ChunkState::SizeStart,
        })
    }

    /// php_dechunk: returns the decoded bytes of `buf`.
    fn dechunk(&mut self, buf: &[u8]) -> Vec<u8> {
        use ChunkState::*;
        let end = buf.len();
        let mut p = 0usize;
        let mut out = Vec::new();

        'outer: while p < end {
            // `label` follows the C switch fallthroughs, which do not update
            // data->state.
            let mut label = self.state;
            loop {
                match label {
                    SizeStart => {
                        self.chunk_size = 0;
                        label = Size;
                    }
                    Size => {
                        while p < end {
                            let c = buf[p];
                            let d = match c {
                                b'0'..=b'9' => (c - b'0') as usize,
                                b'A'..=b'F' => (c - b'A' + 10) as usize,
                                b'a'..=b'f' => (c - b'a' + 10) as usize,
                                _ => {
                                    if self.state == SizeStart {
                                        self.state = Error;
                                    } else {
                                        self.state = SizeExt;
                                    }
                                    break;
                                }
                            };
                            self.chunk_size = self.chunk_size.wrapping_mul(16).wrapping_add(d);
                            self.state = Size;
                            p += 1;
                        }
                        if self.state == Error {
                            continue 'outer;
                        } else if p == end {
                            return out;
                        }
                        label = SizeExt;
                    }
                    SizeExt => {
                        /* skip extension */
                        while p < end && buf[p] != b'\r' && buf[p] != b'\n' {
                            p += 1;
                        }
                        if p == end {
                            return out;
                        }
                        label = SizeCr;
                    }
                    SizeCr => {
                        if buf[p] == b'\r' {
                            p += 1;
                            if p == end {
                                self.state = SizeLf;
                                return out;
                            }
                        }
                        label = SizeLf;
                    }
                    SizeLf => {
                        if buf[p] == b'\n' {
                            p += 1;
                            if self.chunk_size == 0 {
                                /* last chunk */
                                self.state = Trailer;
                                continue 'outer;
                            } else if p == end {
                                self.state = Body;
                                return out;
                            }
                        } else {
                            self.state = Error;
                            continue 'outer;
                        }
                        label = Body;
                    }
                    Body => {
                        if end - p >= self.chunk_size {
                            out.extend_from_slice(&buf[p..p + self.chunk_size]);
                            p += self.chunk_size;
                            if p == end {
                                self.state = BodyCr;
                                return out;
                            }
                        } else {
                            out.extend_from_slice(&buf[p..end]);
                            self.chunk_size -= end - p;
                            self.state = Body;
                            return out;
                        }
                        label = BodyCr;
                    }
                    BodyCr => {
                        if buf[p] == b'\r' {
                            p += 1;
                            if p == end {
                                self.state = BodyLf;
                                return out;
                            }
                        }
                        label = BodyLf;
                    }
                    BodyLf => {
                        if buf[p] == b'\n' {
                            p += 1;
                            self.state = SizeStart;
                        } else {
                            self.state = Error;
                        }
                        continue 'outer;
                    }
                    Trailer => {
                        /* ignore trailer */
                        p = end;
                        continue 'outer;
                    }
                    Error => {
                        out.extend_from_slice(&buf[p..end]);
                        return out;
                    }
                }
            }
        }
        out
    }

    /// php_chunked_filter(): every input bucket is decoded in place and
    /// appended to the output (possibly empty); flags are ignored.
    pub fn filter(&mut self, input: &[Vec<u8>], _flags: i32) -> FilterOutput {
        let mut res = FilterOutput::new();
        for bucket in input {
            res.consumed += bucket.len();
            let o = self.dechunk(bucket);
            res.out.push(o);
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goro_core::array::PhpArray;
    use std::cell::RefCell;
    use std::rc::Rc;

    pub(super) enum P {
        L(i64),
        S(&'static [u8]),
        B(bool),
    }

    pub(super) struct RefCase {
        filter: &'static [u8],
        params: Option<&'static [(&'static [u8], P)]>,
        chunks: &'static [&'static [u8]],
        /// Buckets seen after the filter, per fwrite() then for the final flush.
        steps: &'static [&'static [&'static [u8]]],
        warning: Option<&'static [u8]>,
    }

    fn arr(params: &[(&[u8], P)]) -> Value {
        let mut a = PhpArray::new();
        for (k, v) in params {
            let v = match v {
                P::L(l) => Value::Long(*l),
                P::S(s) => Value::String(PhpString::from_bytes(s)),
                P::B(true) => Value::True,
                P::B(false) => Value::False,
            };
            a.set(ArrayKey::String(PhpString::from_bytes(k)), v);
        }
        Value::Array(Rc::new(RefCell::new(a)))
    }

    fn conv(name: &str, params: &[(&[u8], P)]) -> ConvertFilter {
        let p = arr(params);
        ConvertFilter::create(name.as_bytes(), Some(&p))
            .ok()
            .unwrap()
    }

    /// Feeds `chunks` one bucket per call, then flushes; returns all output.
    fn run(f: &mut ConvertFilter, chunks: &[&[u8]]) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        for c in chunks {
            let r = f.filter(&[c.to_vec()], 0);
            if r.status != PSFS_PASS_ON {
                return Err(r.warnings.join("|"));
            }
            assert_eq!(r.consumed, c.len());
            out.extend(r.out.concat());
        }
        let r = f.filter(&[], 2);
        if r.status != PSFS_PASS_ON {
            return Err(r.warnings.join("|"));
        }
        out.extend(r.out.concat());
        Ok(out)
    }

    fn run_all_splits(name: &str, params: &[(&[u8], P)], input: &[u8]) -> Result<Vec<u8>, String> {
        let whole = run(&mut conv(name, params), &[input]);
        for i in 0..=input.len() {
            for j in i..=input.len() {
                let parts = [&input[..i], &input[i..j], &input[j..]];
                assert_eq!(
                    run(&mut conv(name, params), &parts),
                    whole,
                    "split {i}/{j} of {:?}",
                    String::from_utf8_lossy(input)
                );
            }
        }
        whole
    }

    #[test]
    fn reference_cases() {
        for (n, case) in refdata::REF_CASES.iter().enumerate() {
            let p = case.params.map(arr);
            let mut f = ConvertFilter::create(case.filter, p.as_ref()).ok().unwrap();
            let nsteps = case.steps.len();
            for (i, expect) in case.steps.iter().enumerate() {
                let r = if i < case.chunks.len() {
                    f.filter(&[case.chunks[i].to_vec()], 0)
                } else {
                    f.filter(&[], 2)
                };
                if i + 1 == nsteps {
                    if let Some(w) = case.warning {
                        assert_eq!(r.status, PSFS_ERR_FATAL, "case {n}");
                        assert_eq!(r.warnings, vec![String::from_utf8_lossy(w).into_owned()]);
                        continue;
                    }
                }
                assert_eq!(r.status, PSFS_PASS_ON, "case {n} step {i}");
                let want: Vec<Vec<u8>> = expect.iter().map(|b| b.to_vec()).collect();
                assert_eq!(r.out, want, "case {n} step {i}");
            }
        }
    }

    #[test]
    fn base64_encode_splits() {
        let data: &[u8] = b"Hello, World! \x00\xff\x80 binary";
        let none: &[(&[u8], P)] = &[];
        assert_eq!(
            run_all_splits("convert.base64-encode", none, data).unwrap(),
            b"SGVsbG8sIFdvcmxkISAA/4AgYmluYXJ5"
        );
        for n in 0..8 {
            let d = &data[..n];
            let got = run_all_splits("convert.base64-encode", none, d).unwrap();
            let dec = run_all_splits("convert.base64-decode", none, &got).unwrap();
            assert_eq!(dec, d);
        }
        assert_eq!(
            run_all_splits("convert.base64-encode", &[(b"line-length", P::L(8))], data).unwrap(),
            b"SGVsbG8s\r\nIFdvcmxk\r\nISAA/4Ag\r\nYmluYXJ5"
        );
        assert_eq!(
            run_all_splits(
                "convert.base64-encode",
                &[
                    (b"line-length", P::L(6)),
                    (b"line-break-chars", P::S(b"\n"))
                ],
                b"abcdefghij"
            )
            .unwrap(),
            b"YWJj\nZGVm\nZ2hp\nag=="
        );
        // line-length < 4 disables line breaking.
        assert_eq!(
            run_all_splits(
                "convert.base64-encode",
                &[(b"line-length", P::L(3))],
                b"abcdefghij"
            )
            .unwrap(),
            b"YWJjZGVmZ2hpag=="
        );
    }

    #[test]
    fn base64_decode_splits() {
        let none: &[(&[u8], P)] = &[];
        assert_eq!(
            run_all_splits("convert.base64-decode", none, b"SGVs bG8s\r\nIFdv cmxk").unwrap(),
            b"Hello, World"
        );
        assert_eq!(
            run_all_splits("convert.base64-decode", none, b"YQ==").unwrap(),
            b"a"
        );
        assert_eq!(
            run_all_splits("convert.base64-decode", none, b"YWI=").unwrap(),
            b"ab"
        );
        // Missing padding at EOS is silently accepted (UNEXPECTED_EOS is ignored).
        assert_eq!(
            run_all_splits("convert.base64-decode", none, b"QUJDR").unwrap(),
            b"ABC"
        );
        let e = Err("Stream filter (convert.base64-decode): invalid byte sequence".to_string());
        assert_eq!(
            run(&mut conv("convert.base64-decode", none), &[b"YQ==YQ=="]),
            e
        );
        assert_eq!(run(&mut conv("convert.base64-decode", none), &[b"Y=="]), e);
    }

    #[test]
    fn qprint_decode_splits() {
        let none: &[(&[u8], P)] = &[];
        assert_eq!(
            run_all_splits(
                "convert.quoted-printable-decode",
                none,
                b"a=3Db=\r\nc=\nd=\re=\r\n"
            )
            .unwrap(),
            b"a=bcde"
        );
        assert_eq!(
            run_all_splits(
                "convert.quoted-printable-decode",
                &[(b"line-break-chars", P::S(b"\r\n"))],
                b"a=\r\nb=\rX"
            )
            .unwrap(),
            b"ab\rX"
        );
        // Trailing '=' at EOS: unexpected EOS is silently ignored.
        assert_eq!(
            run_all_splits("convert.quoted-printable-decode", none, b"ab=").unwrap(),
            b"ab"
        );
        assert_eq!(
            run(
                &mut conv("convert.quoted-printable-decode", none),
                &[b"a=G1"]
            ),
            Err(
                "Stream filter (convert.quoted-printable-decode): invalid byte sequence"
                    .to_string()
            )
        );
    }

    #[test]
    fn qprint_encode_bug64166() {
        let p74: &[(&[u8], P)] = &[
            (b"line-break-chars", P::S(b"\n")),
            (b"line-length", P::L(74)),
        ];
        let p6: &[(&[u8], P)] = &[
            (b"line-break-chars", P::S(b"\n")),
            (b"line-length", P::L(6)),
        ];
        let enc = "convert.quoted-printable-encode";
        assert_eq!(
            run(&mut conv(enc, p74), &[b"FIRST \nSECOND"]).unwrap(),
            b"FIRST=20\nSECOND"
        );
        assert_eq!(
            run(&mut conv(enc, p6), &[b"FIRST \nSECOND"]).unwrap(),
            b"FIRST=\n=20\nSECON=\nD"
        );
        assert_eq!(
            run(&mut conv(enc, p74), &[b"FIRST  \nSECOND"]).unwrap(),
            b"FIRST=20=20\nSECOND"
        );
        assert_eq!(
            run(&mut conv(enc, p6), &[b"FIRST  \nSECOND"]).unwrap(),
            b"FIRST=\n=20=\n=20\nSECON=\nD"
        );
    }

    #[test]
    fn create_errors() {
        assert_eq!(
            ConvertFilter::create(b"convert.base64-encode", Some(&Value::Null)).err(),
            Some(vec![
                "Stream filter (convert.base64-encode): invalid filter parameter".to_string()
            ])
        );
        assert_eq!(
            ConvertFilter::create(b"convert.foo", None).err(),
            Some(vec![])
        );
        assert!(ConvertFilter::create(b"CONVERT.Base64-Encode", None).is_ok());
        assert!(DechunkFilter::create(b"DeChunk", None).is_ok());
        assert!(DechunkFilter::create(b"dechunk.x", None).is_err());
    }

    fn dechunk_parts(parts: &[&[u8]]) -> (Vec<Vec<u8>>, usize) {
        let mut f = DechunkFilter::create(b"dechunk", None).ok().unwrap();
        let input: Vec<Vec<u8>> = parts.iter().map(|p| p.to_vec()).collect();
        let r = f.filter(&input, 0);
        assert_eq!(r.status, PSFS_PASS_ON);
        (r.out, r.consumed)
    }

    #[test]
    fn dechunk() {
        let data: &[u8] =
            b"5\r\nHello\r\n7;ext=1\r\n, World\nA\r\n0123456789\r\n0\r\nTrailer: x\r\n\r\nignored";
        let (out, consumed) = dechunk_parts(&[data]);
        assert_eq!(out, vec![b"Hello, World0123456789".to_vec()]);
        assert_eq!(consumed, data.len());
        for i in 0..=data.len() {
            for j in i..=data.len() {
                let (out, _) = dechunk_parts(&[&data[..i], &data[i..j], &data[j..]]);
                assert_eq!(out.len(), 3);
                assert_eq!(out.concat(), b"Hello, World0123456789", "split {i}/{j}");
            }
        }
        // Errors pass the remaining data through unchanged.
        assert_eq!(
            dechunk_parts(&[b"xyz", b"abc"]).0,
            vec![b"xyz".to_vec(), b"abc".to_vec()]
        );
        assert_eq!(
            dechunk_parts(&[b"2\r\nabX\r\n1"]).0,
            vec![b"abX\r\n1".to_vec()]
        );
        assert_eq!(
            dechunk_parts(&[b"2\r\nab\r\n", b"Z"]).0,
            vec![b"ab".to_vec(), b"Z".to_vec()]
        );
        assert_eq!(dechunk_parts(&[b"", b"1\n"]).0, vec![vec![], vec![]]);
    }

    #[test]
    fn dechunk_reference_cases() {
        for (n, (input, want)) in refdata::DECHUNK_CASES.iter().enumerate() {
            let (out, _) = dechunk_parts(input);
            let want: Vec<Vec<u8>> = want.iter().map(|b| b.to_vec()).collect();
            assert_eq!(out, want, "case {n}");
        }
    }

    mod refdata {
        use super::{P, RefCase};
        // Generated with php 8.2.29. Do not edit by hand.
        pub(super) static REF_CASES: &[RefCase] = &[
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"b=\x09a", b"=\x0a\x00\x0d~\x0d==\x00=x=\xff\x0a\x00b\xff =b\x00"],
        steps: &[&[b"b=3D\x09a"], &[b"=3D=0A=00=0D~=0D=3D=3D=00=3Dx=3D=FF=0A=00b=FF =3Db=00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"~ \x0a\xff\x0d\x0a~a\x0d\x00=x\x0a ab= \x00b=\x09\x0ax\x0a", b"\x0aa=z\x0a  aZ\x0a\xffa=x==xa \x0d", b"\x0da\x0dy "],
        steps: &[&[b"~ =0A=FF=0D=0A~a=0D=00=3Dx=0A ab=3D =00b=3D\x09=0Ax=0A"], &[b"=0Aa=3Dz=0A  aZ=0A=FFa=3Dx=3D=3Dxa =0D"], &[b"=0Da=0Dy "], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"\x0dzb b\x09\x00b =\xff~y yyb\x00\xff ="],
        steps: &[&[b"=0Dzb b\x09=00b =3D=FF=\x0d\x0a~y yyb=00=FF=20=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"Zy ", b"=z zxaa\x0dx~a\x0da\x0dx"],
        steps: &[&[b"Zy=20"], &[b"=\x0a=3Dz =\x0azxaa=\x0a=0Dx~=\x0aa=0Da=\x0a=0Dx"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"x"],
        steps: &[&[b"x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"b\xff \x0dZaa\x0az==Zya\x0da aa=z \x00x"],
        steps: &[&[b"=62=FF =\x0d\x0a=0DZaa=\x0d\x0a=0Az=3D=\x0d\x0a=3DZya=\x0d\x0a=0Da aa=\x0d\x0a=3Dz =\x0d\x0a=00x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x00=Z\x00\x00\x0d\x0d\xff\x00zbyayb ", b"x==\x0dybaxZb\xff\x0a\x0d", b"\x0a==\x09 b\x0a=a\x09y x\x00ayx~"],
        steps: &[&[b"=00=3DZ=00=00=0D=0D=FF=00zbyayb=20"], &[b"x=3D=3D=0DybaxZb=FF\x0a=0D"], &[b"\x0a=3D=3D\x09 b\x0a=3Da\x09y x=00ayx~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"b\x09=z=zZb\x00\x00\x0a\x0aa\x0aa\x0dZ"],
        steps: &[&[b"b\x09=\x0d\x0a=3D=\x0d\x0az=\x0d\x0a=3D=\x0d\x0azZb=\x0d\x0a=00=\x0d\x0a=00=\x0d\x0a=0A=\x0d\x0a=0A=\x0d\x0aa=\x0d\x0a=0A=\x0d\x0aa=\x0d\x0a=0D=\x0d\x0aZ"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"xZ\x0aa Z", b"\x0aa xaa\x0az~\x09~=\x0d\x0aab \x0d\x09"],
        steps: &[&[b"xZ=0Aa Z"], &[b"=0Aa xaa=0Az~\x09~=3D=0D=0Aab =0D\x09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"\x09\x0d\x00aa\x0a\xff\x0d\xff\x0d= \x0a\x0d ~", b"\x0dZ\xff", b"a=\x00~\x09", b"\x0dxZ=", b" ", b"\x0d"],
        steps: &[&[b"\x09=0D=00aa=\x0d\x0a=0A=FF=0D=\x0d\x0a=FF=0D=3D =\x0d\x0a=0A=0D=20~"], &[b"=\x0d\x0a=0DZ=FF"], &[b"a=3D=\x0d\x0a=00~=09"], &[b"=0Dx=\x0d\x0aZ=3D"], &[b"=20"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"ayx=Zx\x0d\x0a ~y=~\x0a", b"=\x0a  \x09=", b"y=z \x00\x0d\x0axax\x0dx\x0d\x09\x0a\x0ab"],
        steps: &[&[b"ayx=ab=3DZ=abx=0D=ab=0A =ab~y=ab=3D~=ab=0A"], &[b"=ab=3D=ab=0A=ab=20=ab=20=ab=09=ab=3D"], &[b"y=ab=3Dz=ab =00=ab=0D=ab=0Ax=abax=ab=0Dx=ab=0D\x09=ab=0A=ab=0Ab"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"x~\x0a\x09=z \x0db=Zy \x0aa~b\x0dbb=\x0az=b=\x0ay Z\xff\x00x\xffa\x0d\x0d ", b"~=\x0db\x0d\x00", b"b\x00=b="],
        steps: &[&[b"x~=0A=09=3Dz=20=0Db=3DZy=20=0Aa~b=0Dbb=3D=0Az=3Db=3D=0Ay=20Z=FF=00x=FFa=0D=0D=20"], &[b"~=3D=0Db=0D=00"], &[b"b=00=3Db=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"=  ", b"\x0dyab\x0a", b"b\x00", b"a\x09bb\x00\x09\x0a\x0d"],
        steps: &[&[b"=3D  "], &[b"=0Dyab=0A"], &[b"b=00"], &[b"a\x09bb=00\x09=0A=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"~  ", b"\x0a ", b"\x0a\x0d=", b"=y\x0a", b"Z="],
        steps: &[&[b"~  "], &[b"=0A "], &[b"=0A=0D=3D"], &[b"=3Dy=0A"], &[b"Z=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"\x0a\x09\x0a~\xff\x00Zz", b" \x09\x0a\x09\x0a", b"a"],
        steps: &[&[b"=0A\x09=0A~=FF=00Zz"], &[b" \x09=\x0d\x0a=0A=09=0A"], &[b"a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a~ bzZb=== \x09", b"= \x0a\x0d\x0aby=~= =yabb=\x0a\x0db\x0dxy=Z\x0aa==\x0a= \x0d \x0aZbb~"],
        steps: &[&[b"\x0a~ bzZ=\x0ab=3D=\x0a=3D=\x0a=3D=\x0a=20=\x0a=09"], &[b"=\x0a=3D=\x0a=20\x0a=0D\x0aby=3D=\x0a~=3D =\x0a=3Dya=\x0abb=3D\x0a=0Db=\x0a=0Dxy=\x0a=3DZ\x0aa=3D=\x0a=3D\x0a=3D =\x0a=0D=\x0a=20\x0aZbb~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"\xff= =", b"\x00\x0a"],
        steps: &[&[b"=FF=3D=20=\x0d\x0a=3D"], &[b"=00=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"\x0aZ\xffb~=y =b=\x09 ", b"a~\x0a~ x  =y\x0a\x00\x00=a=xZ\x0a~ a \x0a\x0d\x0d", b"\x0dbz\x0aa \x09", b"ba"],
        steps: &[&[b"=0AZ=FF=\x0d\x0ab~=3Dy =\x0d\x0a=3Db=3D=\x0d\x0a=09=20"], &[b"a=\x0d\x0a~=0A~ x=\x0d\x0a  =3Dy=\x0d\x0a=0A=00=\x0d\x0a=00=3Da=\x0d\x0a=3DxZ=\x0d\x0a=0A~ a =\x0d\x0a=0A=0D=\x0d\x0a=0D"], &[b"=0Db=\x0d\x0a=7A=0Aa=\x0d\x0a=20=09"], &[b"b=\x0d\x0a=61"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a\xff", b"\x0a", b"azb=", b"aa\x0d"],
        steps: &[&[b"\x0a=FF"], &[b"\x0a"], &[b"azb=3D"], &[b"aa=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"aa\x00\xffy", b"ba=\x09xaa\x0d\x0a=", b"=y\x0d\xff", b"\x00\x0ay", b"=\x0ab\x0dZ\x0a\x09\x0d\x09=="],
        steps: &[&[b"aa=\x0d\x0a=00=\x0d\x0a=FF=\x0d\x0ay"], &[b"ba=\x0d\x0a=3D=\x0d\x0a\x09xa=\x0d\x0aa\x0d\x0a=3D"], &[b"=\x0d\x0a=3D=\x0d\x0ay=\x0d\x0a=0D=\x0d\x0a=FF"], &[b"=\x0d\x0a=00=\x0d\x0a=0A=\x0d\x0ay"], &[b"=\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0ab=\x0d\x0a=0D=\x0d\x0aZ=\x0d\x0a=0A=\x0d\x0a\x09=\x0d\x0a=0D=\x0d\x0a\x09=\x0d\x0a=3D=\x0d\x0a=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a\x09\x0a\x0dZyz", b" b \x0aab\x09", b"abb"],
        steps: &[&[b"=0A\x09=0A=0DZyz"], &[b" b =0Aab\x09"], &[b"abb"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b" \x0ab\x09=\x0d\xff \x00", b"=\x0a\x0a\xff \x00\x0a\x09\x0da=~Zx", b"=\xff=byz", b"a \x00\x0ab \x0aaa\xff"],
        steps: &[&[b" =0Ab\x09=3D=\x0d\x0a=0D=FF=20=\x0d\x0a=00"], &[b"=3D=0A=\x0d\x0a=0A=FF =00=\x0d\x0a=0A\x09=0Da=3D=\x0d\x0a~Zx"], &[b"=3D=FF=\x0d\x0a=3Dbyz"], &[b"a =00=\x0d\x0a=0Ab =0Aaa=\x0d\x0a=FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"b= Z\x0a=\x0d~z\x09a="],
        steps: &[&[b"b=3D=ab Z=ab=0A=ab=3D=ab=0D~=abz=09=aba=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"Zb\x0d =xy= z~", b"b \x0a\x09\x0a ~\x09b\x00a~b\x00Z\x0d \x0a\x0abz\x00 ~ y"],
        steps: &[&[b"Zb=0D=20=3Dxy=3D=20z~"], &[b"b=20=0A=09=0A=20~=09b=00a~b=00Z=0D=20=0A=0Abz=00=20~=20y"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"\x0d", b" a z =x\x0d", b" \x0d\x0d\x09\x0db\x09\x0a~\x0d\x00=\x0aZ\xff\x0d"],
        steps: &[&[b"=0D"], &[b" a z =3Dx=0D"], &[b" =0D=0D\x09=0Db\x09=0A~=0D=00=3D=0AZ=FF=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b" \x0ay\x0a\x09b\x0a\x0a\x0d b ~y\x09~b\x0d\x09=\x00b~\x09=b"],
        steps: &[&[b" =0Ay=0A\x09b=0A=0A=0D b ~y\x09~b=0D\x09=3D=00b~\x09=3Db"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"x", b"\x0da\x0a", b"\x0a z", b"\x00 \x0a"],
        steps: &[&[b"x"], &[b"=0Da=0A"], &[b"=0A=20z"], &[b"=00=\x0d\x0a=20=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"~\x00zy~= \x0aa  "],
        steps: &[&[b"~=00z=\x0ay~=3D=\x0a=20\x0aa=20=\x0a=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"Z\xff=\x00\x09\x09y", b"  ", b"=\x0a", b"\x09Z\x0a\x0a", b"a=\xff\x0a=a\x0dx\x00b~ x z\x0a\x0ax\x0a\xff\x0a\xff\x0aa~a\x0az\x0d\x0day\x0d", b"b= yb\x0d\x0a\x0d"],
        steps: &[&[b"Z=FF=3D=\x0d\x0a=00=09=09=\x0d\x0ay"], &[b"=20=20"], &[b"=\x0d\x0a=3D=0A"], &[b"=09=\x0d\x0aZ=0A=0A"], &[b"a=\x0d\x0a=3D=FF=0A=\x0d\x0a=3Da=0Dx=\x0d\x0a=00b~=20x=\x0d\x0a=20z=0A=\x0d\x0a=0Ax=0A=\x0d\x0a=FF=0A=FF=\x0d\x0a=0Aa~a=0A=\x0d\x0az=0D=0Day=\x0d\x0a=0D"], &[b"b=3D=\x0d\x0a=20yb=0D=\x0d\x0a=0A=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"\xff=b=\x0a\x0d\x09b \x00y\x0d", b"\xff\x00=\x0ay\x0d\x0a", b"\x0a==a\x0a\x00y\x0a", b"y\x09y\x0a\x0d\x09\x0dy="],
        steps: &[&[b"=FF=3Db=\x0d\x0a=3D=0A=\x0d\x0a=0D\x09b =\x0d\x0a=00y=0D"], &[b"=\x0d\x0a=FF=00=\x0d\x0a=3D=0Ay\x0d\x0a"], &[b"=0A=3D=\x0d\x0a=3Da=0A=\x0d\x0a=00y=0A"], &[b"=\x0d\x0ay\x09y=0A=\x0d\x0a=0D\x09=0D=\x0d\x0ay=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0d", b"b\x0a\x0aZ\x0d\x0d", b"\x0aa"],
        steps: &[&[b"=0D"], &[b"b\x0a\x0aZ=0D=0D"], &[b"\x0aa"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b" \x0dZa\x0a", b"y\x0a \x0a=\x0a "],
        steps: &[&[b" =\x0d\x0a=0D=\x0d\x0aZa=\x0d\x0a=0A"], &[b"=\x0d\x0ay=\x0d\x0a=0A=\x0d\x0a =\x0d\x0a=0A=\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0a=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\xff\x0ay\x0db\x0dabxz\x0d\x00zZa\x0a ya\x00yx\xff\x0aa=a \x0a=~\x0dzbzbxx\xffyxy "],
        steps: &[&[b"=FF=0Ay=0Db=0Dabxz=0D=00zZa=0A ya=00yx=FF=0Aa=3Da =0A=3D~=0Dzbzbxx=FFyxy "], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"\xff"],
        steps: &[&[b"=FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"bx", b"=\x0d", b"a", b"\x0d\x0aZ\x0ab=\x0aaz", b"y \x0a=\x00b\x0d\x09ax", b"\x0az\x0db\x0a \x0aZ\x0d"],
        steps: &[&[b"bx"], &[b"=ab=3D=ab=0D"], &[b"a"], &[b"=ab=0D=ab=0AZ=ab=0Ab=ab=3D=ab=0Aa=abz"], &[b"y =ab=0A=ab=3D=ab=00b=ab=0D=ab=09a=abx"], &[b"=0A=abz=0D=abb=0A=ab =0A=abZ=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"a", b"="],
        steps: &[&[b"a"], &[b"=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"\x0a\x0a\x0a Zzb b=Z\x09yZ \x0d", b"a x\x0a\x00~", b"az\x09bb~\x0ax\x0a", b"=zy\x00b", b" b\x0d~\x0d\x0a\x0d\x0da\x0a=\x0a\x0d\x09z abbZ=Z", b"bx"],
        steps: &[&[b"=0A=0A=0A Zzb b=3DZ\x09yZ =0D"], &[b"a x=0A=00~"], &[b"az\x09bb~=0Ax=0A"], &[b"=3Dzy=00b"], &[b" b=0D~=0D=0A=0D=0Da=0A=3D=0A=0D\x09z abbZ=3DZ"], &[b"bx"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"=\xff\x0a\x0dx\x0a  z\x0d", b"x==\xffa\x00Zx\x00 by=b", b"b=", b"=~"],
        steps: &[&[b"=3D=FF=0A=0Dx=0A  z=0D"], &[b"x=3D=3D=FFa=00Zx=00 by=3Db"], &[b"b=3D"], &[b"=3D~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"xa\x0a ~b", b"Za\x0d\x09 \xff", b"\x0aZ\xffa\x0a=\x0d\x0d ", b"=b"],
        steps: &[&[b"xa=0A ~b"], &[b"Za=0D=09=20=\x0d\x0a=FF"], &[b"=0AZ=FFa=0A=3D=\x0d\x0a=0D=0D=20"], &[b"=3Db"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x09~=bz", b"a\xffZ", b" \x0db"],
        steps: &[&[b"\x09~=3D=\x0abz"], &[b"a=\x0a=FFZ"], &[b" =\x0a=0Db"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b" \x0ab\x0d ", b"yZ \x0dyx\x00=Z\x0a~a\x0a="],
        steps: &[&[b"=20=0Ab=\x0d\x0a=0D=20"], &[b"yZ=\x0d\x0a=20=0Dyx=\x0d\x0a=00=3DZ=\x0d\x0a=0A~a=0A=\x0d\x0a=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"a=", b"\xff=a\x0d\x0a\x00\x0a y", b"\x0d", b"=b\x0a\x09Z ", b"~a=\xff=\xffa \x0a\x0a= \x00\xffa\x0d", b"yax\x0a="],
        steps: &[&[b"=61=3D"], &[b"=\x0d\x0a=FF=3Da\x0d\x0a=00=0A=\x0d\x0a=20y"], &[], &[b"=0D=\x0d\x0a=3Db=0A=\x0d\x0a\x09Z=20"], &[b"~a=\x0d\x0a=3D=FF=\x0d\x0a=3D=FFa=\x0d\x0a =0A=0A=\x0d\x0a=3D =00=\x0d\x0a=FFa=0D"], &[b"=\x0d\x0ayax=0A=\x0d\x0a=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"bZabba\x0ay Z=\x0d\x0da~\x00", b" x==\x0d \x0aZ\x0axb Z\x00\x09=b\x0a=a  = =\x09=\x0a\x00"],
        steps: &[&[b"bZabba\x0ay Z=3D=0D=0Da~=00"], &[b" x=3D=3D=0D=20\x0aZ\x0axb=20Z=00\x09=3Db\x0a=3Da  =3D =3D\x09=3D\x0a=00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"Za\x0d\x0a\x0a\x0d\x0aZ \x0aZ\x0d=bya \xff  \x0az", b"==az\x00\x0a~xb\x0d\x0a=\x0a\x0d == Zaaz\x0d==b\x0a\x0a\x00b\x09=a ="],
        steps: &[&[b"Za\x0d\x0a=0A\x0d\x0aZ =\x0d\x0a=0A=\x0d\x0aZ=\x0d\x0a=0D=\x0d\x0a=3D=\x0d\x0abya=\x0d\x0a =\x0d\x0a=FF=\x0d\x0a  =\x0d\x0a=0A=\x0d\x0az"], &[b"=\x0d\x0a=3D=\x0d\x0a=3D=\x0d\x0aaz=\x0d\x0a=00=\x0d\x0a=0A=\x0d\x0a~xb\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0a=0D=\x0d\x0a =\x0d\x0a=3D=\x0d\x0a=3D=\x0d\x0a Za=\x0d\x0aaz=\x0d\x0a=0D=\x0d\x0a=3D=\x0d\x0a=3D=\x0d\x0ab=\x0d\x0a=0A=\x0d\x0a=0A=\x0d\x0a=00=\x0d\x0ab\x09=\x0d\x0a=3D=\x0d\x0aa=\x0d\x0a=20=\x0d\x0a=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\xffa~a\x0a=b \x00\xff", b"==\xffa= ", b"\x0a", b"zZ=", b"\x0a =\xff\x09=y\xff"],
        steps: &[&[b"=FFa~a=0A=3Db =00=FF"], &[b"=3D=3D=FFa=3D "], &[b"=0A"], &[b"zZ=3D"], &[b"=0A =3D=FF\x09=3Dy=FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"=\x0aa aZ=\x0d \x09y== =bZb\x09\x0aa\x0d=\x09", b"\x00\x0a"],
        steps: &[&[b"=3D=0Aa aZ=\x0d\x0a=3D=0D \x09y=\x0d\x0a=3D=3D =3Db=\x0d\x0aZb\x09=0Aa=0D=\x0d\x0a=3D=09"], &[b"=00=\x0d\x0a=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"\x0a \x0a= \x00", b"\x0dzb", b"\x00\x0a \x00", b"Za~\x0ax= \x0azZy~\x0a\x00=\x0a\x09=\x0a"],
        steps: &[&[b"=0A =ab=0A=ab=3D=ab=20=ab=00"], &[b"=ab=0Dz=abb"], &[b"=00=ab=0A=ab=20=ab=00"], &[b"Z=aba~=ab=0Ax=ab=3D =ab=0Az=abZy~=ab=0A=ab=00=ab=3D=ab=0A\x09=ab=3D=ab=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"a =\x09\x00\x0ab~a=", b"\x09\x0a\x09 axa\x0d z "],
        steps: &[&[b"a=20=3D=09=00=0Ab~a=3D"], &[b"=09=0A=09=20axa=0D=20z=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"b", b"\xff"],
        steps: &[&[b"b"], &[b"=FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"\x0a=\x0abzb", b"=", b"by", b"=\x0a\x09\x0d=\xffaZy\xffa\x00Zb\x09~\x0a", b"a"],
        steps: &[&[b"=0A=3D=0Abzb"], &[b"=3D"], &[b"by"], &[b"=3D=0A\x09=0D=3D=FFaZy=FFa=00Zb\x09~=0A"], &[b"a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"=\x0a", b"a\x00a~bx b~bb===\x0dz=\x0a ", b"b\x00\x0a\x00=Zb\x09\x0d", b"zx "],
        steps: &[&[b"=3D=0A"], &[b"a=00a~bx b~bb=\x0d\x0a=3D=3D=3D=0Dz=3D=0A=\x0d\x0a=20"], &[b"b=00=0A=00=3DZb=\x0d\x0a=09=0D"], &[b"zx=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0aa\x00 xZ\x0abZa\xff  \x09\x0aza\x09\x00y\x0aa\xffx\x09a\x0dy\x0aa=  zz\x0a", b"=b\x00"],
        steps: &[&[b"\x0aa=00 =\x0axZ\x0abZa=\x0a=FF=\x0a=20=\x0a=20=\x0a=09\x0aza\x09=\x0a=00y\x0aa=FFx=\x0a\x09a=0D=\x0ay\x0aa=3D =\x0a zz\x0a"], &[b"=3Db=\x0a=00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"\x0a=\x00\x00a\x0a\x0aa", b"\x0ab~\x09 \x0a b=aa=\x0dy \x0d\x0a b\xff\xffzZ\x09a\x09Z", b" \x0d    ", b" x", b" \x0a \x0d=a=", b"\x0a~  b\x00"],
        steps: &[&[b"=0A=3D=00=\x0d\x0a=00a=0A=\x0d\x0a=0Aa"], &[b"=0Ab~=\x0d\x0a=09=20=0A=\x0d\x0a=20b=3Daa=\x0d\x0a=3D=0Dy=\x0d\x0a=20=0D=0A=\x0d\x0a=20b=FF=\x0d\x0a=FFzZ=09a=\x0d\x0a=09Z"], &[b"=20=\x0d\x0a=0D=20=20=\x0d\x0a=20=20"], &[b"=20=\x0d\x0ax"], &[b"=20=0A=\x0d\x0a=20=0D=3D=\x0d\x0aa=3D"], &[b"=0A~=\x0d\x0a=20=20b=\x0d\x0a=00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"z~", b" bZ\xff\x0dx\x0da\x0a\x0aZa~\x00z=~=x=~\x00a\x0a\x0a "],
        steps: &[&[b"=7A~"], &[b" bZ=\x0d\x0a=FF=0Dx=\x0d\x0a=0Da=0A=\x0d\x0a=0AZa~=\x0d\x0a=00z=3D=\x0d\x0a~=3Dx=\x0d\x0a=3D~=00=\x0d\x0aa=0A=0A=\x0d\x0a=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\xff\x0d", b"z", b"yy", b"x"],
        steps: &[&[b"=FF=0D"], &[b"z"], &[b"yy"], &[b"x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"y a==b\x0ay", b"=", b"xax\x00 \x0ayy\x09y=x\x09zz\x0d~", b"\x09=~b\x0d ~"],
        steps: &[&[b"y a=\x0d\x0a=3D=\x0d\x0a=3D=\x0d\x0ab=\x0d\x0a=0A=\x0d\x0ay"], &[b"=\x0d\x0a=3D"], &[b"=\x0d\x0axax=\x0d\x0a=00=\x0d\x0a =\x0d\x0a=0A=\x0d\x0ayy\x09=\x0d\x0ay=\x0d\x0a=3D=\x0d\x0ax\x09z=\x0d\x0az=\x0d\x0a=0D=\x0d\x0a~"], &[b"\x09=\x0d\x0a=3D=\x0d\x0a~b=\x0d\x0a=0D=\x0d\x0a=20=\x0d\x0a~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0dZb", b"\x0aa \x0aZxb\xff==\x0a=b=\x0a\xff="],
        steps: &[&[b"=0DZb"], &[b"=0Aa =0AZxb=FF=3D=3D=0A=3Db=3D=0A=FF=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"\xffZ\x0abZ= ~\x0axab\xff Za=~xy=\x0d\xffb\x0d\x09=\xffab =\x0d\x0abya\x09 \x0aba\x0d", b"aZb\x0a \x0a\x0dx", b"\x0a a Z"],
        steps: &[&[b"=FFZ=0AbZ=\x0d\x0a=3D ~=0Axab=\x0d\x0a=FF Za=3D~x=\x0d\x0ay=3D=0D=FFb=\x0d\x0a=0D\x09=3D=FFa=\x0d\x0ab =3D\x0d\x0abya\x09 =0Aba"], &[b"=\x0d\x0a=0DaZb=0A =\x0d\x0a=0A=0Dx"], &[b"=0A =\x0d\x0aa=20Z"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b" Z\x0d=xZ z\x0a\x0a\x0a\x0a~ \x0a=z\x0a\x0a=z Z=a\xff\x0d"],
        steps: &[&[b" Z=ab=0D=ab=3Dx=abZ z=ab=0A=ab=0A=ab=0A=ab=0A~=ab =0A=ab=3Dz=ab=0A=ab=0A=ab=3Dz=ab Z=ab=3Da=ab=FF=ab=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"Z", b"\x0a  a =\x0a b=", b"zz=Z\xff~a\x0aa\xffz\x0a\x00b\xff\x09\x0az", b"\x09b\x0dzbbz~xZ \x09~= \x0a=\x0a"],
        steps: &[&[b"Z"], &[b"=0A=20=20a=20=3D=0A=20b=3D"], &[b"zz=3DZ=FF~a=0Aa=FFz=0A=00b=FF=09=0Az"], &[b"=09b=0Dzbbz~xZ=20=09~=3D=20=0A=3D=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"b", b"\x0a=~\x0a", b"z  \x0a\x0a", b"b ", b"ayy\x00a=\x0ab"],
        steps: &[&[b"b"], &[b"=0A=3D~=0A"], &[b"z  =0A=0A"], &[b"b "], &[b"ayy=00a=3D=0Ab"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"y", b"z", b"=", b"\x0d= \x0dy"],
        steps: &[&[b"y"], &[b"z"], &[b"=3D"], &[b"=0D=3D =0Dy"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"\x09  ", b"=xz", b"\x0a=\x0a~a\x09\x0d\x0a\x0d "],
        steps: &[&[b"=09=20=20"], &[b"=3Dxz"], &[b"=0A=\x0d\x0a=3D=0A~a=09\x0d\x0a=0D=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x09 \x0dz\xffZ\x0d\x0d=x", b"\x0d\x0a", b"\x0d=", b"a bZ\x00z yZ=b\x00 b= x\x0dy=yb ~~\x0d\xffbb\x0d \x0a=bZyx\x00\x0axZ~Z="],
        steps: &[&[b"\x09 =0D=\x0az=FFZ=\x0a=0D=\x0a=0D=\x0a=3Dx"], &[b"=\x0a=0D\x0a"], &[b"=0D=\x0a=3D"], &[b"a =\x0abZ=00=\x0az yZ=\x0a=3Db=\x0a=00 b=\x0a=3D x=\x0a=0Dy=\x0a=3Dyb=\x0a ~~=\x0a=0D=\x0a=FFbb=\x0a=0D=\x0a=20\x0a=3DbZ=\x0ayx=00\x0axZ~Z=\x0a=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"  xa~z\x0ax\x0daab~\xffb\xffa=\xffa\x0dbb\x0a~Z\x00ba=x b=y\x0d\x0a"],
        steps: &[&[b"=20=20xa~=\x0d\x0az=0Ax=0Da=\x0d\x0aab~=FFb=\x0d\x0a=FFa=3D=\x0d\x0a=FFa=0Dbb=\x0d\x0a=0A~Z=00b=\x0d\x0aa=3Dx=20b=\x0d\x0a=3Dy=0D=\x0d\x0a=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"a\x0abzy\x0a  x= ", b"\x0a\x0aaZ\x09ay=y==z= "],
        steps: &[&[b"=61=0Ab=\x0d\x0azy=0A  =\x0d\x0ax=3D=20"], &[b"=\x0d\x0a=0A=0Aa=\x0d\x0aZ\x09ay=3D=\x0d\x0ay=3D=3D=\x0d\x0az=3D=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b" a", b"=\xffa\x0d\x0daaaa\x0db=\x0a\x0d=ab= =  ", b"=~b~\x0db==bb azzzb~\x0d==x\xff"],
        steps: &[&[b"=20a"], &[b"=3D=FFa=0D=0Daaaa=0Db=3D\x0a=0D=3Dab=3D =3D=20=20"], &[b"=3D~b~=0Db=3D=3Dbb azzzb~=0D=3D=3Dx=FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b" =byb~bZ\xff\x0d= a ~xZ\x0a=z\x0da~~=b\x0a~\xffzaZ~=\x0a\x0d"],
        steps: &[&[b" =\x0d\x0a=3D=\x0d\x0abyb=\x0d\x0a~bZ=\x0d\x0a=FF=\x0d\x0a=0D=\x0d\x0a=3D=\x0d\x0a a =\x0d\x0a~xZ=\x0d\x0a=0A=\x0d\x0a=3D=\x0d\x0az=\x0d\x0a=0D=\x0d\x0aa~~=\x0d\x0a=3D=\x0d\x0ab=\x0d\x0a=0A=\x0d\x0a~=\x0d\x0a=FF=\x0d\x0azaZ=\x0d\x0a~=\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0a=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"yxb\x0a Z\x0aZ=", b"~\x0db\x0aaabz~ ", b"\xff~ Z\x0dax\x09~\x0d\x0d\x0a\x0a\x0a\x0a\x0a \x09\x0dZ\xff\x0d\x09\x0a\x0aa\x0db="],
        steps: &[&[b"yxb=0A Z=0AZ=3D"], &[b"~=0Db=0Aaabz~ "], &[b"=FF~ Z=0Dax\x09~=0D=0D=0A=0A=0A=0A=0A \x09=0DZ=FF=0D\x09=0A=0Aa=0Db=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"a\x0a b\x0ab\x0a =a\x0a\x0d\x0dz==\x00\x0d~\x0a\xffaZb=z\x0axZ\x0a \x0ab \x0ab\x0a ", b"\x00\x0d==b\x0db\x00", b" a=~x \x0a =\x0ay "],
        steps: &[&[b"a=0A b=0Ab=\x0d\x0a=0A =3Da=0A=\x0d\x0a=0D=0Dz=3D=\x0d\x0a=3D=00=0D~=\x0d\x0a=0A=FFaZb=\x0d\x0a=3Dz=0AxZ=\x0d\x0a=0A =0Ab =\x0d\x0a=0Ab=0A=20"], &[b"=\x0d\x0a=00=0D=3D=\x0d\x0a=3Db=0Db=00"], &[b"=\x0d\x0a a=3D~x =0A=\x0d\x0a =3D=0Ay=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b" ="],
        steps: &[&[b"=20=ab=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"\x09Z==\x0aZ", b"~\x09a\x09\x0aZ", b"=\x0a\x0d="],
        steps: &[&[b"=09Z=3D=3D=0AZ"], &[b"~=09a=09=0AZ"], &[b"=3D=0A=0D=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"  \x0d\xff=\x00\x0a===bZ", b" b\x0d= \x0aZ\x0a\x0a\x0a\x0a b\x0d\xff"],
        steps: &[&[b"  =0D=FF=3D=00=0A=3D=3D=3DbZ"], &[b" b=0D=3D =0AZ=0A=0A=0A=0A b=0D=FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"=a \x0a=\x0aa\x0a\x0ab a=\x0d\x00b\x0dz\x0d\x00\x00\xff \x09=\x00~~", b"z\x0da=\x09y~ =\x0a\x00z\xffb\x09 ~Z\x0a==~y\x0d\x09\x0d\x0a", b"a\x0a", b"\xff\x0a"],
        steps: &[&[b"=3Da =0A=3D=0Aa=0A=0Ab a=3D=0D=00b=0Dz=0D=00=00=FF \x09=3D=00~~"], &[b"z=0Da=3D\x09y~ =3D=0A=00z=FFb\x09 ~Z=0A=3D=3D~y=0D\x09=0D=0A"], &[b"a=0A"], &[b"=FF=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"\x0ax\x0a\x0d\xff\x0a~b y\x0a\x0a~~", b"a\x09\x09b\x0dZxz\xffa==bby a\x0d\x00\x0dx", b" =a\x00b\x09\x0a\x0a=baxa =\x00"],
        steps: &[&[b"=0Ax=0A=0D=FF=0A~b =\x0d\x0ay=0A=0A~~"], &[b"a\x09\x09b=0DZxz=\x0d\x0a=FFa=3D=3Dbby a=0D=\x0d\x0a=00=0Dx"], &[b" =3Da=00b\x09=\x0d\x0a=0A=0A=3Dbaxa =3D=\x0d\x0a=00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b" "],
        steps: &[&[b"=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b" \x0ayy\x0a\x0ax ya\x0a\x0a\x09y\xff=xa=\x0a", b"y\x0a", b"\x09\x0ab a\x09a ", b"bx"],
        steps: &[&[b"=20=0Ayy=\x0d\x0a=0A=0Ax=\x0d\x0a=20ya=0A=\x0d\x0a=0A=09y=\x0d\x0a=FF=3Dxa=\x0d\x0a=3D=0A"], &[b"y=\x0d\x0a=0A"], &[b"=09=0A=\x0d\x0ab=20a=09a=\x0d\x0a=20"], &[b"bx"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"  \x0az ba", b"=\xffaz\x0d\x09 ", b" \x09\x09"],
        steps: &[&[b"  =0Az =\x0d\x0aba"], &[b"=3D=\x0d\x0a=FFaz=\x0d\x0a=0D=09=\x0d\x0a=20"], &[b"=20=\x0d\x0a=09=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"="],
        steps: &[&[b"=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"a", b"\x0aa=\x0az==a\xff\x0ab\xffab", b"\x09\x0a\x0dxa", b"z ", b" "],
        steps: &[&[b"a"], &[b"=\x0d\x0a=0A=\x0d\x0aa=\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0az=\x0d\x0a=3D=\x0d\x0a=3D=\x0d\x0aa=\x0d\x0a=FF=\x0d\x0a=0A=\x0d\x0ab=\x0d\x0a=FF=\x0d\x0aab"], &[b"\x09=\x0d\x0a=0A=\x0d\x0a=0D=\x0d\x0axa"], &[b"z=\x0d\x0a=20"], &[b"=\x0d\x0a=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0d=aba= \x0a", b"~\x0a \xff~ =", b"a~\x00", b"\x09=\x0d\x0a\x09aZ==  a\x0a z~"],
        steps: &[&[b"=0D=3Daba=3D =0A"], &[b"~=0A =FF~ =3D"], &[b"a~=00"], &[b"\x09=3D=0D=0A\x09aZ=3D=3D  a=0A z~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"aZZzb=\x0a\x00zyy\x0a\x00\x0ab\xff\x0d==x\x0ay=y \x0a\x0a\x0aby \x00\x09z="],
        steps: &[&[b"aZZzb=3D=0A=\x0d\x0a=00zyy=0A=\x0d\x0a=00=0Ab=FF=\x0d\x0a=0D=3D=3Dx=\x0d\x0a=0Ay=3Dy =\x0d\x0a=0A=0A=0Aby=\x0d\x0a =00\x09z=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"x=\x00=b "],
        steps: &[&[b"x=3D=ab=00=ab=3Db=ab=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b" =\x0ayb\x0aaZy\x0d\x09\x0a\x00=b=", b"=~== ==\x0db b", b"axz \x00   \x0a", b"a", b"=x~ ="],
        steps: &[&[b"=20=3D=0Ayb=0AaZy=0D=09=0A=00=3Db=3D"], &[b"=3D~=3D=3D=20=3D=3D=0Db=20b"], &[b"axz=20=00=20=20=20=0A"], &[b"a"], &[b"=3Dx~=20=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"\x0d x\x0az =\x00 b b", b"\x0d~=", b"x\x0a\xff\x0a\x0ab \x0dy Za\x0ay b\xff\xffya\x09\x00", b"a\x0dy\x00\x0a= \x0d= =ba", b"\x00ab", b"=\x0a"],
        steps: &[&[b"=0D x=0Az =3D=00 b b"], &[b"=0D~=3D"], &[b"x=0A=FF=0A=0Ab =0Dy Za=0Ay b=FF=FFya\x09=00"], &[b"a=0Dy=00=0A=3D =0D=3D =3Dba"], &[b"=00ab"], &[b"=3D=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"~\x09 \x0ax\x0aa\x0aZ\x09=aZba=a \xffaabb\xff\x00z b\x0a~x\x0d     ~\x0a\x0az \x0d=b\x00\x0d=yb aaba=b~"],
        steps: &[&[b"~\x09 =0Ax=0Aa=0AZ\x09=3DaZba=3Da =FFaabb=FF=00z b=0A~x=0D     ~=0A=0Az =0D=3Db=00=0D=3Dyb aaba=3Db~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"b ", b"a\xffab", b"\x00\x0a\x0d\x0dza", b"\x09zbb  by=y\xff\x00~ \x0ay= ", b"\x0d"],
        steps: &[&[b"b=20"], &[b"a=FFab"], &[b"=00=0A=0D=\x0d\x0a=0Dza"], &[b"\x09zbb  by=3Dy=\x0d\x0a=FF=00~ =0Ay=3D=20"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"  =\x09\x09 \x0dyya \x09= ~\x0d \x0d\xff=~a"],
        steps: &[&[b"  =3D=\x0a\x09\x09 =\x0a=0Dyy=\x0aa \x09=\x0a=3D ~=\x0a=0D =\x0a=0D=\x0a=FF=\x0a=3D~a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"\x00aaz=\xffz\x0a\x0a~\x0a\x0a", b"a\x0d", b"\x0a=x=\x00==\x0a\x0d", b"a\x0a", b"\x0d", b"~\xff~z"],
        steps: &[&[b"=00aaz=3D=\x0d\x0a=FFz=0A=\x0d\x0a=0A~=0A=\x0d\x0a=0A"], &[b"a=0D"], &[b"=\x0d\x0a=0A=3Dx=\x0d\x0a=3D=00=3D=\x0d\x0a=3D=0A=0D"], &[b"=\x0d\x0aa=0A"], &[b"=0D"], &[b"~=\x0d\x0a=FF~z"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"~=\xffz\x0d\x0ax= \x0a y\xff\x0a", b"aZbx=yx=\x0a ", b"x  a\x00b"],
        steps: &[&[b"=7E=3D=\x0d\x0a=FFz\x0d\x0a=78=3D =\x0d\x0a=0A y=\x0d\x0a=FF=0A"], &[b"a=\x0d\x0aZbx=3Dy=\x0d\x0ax=3D=0A=\x0d\x0a=20"], &[b"x  a=\x0d\x0a=00b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"xb\x0a x~ =z\x0aa \x0d\x0a\x0a", b" bb\x0ab", b"\x00\xff=\x0d", b"=zz\x0a\x00\x0a", b"b"],
        steps: &[&[b"xb\x0a x~ =3Dz\x0aa =0D\x0a\x0a"], &[b" bb\x0ab"], &[b"=00=FF=3D=0D"], &[b"=3Dzz\x0a=00\x0a"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"z", b"=a", b"b", b"\x09y"],
        steps: &[&[b"z"], &[b"=\x0d\x0a=3D=\x0d\x0aa"], &[b"b"], &[b"=\x0d\x0a=09=\x0d\x0ay"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x09=", b"bb\x0a \x0dxx~\x09Z", b"\x00\x0d=\x0a=", b" ", b"\x0d\xff \x0d", b"~"],
        steps: &[&[b"\x09=3D"], &[b"bb=0A =0Dxx~\x09Z"], &[b"=00=0D=3D=0A=3D"], &[b" "], &[b"=0D=FF =0D"], &[b"~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"x\x0aa~=~\x0ab~\x0ab\x0dzz\x0a \x0a x\x0aa\x0a\x0d\x0a\x0dz"],
        steps: &[&[b"x=0Aa~=3D~=\x0d\x0a=0Ab~=0Ab=\x0d\x0a=0Dzz=0A =\x0d\x0a=0A x=0Aa=\x0d\x0a=0A\x0d\x0a=0Dz"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b" \x0d=b\x00 zx==  \x00x\xffa\x0a\xffx\x0aa =\x0a"],
        steps: &[&[b" =0D=ab=3Db=ab=00 =abzx=ab=3D=ab=3D =ab =00=abx=FF=aba=0A=ab=FFx=ab=0Aa=ab =3D=ab=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"a\x00\x09 ZZ a\x0d \x0d~yxb\x0a  "],
        steps: &[&[b"a=00=09=20ZZ=20a=0D=20=0D~yxb=0A=20=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b" axya\x0aax x\xff=b", b"\x0aa=bya\x0ay"],
        steps: &[&[b" axya=0Aax x=FF=3Db"], &[b"=0Aa=3Dbya=0Ay"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b" aZxb", b"=\x0a ", b" Z\x0a~", b" \xff \x00bzx~ y\x0a\x0a ", b"\x0a~\x09a=\x00", b"y\x09"],
        steps: &[&[b" aZxb"], &[b"=3D=0A "], &[b" Z=0A~"], &[b" =FF =00bzx~ y=0A=0A "], &[b"=0A~\x09a=3D=00"], &[b"y\x09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"b b za~~y\x0az~b\x0a ~=a\xff", b"=bb\x0a \x00~=~="],
        steps: &[&[b"b b za~~y=0Az~b=0A =\x0d\x0a~=3Da=FF"], &[b"=3Dbb=0A =\x0d\x0a=00~=3D~=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"a\x0a=\x0d", b"yaa \x0d", b"=~", b"b"],
        steps: &[&[b"a\x0a=3D=\x0a=0D"], &[b"ya=\x0aa=20=\x0a=0D"], &[b"=\x0a=3D~"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"\x00\x0a=\x00\x09  \x00\x0ay\x09\xff", b" \x0d\x0d yz ", b"b\x0d~==by\x0a\x00a =yy=\x0aa", b"~\x00\x09\xff", b"z\x00\x0d"],
        steps: &[&[b"=00=0A=3D=\x0d\x0a=00=09=20=\x0d\x0a=20=00=0A=\x0d\x0ay=09=FF"], &[b"=\x0d\x0a=20=0D=0D=\x0d\x0a=20yz=20"], &[b"b=\x0d\x0a=0D~=3D=\x0d\x0a=3Dby=0A=\x0d\x0a=00a=20=\x0d\x0a=3Dyy=3D=\x0d\x0a=0Aa"], &[b"~=00=\x0d\x0a=09=FF"], &[b"z=\x0d\x0a=00=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"a", b"b"],
        steps: &[&[b"=61"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a\x0da\x00 Z  z\x0aa\x00ba=", b" Z\x0abx~ Zaxaa\xff="],
        steps: &[&[b"\x0a=0Da=00 Z  z\x0aa=00ba=3D"], &[b" Z\x0abx~ Zaxaa=FF=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"a\x0a b=", b"a\x00\x0a=", b"y\x0dy  ", b"\x00=\x0a \x0dy\x0a~\x0d\x0abb\xff\x0a\x0daa a", b"\x0d"],
        steps: &[&[b"a=\x0d\x0a=0A=\x0d\x0a b=\x0d\x0a=3D"], &[b"=\x0d\x0aa=\x0d\x0a=00=\x0d\x0a=0A=\x0d\x0a=3D"], &[b"=\x0d\x0ay=\x0d\x0a=0D=\x0d\x0ay=\x0d\x0a=20=\x0d\x0a=20"], &[b"=\x0d\x0a=00=\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0a =\x0d\x0a=0D=\x0d\x0ay=\x0d\x0a=0A=\x0d\x0a~\x0d\x0abb=\x0d\x0a=FF=\x0d\x0a=0A=\x0d\x0a=0D=\x0d\x0aaa=\x0d\x0a=20=\x0d\x0aa"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b" \x0dZ=ybzaa"],
        steps: &[&[b" =0DZ=3Dybzaa"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b" =x=y=\x00 yzZb bZ \xffa=\x0aaya= yyZ\x0d  ~\x09Zbb", b"ayZ~\x00 z", b"\xffb\x0dZ=x\x09\x0d=Z~Z\x09"],
        steps: &[&[b" =3Dx=3Dy=\x0d\x0a=3D=00 yzZb=\x0d\x0a bZ =FFa=3D=\x0d\x0a=0Aaya=3D y=\x0d\x0ayZ=0D  ~\x09Zb=\x0d\x0ab"], &[b"ayZ~=00=20=\x0d\x0az"], &[b"=FFb=0DZ=\x0d\x0a=3Dx\x09=0D=3D=\x0d\x0aZ~Z=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"\xff\x0aZ \x00\xffZ =", b"x~", b"=\x09", b" \x00"],
        steps: &[&[b"=FF=ab=0AZ=ab =00=ab=FFZ=ab=20=ab=3D"], &[b"x=ab~"], &[b"=3D=ab=09"], &[b"=ab=20=ab=00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"b y\x09za\x0d", b"\x0a a\x09\x0aZz"],
        steps: &[&[b"b=20y=09za=0D"], &[b"=0A=20a=09=0AZz"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: None,
        chunks: &[b"\x0a\xff", b"\x0a", b"x\x0a\x0da== xb\x0aZb\x00=", b"==~\xff", b"\x0d\xff", b"\x0aa\x0a~=x"],
        steps: &[&[b"=0A=FF"], &[b"=0A"], &[b"x=0A=0Da=3D=3D xb=0AZb=00=3D"], &[b"=3D=3D~=FF"], &[b"=0D=FF"], &[b"=0Aa=0A~=3Dx"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[]),
        chunks: &[b"ax=\x00\x0da=b\xff\xffz \x00\x0d b=zx\x00=\x0a y\x0dx\x0abzxx  =zb\x0dz\x00Zz\xff\x0aa\x00\x09\x0a\x0a\x0aa\xffx \xff"],
        steps: &[&[b"ax=3D=00=0Da=3Db=FF=FFz =00=0D b=3Dzx=00=3D=0A y=0Dx=0Abzxx  =3Dzb=0Dz=00Zz=FF=0Aa=00\x09=0A=0A=0Aa=FFx =FF"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(20)), ]),
        chunks: &[b"\x09Z=~a  ", b"= ~ \x0d\x0d\x0a", b"y zbz=y\x0a~= ", b"y\x00x"],
        steps: &[&[b"\x09Z=3D~a=20=20"], &[b"=3D ~ =\x0d\x0a=0D=0D=0A"], &[b"y zbz=3Dy=\x0d\x0a=0A~=3D=20"], &[b"y=00x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(6)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"a\xff=z\x0a=~", b"\x0azz\x0d=b Z\x0d ", b"bby\xff\x0ay\x0d", b"Z\x0aby\x09z\x0a= ba=~aabay=", b"x\x00\x09~\x09bb "],
        steps: &[&[b"a=FF=\x0a=3Dz\x0a=3D~"], &[b"\x0azz=0D=\x0a=3Db =\x0aZ=0D=\x0a=20"], &[b"bb=\x0ay=FF\x0ay=0D"], &[b"Z\x0aby\x09z\x0a=3D b=\x0aa=3D~=\x0aaabay=\x0a=3D"], &[b"x=\x0a=00\x09~=\x0a\x09bb=\x0a=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"binary", P::B(true)), ]),
        chunks: &[b"\x0a=aab === a\x0dbx==\x0dyba\x09z xb  ", b"\x0a", b" ", b"\x0d=\x00\x0a "],
        steps: &[&[b"=0A=3Daab=\x0d\x0a=20=3D=3D=\x0d\x0a=3D=20a=\x0d\x0a=0Dbx=3D=\x0d\x0a=3D=0Dyba=\x0d\x0a=09z=20xb=\x0d\x0a=20=20"], &[b"=0A"], &[b"=\x0d\x0a=20"], &[b"=0D=3D=\x0d\x0a=00=0A=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(8)), (b"force-encode-first", P::L(1)), ]),
        chunks: &[b"~a", b"x=", b"\x09x\x0a\x0a\x0d == ", b"~x"],
        steps: &[&[b"=7Ea"], &[b"x=\x0d\x0a=3D"], &[b"\x09x=\x0d\x0a=0A=0A=\x0d\x0a=0D =3D=\x0d\x0a=3D=20"], &[b"~=\x0d\x0a=78"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(74)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"b", b"a\xffa", b" =\x00\xff", b"~a=", b"b", b"ba \x0a=\x0a"],
        steps: &[&[b"b"], &[b"a=FFa"], &[b" =3D=00=FF"], &[b"~a=3D"], &[b"b"], &[b"ba=20\x0a=3D\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(4)), ]),
        chunks: &[b"=\x0aba~bZ b", b"\x0d \x0db b=\x0a==x \x0d \x0d"],
        steps: &[&[b"=3D=\x0d\x0a=0A=\x0d\x0aba~=\x0d\x0abZ=\x0d\x0a=20=\x0d\x0ab"], &[b"=\x0d\x0a=0D=\x0d\x0a =\x0d\x0a=0D=\x0d\x0ab b=\x0d\x0a=3D=\x0d\x0a=0A=\x0d\x0a=3D=\x0d\x0a=3D=\x0d\x0ax =\x0d\x0a=0D=\x0d\x0a=20=\x0d\x0a=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(3)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"b", b"a"],
        steps: &[&[b"b"], &[b"a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::S(b"12")), (b"line-break-chars", P::S(b"\x0d\x0a")), (b"binary", P::L(0)), ]),
        chunks: &[b"\x0a\xffa\x00Z \x0d\x0a", b"\x0dz\x0dz == \x0a\x00\x0dbb=b~\x09\x0a\x0a==\x0d\x0d", b"\x0a\x09\x00\x00\x0da~ bb\x0ax  yxxb \x09", b"\x0d \x09b\x00y=="],
        steps: &[&[b"=0A=FFa=00Z=\x0d\x0a=20\x0d\x0a"], &[b"=0Dz=0Dz =\x0d\x0a=3D=3D =0A=\x0d\x0a=00=0Dbb=3D=\x0d\x0ab~\x09=0A=0A=\x0d\x0a=3D=3D=0D=\x0d\x0a=0D"], &[b"=0A\x09=00=\x0d\x0a=00=0Da~ bb=\x0d\x0a=0Ax  yxxb=\x0d\x0a=20=09"], &[b"=0D \x09=\x0d\x0ab=00y=3D=3D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(5)), (b"line-break-chars", P::S(b"ab")), ]),
        chunks: &[b"\x0a\x09\x0d=   \xff=a a~~\x0dy==b b \x09\x09a~\x0az\x0a b~\xff\x00yb\x0a ==\x0a"],
        steps: &[&[b"=0A\x09=ab=0D=ab=3D =ab  =ab=FF=ab=3Da=ab a~~=ab=0Dy=ab=3D=ab=3Db=ab b \x09=ab\x09a~=ab=0Az=ab=0A =abb~=ab=FF=ab=00y=abb=0A=ab =3D=ab=3D=ab=0A"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"binary", P::B(true)), ]),
        chunks: &[b"  ", b" \x0dz\xff\x0ayy", b"Z\x0da"],
        steps: &[&[b"=20=20"], &[b"=20=0Dz=FF=0Ayy"], &[b"Z=0Da"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"FIRST \x0aSECOND"],
        steps: &[&[b"FIRST=20\x0aSECOND"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"F", b"I", b"R", b"S", b"T", b" ", b"\x0a", b"S", b"E", b"C", b"O", b"N", b"D"],
        steps: &[&[b"F"], &[b"I"], &[b"R"], &[b"S"], &[b"T"], &[b"=20"], &[b"\x0a"], &[b"S"], &[b"E"], &[b"C"], &[b"O"], &[b"N"], &[b"D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"FIRST \x0aSECOND"],
        steps: &[&[b"FIRST=\x0a=20\x0aSECON=\x0aD"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"F", b"I", b"R", b"S", b"T", b" ", b"\x0a", b"S", b"E", b"C", b"O", b"N", b"D"],
        steps: &[&[b"F"], &[b"I"], &[b"R"], &[b"S"], &[b"T"], &[b"=\x0a=20"], &[b"\x0a"], &[b"S"], &[b"E"], &[b"C"], &[b"O"], &[b"N"], &[b"=\x0aD"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"FIRST \x0aSECOND"],
        steps: &[&[b"FIRST =0A=\x0d\x0aSECOND"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"F", b"I", b"R", b"S", b"T", b" ", b"\x0a", b"S", b"E", b"C", b"O", b"N", b"D"],
        steps: &[&[b"F"], &[b"I"], &[b"R"], &[b"S"], &[b"T"], &[b"=20"], &[b"=\x0d\x0a=0A"], &[b"S"], &[b"E"], &[b"C"], &[b"O"], &[b"N"], &[b"D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"FIRST  \x0aSECOND"],
        steps: &[&[b"FIRST=20=20\x0aSECOND"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"F", b"I", b"R", b"S", b"T", b" ", b" ", b"\x0a", b"S", b"E", b"C", b"O", b"N", b"D"],
        steps: &[&[b"F"], &[b"I"], &[b"R"], &[b"S"], &[b"T"], &[b"=20"], &[b"=20"], &[b"\x0a"], &[b"S"], &[b"E"], &[b"C"], &[b"O"], &[b"N"], &[b"D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"FIRST  \x0aSECOND"],
        steps: &[&[b"FIRST=\x0a=20=\x0a=20\x0aSECON=\x0aD"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"F", b"I", b"R", b"S", b"T", b" ", b" ", b"\x0a", b"S", b"E", b"C", b"O", b"N", b"D"],
        steps: &[&[b"F"], &[b"I"], &[b"R"], &[b"S"], &[b"T"], &[b"=\x0a=20"], &[b"=\x0a=20"], &[b"\x0a"], &[b"S"], &[b"E"], &[b"C"], &[b"O"], &[b"N"], &[b"=\x0aD"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"FIRST  \x0aSECOND"],
        steps: &[&[b"FIRST  =\x0d\x0a=0ASECOND"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"F", b"I", b"R", b"S", b"T", b" ", b" ", b"\x0a", b"S", b"E", b"C", b"O", b"N", b"D"],
        steps: &[&[b"F"], &[b"I"], &[b"R"], &[b"S"], &[b"T"], &[b"=20"], &[b"=\x0d\x0a=20"], &[b"=0A"], &[b"S"], &[b"E"], &[b"C"], &[b"=\x0d\x0aO"], &[b"N"], &[b"D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"a \x0d\x0ab"],
        steps: &[&[b"a =0D\x0ab"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"a", b" ", b"\x0d", b"\x0a", b"b"],
        steps: &[&[b"a"], &[b"=20"], &[b"=0D"], &[b"\x0a"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"a \x0d\x0ab"],
        steps: &[&[b"a =0D\x0ab"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"a", b" ", b"\x0d", b"\x0a", b"b"],
        steps: &[&[b"a"], &[b"=20"], &[b"=\x0a=0D"], &[b"\x0a"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"a \x0d\x0ab"],
        steps: &[&[b"a=20\x0d\x0ab"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"a", b" ", b"\x0d", b"\x0a", b"b"],
        steps: &[&[b"a"], &[b"=20"], &[], &[b"\x0d\x0a"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"a \x0d"],
        steps: &[&[b"a=20=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"a", b" ", b"\x0d"],
        steps: &[&[b"a"], &[b"=20"], &[b"=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"a \x0d"],
        steps: &[&[b"a=20=\x0a=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"a", b" ", b"\x0d"],
        steps: &[&[b"a"], &[b"=20"], &[b"=\x0a=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"a \x0d"],
        steps: &[&[b"a=20"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"a", b" ", b"\x0d"],
        steps: &[&[b"a"], &[b"=20"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"\x0d\x0a\x0d\x0a"],
        steps: &[&[b"=0D\x0a=0D\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"\x0d", b"\x0a", b"\x0d", b"\x0a"],
        steps: &[&[b"=0D"], &[b"\x0a"], &[b"=0D"], &[b"\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"\x0d\x0a\x0d\x0a"],
        steps: &[&[b"=0D\x0a=0D\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"\x0d", b"\x0a", b"\x0d", b"\x0a"],
        steps: &[&[b"=0D"], &[b"\x0a"], &[b"=0D"], &[b"\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"\x0d\x0a\x0d\x0a"],
        steps: &[&[b"\x0d\x0a\x0d\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"\x0d", b"\x0a", b"\x0d", b"\x0a"],
        steps: &[&[], &[b"\x0d\x0a"], &[], &[b"\x0d\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"x\x09\x09\x0d\x0ay"],
        steps: &[&[b"x\x09\x09=0D\x0ay"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"x", b"\x09", b"\x09", b"\x0d", b"\x0a", b"y"],
        steps: &[&[b"x"], &[b"=09"], &[b"=09"], &[b"=0D"], &[b"\x0a"], &[b"y"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"x\x09\x09\x0d\x0ay"],
        steps: &[&[b"x\x09\x09=\x0a=0D\x0ay"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"x", b"\x09", b"\x09", b"\x0d", b"\x0a", b"y"],
        steps: &[&[b"x"], &[b"=09"], &[b"=\x0a=09"], &[b"=\x0a=0D"], &[b"\x0a"], &[b"y"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"x\x09\x09\x0d\x0ay"],
        steps: &[&[b"x=09=09\x0d\x0ay"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"x", b"\x09", b"\x09", b"\x0d", b"\x0a", b"y"],
        steps: &[&[b"x"], &[b"=09"], &[b"=09"], &[], &[b"\x0d\x0a"], &[b"y"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"ab\x0d"],
        steps: &[&[b"ab=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"a", b"b", b"\x0d"],
        steps: &[&[b"a"], &[b"b"], &[b"=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"ab\x0d"],
        steps: &[&[b"ab=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"a", b"b", b"\x0d"],
        steps: &[&[b"a"], &[b"b"], &[b"=0D"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"ab\x0d"],
        steps: &[&[b"ab"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"a", b"b", b"\x0d"],
        steps: &[&[b"a"], &[b"b"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"ab \x09"],
        steps: &[&[b"ab=20=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(74)), ]),
        chunks: &[b"a", b"b", b" ", b"\x09"],
        steps: &[&[b"a"], &[b"b"], &[b"=20"], &[b"=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"ab \x09"],
        steps: &[&[b"ab=20=\x0a=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), (b"line-length", P::L(6)), ]),
        chunks: &[b"a", b"b", b" ", b"\x09"],
        steps: &[&[b"a"], &[b"b"], &[b"=20"], &[b"=\x0a=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"ab \x09"],
        steps: &[&[b"ab=20=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-encode",
        params: Some(&[(b"line-length", P::L(10)), ]),
        chunks: &[b"a", b"b", b" ", b"\x09"],
        steps: &[&[b"a"], &[b"b"], &[b"=20"], &[b"=09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"\x0a=\x0az \xff \x0d", b"\xff", b" ", b"\x0d\x0a\x0a\x0d\xff\x0da\x0aZ=", b"a", b"a\x0da\x0a=x"],
        steps: &[&[b"Cj0KeiD/"], &[b"IA3/"], &[], &[b"IA0KCg3/DWEK"], &[b"Wj1h"], &[b"YQ1hCj14"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"\x09b\x09=ax\x09 a=a\x0d\x0d\x0d =\x0a\x0a\x09\x0dy\x0a=", b"\x0ababb\x0d =b=a\x0aabx=\x0a xay\x09  zy\x00"],
        steps: &[&[b"CWIJPWF4CSBhPWENDQ0gPQoKCQ15"], &[b"Cj0KYmFiYg0gPWI9YQphYng9CiB4YXkJICB6"], &[b"eQA="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"\x0a=\x0a\x0ay ", b"x\x00~yyaZ=\x0a\x09\x00\x0a\x0abx\x0aba\x0da"],
        steps: &[&[b"Cj0KCnkg"], &[b"\x0d\x0aeAB+eXlh\x0d\x0aWj0KCQAK\x0d\x0aCmJ4CmJh"], &[b"\x0d\x0aDWE="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"z=  ", b"\x0a=", b"a"],
        steps: &[&[b"ej0g"], &[b"\x0aIAo9"], &[], &[b"\x0aYQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"\x00a \x0a \x0ay", b"x\x0a~aa~b", b"  b=z\x0az ay\x0a", b" \x0d\xff=b\x00 ", b"=b=~ax\x0abz=~\x0a b"],
        steps: &[&[b"AGEgCiAK"], &[b"eXgKfmFh"], &[b"fmIgIGI9egp6IGF5"], &[b"CiAN/z1i"], &[b"ACA9Yj1+YXgKYno9fgog"], &[b"Yg=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"Z\x00 \x0d", b"\x0a  b bb\xff\x0a=", b"a\xffba\x0a", b"\x0a"],
        steps: &[&[b"WgAg"], &[b"DQog--IGIgYmL/"], &[b"--Cj1h/2Jh"], &[], &[b"--Cgo="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"\x0a", b"Z aa", b"ab\xffZyz", b" \x0d ", b" aa", b"zZa \x09\x0d\x00a"],
        steps: &[&[], &[b"Clog"], &[b"YWFhYv9a"], &[b"eXog"], &[b"DSAg"], &[b"YWF6WmEgCQ0A"], &[b"YQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"\xffx zba\x0ay~", b"\x09Z\x0a\x09\x0a=", b"x\x0d\x09Za=z", b"\x00\x00\x00=\x09", b"=b"],
        steps: &[&[b"/3ggemJhCnl+"], &[b"CVoKCQo9"], &[b"eA0JWmE9"], &[b"egAAAD0J"], &[], &[b"PWI="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"~\x09y\x0a\x0d\x0a\x0ab \x0aa", b"Z \x0a", b"\x0d =y", b"\x0a", b"z", b"a"],
        steps: &[&[b"fgl5Cg0KCmIg"], &[b"CmFa"], &[b"IAoNID15"], &[], &[], &[b"Cnph"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"a\x0a\x0a  ", b"Z\x00\x0a\xff\xff=", b"x\x0a", b"=\x09", b"="],
        steps: &[&[b"YQoK"], &[b"ICBa\x0d\x0aAAr/"], &[b"/z14"], &[b"\x0d\x0aCj0J"], &[], &[b"PQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"a\x09\xff\x0db\x00=Zx==x\x00  \x0a  a \x00y\x09", b"z\x0a\x00\x00\x0a y  ab\x0a \x0a"],
        steps: &[&[b"YQn/\x0aDWIA\x0aPVp4\x0aPT14\x0aACAg\x0aCiAg\x0aYSAA"], &[b"\x0aeQl6\x0aCgAA\x0aCiB5\x0aICBh\x0aYgog"], &[b"\x0aCg=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b" ", b"xz\x0d"],
        steps: &[&[], &[b"IHh6"], &[b"DQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"\x0d\x00\x0a\x0a\x0d", b" \x0axy\x09 aa=\xffy b\x00 \x09\x0dZ=x\x0daa=b by= \x0d  b\x0dy=Z\x09~ "],
        steps: &[&[b"DQAK"], &[b"Cg0g--Cnh5CSBh--YT3/eSBi--ACAJDVo9--eA1hYT1i--IGJ5PSAN--ICBiDXk9--Wgl+"], &[b"IA=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"\xff", b"Z", b"=\x0a", b" ", b" "],
        steps: &[&[], &[], &[b"/1o9"], &[], &[b"CiAg"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"a\x0ab\x0d=\x0a\x0a\x00zb \x0daba", b" =\x0a\x0az", b"bz\x0axa=", b"\x0daz"],
        steps: &[&[b"YQpiDT0KCgB6YiANYWJh"], &[b"ID0K"], &[b"Cnpiegp4"], &[b"YT0N"], &[b"YXo="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b" a", b"   =a \x0d~=~a=aa  b=\xff==zax~\x00xx\x00\x00y= \x0a=\xff= \x0aZa", b"=a\x0a =="],
        steps: &[&[], &[b"IGEgICA9YSANfj1+YT1hYSAgYj3/PT16YXh+AHh4AAB5PSAKPf89IApa"], &[b"YT1hCiA9"], &[b"PQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"\x09\x0aza\xff== =\x0a\x0a  \x0aby\x0d\xff=\x0d"],
        steps: &[&[b"CQp6Yf89\x0d\x0aPSA9Cgog\x0d\x0aIApieQ3/"], &[b"\x0d\x0aPQ0="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0ay  =\x0a   z~bb "],
        steps: &[&[b"Cnkg\x0aID0K\x0aICAg\x0aen5i"], &[b"\x0aYiA="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"= z \xff===zb\x0da~\xff"],
        steps: &[&[b"PSB6IP89PT16Yg1h"], &[b"fv8="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"~\xff  b =\x0aa\x00a a\x00x", b"\x0d\x09a\x0a\x0d"],
        steps: &[&[b"fv8gIGIg--PQphAGEg--YQB4"], &[b"DQlh"], &[b"--Cg0="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"b =", b"~ ab y\x0a", b"\x00=~Z\x0az\x0a\x00~\x0dbaay=\x0a", b"x\x0a~\x00\x0a=\x0a=\x0d "],
        steps: &[&[b"YiA9"], &[b"fiBhYiB5"], &[b"CgA9floKegoAfg1iYWF5"], &[b"PQp4Cn4ACj0KPQ0g"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b" \x0a", b"=", b"=", b"b", b"\x0d=\x0a\x00", b"z"],
        steps: &[&[], &[b"IAo9"], &[], &[], &[b"PWINPQoA"], &[], &[b"eg=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"~", b"\x00"],
        steps: &[&[], &[], &[b"fgA="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"ax \x09\x0dz\x0a\x00Z\x0a\x09Z\x0aa~ \x00\x00z\x0da~\x00\x0d\x0d x\x0d\x09\x0azy \x0a\x0a=\x00b\x09=bZ\x0a\x0d\x0a~\x00"],
        steps: &[&[b"YXggCQ16\x0d\x0aCgBaCgla\x0d\x0aCmF+IAAA\x0d\x0aeg1hfgAN\x0d\x0aDSB4DQkK\x0d\x0aenkgCgo9\x0d\x0aAGIJPWJa\x0d\x0aCg0K"], &[b"fgA="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a\x09 \x0a\x0d== a\x0d \x09 ab= b~\x09~\x0aZ\x0a=\x0aa y\x0a\x09 ", b" =\x00~\x0d=   \x0a=a\x0a=yx\x0a\xff"],
        steps: &[&[b"Cgkg\x0aCg09\x0aPSBh\x0aDSAJ\x0aIGFi\x0aPSBi\x0afgl+\x0aCloK\x0aPQph\x0aIHkK"], &[b"\x0aCSAg\x0aPQB+\x0aDT0g\x0aICAK\x0aPWEK\x0aPXl4"], &[b"\x0aCv8="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"Zba=\xff a\x0az~=\x0a\x0a\x09~\x0a\x0aZa\x00\x09=  \x0d yxa "],
        steps: &[&[b"WmJhPf8gYQp6fj0KCgl+CgpaYQAJPSAgDSB5eGEg"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"\x00\x0d~a=\x0a\x0d\xff\xff\x00\x09\x0d Z\x0d ~=zaa\x00\x0d\x0d\x0a\x0a", b"b=a\x0a =\x0d\x0a \x09b \x0a=ba \x0aZ\x0dyy="],
        steps: &[&[b"AA1+YT0K--Df//AAkN--IFoNIH49--emFhAA0N"], &[b"--CgpiPWEK--ID0NCiAJ--YiAKPWJh--IApaDXl5"], &[b"--PQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"Z~", b"z~bb b =\x0a===", b"b\x0d", b"=\x0d\x0d ", b"=~ =a=", b"b"],
        steps: &[&[], &[b"Wn56fmJiIGIgPQo9"], &[b"PT1i"], &[b"DT0N"], &[b"DSA9fiA9"], &[b"YT1i"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"z\x0a\x0aa=a=a\xff", b"a\x00\x0ay\x0a\x0a =", b"\x00="],
        steps: &[&[b"egoKYT1hPWH/"], &[b"YQAKeQoK"], &[b"ID0A"], &[b"PQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"\x0a\x0a=\xff Z~yx\x0a  az =Za\xff=y= zbb\x0d\x0db \x09", b"a\x09by=\x0da==\x0ab\xff"],
        steps: &[&[b"Cgo9/yBafnl4CiAgYXogPVph/z15PSB6YmINDWIg"], &[b"CWEJYnk9DWE9PQpi"], &[b"/w=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"\x0a=bx\x0a\x0d\x0d\x09~", b"\x0d\x0ab", b"xy~Z=\x09\x0a  \x0d a ", b"\x0abax\x0d"],
        steps: &[&[b"Cj1ieAoN\x0d\x0aDQl+"], &[b"DQpi"], &[b"\x0d\x0aeHl+Wj0J\x0d\x0aCiAgDSBh"], &[b"\x0d\x0aIApiYXgN"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a= a\x0a\x09\x0ayz\x0a=\x09=a \xffZ=y\x09\x0a\x0a=\x0d\x0a~b y", b"a", b"Z\xffby\x0db\xff\x0a", b"\x0ay"],
        steps: &[&[b"Cj0g\x0aYQoJ\x0aCnl6\x0aCj0J\x0aPWEg\x0a/1o9\x0aeQkK\x0aCj0N\x0aCn5i"], &[b"\x0aIHlh"], &[b"\x0aWv9i\x0aeQ1i"], &[b"\x0a/woK"], &[b"\x0aeQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"\x0d\x0a=", b"\xff", b"ya\x09~~a\x0d"],
        steps: &[&[b"DQo9"], &[], &[b"/3lhCX5+"], &[b"YQ0="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"a\x0d", b" a\x0ab\x0a=b\x09x\xff \x0a\x0az~b  ==b"],
        steps: &[&[], &[b"YQ0gYQpi--Cj1iCXj/--IAoKen5i--ICA9"], &[b"PWI="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"x~x=b\x0aZ==  \x0a~=za\x0a\x0a y ~xbZxaz \x09bz\x00  a\xff"],
        steps: &[&[b"eH54PWIKWj09ICAKfj16YQoKIHkgfnhiWnhheiAJYnoAICBh"], &[b"/w=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"\x09 z=b=Z = y\x09~\x0a=\x0a\x0ax=\x00==\xffZ=\x00"],
        steps: &[&[b"CSB6PWI9WiA9IHkJfgo9Cgp4PQA9Pf9a"], &[b"PQA="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"= =y\xff=zx\x0d=zb=z ", b"ab", b"=\x09=a=b\x00Z", b"xa\x00\x0abz\x0ab\xff\x0a\x0a\x0d", b"ax", b"Z\x0d "],
        steps: &[&[b"PSA9ef89engNPXpiPXog"], &[], &[b"YWI9CT1hPWIA"], &[b"WnhhAApiegpi/woK"], &[b"DWF4"], &[b"Wg0g"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"\x0ab ", b"=", b"\x0a\x0a", b"x", b"b\x0db= Z\x0d\x0a"],
        steps: &[&[b"CmIg"], &[], &[b"PQoK"], &[], &[b"\x0d\x0aeGINYj0g\x0d\x0aWg0K"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a", b"\x00", b"\x0ab\x0d", b"~bya\x0a", b" ="],
        steps: &[&[], &[], &[b"CgAK"], &[b"\x0aYg1+\x0aYnlh"], &[b"\x0aCiA9"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"= =Za\x09b \x00===aZ\x0d", b"\xffy\x0aa= ===\x09="],
        steps: &[&[b"PSA9WmEJYiAAPT09YVoN"], &[b"/3kKYT0gPT09"], &[b"CT0="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"\x09bb=="],
        steps: &[&[b"CWJi"], &[b"PT0="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"= \x00z", b"\x0aa~y", b"\x0aa\x00\x09=Z\x09 ", b"=\x00~a", b" ="],
        steps: &[&[b"PSAA"], &[b"egph"], &[b"fnkKYQAJPVoJ"], &[b"ID0A"], &[b"fmEg"], &[b"PQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"~babz~  \x0a\x00Zza  a\x0d\x0aa=z=a\x0a\x09a=\x0aZ  \x0d=", b"==~ b"],
        steps: &[&[b"fmJhYnp+ICAKAFp6YSAgYQ0KYT16PWEKCWE9ClogIA09"], &[b"PT1+"], &[b"IGI="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"Z~y~\x0dx\x0a \x09Z\x0d\x0a\x00\x0a \x0ax\x0a\x00=\xff\x0a", b"\x0a=\x00= =Z\x0da\x0a\x0a\x09ba"],
        steps: &[&[b"Wn55fg14CiAJWg0KAAogCngKAD3/"], &[b"Cgo9AD0gPVoNYQoKCWJh"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"=a=b", b"==x\x09b", b" x  \xffb\x0a\x0aa", b"\x0d"],
        steps: &[&[b"PWE9"], &[b"Yj09\x0d\x0aeAli"], &[b"IHgg\x0d\x0aIP9iCgph"], &[], &[b"\x0d\x0aDQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0a\x0db\x0a   =\x0a=~x\x0a == z a\x00 ", b" \x00"],
        steps: &[&[b"Cg1i\x0aCiAg\x0aID0K\x0aPX54\x0aCiA9\x0aPSB6\x0aIGEA"], &[b"\x0aICAA"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"Zxx\x0a", b"\x00\xffy\x0d\x00a", b"=~=\x0a", b" \x0a\x00\x0a\x0d\x0a "],
        steps: &[&[b"Wnh4"], &[b"CgD/eQ0A"], &[b"YT1+"], &[b"PQogCgAKDQog"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"y", b"yxb=a\x0a= b=yx=", b"  "],
        steps: &[&[], &[b"eXl4Yj1h--Cj0gYj15"], &[b"--eD0g"], &[b"IA=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"\x00 \x09\x0a\xff\x0aa\x0aZ\x0a Zbbby=y\x0axyaZ=\x09a~", b"\x0d\x0a\xff\x00\x0dx=\xff y Zy\xffy\x0az\x00 "],
        steps: &[&[b"ACAJCv8KYQpaCiBaYmJieT15Cnh5YVo9CWF+"], &[b"DQr/AA14Pf8geSBaef95CnoA"], &[b"IA=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"~Zab\x0a\x0dZ\x0db\x0aa \x0d\x09\x0d\x0a", b"= \x0az=\x0d", b"b=\x0a\x09xz", b"~=\x0d ", b"~b\x09"],
        steps: &[&[b"flphYgoNWg1iCmEgDQkN"], &[b"Cj0gCno9"], &[b"DWI9Cgl4"], &[b"en49"], &[b"DSB+"], &[b"Ygk="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"~ ", b"\x0a", b"\x0a"],
        steps: &[&[], &[b"fiAK"], &[], &[b"Cg=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b"yZb", b"= x", b"a"],
        steps: &[&[b"eVpi"], &[b"PSB4"], &[], &[b"\x0d\x0aYQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=b   Z\x0a   \x0aa= =", b"=\x0d \x09Z\x0a=\x00", b" ", b"\x0a", b"b  "],
        steps: &[&[b"PWIg\x0aICBa\x0aCiAg\x0aIAph\x0aPSA9"], &[b"\x0aPQ0g\x0aCVoK"], &[b"\x0aPQAg"], &[], &[b"\x0aCmIg"], &[b"\x0aIA=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(3)), ]),
        chunks: &[b"\x0a==\x0a\x09 b\x0aybZ\xff\xffxaa =aZ\x0a\x0a\x0a\x09z ab\x0a\xff"],
        steps: &[&[b"Cj09CgkgYgp5Ylr//3hhYSA9YVoKCgoJeiBhYgr/"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(10)), (b"line-break-chars", P::S(b"--")), ]),
        chunks: &[b"Z b\x0aaz\x0az\x09===\x0a =~\x0dbZ\x0a=z= =y=a=~~= Zab\x0a\x0d"],
        steps: &[&[b"WiBiCmF6--CnoJPT09--CiA9fg1i--Wgo9ej0g--PXk9YT1+--fj0gWmFi"], &[b"--Cg0="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(76)), ]),
        chunks: &[b"b", b"==x\x0d\x09ayz\x0d", b"  \xff"],
        steps: &[&[], &[b"Yj09eA0JYXl6"], &[b"DSAg"], &[b"/w=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: None,
        chunks: &[b"\x00aa", b"\xff~ Z \x0a\x0a=Z= Z", b"b\x0a", b"b=\x00a", b"=a", b"=\xff=y"],
        steps: &[&[b"AGFh"], &[b"/34gWiAKCj1aPSBa"], &[], &[b"YgpiPQBh"], &[], &[b"PWE9/z15"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[]),
        chunks: &[b"=b", b"\x09a ", b"\x0d\x00\x0d\x0d=  =b==\x0a\x0a~\x09a", b" \x0a\xff="],
        steps: &[&[], &[b"PWIJ"], &[b"YSANAA0NPSAgPWI9PQoKfglh"], &[b"IAr/"], &[b"PQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(8)), ]),
        chunks: &[b" z\x0a\x0aZb yZ\x0aaZ=\x00\x0da\x0dy \x0a", b"\x00a\x0daaa\x0d=aZb=\x0a\x0ab\x09 b\x0a\x0d\x0a\x0a\x0db\x0a aa\x0d"],
        steps: &[&[b"IHoKClpi\x0d\x0aIHlaCmFa\x0d\x0aPQANYQ15"], &[b"\x0d\x0aIAoAYQ1h\x0d\x0aYWENPWFa\x0d\x0aYj0KCmIJ\x0d\x0aIGIKDQoK\x0d\x0aDWIKIGFh"], &[b"\x0d\x0aDQ=="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-encode",
        params: Some(&[(b"line-length", P::L(4)), (b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=", b"\x09\x0ab \x0aa~", b"a", b"\x09b"],
        steps: &[&[], &[b"PQkK\x0aYiAK"], &[b"\x0aYX5h"], &[], &[b"\x0aCWI="], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"*PS", b"BafiA9DSAK", b"YT3//1p5ACAg/3pa", b"ICBiPWIgIGJ+eg", b"oNfiAKDQo", b"="],
        steps: &[&[b"="], &[b" Z~ =\x0d \x0a"], &[b"a=\xff\xffZy\x00  \xffzZ"], &[b"  b=b  b~z"], &[b"\x0a\x0d~ \x0a\x0d\x0a"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"CiB+YWEJPXphI=Ap4DX49Wg0NDXphPXoKCQ1heXlieg0AAD09eAA="],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Ygp6CT0g", b"DQ3/YQo9IAk", b"*9PXphCgoN/wpheQo9YQ0gDXh6CSBifg=="],
        steps: &[&[b"b\x0az\x09= "], &[b"\x0d\x0d\xffa\x0a= \x09"], &[b"==za\x0a\x0a\x0d\xff\x0aay\x0a=a\x0d \x0dxz\x09 b~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Y*WEgYWIAA", b"D15DSB4eCANIAkKYgp5DT0KfmENWnhiYv8="],
        steps: &[&[b"aa ab\x00"], &[b"\x00=y\x0d xx \x0d \x09\x0ab\x0ay\x0d=\x0a~a\x0dZxbb\xff"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"P", b"Q14CmFh\x0d\x0aYgo9", b"CiAN\x0d\x0aPQBhen55\x0d\x0a", b"YT09AA14\x0d\x0aIABaIAog\x0d\x0a", b"PXp5PQoJ\x0d\x0aCg==\x0d\x0a"],
        steps: &[&[], &[b"=\x0dx\x0aaab\x0a="], &[b"\x0a \x0d=\x00az~y"], &[b"a==\x00\x0dx \x00Z \x0a "], &[b"=zy=\x0a\x09\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"DT1iAAoNWlp", b"hYSA", b"=="],
        steps: &[&[b"\x0d=b\x00\x0a\x0dZZ"], &[b"aa "], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"e", b"AB", b"5Ygp", b"4QQ", b"=", b"=QQ"],
        steps: &[&[], &[b"x\x00"], &[b"yb\x0a"], &[b"xA"], &[], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eGJh", b"CnoAAAp", b"a/3ggWg==QQ==QQ"],
        steps: &[&[b"xba"], &[b"\x0az\x00\x00\x0a"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"ICAADT0geQkNeGFa"],
        steps: &[&[b"  \x00\x0d= y\x09\x0dxaZ"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"PQ1", b"heD0gCQoJCmI9DQl+IAl4Ynli", b"ICBiYj0NWgk="],
        steps: &[&[b"=\x0d"], &[b"ax= \x09\x0a\x09\x0ab=\x0d\x09~ \x09xbyb"], &[b"  bb=\x0dZ\x09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Ygr/CT3/\x0d\x0aeQ16PXg9\x0d\x0aeiA9\x0d\x0a"],
        steps: &[&[b"b\x0a\xff\x09=\xffy\x0dz=x=z ="], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"PQB5/2ENIA1iYnkKIABi", b"ej", b"1ieCBh"],
        steps: &[&[b"=\x00y\xffa\x0d \x0dbby\x0a \x00b"], &[b"z"], &[b"=bx a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"=AAl5YQ=="],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eQk=Q", b"Q==QQ"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Ci", b"*B", b"6", b"Ynp4/3li"],
        steps: &[&[b"\x0a"], &[b" "], &[b"z"], &[b"bzx\xffyb"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"WgogYj1i\x0d\x0aYQoNYVp4\x0d\x0aeP9+eQ0K\x0d\x0aYg0NYj1i\x0d\x0aPQph/wAK\x0d\x0a"],
        steps: &[&[b"Z\x0a b=ba\x0a\x0daZxx\xff~y\x0d\x0ab\x0d\x0db=b=\x0aa\xff\x00\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"CgpaPQAKDXoJ", b"CT16eWJh", b"CgAgeXo9eQ1hYnkg/yB+AD0gDT0KCX4gDQ==QQ==QQ"],
        steps: &[&[b"\x0a\x0aZ=\x00\x0a\x0dz\x09"], &[b"\x09=zyba"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eHp4YXggDQ1i", b"YXh6Wg0="],
        steps: &[&[b"xzxax \x0d\x0db"], &[b"axzZ\x0d"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Y", b"m", b"IKIH5", b"6e", b"A"],
        steps: &[&[], &[b"b"], &[b"b\x0a ~"], &[b"z"], &[b"x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"ev8gCgo9DQoACSB6eQBhIGEN/2", b"IK", b"DQp+DQ0gYv8JPQ=="],
        steps: &[&[b"z\xff \x0a\x0a=\x0d\x0a\x00\x09 zy\x00a a\x0d\xff"], &[b"b\x0a"], &[b"\x0d\x0a~\x0d\x0d b\xff\x09="], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Yn56YmIKIGENYg=", b"=QQ==QQ"],
        steps: &[&[b"b~zbb\x0a a\x0db"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"elo", b"geT0gDWIgYf8NCXogeA09Y=", b"ngg", b"DT1hCWI="],
        steps: &[&[b"zZ"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"/", b"3", b"h5YWEgeQ=="],
        steps: &[&[], &[b"\xff"], &[b"xyaa y"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"I", b"A==", b"\x0d", b"\x0a"],
        steps: &[&[], &[b" "], &[], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Wg0AIA0", b"K", b"DT0NID0KeGEN"],
        steps: &[&[b"Z\x0d\x00 \x0d"], &[b"\x0a"], &[b"\x0d=\x0d =\x0axa\x0d"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"IAk", b"9", b"D", b"T0", b"=", b"*"],
        steps: &[&[b" \x09"], &[b"="], &[], &[b"\x0d="], &[], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"DSB6D", b"Q0NPf8g", b"DXl", b"hPQ==QQ", b"==QQ"],
        steps: &[&[b"\x0d z"], &[b"\x0d\x0d\x0d=\xff "], &[b"\x0dy"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"IA", b"kAeT09", b"\x0d\x0a"],
        steps: &[&[b" "], &[b"\x09\x00y=="], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"CT0gPVpiAAoAYQpieFp4f", b"mJaIFoNI=Ao9CQoNIGEgACAg/wp6Y", b"gA", b"="],
        steps: &[&[b"\x09= =Zb\x00\x0a\x00a\x0abxZx"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"CSB6CgliICBhCn5heAkgCT0gYv8AA"],
        steps: &[&[b"\x09 z\x0a\x09b  a\x0a~ax\x09 \x09= b\xff\x00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"DSB4IA=", b"*="],
        steps: &[&[b"\x0d x "], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eg1hCQAA\x0d\x0aPX4", b"gYWE=", b"\x0d", b"\x0a"],
        steps: &[&[b"z\x0da\x09\x00\x00=~"], &[b" aa"], &[], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"CT16IABa", b"Wv96IHk9*C", b"QoKCX4gYi", b"A="],
        steps: &[&[b"\x09=z \x00Z"], &[b"Z\xffz y="], &[b"\x09\x0a\x0a\x09~ b"], &[b" "], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"YnliID1hAAoKDT1+", b"ID1+Wg0gf", b"n5+eT1i", b"YmEgPWI9PQphAA==QQ==QQ"],
        steps: &[&[b"byb =a\x00\x0a\x0a\x0d=~"], &[b" =~Z\x0d "], &[b"~~~y=b"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"AAog", b"IAphA", b"H5hYg", b"oNIF=og", b"eAkJ"],
        steps: &[&[b"\x00\x0a "], &[b" \x0aa"], &[b"\x00~ab"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"DXl6ID0KPWIgeAphCgAg", b"IHgNPWEKCn4g//8KYWINCv9iAA0J="],
        steps: &[&[b"\x0dyz =\x0a=b x\x0aa\x0a\x00 "], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"//", b"8", b"=QQ==", b"Q", b"Q"],
        steps: &[&[b"\xff"], &[b"\xff"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"C", b"g0Nej0N\x0d\x0aPT09DSBi\x0d\x0aCgp6eloA\x0d\x0a/w", b"AgPQo", b"A\x0d\x0a/wr/YVo=", b"\x0d\x0a"],
        steps: &[&[], &[b"\x0a\x0d\x0dz=\x0d===\x0d b\x0a\x0azzZ\x00\xff"], &[b"\x00 =\x0a"], &[b"\x00\xff\x0a\xffaZ"], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"YmENeiBhAD0", b"9CXoACg16AHp5AAAgYg16DWE9Wj0g", b"PT09IA0=QQ==QQ"],
        steps: &[&[b"ba\x0dz a\x00="], &[b"=\x09z\x00\x0a\x0dz\x00zy\x00\x00 b\x0dz\x0da=Z= "], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"PQ=", b"="],
        steps: &[&[b"="], &[], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Yj3", b"/eHg9\x0d\x0ae", b"Ao9IAk=\x0d\x0a"],
        steps: &[&[b"b="], &[b"\xffxx="], &[b"x\x0a= \x09"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"DX5hIA0", b"gCgoJ", b"fiB5CnkKeD0NDf8A", b"PWIJIHg9egl6YmJiAAA"],
        steps: &[&[b"\x0d~a \x0d"], &[b" \x0a\x0a\x09"], &[b"~ y\x0ay\x0ax=\x0d\x0d\xff\x00"], &[b"=b\x09 x=z\x09zbbb\x00\x00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"PWI9DSD/I", b"GJ5PX4KP*f8N", b"DQphIAogeA=="],
        steps: &[&[b"=b=\x0d \xff"], &[b" by=~\x0a=\xff\x0d"], &[b"\x0d\x0aa \x0a x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eSA9IH5ieX", b"4NDQB4DQ16QQ==", b"QQ"],
        steps: &[&[b"y = ~by"], &[b"~\x0d\x0d\x00x\x0d\x0dzA"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eiANAP//eg=="],
        steps: &[&[b"z \x0d\x00\xff\xffz"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"fnoKDWJhDX", b"l5IH4AYSA9IF", b"ogDQp4Y", b"i", b"D/YWI=QQ==QQ"],
        steps: &[&[b"~z\x0a\x0dba\x0d"], &[b"yy ~\x00a = "], &[b"Z \x0d\x0ax"], &[b"b"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"PQoNPT0NAHo=QQ==QQ"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eXphAGIg", b"Cg", b"paPf9ieX4K", b"Wg", b"0="],
        steps: &[&[b"yza\x00b "], &[b"\x0a"], &[b"\x0aZ=\xffby~\x0a"], &[b"Z"], &[b"\x0d"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"/3kgYj0JCSB5DS", b"A9", b"Wj0NCSA", b"=QQ==QQ"],
        steps: &[&[b"\xffy b=\x09\x09 y\x0d"], &[b" ="], &[b"Z=\x0d\x09 "], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"W", b"iB", b"4Y", b"T0ACg", b"B="],
        steps: &[&[], &[b"Z "], &[b"x"], &[b"a=\x00\x0a"], &[b"\x00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"e", b"ngAYiAK", b"\x0d\x0aICAKAD", b"09", b"\x0d\x0a", b"CWI9CXo=\x0d\x0a"],
        steps: &[&[], &[b"zx\x00b \x0a"], &[b"  \x0a\x00"], &[b"=="], &[], &[b"\x09b=\x09z"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"YnoKCj1iCiAACmI*JC", b"gp+PSBaYgkA/z1hYQ1iegpaIAp", b"i"],
        steps: &[&[b"bz\x0a\x0a=b\x0a \x00\x0ab\x09"], &[b"\x0a\x0a~= Zb\x09\x00\xff=aa\x0dbz\x0aZ \x0a"], &[b"b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eHh+AA", b"AKCmF+eQp", b"h/", b"yA9Y*SBiCiD/", b"PT0g"],
        steps: &[&[b"xx~\x00"], &[b"\x00\x0a\x0aa~y\x0a"], &[b"a"], &[b"\xff =a b\x0a \xff"], &[b"== "], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"eXgKCj0gDf", b"8JPQl4eXp+PQoNClphCgo9", b"YWI9CWINB"],
        steps: &[&[b"yx\x0a\x0a= \x0d"], &[b"\xff\x09=\x09xyz~=\x0a\x0d\x0aZa\x0a\x0a="], &[b"ab=\x09b\x0d"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Cgp", b"hPSAgY", b"n", b"5hIGI9CiBhPQ", b"ANPQ", b"o=QQ==QQ"],
        steps: &[&[b"\x0a\x0a"], &[b"a=  "], &[b"b"], &[b"~a b=\x0a a="], &[b"\x00\x0d="], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"YWENPXg9CnoKeGJ+eQoAYj1+YiB4DQp", b"a/3ogYnoKYiBhQQ==QQ"],
        steps: &[&[b"aa\x0d=x=\x0az\x0axb~y\x0a\x00b=~b x\x0d\x0a"], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Wg", b"1aAGIAPW", b"JhA", b"AB+"],
        steps: &[&[b"Z"], &[b"\x0dZ\x00b\x00="], &[b"ba"], &[b"\x00\x00~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"IG", b"FheT", b"15YmF6DT1*+Wg0JeA=="],
        steps: &[&[b" "], &[b"aay"], &[b"=ybaz\x0d=~Z\x0d\x09x"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"Wgo9CWJaYm", b"JhACAgYn5hYQD/", b"Ci", b"B4DQk9Wg09", b"eHkKCj1", b"5PQ=B"],
        steps: &[&[b"Z\x0a=\x09bZb"], &[b"ba\x00  b~aa\x00\xff"], &[b"\x0a"], &[b" x\x0d\x09=Z\x0d="], &[b"xy\x0a\x0a="], &[], ],
        warning: Some(b"Stream filter (convert.base64-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.base64-decode",
        params: None,
        chunks: &[b"YT1+eWE9PQo", b"K", b"IGEKYgoKYiAA/w3/PWIKAC", b"B", b"hIAB"],
        steps: &[&[b"a=~ya==\x0a"], &[b"\x0a"], &[b" a\x0ab\x0a\x0ab \x00\xff\x0d\xff=b\x0a\x00"], &[b" "], &[b"a \x00"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0Ab=0Dz=0A=FF=0A=80=3D=0D =0Aby=C3=3Dy=3Dx=C3b=3D=FF=0Da=3DZ=0A=09x =80=0A=\x0d\x0a=C3=00z=0A=0Dba=3D=00=3Da=0D=00bz=3D=\x0aaa=0D=09~=00=0A=0A=3D=C3=C3a=3D=0Axa=3D=\x0d\x0aa~=3Db=3D\x0d\x0a=0D ~x=0D=3D    =FF=0A=0A=C3 ya=0Dab=20=0D a=0A=C3=3D Z=00a=80b  =00z "],
        steps: &[&[b"\x0ab\x0dz\x0a\xff\x0a\x80=\x0d \x0aby\xc3=y=x\xc3b=\xff\x0da=Z\x0a\x09x \x80\x0a\xc3\x00z\x0a\x0dba=\x00=a\x0d\x00bz=aa\x0d\x09~\x00\x0a\x0a=\xc3\xc3a=\x0axa=a~=b=\x0d\x0a\x0d ~x\x0d=    \xff\x0a\x0a\xc3 ya\x0dab \x0d a\x0a\xc3= Z\x00a\x80b  \x00z "], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=09=00 =0Aa\x0d\x0ayb=0D=09=3D=0Abbb b", b"=0D=FF=3D=C3", b"=0Db~\x0d\x0a=FF~=F", b"F=FFa az y=0A   Z~=00a=0A ~Zya=C3=0D=80 b=0D=80=80=00b=0Ab=3D~=0Dy=0D=\x0d\x0a=C3=C3baya Zba=0Aa=0D =0A=FF~a=FFz=00Z~=00z=3D=FFa=0Az a=3d=3D=0Aya=FF=3D=\x0d\x0a=80"],
        steps: &[&[b"\x09\x00 \x0aa\x0d\x0ayb\x0d\x09=\x0abbb b"], &[b"\x0d\xff=\xc3"], &[b"\x0db~\x0d\x0a\xff~"], &[b"\xff\xffa az y\x0a   Z~\x00a\x0a ~Zya\xc3\x0d\x80 b\x0d\x80\x80\x00b\x0ab=~\x0dy\x0d\xc3\xc3baya Zba\x0aa\x0d \x0a\xff~a\xffz\x00Z~\x00z=\xffa\x0az a==\x0aya\xff=\x80"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=0", b"D=0= x0=", b"3Daa", b"=", b"0A=0", b"A"],
        steps: &[&[], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"a=3D=FFa=0A=FFx=09=0Dzaa~=3", b"D=09=C3=0D =C3 =0A=0AZ=0A~=C3bx=0A=C3=3Dz~ =0A=\x0d\x0a=3Dy=0A =0A =09z z~=0", b"D  =3day =FF=0A=FF=FF=0Ayb=3D=80=0D=00=00=3D=3D=3DZ=FFy a\x0d\x0aZ=3D=C3=0A", b"=C3 xx=3Dz=C3=3D z=C3=0A=09=00ay =C3=FF Z=0A z=00=0D"],
        steps: &[&[b"a=\xffa\x0a\xffx\x09\x0dzaa~"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0AZz", b"Zab =3Da=0Az=0D=00=0D=0D=0D=0D=0Dbba= x=0Da=0Db=80=0A"],
        steps: &[&[b"\x0aZz"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"xab=3D =FF=0Ax=3DZ=0Aba=00=00  xab~=00~=0A=C3=3Dzb=0Aa=09=00=09=3D=0Dz Zx=\x0d\x0a=3Dz=3D =80=0Dba=20=0Dxa=3D =0A=3Da z=C3Zbz=00=0Ab=C3=C3a=3D=C3=09\x0d\x0aba=00ay=00a b=0D=80 z=80=C3 =3", b"D=0A ab=09=3D=FFab=0Da=C3=00=0A=0Aay=FFzz=\x0d\x0a=80~by~=0A"],
        steps: &[&[b"xab= \xff\x0ax=Z\x0aba\x00\x00  xab~\x00~\x0a\xc3=zb\x0aa\x09\x00\x09=\x0dz Zx=z= \x80\x0dba \x0dxa= \x0a=a z\xc3Zbz\x00\x0ab\xc3\xc3a=\xc3\x09\x0d\x0aba\x00ay\x00a b\x0d\x80 z\x80\xc3 "], &[b"=\x0a ab\x09=\xffab\x0da\xc3\x00\x0a\x0aay\xffzz\x80~by~\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=00x=00=09=0D=0Dy=0=0=0Db=20=0D=3D=0A=3D=80=3Dbzb=0DZx=3Db =C3a=0D=3Dz=C3=3D=\x0d\x0a", b"b=3D=", b"00~xy=3D=C3a =3Da=09 zZ=09=20\x0d\x0a=20=0Dx=0A=C", b"3 =0Abb=00\x0d\x0a=3D=3D=3D=0A=0Dy =09=3D~=0D=00", b"y Zb =C3=0A=0A=3Dx=FF b=0D =FF=FF=09 yx  b=3D\x0d\x0a=C3=0A =3Da=80=20=0Da"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b" b=FF=0Da=C3z=0D=", b"80=FF=C3=80=09a ", b"~a=0A=0D=3D=\x0d\x0d=3Db=", b"0A~ax=C3=09x=20", b"=0D~=20\x0d\x0a=09"],
        steps: &[&[b" b\xff\x0da\xc3z\x0d"], &[b"\x80\xff\xc3\x80\x09a "], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"ba=\x0dX=0Ax=3D=00a =00~=0", b"9ab=0AbZ =0A=FF=0D", b" b=80a=0D=00=0A=0A=", b"09=FF=0A=C3=00b"],
        steps: &[&[b"baX\x0ax=\x00a \x00~"], &[b"\x09ab\x0abZ \x0a\xff\x0d"], &[b" b\x80a\x0d\x00\x0a\x0a"], &[b"\x09\xff\x0a\xc3\x00b"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=0D~Z=0DzZ=80a=0Aa=00=3DZ==3Dax=C3=FF=3Da y=80a=0D=C3=3Dxa=C3~=0Ab=3D x =0Az=\x0d\x0a=0Aa=0A aa=00b=FFb=09=0D", b"a=0Aa=0A=00a =3Dyz=0A~=80b=0Da=0A =00=0", b"A=3Dz =0A=0A=\x0d\x0a =0A=3D=3Dx=C3~=0A=80~=3D=3D=3D=3D=0D=3D ", b" xaZ=09=0D=0D y =FF=0D=09=C3x", b" ba=\x0d\x0a=0A=FF=3D=20=0Dx=3Dab"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b" ~=0A=FF= \x0d\x0az=", b"3Da"],
        steps: &[&[b" ~\x0a\xffz"], &[b"=a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"ya=80~=80Z=80=0", b"Ab=C3 =3Dy", b"b  = \x0d\x0a =09a=3", b"D=80=3D=0A=80=80=0Db=80 =", b"3D=0A=09=FF =3D=\x0d\x0a=0A", b"=0D"],
        steps: &[&[b"ya\x80~\x80Z\x80"], &[b"\x0ab\xc3 =y"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"a~  a=20=0D=0Dz a=", b"00a=80=3D~y=3D =G1b=3DZ~ "],
        steps: &[&[b"a~  a \x0d\x0dz a"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=0D xz ~=C3=FF=0A=FF=09 =FF=0Da=80=80=0Dy=0AaZ=FF=00babb=09b=0A=3Dab=09=FF=\x0d\x0a=09b=80 yz b=3DZ=0A", b"=09a~=00a~=C3=G1 =0A=3DZyyb=00a=0Dx=3DZZZ=0A=0Dx   yb=00Z  =\x0d\x0a=09b x=3DaZ=09yb=FFZ=0A~a=80=09~ =0A=0D=0D=00=3DZ~=0A=0D=0D=00bZ=3Dy=3D=0Aa"],
        steps: &[&[b"\x0d xz ~\xc3\xff\x0a\xff\x09 \xff\x0da\x80\x80\x0dy\x0aaZ\xff\x00babb\x09b\x0a=ab\x09\xff\x09b\x80 yz b=Z\x0a"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=3DZa=0AzbZZaay=00  =FF=0AaZzyb=FF=80=0D=0D=0D=3D=C3=0A=0A=0A=3D=", b"C", b"3=3D=3dZ =", b"0A=\x0d\x0axaa=0A=3Dx=C3=09a=0D=3D=3D =80b=3D ", b"zb"],
        steps: &[&[b"=Za\x0azbZZaay\x00  \xff\x0aaZzyb\xff\x80\x0d\x0d\x0d=\xc3\x0a\x0a\x0a="], &[], &[b"\xc3==Z "], &[b"\x0axaa\x0a=x\xc3\x09a\x0d== \x80b= "], &[b"zb"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"b=0D=80~=C3=0Db=80=3D b=3Dx=3D=09=FFya=", b"0Da =20=0D=0D=80b=0Db=0D"],
        steps: &[&[b"b\x0d\x80~\xc3\x0db\x80= b=x=\x09\xffya"], &[b"\x0da  \x0d\x0d\x80b\x0db\x0d"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"b=00a =0A=80=80=3Dxbx=", b"0A", b"=00=0= xA", b"=C3=", b"3D"],
        steps: &[&[b"b\x00a \x0a\x80\x80=xbx"], &[b"\x0a"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=", b"C3=0A=3D=0A=3Dz", b"=C3=00Z=0D ~az=20=0", b"DZa =0A=FF  =09Zaz=3D=0A=09=3D a=0A=0Dz=FF=\x0d\x0a=0A=3Dy=0A=3D=3Db=0A=00=3D   =80=FF=3D=0Ay=09~=3D=FF=09=3Dz=0=\x0d", b"9=00", b"yx"],
        steps: &[&[], &[b"\xc3\x0a=\x0a=z"], &[b"\xc3\x00Z\x0d ~az "], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=0Ax=FFb=0A=3D~y=3Db=0A =0AZ=3D=0A=3D=09=FF=09bx  a=FF=FF=C3=0Dxx=3D=00=0Dy=\x0d\x0a Z =20=0D=C3=0Ay=0A=0A=80=0A=0A=3D =FF=FFb=0A=FF=0A=0D=80=0Dbb b=3Dx=0A\x0d\x0a=80bzZ =3D=C3=0D=C3=3D=3=3dDx=0A=80=3D=0A\x0d\x0az=09~=09 =20=0D =80Z=0D=80~=80=0A=80=0A=09=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=3Db\x0d", b"\x0a=C3=80a=0", b"0=00z=3D=3D=3D =0A=3D=FF=3D=20=0D=3D =3D=0D=80=0D=00=20=", b"0Dyax~y=3d=\x0d\x0a=", b"C3", b"=C3z=00Z b=C3bb"],
        steps: &[&[b"=b\x0d"], &[b"\x0a\xc3\x80a"], &[b"\x00\x00z=== \x0a=\xff= \x0d= =\x0d\x80\x0d\x00 "], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0Db =80b  =FFb=0A=3D=0Ab Z=0A=3DxZ=0A=09=0A =FF=3D=3D =3D ~=0Da=0Ax=3D =3D=\x0d\x0a x=3Dxy=0A=0A =C3=0A=00ba=00=0A b==00=80 =0Azb=0Aa=80Z y=FF =3Da=3Db=0Ay=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=3Da=3D", b"z=0A=0Az=3D~~=FF=00z=C3=80=0Axa =0A=0A=80~~", b"=00=0Dx"],
        steps: &[&[b"=a="], &[b"z\x0a\x0az=~~\xff\x00z\xc3\x80\x0axa \x0a\x0a\x80~~"], &[b"\x00\x0dx"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"ayzaa=0A =09=3Da=0Db=C3 =3D== \x0d\x0a0D=FF=FF=3D=0AaZ=00 z=3D=09a=3Dyy=3Dxa=0DZb b=\x0d\x0a=00 a =FF =0A=FFz=FF=0AZx=0Az =09=3D  =09~=20=0D=3D=09=80 =3D=80=0A=00=0A =\x0d\x0a=3DZ=80Z~=FF=3D=FF=80=0D=0D=0D=80=0A =3D"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"a =3D =C3a=3D=3D~b=80=0A=C3=0Db =3D=3d=00\x0d\x0abz=80=00=0D=09=0D baZ  a=00=0Da=3DaaZba =09 y=0A  =09=00=3D~b=FFax=3DZ=\x0d\x0a=C3Z=0DZ=0A =80=00  bZbz=80"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0Dx a =00=0Ay=C3=0A  b=3D=0Db=20=0D=0D=0D=C3=0D=3D=", b"0D=0D  ay=3Da=80~=09", b"=FF=\x0d\x0a=0Aa=3D=C3b=0A =0A  ~a=3Daz=0Daab", b"= xy~=0D=09  y=09=0Db=3D=0D=FFa~=FF=3Dz=3Db=\x0d\x0a=0Az=0A =C3 x=3", b"D=0D=C3=0D"],
        steps: &[&[b"\x0dx a \x00\x0ay\xc3\x0a  b=\x0db \x0d\x0d\x0d\xc3\x0d="], &[b"\x0d\x0d  ay=a\x80~\x09"], &[b"\xff\x0aa=\xc3b\x0a \x0a  ~a=az\x0daab"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"Zyb=0D=09==09=3Db=3DZ=8", b"0=0A=3D=3D=C3=FF y =C3=FF=0Aa=80a=3Da=0Daa=3D=C3b=3D=\x0d\x0a=0D=3D=3DZ=80=20=0D=0D=3Da=0Da a=00~", b"=C3=0D=3D", b"=C3 =00=3D=09=09y=0DZx Z=80=09=\x0d\x0a =00=0D=09=3Dzx=3D=3D=0A\x0d\x0aby =80=3Da=3D=3D=20=0D\x0d\x0a=FF=FF=20=0D=0Dz=FF=3Dz=0D=80=0Ab=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=09=3D=00x =3D=3D=80=3D =0AZ=00=09=80a=0Dy=0A=0A=3Db=0D xy=09=C3=09y =FF=0D=\x0d\x0a=3D=09 =3D=09zx =3D=a=0Aa~Z=3D=0D=0D=C3=C3=3Da\x0d\x0a~=0D~=0Ab~=0D"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"x\x0d\x0a=3D=FF  b=0D b =0Aa=FF=0Ax =00b=3Dazb b=09z=0A   =3D=09  ", b"=00zy=09=0A=FFb=\x0d\x0a=C3~ y=0D ba=3D=00=09x=3D~=0A=3D=FF=3D=09y =3D=3D=09=C3b=0D=", b"C3b =0A=0Dx=FF=\x0d\x0a=00=0A~=0Db=3D", b"=FF=0Ay=0A =80Z=0D=FF=", b"09bab=3D=3D=0Ab =C3a=0A=20=0D=3D=3D=\x0d\x0a=", b"80=0D=3D~=0A=3D"],
        steps: &[&[b"x\x0d\x0a=\xff  b\x0d b \x0aa\xff\x0ax \x00b=azb b\x09z\x0a   =\x09  "], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0", b"D=0D=0D=09a=0D    b a=3D=3D=09=0D  =0=\x0aA ayz=3D=3D=0A=C3=0A b=09=0A=0Aay=0A=\x0d\x0a=0Daaz~y"],
        steps: &[&[], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=3d=3Dby=0A=0A a=3D=0A=FF=FF=FF=C3Z=80x\x0d\x0a=0Aa=80a=80Z=C3  =09a=0D=3DZyx=00", b"=0Db=3D=0A=C3=3D=3D =3Db=09 =80=20=0Dx=20=\x0d\x0a=0D =0A"],
        steps: &[&[b"==by\x0a\x0a a=\x0a\xff\xff\xff\xc3Z\x80x\x0d\x0a\x0aa\x80a\x80Z\xc3  \x09a\x0d=Zyx\x00"], &[b"\x0db=\x0a\xc3== =b\x09 \x80 \x0dx \x0d \x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"bzbaxZ=0Az=3D=0A=3D =0Axb=0DZb~=3D=3D=C3=09=20=0D=FF~=80=09\x0d\x0az=FF=0A=0D=09=C3=0D=3D~=09xy=3Dz x=0A b=3D=80= =80 =C3=0A=3Db=FFa"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"za=00 b=3D =80==\x0d\x0a00bbb=0A ~=3D=0Aa a~=0A=00=0AyzZyxz=00Z=0D=C3Zby=3D=0A=09x=\x0d\x0a=0D  =09=3D=0D=0D"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"a=3D", b"b=3D=C= ", b" ", b"\x09\x0a3=", b"0D"],
        steps: &[&[b"a="], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"a=09=00y=0D~=0A =80=C3=3D=09=0Db=00=C3=80xy =09=0Axaz=C3b=0A=0A=0A=FF=3D=FF=\x0d\x0aZzxa=0A~=3D=80y Zb=3Da~bbz=0Aa=80=09yz=09=80Zabx=0Ax=FF=FF~y=3D\x0d\x0aa=FFZ=09=3Da=3D=0A=0A"],
        steps: &[&[b"a\x09\x00y\x0d~\x0a \x80\xc3=\x09\x0db\x00\xc3\x80xy \x09\x0axaz\xc3b\x0a\x0a\x0a\xff=\xffZzxa\x0a~=\x80y Zb=a~bbz\x0aa\x80\x09yz\x09\x80Zabx\x0ax\xff\xff~y=\x0d\x0aa\xffZ\x09=a=\x0a\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=3D =FF=FF=3Daba=0DZ=0A=3Dba=C3=FF=0DbxZbya=C3=0D=3Dz=0A=0A~=00~ =C3zZ=0Ab =\x0d=3d\x0abbz=0D=09a=3D=80b=0D\x0d\x0ab=FFxZ=0D =3D=80~x=3D x y =00b=0Ab~=", b"09=0Axy=3D a=C3=00y=0A\x0d\x0aa=3D  =0Aay=09a=0", b"D=0D~"],
        steps: &[&[b"= \xff\xff=aba\x0dZ\x0a=ba\xc3\xff\x0dbxZbya\xc3\x0d=z\x0a\x0a~\x00~ \xc3zZ\x0ab \x0d=\x0abbz\x0d\x09a=\x80b\x0d\x0d\x0ab\xffxZ\x0d =\x80~x= x y \x00b\x0ab~"], &[b"\x09\x0axy= a\xc3\x00y\x0a\x0d\x0aa=  \x0aay\x09a"], &[b"\x0d\x0d~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"a== x", b"0D=3Db=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=C=  \x09\x0a3=C3 =20=0D\x0d\x0a=C3 =00a"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=09b=20\x0d\x0aZ=0Azbx=0D =3D=3D=0Ax=8=  \x09\x0a0 yb=3D=0A=0", b"Db aby=0D=0D=C3  =0A=3D=3", b"D=09ax=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=C3=0=\x0d0=C3b=", b"0A=0Db\x0d\x0a=FF=20=0D~=0A=20=0D=C", b"3~ za=3D=3D=0Dzb=FF\x0d\x0az=0A  =80", b"=0A=0Dz=0Db=FF  =3D~=3D=FF=80=3D=C3xa", b"=0A=0Db=0D=80x=80=FF=0A=FFxb=", b"\x0d\x0a=3D=3D=09ay=00Z=09a=C3=3Da=0Ab~"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"x", b"=3D=00Z=09x=0D=09x=80=00=0A=0D=00=FFz\x0d\x0a=0AZ=0DZ=80=09a=3Dz z=0AxZZZz=C3", b"=0= \x0d\x0aA~=0A=FF=3Dz=3D=0D aya=3Db=3Da=0D=0D=09a =\x0d\x0ab=FF~x=3D=20=0Dx=3D~=00", b"Zaa=0A=3Db=00 zx=0A=FF=0A=80 =", b"C3=0D=3D=0Ab =0", b"Abzxb=\x0d\x0a=80=3Db=0A=00 "],
        steps: &[&[b"x"], &[b"=\x00Z\x09x\x0d\x09x\x80\x00\x0a\x0d\x00\xffz\x0d\x0a\x0aZ\x0dZ\x80\x09a=z z\x0axZZZz\xc3"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"xb=C3=3Dyy=3Da=0A=F", b"F=", b"3Dx=C3=C3=", b"C3xy=C", b"3=0A=", b"0Az=0A"],
        steps: &[&[b"xb\xc3=yy=a\x0a"], &[b"\xff"], &[b"=x\xc3\xc3"], &[b"\xc3xy"], &[b"\xc3\x0a"], &[b"\x0az\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=0A=0Abb=FFb=0D =FFZ =00a=0Dx=0Ayy=\x0dy y ~", b"=3D=0Az=3D  =0Ax =3D=0D =C3=0Aaz ~=\x0d\x0a=C3 =09 Z=3D=3Db=0A=00=80=3D=3D~ =20\x0d\x0a ab~x=09=0A=C3=3D=00a=0A=0", b"A~=FF=00 zbb=3D=0A=00b=3D=0A=0A=3D=0Dx=FFb b=\x0d\x0a=C3~=FF=FF=0A"],
        steps: &[&[b"\x0a\x0abb\xffb\x0d \xffZ \x00a\x0dx\x0ayyy y ~"], &[b"=\x0az=  \x0ax =\x0d \xc3\x0aaz ~\xc3 \x09 Z==b\x0a\x00\x80==~  \x0d\x0a ab~x\x09\x0a\xc3=\x00a\x0a"], &[b"\x0a~\xff\x00 zbb=\x0a\x00b=\x0a\x0a=\x0dx\xffb b\xc3~\xff\xff\x0a"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"bzZb=0D=G1=3D yZ\x0d\x0a =3D~=3D=09=0A=0A=3D=0D=09=00 =00 =0A=3Daaza=80xy=09=FFy=3D=09=09az=0DZ=0D=\x0d\x0a=20=0D=80=3D=0Aa=0A bba=0Da~=3D=0A=0D"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"z=0A=0A=3DZ=20\x0d\x0a=0A=3Da =00=0AZb=3", b"D=3D=3D =0A=09ay =C3=09x=80", b"=0A=0= x0=FF", b"x   =09=09=00=3D =\x0d\x0a=80=09=00=09 y=0A=0Dy=20=0Db=FFy=0DZy=0A=0A"],
        steps: &[&[b"z\x0a\x0a=Z \x0d\x0a\x0a=a \x00\x0aZb"], &[b"=== \x0a\x09ay \xc3\x09x\x80"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=3D", b"=3Dyb=09~=80=0A  =C3b =0A=0D=C3=FF=FF", b"==\x0d", b"\x0d3D=09 ~=3Dbxb=0A=0A=3D=0A=80 x=3D=\x0d\x0a=09a=3Dz=C3=0Azb=0Aa=FF =0A=0A=00=80ab=3D=80=0A=FF=FF =80aZa=3D=3Db=20=0D~=\x0d\x0a=20\x0d\x0a=3D=00Z=0Ab=0", b"D"],
        steps: &[&[b"="], &[b"=yb\x09~\x80\x0a  \xc3b \x0a\x0d\xc3\xff\xff"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"\x0d\x0az", b"zy=", b"0", b"=", b"3", b"dD=C3bz~"],
        steps: &[&[b"\x0d\x0az"], &[b"zy"], &[], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"a", b"=0", b"D=0DbZb=0", b"9Zyb=0D=3=3dDy=3Dx=FFbx", b"=3D=0D"],
        steps: &[&[b"a"], &[], &[b"\x0d\x0dbZb"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=3D=3D=0A=3D=3D", b"=0A=00=C3=3D=0D =C3 =0Aab=09=0Ab=0A a a x=0A=80=80b=0D=80 z=\x0d\x0a=00 aaZ=0Db=09 Z=3Da=80=0A=0A=0A=3D=0Aa=3Da=0Ab=3D z ~=0D=09=0D=3D=0D=80=3D=\x0d\x0a=3D xyx=\x0dXb ~=3D=0Daxxa=0A~=3Da=0Db=0D=0Db~=3", b"D~b=3Dx =80b~~=0D=3D=0D"],
        steps: &[&[b"==\x0a=="], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0A", b"b=00Z", b"=3Db=3D=3D=3D=0Dy=00 =00~=0Dy=09=F=\x0aF=0Daz", b"=3D=0AaZz=FF"],
        steps: &[&[b"\x0a"], &[b"b\x00Z"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=\x0d", b"X=3Da=3D", b"=0A=3", b"Dbx=3DZ"],
        steps: &[&[], &[b"X=a="], &[b"\x0a"], &[b"=bx=Z"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"~~=09=00=", b"FFZ=0Aba", b"=09=3D", b"aZa=0D z~ yyb=C3y=3D ay"],
        steps: &[&[b"~~\x09\x00"], &[b"\xffZ\x0aba"], &[b"\x09="], &[b"aZa\x0d z~ yyb\xc3y= ay"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"==0A=00=C3=00a=3D=3D=3Dyy=", b"80y=0D  =80 ~y"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"b=0D", b" =09=3D=00", b"=3Db", b"=C3=00a=80=3D= ", b"x=0A =0A=0A=3D"],
        steps: &[&[b"b\x0d"], &[b" \x09=\x00"], &[b"=b"], &[b"\xc3\x00a\x80="], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"bba=0DZb=FF=0A=0A~=0A=FF=3Db=80=0A=0D=FFba=0DZ=0D x", b"b=0A~=3D=3D=3D=", b"0A=09z=09=\x0d\x0a =0A~=0Ay=80=0Aza=0A=C3y=00=0Dabyb=", b"0Aay=0A z   a=3DZ=3Dx", b"b=C3=0Ab=3D=FF=\x0d\x0a=80=FF~"],
        steps: &[&[b"bba\x0dZb\xff\x0a\x0a~\x0a\xff=b\x80\x0a\x0d\xffba\x0dZ\x0d x"], &[b"b\x0a~==="], &[b"\x0a\x09z\x09 \x0a~\x0ay\x80\x0aza\x0a\xc3y\x00\x0dabyb"], &[b"\x0aay\x0a z   a=Z=x"], &[b"b\xc3\x0ab=\xff\x80\xff~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=3", b"D ~ =20=0D=3", b"D=09b=G1y=FF=0A", b" =3D=0D=3D=0Dbbya=FF=0A=0", b"0=FF ", b"=C3a"],
        steps: &[&[], &[b"= ~  \x0d"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=0AZ=C3=0DZaa=0A~=09 zb=0A=FF=09a=3D=0AZyyZ", b"\x0d\x0a xy=0Aa=80=00x=3Dyby =3D=C3=C3=0Aa=C3=C3a =3Dz=0D=3Db=0Ab=3D=3D =0A=0A=0", b"A=\x0d\x0a=0D=0D=20=  \x09", b"\x0a=0D z=3D~ bbbZ=0A"],
        steps: &[&[b"\x0aZ\xc3\x0dZaa\x0a~\x09 zb\x0a\xff\x09a=\x0aZyyZ"], &[b"\x0d\x0a xy\x0aa\x80\x00x=yby =\xc3\xc3\x0aa\xc3\xc3a =z\x0d=b\x0ab== \x0a\x0a"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b" =0A==G13D=3D=0A=0A=0A\x0d\x0ab=3Dbz=0A=80=20=0D =00ba=0A=3D=0D=", b"3D=09=00aax=3DZ=", b"0A=3Db aa =3D=0", b"Ayb=3D=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"~", b"Z==\x0d\x0d09=0Aa =0A=80=0A aa ", b"~=80=", b"80bZ=0A  =3D"],
        steps: &[&[b"~"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=80=00=0A=3D=09=0A=0D Z=FFa=0Axb==\x0dX3D=3Da =3Daa=09=3D=3D", b"x=3Dz=0Aa", b"a=3D=3D za~z=\x0d\x0a=0DZ=80=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b" =3D=3D zy=0A=0D=0D=80 =C3b=00a =00=FFa=09x=3Da=FF=3Dz b=FF=3D=00xx=80z=0D=\x0d\x0a=09= x=0AZ=0D=C3Z =C3b=3Dbb=00x=0Daax=0A=3D=0Da=09x=0AZ=00=3Dz=FF=0AZ=0D=80=0D=\x0d\x0aZ"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=0A=C3y=09=0A=09ab bxa=0D~=09 =0Abx=09=0Daz==3d3D~zZz", b"=0Da=C3=C3=FF=3D =3D=0Az=\x0d\x0a=3D=0A"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"=09=0A=3DZZZ=3D=00=3DZ=C3y=0D~bz=0A=FFx=0D=FF=00=3D=0D=FF=0A=0D=80=00=0Dz=\x0d\x0a=0A~Z =09y=0Aa=0Ab =80=3D=3D=3D=3Dbaz=G1=3D=0Da=C3z=FFa=3D a=3D=FFb=00y=0A=0D=\x0d\x0a=0Dy=00b=09zZ=3Db=C3=00y=0D=C3=09=0Ab Z=0Ax=80y\x0d\x0az=0A=", b"0Aa=0D=0Dz=0A=3Dbz=00b=80=FFxaxZ=C3x=C3 =3D=3D=C3"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=09~a=0Ax=3D=0D=C3Z=0D=0D~x=80 =0A=09Zyy=C3=0A=FF=3DZ a=0D=0Dy=00=0Aa=0A=0D=\x0d\x0aa a=0Dxya=80a~\x0d\x0a=0D=\x0dX=FF=20=0Dyb=0D =0A bb=3D=3Db=0Dbb=C3=09=FF=09=3DyaZ=80=", b"3D=", b"00=00  az=3Da =\x0d\x0abbZ"],
        steps: &[&[b"\x09~a\x0ax=\x0d\xc3Z\x0d\x0d~x\x80 \x0a\x09Zyy\xc3\x0a\xff=Z a\x0d\x0dy\x00\x0aa\x0a\x0da a\x0dxya\x80a~\x0d\x0a\x0d\x0dX\xff \x0dyb\x0d \x0a bb==b\x0dbb\xc3\x09\xff\x09=yaZ\x80"], &[b"="], &[b"\x00\x00  az=a bbZ"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"\x0d\x0a=0", b"A=80 byz=", b"ba=FF=0Ay=3D=0A=00=80Zy=3D=C3=FFb"],
        steps: &[&[b"\x0d\x0a"], &[b"\x0a\x80 byz"], &[b"\xba\xff\x0ay=\x0a\x00\x80Zy=\xc3\xffb"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=", b"00a=3D=0Ab=80=C3=3Dba=C3=0A=0D ", b"=", b"3D=3D=0A =3D=FF=00 =FF=3D=0Abx=  \x09\x0a=09xa=0Dxy=\x0d\x0a=C3=3D=3D=3Dy "],
        steps: &[&[], &[b"\x00a=\x0ab\x80\xc3=ba\xc3\x0a\x0d "], &[], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"y=00=FFyaa=", b"0D=3D=3D=80zb=0D  =FF=3D  =C3=3D=0Db=3Dz=0A=00Z=0A=0Aayb=0Ab=0A=FF=\x0d\x0aa=00=C3=2", b"0=", b"0DaZy =80=3D=09 =C3=0A=3Da=3Db", b"=0A=C3=0A=0Ax=0DZ=00=0A  a=3D=0D=\x0d\x0a=C3=00 =3D=0Ay=3D=3D bx=0Aa=80=C3=3D ~=20=0D=  \x09\x0a=C3"],
        steps: &[&[b"y\x00\xffyaa"], &[b"\x0d==\x80zb\x0d  \xff=  \xc3=\x0db=z\x0a\x00Z\x0a\x0aayb\x0ab\x0a\xffa\x00\xc3"], &[b" "], &[b"\x0daZy \x80=\x09 \xc3\x0a=a=b"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"zxb =80 ", b"a=0Dabx==\x0a80=00=0Db=FF=09ab=80=09=80a=3D=FF=00=0A=09", b"x=0D=0D=80x=3Dx=\x0d\x0a=0Da xbxa=0D=3Da=0Dy=80=0Ab=0A=0D ax=3D b=0Aa b=0A=3D"],
        steps: &[&[b"zxb \x80 "], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"aa~z ~=3D=0D=80", b"=00 =3Da=0", b"A=80~=0Ab=3D=FF =3D=00=0Ab=0", b"9b =C3Z=00=09 Z=80=\x0d\x0a=C3z=0Db=0A=3D =C3=3Da=0A=0Dz"],
        steps: &[&[b"aa~z ~=\x0d\x80"], &[b"\x00 =a"], &[b"\x0a\x80~\x0ab=\xff =\x00\x0ab"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=3D", b"=3Dzb =0", b"0bZ", b"=80aa=0DZ=\x0d\x0db=FF=80~=0A=0A", b"=3D=0A =0", b"0z =3D=3D=3Dabax =00 ~"],
        steps: &[&[b"="], &[b"=zb "], &[b"\x00bZ"], &[b"\x80aa\x0dZ\x0db\xff\x80~\x0a\x0a"], &[b"=\x0a "], &[b"\x00z ===abax \x00 ~"], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"Z=3D=0Ax=3D=0", b"A =0A\x0d\x0a=8=\x0d\x0d0=00", b"=3D=3Db Z=80=C3=FF=C3aZ=0D a=FF", b"=3D =09=20=0D =C3=0A=0D=80Z~z=0Ay=0D=\x0d", b"\x0a=00=0D=FF=0Az"],
        steps: &[&[b"Z=\x0ax="], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=09=0A=00=3D\x0d\x0a=00=3D=3D=FF=C3=00 b~=0Dz=80=00=09=0A=0A=C3=3D=0A Z =FF=FF~=3D=0A=3Dab=\x0d\x0a=C3=3D=0Ay=C3a=FF=C3=3DZz=C3=0A=00b~ a=0A =80y=3D =00=0Ay=Z=09a =00=0D=09=3D=FF=\x0d\x0a=00=0A=3D=0D=00=0D  =0A=3D=0A ~=0A=09 b=00=0D=3D=0D =0Ay=0A=0A=00"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b" =80=FF=0A=0A=3Dy=3Da=09=0Aa bZa ~y =80=C3  =0A =C3z=80=0A  b~=09ZZ=C3byZ=\x0d\x0a=80 x=80=0Ab=0D=3DZb=80=FF=00=3D=09  =3D y=3D=09=0A=00=FF =20=0D~bx=0A=3D=FF=\x0d\x0a=00xaa  =09=80bb=0A=0A=0Da=3Db  Z  =3D=0Dy =C3 Z=0A=80a=0D=00y=0A=C3=0A=3D=\x0d\x0a=3D=20=0Dz~"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=09=0Ab=3Dz=20\x0d\x0a y =3", b"D=0A=0A z=3Dz=0D~=FFy=3Daz=00a=FF =0A =80a=0A=0A =3D=C3=3D Z=3D=0A=\x0d\x0a=C3b=09=0Ay =09 =0AZa =3D=0D=00=0D =0A=0AxZz=3D=3Day=0D=FF b=0A bxa=0D=20=\x0d\x0a=0Da=09z=C3 a=FF=0D=00Z =0Ayb=0Ax", b"=C3xb=3D=3D=C3=0A a=09=0D\x0d\x0a=3D=0=A"],
        steps: &[&[b"\x09\x0ab=z \x0d\x0a y "], &[b"=\x0a\x0a z=z\x0d~\xffy=az\x00a\xff \x0a \x80a\x0a\x0a =\xc3= Z=\x0a\xc3b\x09\x0ay \x09 \x0aZa =\x0d\x00\x0d \x0a\x0axZz==ay\x0d\xff b\x0a bxa\x0d \x0da\x09z\xc3 a\xff\x0d\x00Z \x0ayb\x0ax"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"Z=3Da=C3=09=0D\x0d\x0a ybbx=0A=3D=FF=3D=09=C3~a=00=", b"0A=80=0Aa=0D=20=0Dx=0Ab=80=80=80=FF Z  =0AbZ=\x0d\x0a=C", b"3=00=3D=0A=0A=0Aax=0D =3D b=80=20=0D=00az=0A=09=", b"3Db=3D~=3D=0D=3D za~~=3=", b"3dD=\x0d\x0a=0A a=20=0D=FF~y~b=0D=C3a=3Dxx=0AZ=0D =3D=C3=3D=0A =0A ~=FF  "],
        steps: &[&[b"Z=a\xc3\x09\x0d\x0d\x0a ybbx\x0a=\xff=\x09\xc3~a\x00"], &[b"\x0a\x80\x0aa\x0d \x0dx\x0ab\x80\x80\x80\xff Z  \x0abZ"], &[b"\xc3\x00=\x0a\x0a\x0aax\x0d = b\x80 \x0d\x00az\x0a\x09"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=80=0Ay =09b =09xz=C3=C3x=3Dy=00 =80ab=3Dx =3D=3D=3D=0A =80", b"=3D=C3b=0Da=0A=\x0d\x0a=0", b"DZ=C3b= x =00x=00y=0D=3D=09 a=3D=3D=3D=0A=3D=0A=09a=00=3DZ=09=3D", b"b=09 =3D=0D=\x0d\x0a=3D =0Ay=0A~=0A=FF=3Dz~ =C3=00b=FFa=00 =09=0A=0D =80=00zx~"],
        steps: &[&[b"\x80\x0ay \x09b \x09xz\xc3\xc3x=y\x00 \x80ab=x ===\x0a \x80"], &[b"=\xc3b\x0da\x0a"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=09", b"  ", b"==\x0d", b"\x0a80"],
        steps: &[&[b"\x09"], &[b"  "], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: None,
        chunks: &[b"=20\x0d\x0ay=0D=3D=80=3D =0A=C3=3D =3Da=0Az=0A=C3Z=FF=C3a=00 =09b~=0Ax", b"y=0Dx=0A=0A=3Dx=\x0d\x0a=3D=3DbZ=C3z=3Dz=0A =C3=0Ayzbb=3D =FF=3D =0A=0A=0A~=00 aZ=0Ax=0D=C3x=3D==00 =FF=\x0d\x0a =0Ab=0A=80", b"=FF=FF=00 ", b"=3D=3Dxx~=80~Za=3D=0A=00=0A=3D=09=3D=0Ax"],
        steps: &[&[b" \x0d\x0ay\x0d=\x80= \x0a\xc3= =a\x0az\x0a\xc3Z\xff\xc3a\x00 \x09b~\x0ax"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[]),
        chunks: &[b"aby=0A=00=0Az~=0AZx~=3d=C3=C3=C3 =0A=3DxZbz=0AZ=0A", b"y", b"=0Abx=3D=3D=00ba =09 "],
        steps: &[&[b"aby\x0a\x00\x0az~\x0aZx~=\xc3\xc3\xc3 \x0a=xZbz\x0aZ\x0a"], &[b"y"], &[b"\x0abx==\x00ba \x09 "], &[], ],
        warning: None,
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0d\x0a")), ]),
        chunks: &[b"=80 =0A=0Ab=0Da=0A ba=09=0D =3D=0D=0D=0Da=FF", b" =0AabZ=0AZ\x0d\x0a=0A =FF=", b"0AZya=FF=0A=C3~z", b"=3D=FF=09b=80a=09b=F=\x0aF"],
        steps: &[&[b"\x80 \x0a\x0ab\x0da\x0a ba\x09\x0d =\x0d\x0d\x0da\xff"], &[b" \x0aabZ\x0aZ\x0d\x0a\x0a \xff"], &[b"\x0aZya\xff\x0a\xc3~z"], &[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
    RefCase {
        filter: b"convert.quoted-printable-decode",
        params: Some(&[(b"line-break-chars", P::S(b"\x0a")), ]),
        chunks: &[b"=09 =0A=3D=0A=0Dza=0A=3Da=0Da=09 Zz=3Dba=80=09y=C3=C3=09=3D=00b=09 =09 a=\x0d\x0a=80=  \x09\x0a=0A=00=00=0Ab=C3"],
        steps: &[&[], ],
        warning: Some(b"Stream filter (convert.quoted-printable-decode): invalid byte sequence"),
    },
];
        pub(super) static DECHUNK_CASES: &[(&[&[u8]], &[&[u8]])] = &[
            (&[], &[]),
            (&[b"7392870"], &[b""]),
            (
                &[
                    b"a\x0aaaaaaa",
                    b"aaa",
                    b"a\x0a5\x0d\x0abb",
                    b"bbb\x0a",
                    b"5\x0d\x0acc",
                    b"ccc\x0d\x0a",
                ],
                &[b"aaaaaa", b"aaa", b"abb", b"bbb", b"cc", b"ccc"],
            ),
            (
                &[b";", b"9", b"58xx9\x0a1", b"x86Z8773", b"Z;6Fa\x0d", b"Z"],
                &[b";", b"9", b"58xx9\x0a1", b"x86Z8773", b"Z;6Fa\x0d", b"Z"],
            ),
            (&[], &[]),
            (
                &[
                    b";9\x0a",
                    b"\x0d95a\x0d\x0a0\x0d",
                    b"9",
                    b"7ZF;Z",
                    b"0x\x0a191",
                    b"\x0a;\x0d1",
                    b"\x0a\x0d",
                    b"70",
                    b"96644",
                    b"\x0a59\x0a",
                ],
                &[
                    b";9\x0a",
                    b"\x0d95a\x0d\x0a0\x0d",
                    b"9",
                    b"7ZF;Z",
                    b"0x\x0a191",
                    b"\x0a;\x0d1",
                    b"\x0a\x0d",
                    b"70",
                    b"96644",
                    b"\x0a59\x0a",
                ],
            ),
            (
                &[
                    b"b;e=1\x0d",
                    b"\x0aaa",
                    b"aaaaaaaa",
                    b"a\x0d\x0a5",
                    b"\x0d\x0abbbb",
                    b"b\x0d\x0a9;",
                    b"e=1\x0ac",
                    b"cccccccc",
                    b"\x0a7\x0d\x0add",
                    b"d",
                    b"dddd\x0d\x0a0",
                    b"\x0d\x0aT: x\x0d",
                    b"\x0a\x0d\x0amor",
                    b"e",
                ],
                &[
                    b"",
                    b"aa",
                    b"aaaaaaaa",
                    b"a",
                    b"bbbb",
                    b"b",
                    b"c",
                    b"cccccccc",
                    b"dd",
                    b"d",
                    b"dddd",
                    b"",
                    b"",
                    b"",
                ],
            ),
            (
                &[
                    b"\x0d\x0d\x0aZ;",
                    b";\x0d\x0d\x0a982",
                    b"13775480",
                    b";1\x0d9\x0aa",
                    b"\x0d5",
                    b"F\x0ax\x0ax\x0a1;",
                    b"7x",
                ],
                &[
                    b"\x0d\x0d\x0aZ;",
                    b";\x0d\x0d\x0a982",
                    b"13775480",
                    b";1\x0d9\x0aa",
                    b"\x0d5",
                    b"F\x0ax\x0ax\x0a1;",
                    b"7x",
                ],
            ),
            (
                &[
                    b"4;e=1\x0d\x0aa",
                    b"aaa\x0d\x0a3;e",
                    b"=1",
                    b"\x0d\x0abbb",
                    b"\x0d\x0aa\x0d\x0a",
                    b"cccccc",
                    b"cccc\x0d",
                    b"\x0a",
                ],
                &[b"a", b"aaa", b"", b"bbb", b"", b"cccccc", b"cccc", b""],
            ),
            (
                &[b"5675;7\x0a", b"a2ZZ;1", b"\x0d\x0d", b"309", b"x", b"70Z"],
                &[b"", b"a2ZZ;1", b"\x0d\x0d", b"309", b"x", b"70Z"],
            ),
            (
                &[
                    b"1\x0d\x0aa\x0d\x0a",
                    b"a\x0d\x0ab",
                    b"bbbbbbbb",
                    b"b\x0d\x0a",
                    b"4",
                    b"\x0d\x0acccc",
                    b"\x0a1;",
                    b"e=1",
                    b"\x0d\x0ad\x0d\x0a",
                ],
                &[b"a", b"b", b"bbbbbbbb", b"b", b"", b"cccc", b"", b"", b"d"],
            ),
            (
                &[
                    b"\x0d",
                    b"1",
                    b"65\x0a\x0a2",
                    b"\x0a\x0a",
                    b"2773F\x0d9",
                    b"\x0d",
                    b"99Z",
                    b"x",
                ],
                &[
                    b"\x0d",
                    b"1",
                    b"65\x0a\x0a2",
                    b"\x0a\x0a",
                    b"2773F\x0d9",
                    b"\x0d",
                    b"99Z",
                    b"x",
                ],
            ),
            (
                &[
                    b"8\x0aaaa",
                    b"a",
                    b"aaaa\x0d\x0a6\x0a",
                    b"bbbbbb\x0d\x0a",
                    b"5;e=1",
                    b"\x0d\x0accccc\x0a",
                    b"c\x0d\x0a",
                    b"ddd",
                    b"d",
                    b"ddddddd",
                    b"d\x0d\x0a",
                ],
                &[
                    b"aaa", b"a", b"aaaa", b"bbbbbb", b"", b"ccccc", b"", b"ddd", b"d", b"ddddddd",
                    b"d",
                ],
            ),
            (&[b"\x0a92x3Z7"], &[b"\x0a92x3Z7"]),
            (&[b"Q"], &[b"Q"]),
            (
                &[b"4\x0aZ4\x0aaa", b"4\x0d", b";\x0aa\x0d3x;8", b"5F4F"],
                &[b"Z4\x0aaa", b"4\x0d", b";\x0aa\x0d3x;8", b"5F4F"],
            ),
            (
                &[
                    b"2\x0aaa\x0d\x0a9",
                    b"\x0d\x0ab",
                    b"bbbbbb",
                    b"bb\x0d\x0a6\x0d\x0ac",
                    b"ccccc\x0d\x0a",
                ],
                &[b"aa", b"b", b"bbbbbb", b"bbc", b"ccccc"],
            ),
            (
                &[
                    b"Z81\x0axFa",
                    b"9aFx\x0d\x0d1",
                    b"x1x50F9",
                    b"1;\x0a",
                    b"9686x6",
                ],
                &[
                    b"Z81\x0axFa",
                    b"9aFx\x0d\x0d1",
                    b"x1x50F9",
                    b"1;\x0a",
                    b"9686x6",
                ],
            ),
            (
                &[
                    b"2\x0d\x0aaa\x0d",
                    b"\x0ac;",
                    b"e=1\x0d",
                    b"\x0abbbbbbb",
                    b"bbbb",
                    b"b\x0d\x0a6\x0d",
                    b"\x0a",
                    b"cccccc",
                    b"\x0d\x0a",
                ],
                &[
                    b"aa", b"", b"", b"bbbbbbb", b"bbbb", b"b", b"", b"cccccc", b"",
                ],
            ),
            (
                &[
                    b"265",
                    b"0a\x0a2",
                    b"\x0d5\x0a\x0a8",
                    b"a",
                    b"09\x0d",
                    b"x\x0a",
                    b"36x4",
                    b"\x0aa",
                    b"F\x0dFx2;",
                ],
                &[
                    b"",
                    b"2",
                    b"\x0d5\x0a\x0a8",
                    b"a",
                    b"09\x0d",
                    b"x\x0a",
                    b"36x4",
                    b"\x0aa",
                    b"F\x0dFx2;",
                ],
            ),
            (
                &[
                    b"7",
                    b";e=",
                    b"1\x0d\x0aaaaa",
                    b"aaa\x0d\x0a",
                    b"c\x0d",
                    b"\x0abbbbbbb",
                    b"b",
                    b"bbb",
                    b"b",
                    b"\x0d\x0a",
                ],
                &[
                    b"", b"", b"aaaa", b"aaa", b"", b"bbbbbbb", b"b", b"bbb", b"b", b"",
                ],
            ),
            (
                &[
                    b"83066\x0d\x0d",
                    b";\x0a3\x0d\x0a84",
                    b"36;\x0d6a\x0a",
                    b"\x0a27\x0dF",
                    b"F",
                ],
                &[
                    b"\x0d",
                    b";\x0a3\x0d\x0a84",
                    b"36;\x0d6a\x0a",
                    b"\x0a27\x0dF",
                    b"F",
                ],
            ),
            (&[], &[]),
            (
                &[
                    b"xF09F",
                    b"0\x0d0a6",
                    b"25\x0a2\x0a\x0a",
                    b"\x0d\x0d",
                    b"8;Z936x\x0d",
                    b"\x0a",
                    b"x8",
                    b"7",
                    b"809",
                    b"1",
                ],
                &[
                    b"xF09F",
                    b"0\x0d0a6",
                    b"25\x0a2\x0a\x0a",
                    b"\x0d\x0d",
                    b"8;Z936x\x0d",
                    b"\x0a",
                    b"x8",
                    b"7",
                    b"809",
                    b"1",
                ],
            ),
            (
                &[
                    b"7;e=1",
                    b"\x0d\x0a",
                    b"aaaa",
                    b"aaa\x0a8;",
                    b"e=1\x0d\x0abb",
                    b"bbbbbb",
                    b"\x0a",
                    b"9;e=1",
                    b"\x0d\x0accccc",
                    b"cccc\x0d\x0a0\x0d",
                    b"\x0aT: x",
                    b"\x0d\x0a\x0d\x0a",
                    b"more",
                ],
                &[
                    b"", b"", b"aaaa", b"aaa", b"bb", b"bbbbbb", b"", b"", b"ccccc", b"cccc", b"",
                    b"", b"",
                ],
            ),
            (
                &[b"80\x0a40\x0d60", b"F0Fx\x0a3\x0d", b"5", b"4"],
                &[b"40\x0d60", b"F0Fx\x0a3\x0d", b"5", b"4"],
            ),
            (
                &[
                    b"7\x0d\x0aa",
                    b"aaaaaa\x0d\x0a",
                    b"c",
                    b"\x0d\x0abbb",
                    b"bbbbbbbb",
                    b"b\x0d\x0a1",
                    b"\x0d\x0ac",
                    b"\x0d\x0a",
                ],
                &[b"a", b"aaaaaa", b"", b"bbb", b"bbbbbbbb", b"b", b"c", b""],
            ),
            (
                &[
                    b"Z31",
                    b"9;\x0a",
                    b"6\x0a5\x0d30",
                    b"Z\x0a64",
                    b"\x0a5",
                    b";1\x0a",
                ],
                &[
                    b"Z31",
                    b"9;\x0a",
                    b"6\x0a5\x0d30",
                    b"Z\x0a64",
                    b"\x0a5",
                    b";1\x0a",
                ],
            ),
            (
                &[
                    b"9",
                    b";e=1\x0d\x0a",
                    b"aaaaa",
                    b"aa",
                    b"aa\x0d",
                    b"\x0a1\x0ab\x0aa\x0ac",
                    b"ccc",
                    b"ccccc",
                    b"c\x0a",
                ],
                &[
                    b"", b"", b"aaaaa", b"aa", b"aa", b"bc", b"ccc", b"ccccc", b"c",
                ],
            ),
            (
                &[b"x\x0a;F", b"\x0a\x0d", b"4\x0a\x0d0a", b"1", b"9\x0a3;"],
                &[b"x\x0a;F", b"\x0a\x0d", b"4\x0a\x0d0a", b"1", b"9\x0a3;"],
            ),
            (&[b"4\x0d\x0aaa", b"aa", b"\x0d\x0a"], &[b"aa", b"aa", b""]),
            (
                &[b"a", b"\x0d4619a9", b"Z\x0d\x0a;", b"\x0d4"],
                &[b"", b"4619a9", b"Z\x0d\x0a;", b"\x0d4"],
            ),
            (
                &[
                    b"b\x0d\x0aa",
                    b"aaaaaaa",
                    b"aaa\x0d\x0a5",
                    b"\x0abbbbb",
                    b"\x0d",
                    b"\x0a6\x0d\x0acccc",
                    b"cc\x0d\x0a0\x0d\x0a",
                    b"T",
                    b": x",
                    b"\x0d\x0a\x0d\x0am",
                    b"o",
                    b"re",
                ],
                &[
                    b"a", b"aaaaaaa", b"aaa", b"bbbbb", b"", b"cccc", b"cc", b"", b"", b"", b"",
                    b"",
                ],
            ),
            (
                &[
                    b"\x0d\x0dZ\x0a\x0d",
                    b"8634",
                    b"1",
                    b"558\x0d6\x0a2",
                    b"\x0d6a",
                    b"3",
                ],
                &[
                    b"\x0d\x0dZ\x0a\x0d",
                    b"8634",
                    b"1",
                    b"558\x0d6\x0a2",
                    b"\x0d6a",
                    b"3",
                ],
            ),
            (
                &[b"0\x0d\x0aT", b": x", b"\x0d\x0a\x0d\x0a", b"mo", b"re"],
                &[b"", b"", b"", b"", b""],
            ),
            (
                &[
                    b"0\x0dF\x0d\x0d",
                    b"Z733Z",
                    b"0",
                    b"a75311",
                    b"96;",
                    b"\x0a2\x0aZ0x72",
                    b"6x877\x0d",
                    b";",
                ],
                &[
                    b"F\x0d\x0d",
                    b"Z733Z",
                    b"0",
                    b"a75311",
                    b"96;",
                    b"\x0a2\x0aZ0x72",
                    b"6x877\x0d",
                    b";",
                ],
            ),
            (
                &[
                    b"b\x0d\x0aaaaaa",
                    b"aaaaaa",
                    b"\x0d",
                    b"\x0a4\x0d\x0ab",
                    b"bbb",
                    b"\x0d\x0a9\x0acccc",
                    b"ccccc",
                    b"\x0d\x0a9",
                    b"\x0addddQ",
                    b"d\x0d\x0a0\x0d\x0a",
                    b"T: x\x0d\x0a",
                    b"\x0d\x0amore",
                ],
                &[
                    b"aaaaa",
                    b"aaaaaa",
                    b"",
                    b"b",
                    b"bbb",
                    b"cccc",
                    b"ccccc",
                    b"",
                    b"ddddQ",
                    b"d\x0d\x0a0",
                    b"T: x\x0d\x0a",
                    b"\x0d\x0amore",
                ],
            ),
            (&[b"3"], &[b""]),
            (
                &[
                    b"4\x0d\x0aa",
                    b"aaa\x0d\x0a",
                    b"0",
                    b"\x0d\x0aT",
                    b": Q ",
                    b"x",
                    b"\x0d\x0a\x0d\x0amo",
                    b"re",
                ],
                &[b"a", b"aaa", b"", b"", b"", b"", b"", b""],
            ),
            (&[b"\x0d4\x0a;Z7"], &[b"\x0d4\x0a;Z7"]),
            (
                &[b"0\x0d\x0aT", b": x\x0d\x0a\x0d\x0a", b"more"],
                &[b"", b"", b""],
            ),
            (
                &[
                    b"6",
                    b"05F",
                    b"\x0da",
                    b"F",
                    b"48",
                    b"Z7",
                    b"08a1",
                    b"6a3Zx964",
                    b"499;",
                    b"\x0d98\x0a;;48",
                    b"50",
                ],
                &[
                    b"",
                    b"",
                    b"a",
                    b"F",
                    b"48",
                    b"Z7",
                    b"08a1",
                    b"6a3Zx964",
                    b"499;",
                    b"\x0d98\x0a;;48",
                    b"50",
                ],
            ),
            (
                &[
                    b"a\x0d\x0aaaaa",
                    b"aa",
                    b"aa",
                    b"aa",
                    b"\x0d\x0a0",
                    b"\x0d\x0aT: x\x0d\x0a",
                    b"\x0d\x0a",
                    b"more",
                ],
                &[b"aaaa", b"aa", b"aa", b"aa", b"", b"", b"", b""],
            ),
            (
                &[
                    b"ZFx5\x0a788",
                    b";4;3",
                    b"71\x0d\x0a",
                    b"\x0d46",
                    b"a\x0a3\x0a",
                    b"3\x0a4",
                    b"3;69Z",
                    b"1",
                ],
                &[
                    b"ZFx5\x0a788",
                    b";4;3",
                    b"71\x0d\x0a",
                    b"\x0d46",
                    b"a\x0a3\x0a",
                    b"3\x0a4",
                    b"3;69Z",
                    b"1",
                ],
            ),
            (
                &[
                    b"1\x0d\x0aa\x0d\x0aa;",
                    b"e=1",
                    b"\x0d\x0ab",
                    b"bbbbb",
                    b"bbbb",
                    b"\x0d\x0a",
                ],
                &[b"a", b"", b"b", b"bbbbb", b"bbbb", b""],
            ),
            (
                &[
                    b"FaZ",
                    b"\x0daZ",
                    b"15a5\x0a1Z0",
                    b"x03\x0ax6;",
                    b"5a",
                    b"8",
                ],
                &[b"", b"aZ", b"15a5\x0a1Z0", b"x03\x0ax6;", b"5a", b"8"],
            ),
            (
                &[
                    b"1\x0d\x0a",
                    b"a\x0d\x0ab;e=1",
                    b"\x0d\x0abbbb",
                    b"bbbb",
                    b"bbb\x0d\x0ac\x0d\x0a",
                    b"cccc",
                    b"cc",
                    b"cccccc",
                    b"\x0a",
                ],
                &[
                    b"", b"a", b"bbbb", b"bbbb", b"bbb", b"cccc", b"cc", b"cccccc", b"",
                ],
            ),
            (
                &[
                    b"\x0d6",
                    b"9x\x0a5\x0d",
                    b"a\x0a2xF40",
                    b"F",
                    b"\x0a3",
                    b"88\x0a35\x0d26",
                ],
                &[
                    b"\x0d6",
                    b"9x\x0a5\x0d",
                    b"a\x0a2xF40",
                    b"F",
                    b"\x0a3",
                    b"88\x0a35\x0d26",
                ],
            ),
            (
                &[
                    b"7\x0d\x0a",
                    b"a",
                    b"aaaaaa\x0d",
                    b"\x0a0",
                    b"\x0d\x0aT: x\x0d",
                    b"\x0a",
                    b"\x0d\x0amore",
                ],
                &[b"", b"a", b"aaaaaa", b"", b"", b"", b""],
            ),
            (
                &[
                    b"915",
                    b"4x38",
                    b"F",
                    b"1",
                    b"2\x0a7\x0ax;",
                    b"x3017;",
                    b"x\x0d57325",
                    b"15",
                    b"5\x0d",
                    b"5\x0d7;17",
                    b"7",
                    b"0",
                ],
                &[
                    b"",
                    b"",
                    b"",
                    b"",
                    b"7\x0ax;",
                    b"x3017;",
                    b"x\x0d57325",
                    b"15",
                    b"5\x0d",
                    b"5\x0d7;17",
                    b"7",
                    b"0",
                ],
            ),
            (&[b"Q"], &[b"Q"]),
            (
                &[b"F8", b"5a2a0ZF", b"\x0d18", b"3\x0a82F\x0d3"],
                &[b"", b"", b"18", b"3\x0a82F\x0d3"],
            ),
            (
                &[
                    b"7",
                    b"\x0d\x0aaaa",
                    b"a",
                    b"a",
                    b"a",
                    b"a\x0d",
                    b"\x0ab\x0d\x0abbbb",
                    b"bb",
                    b"bbb",
                    b"b",
                    b"b\x0d\x0ab\x0d\x0ac",
                    b"cc",
                    b"cccc",
                    b"cccc\x0d\x0a8\x0d",
                    b"\x0add",
                    b"dd",
                    b"ddd",
                    b"d\x0a",
                ],
                &[
                    b"", b"aaa", b"a", b"a", b"a", b"a", b"bbbb", b"bb", b"bbb", b"b", b"bc",
                    b"cc", b"cccc", b"cccc", b"dd", b"dd", b"ddd", b"d",
                ],
            ),
            (
                &[
                    b"\x0a",
                    b"\x0d;7x",
                    b"\x0dx\x0d99",
                    b"x71",
                    b"aa070;",
                    b"92",
                    b"0F;\x0d",
                    b"x",
                    b"8\x0dxFF8",
                    b"0;;",
                ],
                &[
                    b"\x0a",
                    b"\x0d;7x",
                    b"\x0dx\x0d99",
                    b"x71",
                    b"aa070;",
                    b"92",
                    b"0F;\x0d",
                    b"x",
                    b"8\x0dxFF8",
                    b"0;;",
                ],
            ),
            (
                &[
                    b"6\x0aaaaaa",
                    b"a\x0d\x0a8\x0d\x0abb",
                    b"bb",
                    b"bbbb\x0d\x0a",
                    b"0\x0d\x0aT: ",
                    b"x\x0d\x0a\x0d",
                    b"\x0amore",
                ],
                &[b"aaaaa", b"abb", b"bb", b"bbbb", b"", b"", b""],
            ),
            (
                &[
                    b"40\x0d;607\x0a",
                    b"x",
                    b"\x0a;\x0d;a202",
                    b"xF476\x0d",
                    b"\x0a7\x0a",
                    b"711",
                ],
                &[
                    b";607\x0a",
                    b"x",
                    b"\x0a;\x0d;a202",
                    b"xF476\x0d",
                    b"\x0a7\x0a",
                    b"711",
                ],
            ),
            (
                &[
                    b"c\x0d\x0a",
                    b"aaaaaa",
                    b"aaaaaa",
                    b"\x0d\x0a7;e",
                    b"=1\x0d\x0abbb",
                    b"bbbb\x0d\x0a",
                    b"b;e=1",
                    b"\x0d\x0acccc",
                    b"cc",
                    b"ccccc\x0d\x0a1",
                    b";e=1\x0d",
                    b"\x0ad\x0d\x0a0",
                    b"\x0d\x0aT: ",
                    b"x\x0d\x0a\x0d\x0amo",
                    b"re",
                ],
                &[
                    b"", b"aaaaaa", b"aaaaaa", b"", b"bbb", b"bbbb", b"", b"cccc", b"cc", b"ccccc",
                    b"", b"d", b"", b"", b"",
                ],
            ),
            (
                &[
                    b"ZF6",
                    b"\x0dZ0a",
                    b"\x0d6Z52606",
                    b"4\x0dZ9\x0a3",
                    b"a79x85\x0d",
                    b"\x0d",
                    b"2",
                    b"1;FZ1",
                ],
                &[
                    b"ZF6",
                    b"\x0dZ0a",
                    b"\x0d6Z52606",
                    b"4\x0dZ9\x0a3",
                    b"a79x85\x0d",
                    b"\x0d",
                    b"2",
                    b"1;FZ1",
                ],
            ),
            (
                &[
                    b"4\x0d\x0aaaaa",
                    b"\x0d\x0a0\x0d\x0a",
                    b"T: x",
                    b"\x0d\x0a",
                    b"\x0d\x0aQre",
                ],
                &[b"aaaa", b"", b"", b"", b""],
            ),
            (
                &[b"02", b"6\x0d7\x0a", b"86a", b"3F\x0aa4F\x0a4", b"2x"],
                &[b"", b"7\x0a", b"86a", b"3F\x0aa4F\x0a4", b"2x"],
            ),
            (
                &[
                    b"b\x0d\x0aaaaaa",
                    b"aaaa",
                    b"aa",
                    b"\x0d\x0ab\x0d",
                    b"\x0a",
                    b"bb",
                    b"b",
                    b"bbbbb",
                    b"bbb\x0d\x0a5;",
                    b"e=1\x0d",
                    b"\x0accccc\x0d\x0a",
                ],
                &[
                    b"aaaaa", b"aaaa", b"aa", b"", b"", b"bb", b"b", b"bbbbb", b"bbb", b"",
                    b"ccccc",
                ],
            ),
            (
                &[
                    b"a9",
                    b"x381",
                    b"\x0dFZ2a\x0d",
                    b"8\x0d\x0a3\x0dx",
                    b";07\x0a92\x0a3",
                    b"5a",
                    b"\x0d",
                    b"\x0a\x0d3302",
                    b"213",
                ],
                &[
                    b"",
                    b"",
                    b"FZ2a\x0d",
                    b"8\x0d\x0a3\x0dx",
                    b";07\x0a92\x0a3",
                    b"5a",
                    b"\x0d",
                    b"\x0a\x0d3302",
                    b"213",
                ],
            ),
            (
                &[
                    b"5\x0a",
                    b"aaaaa\x0d",
                    b"\x0ac;e",
                    b"=1",
                    b"\x0d\x0abbb",
                    b"bb",
                    b"bb",
                    b"bbbbb",
                    b"\x0d\x0a",
                    b"2;e=1\x0d\x0a",
                    b"cc\x0d\x0a",
                ],
                &[
                    b"", b"aaaaa", b"", b"", b"bbb", b"bb", b"bb", b"bbbbb", b"", b"", b"cc",
                ],
            ),
            (
                &[
                    b"8\x0d7Z",
                    b";75\x0a",
                    b"1",
                    b"x988",
                    b"36\x0a",
                    b";25\x0d3",
                    b"\x0a4\x0a055",
                    b"8",
                ],
                &[
                    b"7Z",
                    b";75\x0a",
                    b"1",
                    b"x988",
                    b"36\x0a",
                    b";25\x0d3",
                    b"\x0a4\x0a055",
                    b"8",
                ],
            ),
            (
                &[
                    b"1\x0d\x0aa\x0d\x0a",
                    b"0\x0d",
                    b"\x0aT",
                    b": ",
                    b"x\x0d",
                    b"\x0a\x0d\x0a",
                    b"mo",
                    b"re",
                ],
                &[b"a", b"", b"", b"", b"", b"", b"", b""],
            ),
            (&[b"8"], &[b""]),
            (
                &[b"6\x0d", b"\x0aaaaaaaQ", b"aaaa\x0d", b"\x0a"],
                &[b"", b"aaaaaaQ", b"aaaa\x0d", b"\x0a"],
            ),
            (
                &[b"\x0aFx6\x0d", b"52\x0d;4", b"7xF;481", b"3194"],
                &[b"\x0aFx6\x0d", b"52\x0d;4", b"7xF;481", b"3194"],
            ),
            (
                &[
                    b"9\x0d\x0aa",
                    b"aaaaaaaa",
                    b"\x0a4\x0d\x0abbbb",
                    b"\x0d\x0ab\x0acc",
                    b"cc",
                    b"ccccccc\x0a",
                    b"1\x0a",
                    b"d\x0d",
                    b"\x0a0\x0d\x0aT:",
                    b" x\x0d",
                    b"\x0a\x0d\x0am",
                    b"ore",
                ],
                &[
                    b"a",
                    b"aaaaaaaa",
                    b"bbbb",
                    b"cc",
                    b"cc",
                    b"ccccccc",
                    b"",
                    b"d",
                    b"",
                    b"",
                    b"",
                    b"",
                ],
            ),
            (
                &[b"x", b"9", b"88\x0d06\x0a", b"a5;a", b"1;3"],
                &[b"x", b"9", b"88\x0d06\x0a", b"a5;a", b"1;3"],
            ),
            (
                &[
                    b"0\x0d\x0a",
                    b"T:",
                    b" x\x0d\x0a\x0d",
                    b"\x0amo",
                    b"rQ\x0d\x0amor",
                    b"e",
                ],
                &[b"", b"", b"", b"", b"", b""],
            ),
            (
                &[
                    b"\x0aZ1x",
                    b"x2\x0a2xF",
                    b"18Z88\x0d",
                    b"\x0a8\x0d\x0d0\x0a",
                ],
                &[
                    b"\x0aZ1x",
                    b"x2\x0a2xF",
                    b"18Z88\x0d",
                    b"\x0a8\x0d\x0d0\x0a",
                ],
            ),
            (
                &[
                    b"8\x0d\x0aaaa",
                    b"aaaaa\x0a",
                    b"2;e=1\x0d\x0ab",
                    b"b",
                    b"\x0ab\x0d\x0a",
                    b"cccccc",
                    b"ccccc\x0d\x0a2",
                    b"\x0d\x0add\x0a",
                    b"0\x0d\x0a",
                    b"T: x\x0d",
                    b"\x0a\x0d\x0a",
                    b"m",
                    b"ore",
                ],
                &[
                    b"aaa", b"aaaaa", b"b", b"b", b"", b"cccccc", b"ccccc", b"dd", b"", b"", b"",
                    b"", b"",
                ],
            ),
            (
                &[
                    b"5\x0d\x0dFx;x8",
                    b"\x0a",
                    b"\x0aFZx\x0a",
                    b"0Z1208\x0a",
                    b"2",
                ],
                &[b"\x0dFx;x8", b"\x0a", b"\x0aFZx\x0a", b"0Z1208\x0a", b"2"],
            ),
            (&[b"7\x0d\x0aa", b"aaaaaa\x0a"], &[b"a", b"aaaaaa"]),
            (
                &[b"4\x0d37;7", b"57", b"53267812", b";54"],
                &[b"37;7", b"57", b"53267812", b";54"],
            ),
            (
                &[
                    b"9",
                    b"\x0d\x0aaaa",
                    b"aaaaaa",
                    b"\x0d\x0aa\x0d\x0abb",
                    b"bbbbbbbb",
                    b"\x0d\x0ac\x0d\x0accc",
                    b"c",
                    b"cccccc",
                    b"cc\x0d\x0a",
                ],
                &[
                    b"",
                    b"aaa",
                    b"aaaaaa",
                    b"bb",
                    b"bbbbbbbb",
                    b"ccc",
                    b"c",
                    b"cccccc",
                    b"cc",
                ],
            ),
            (
                &[b"\x0a1a", b"715", b"\x0dx\x0a\x0aa", b"ZZ", b"8;9"],
                &[b"\x0a1a", b"715", b"\x0dx\x0a\x0aa", b"ZZ", b"8;9"],
            ),
            (
                &[
                    b"7\x0d\x0aaa",
                    b"aaaaa\x0a",
                    b"3;e=1",
                    b"\x0d\x0abbb\x0d",
                    b"\x0a9",
                    b";e=1",
                    b"\x0d\x0accccc",
                    b"c",
                    b"cc",
                    b"c\x0d\x0a",
                ],
                &[
                    b"aa", b"aaaaa", b"", b"bbb", b"", b"", b"ccccc", b"c", b"cc", b"c",
                ],
            ),
            (
                &[b"1\x0a", b"\x0d0", b"504;xZ"],
                &[b"", b"\x0d0", b"504;xZ"],
            ),
            (
                &[
                    b"6Q\x0aaa",
                    b"aaaa\x0d\x0a3;",
                    b"e=1\x0d\x0abb",
                    b"b",
                    b"\x0d\x0a0\x0d\x0aT",
                    b": ",
                    b"x\x0d",
                    b"\x0a\x0d\x0a",
                    b"more",
                ],
                &[b"aa", b"aaaa", b"bb", b"b", b"", b"", b"", b"", b""],
            ),
            (&[b"x7"], &[b"x7"]),
            (
                &[
                    b"8",
                    b";e",
                    b"=",
                    b"1\x0d\x0aaaaaa",
                    b"a",
                    b"aa\x0d",
                    b"\x0a0\x0d\x0aT: ",
                    b"x\x0d\x0a",
                    b"\x0d\x0amore",
                ],
                &[b"", b"", b"", b"aaaaa", b"a", b"aa", b"", b"", b""],
            ),
            (
                &[b"xZ\x0d1Fa4", b"\x0d", b"\x0d4\x0d90", b"51\x0a3", b"42"],
                &[b"xZ\x0d1Fa4", b"\x0d", b"\x0d4\x0d90", b"51\x0a3", b"42"],
            ),
            (&[], &[]),
            (&[b"5", b"0\x0d0", b"910;"], &[b"", b"0", b"910;"]),
            (
                &[b"0\x0d\x0aT: x\x0d", b"\x0a", b"\x0d\x0amor", b"e"],
                &[b"", b"", b"", b""],
            ),
            (
                &[
                    b"Z\x0d55",
                    b"ZZa6\x0aFa",
                    b"3;6",
                    b"7",
                    b";x8",
                    b"x\x0d",
                    b"22a\x0d\x0d",
                    b"3;F1",
                    b"3\x0aF394",
                    b"1",
                    b"7",
                ],
                &[
                    b"Z\x0d55",
                    b"ZZa6\x0aFa",
                    b"3;6",
                    b"7",
                    b";x8",
                    b"x\x0d",
                    b"22a\x0d\x0d",
                    b"3;F1",
                    b"3\x0aF394",
                    b"1",
                    b"7",
                ],
            ),
            (
                &[b"0", b"\x0d\x0aT:", b" x", b"\x0d\x0a\x0d\x0am", b"ore"],
                &[b"", b"", b"", b"", b""],
            ),
            (
                &[
                    b"3\x0a\x0d5\x0d",
                    b"52F\x0d;",
                    b"31;52\x0a",
                    b"3;06\x0a5",
                    b"\x0a;216F",
                    b";9",
                ],
                &[
                    b"\x0d5\x0d",
                    b"52F\x0d;",
                    b"31;52\x0a",
                    b"3;06\x0a5",
                    b"\x0a;216F",
                    b";9",
                ],
            ),
            (
                &[
                    b"b",
                    b"\x0d",
                    b"\x0aaaaaa",
                    b"aaa",
                    b"aaa\x0d\x0a",
                    b"5",
                    b"\x0d\x0ab",
                    b"bbb",
                    b"b",
                    b"\x0d\x0a",
                    b"c\x0d\x0ac",
                    b"cc",
                    b"ccccccc",
                    b"c",
                    b"c",
                    b"\x0d\x0a9\x0d\x0ad",
                    b"dd",
                    b"dd",
                    b"d",
                    b"ddd\x0d\x0a0",
                    b"\x0d\x0aT",
                    b": x\x0d\x0a",
                    b"\x0d\x0amore",
                ],
                &[
                    b"", b"", b"aaaaa", b"aaa", b"aaa", b"", b"b", b"bbb", b"b", b"", b"c", b"cc",
                    b"ccccccc", b"c", b"c", b"d", b"dd", b"dd", b"d", b"ddd", b"", b"", b"",
                ],
            ),
            (
                &[
                    b"8",
                    b";a5\x0a698",
                    b"\x0d\x0dZ",
                    b"x1Z",
                    b";",
                    b"4;\x0d",
                    b"08a",
                    b"\x0a",
                    b"\x0aaF5a951",
                    b";416x58;",
                    b"x",
                ],
                &[
                    b"",
                    b"698",
                    b"\x0d\x0dZ",
                    b"x1Z",
                    b";",
                    b"4;\x0d",
                    b"08a",
                    b"\x0a",
                    b"\x0aaF5a951",
                    b";416x58;",
                    b"x",
                ],
            ),
            (
                &[
                    b"0\x0d",
                    b"\x0aT: x",
                    b"\x0d\x0a\x0d",
                    b"\x0am",
                    b"or",
                    b"e",
                ],
                &[b"", b"", b"", b"", b"", b""],
            ),
            (
                &[
                    b"87",
                    b"a27",
                    b"a46",
                    b"\x0d74743;x",
                    b"\x0a9a",
                    b"a0\x0d\x0d",
                    b"9",
                    b"1x\x0a\x0a4x",
                ],
                &[
                    b"",
                    b"",
                    b"",
                    b"74743;x",
                    b"\x0a9a",
                    b"a0\x0d\x0d",
                    b"9",
                    b"1x\x0a\x0a4x",
                ],
            ),
            (
                &[
                    b"7",
                    b";e=1\x0d",
                    b"\x0a",
                    b"aaaaaaa",
                    b"\x0d\x0a8\x0abbb",
                    b"bb",
                    b"b",
                    b"bb\x0d\x0a",
                ],
                &[b"", b"", b"", b"aaaaaaa", b"bbb", b"bb", b"b", b"bb"],
            ),
            (
                &[b"\x0d;04\x0d0\x0d", b"57", b"468a", b"\x0a1"],
                &[b"\x0d;04\x0d0\x0d", b"57", b"468a", b"\x0a1"],
            ),
            (
                &[
                    b"3\x0d\x0aaaa\x0a",
                    b"5;e=1\x0d",
                    b"\x0abQ",
                    b"a\x0a5",
                    b";e=1",
                    b"\x0d\x0a",
                    b"bbbbb",
                    b"\x0a",
                ],
                &[
                    b"aaa",
                    b"",
                    b"bQ",
                    b"a\x0a5",
                    b";e=1",
                    b"\x0d\x0a",
                    b"bbbbb",
                    b"\x0a",
                ],
            ),
            (
                &[
                    b"72;5Z\x0d1",
                    b"\x0d19",
                    b"81\x0d",
                    b"\x0aZ",
                    b"228",
                    b"9",
                    b"\x0d45a2",
                ],
                &[
                    b"1",
                    b"\x0d19",
                    b"81\x0d",
                    b"\x0aZ",
                    b"228",
                    b"9",
                    b"\x0d45a2",
                ],
            ),
            (
                &[
                    b"7\x0d\x0aaaa",
                    b"aaaa\x0d",
                    b"\x0a9\x0abbbb",
                    b"bbbbb\x0d",
                    b"\x0aa\x0acccc",
                    b"ccc",
                    b"ccc\x0d",
                    b"\x0a2;e=1",
                    b"\x0d\x0add",
                    b"\x0d\x0a0\x0d\x0aT",
                    b": x",
                    b"\x0d\x0a\x0d\x0amo",
                    b"re",
                ],
                &[
                    b"aaa", b"aaaa", b"bbbb", b"bbbbb", b"cccc", b"ccc", b"ccc", b"", b"dd", b"",
                    b"", b"", b"",
                ],
            ),
            (&[b"4x"], &[b""]),
            (
                &[b"9\x0d\x0aaaaa", b"aa", b"aaa", b"\x0d\x0a"],
                &[b"aaaa", b"aa", b"aaa", b""],
            ),
            (
                &[b"Z6;\x0dxF3", b"\x0a0", b"7;x", b"\x0da8\x0d\x0a"],
                &[b"Z6;\x0dxF3", b"\x0a0", b"7;x", b"\x0da8\x0d\x0a"],
            ),
            (&[], &[]),
            (
                &[
                    b"xa22a\x0d8",
                    b"1044",
                    b"0Z;38\x0d",
                    b"7934\x0dFx1",
                    b"93",
                    b"\x0d0\x0a\x0axx",
                    b"a\x0d",
                ],
                &[
                    b"xa22a\x0d8",
                    b"1044",
                    b"0Z;38\x0d",
                    b"7934\x0dFx1",
                    b"93",
                    b"\x0d0\x0a\x0axx",
                    b"a\x0d",
                ],
            ),
            (
                &[b"0\x0d\x0aT: x", b"\x0d\x0a\x0d\x0am", b"ore"],
                &[b"", b"", b""],
            ),
            (
                &[
                    b"394F998",
                    b"0\x0a",
                    b"21F503",
                    b"9Z\x0d17\x0d",
                    b"1",
                    b"\x0d",
                    b"00",
                ],
                &[b"", b"", b"21F503", b"9Z\x0d17\x0d", b"1", b"\x0d", b"00"],
            ),
            (
                &[
                    b"5",
                    b"\x0d\x0aaaaaa\x0a",
                    b"a",
                    b"\x0d\x0abbbb",
                    b"bbbbb",
                    b"b\x0d\x0a0\x0d\x0aT:",
                    b" x\x0d",
                    b"\x0a",
                    b"\x0d\x0a",
                    b"mor",
                    b"e",
                ],
                &[
                    b"", b"aaaaa", b"", b"bbbb", b"bbbbb", b"b", b"", b"", b"", b"", b"",
                ],
            ),
            (&[b";6Zx;5"], &[b";6Zx;5"]),
            (
                &[b"7;", b"e=Q", b"\x0a", b"aaaaaa", b"a\x0d\x0a"],
                &[b"", b"", b"", b"aaaaaa", b"a"],
            ),
            (&[b"53x;53"], &[b""]),
            (&[], &[]),
            (
                &[b"74\x0a", b"470\x0a2", b"5;8;\x0d\x0d"],
                &[b"", b"470\x0a2", b"5;8;\x0d\x0d"],
            ),
            (
                &[
                    b"2\x0aaa\x0d\x0a4\x0a",
                    b"bbQ",
                    b"a\x0d\x0a4",
                    b"\x0abbbb\x0d\x0a",
                ],
                &[b"aa", b"bbQ", b"a", b"bbbb"],
            ),
            (
                &[b"30;9\x0aZ3", b"F\x0a7", b"a\x0dF5\x0d67", b"14F3aF3"],
                &[b"Z3", b"F\x0a7", b"a\x0dF5\x0d67", b"14F3aF3"],
            ),
            (
                &[
                    b"8",
                    b"\x0aa",
                    b"aaaaaa",
                    b"a\x0d",
                    b"\x0a0\x0d\x0aT: ",
                    b"x\x0d\x0a\x0d\x0amor",
                    b"e",
                ],
                &[b"", b"a", b"aaaaaa", b"a", b"", b"", b""],
            ),
            (
                &[
                    b"ZZZ12",
                    b"963",
                    b"aF\x0a",
                    b";76",
                    b"\x0d\x0d",
                    b"028\x0a",
                    b"\x0a78",
                    b"0682",
                    b"x",
                ],
                &[
                    b"ZZZ12",
                    b"963",
                    b"aF\x0a",
                    b";76",
                    b"\x0d\x0d",
                    b"028\x0a",
                    b"\x0a78",
                    b"0682",
                    b"x",
                ],
            ),
            (
                &[
                    b"a\x0d\x0aa",
                    b"aaaaa",
                    b"aaaa",
                    b"\x0d\x0a6;e=",
                    b"1\x0d\x0abbbb",
                    b"bb\x0d\x0aa\x0d\x0ac",
                    b"c",
                    b"c",
                    b"ccccccc",
                    b"\x0d\x0a",
                    b"8\x0d\x0addddd",
                    b"ddd",
                    b"\x0a",
                ],
                &[
                    b"a", b"aaaaa", b"aaaa", b"", b"bbbb", b"bbc", b"c", b"c", b"ccccccc", b"",
                    b"ddddd", b"ddd", b"",
                ],
            ),
            (
                &[
                    b"97\x0dF\x0d7",
                    b"18\x0a1F76",
                    b"801x\x0a;28",
                    b"2x",
                    b"3\x0d;3\x0aZ",
                ],
                &[
                    b"F\x0d7",
                    b"18\x0a1F76",
                    b"801x\x0a;28",
                    b"2x",
                    b"3\x0d;3\x0aZ",
                ],
            ),
            (
                &[
                    b"1\x0d\x0aa\x0d\x0a",
                    b"3\x0d",
                    b"\x0abbb\x0d\x0a0\x0d",
                    b"\x0aT: x\x0d",
                    b"\x0a\x0d\x0amore",
                ],
                &[b"a", b"", b"bbb", b"", b""],
            ),
            (
                &[
                    b"813",
                    b"F9;0\x0d",
                    b"16\x0d4\x0d7F",
                    b"2x\x0d85",
                    b"x0",
                    b"84",
                    b"\x0d",
                    b"15\x0a0",
                    b"x3;Z\x0a",
                ],
                &[
                    b"",
                    b"",
                    b"16\x0d4\x0d7F",
                    b"2x\x0d85",
                    b"x0",
                    b"84",
                    b"\x0d",
                    b"15\x0a0",
                    b"x3;Z\x0a",
                ],
            ),
            (
                &[
                    b"5\x0d\x0aaa",
                    b"aaa\x0d\x0a4;e",
                    b"=",
                    b"1\x0d\x0abb",
                    b"bb\x0d\x0a5;",
                    b"e=1\x0acc",
                    b"ccc",
                    b"\x0a9\x0d\x0addd",
                    b"ddddd",
                    b"d\x0a",
                ],
                &[
                    b"aa", b"aaa", b"", b"bb", b"bb", b"cc", b"ccc", b"ddd", b"ddddd", b"d",
                ],
            ),
            (
                &[
                    b"1F25Z",
                    b"7\x0d",
                    b";Z",
                    b"\x0d",
                    b"4;Z2",
                    b"10802a9x",
                    b"\x0dZ\x0a\x0dx",
                    b"8a91FF54",
                    b"\x0d6\x0d2",
                ],
                &[
                    b"",
                    b"",
                    b";Z",
                    b"\x0d",
                    b"4;Z2",
                    b"10802a9x",
                    b"\x0dZ\x0a\x0dx",
                    b"8a91FF54",
                    b"\x0d6\x0d2",
                ],
            ),
            (
                &[
                    b"c;e",
                    b"=1\x0d\x0a",
                    b"aaaaaaa",
                    b"aaaaa",
                    b"\x0d",
                    b"\x0a8\x0d\x0a",
                    b"bbbb",
                    b"b",
                    b"bbb\x0d",
                    b"\x0ac\x0d\x0ac",
                    b"ccc",
                    b"cc",
                    b"cccccc\x0d",
                    b"\x0a",
                ],
                &[
                    b"", b"", b"aaaaaaa", b"aaaaa", b"", b"", b"bbbb", b"b", b"bbb", b"c", b"ccc",
                    b"cc", b"cccccc", b"",
                ],
            ),
            (
                &[
                    b"62a",
                    b"94a",
                    b"\x0d650",
                    b"9;\x0d8",
                    b"a2\x0a26",
                    b"\x0d\x0d\x0d",
                ],
                &[b"", b"", b"650", b"9;\x0d8", b"a2\x0a26", b"\x0d\x0d\x0d"],
            ),
            (
                &[
                    b"3\x0d\x0aa",
                    b"aa\x0d\x0a",
                    b"1",
                    b";e=",
                    b"1\x0d\x0ab",
                    b"\x0d\x0ac;e=1",
                    b"\x0d\x0accccc",
                    b"ccccccc\x0a",
                    b"9\x0adddd",
                    b"ddddd\x0d\x0a",
                ],
                &[
                    b"a", b"aa", b"", b"", b"b", b"", b"ccccc", b"ccccccc", b"dddd", b"ddddd",
                ],
            ),
            (
                &[
                    b"73\x0a\x0dFa",
                    b"5;",
                    b"x8\x0d6x",
                    b"\x0a0",
                    b";\x0aa6",
                    b";",
                    b"F4x;Z0",
                    b"\x0ax\x0aZ\x0a28",
                    b"Z",
                ],
                &[
                    b"\x0dFa",
                    b"5;",
                    b"x8\x0d6x",
                    b"\x0a0",
                    b";\x0aa6",
                    b";",
                    b"F4x;Z0",
                    b"\x0ax\x0aZ\x0a28",
                    b"Z",
                ],
            ),
            (
                &[
                    b"0",
                    b"\x0d",
                    b"\x0aT: ",
                    b"x",
                    b"\x0d\x0a\x0d\x0am",
                    b"or",
                    b"e",
                ],
                &[b"", b"", b"", b"", b"", b"", b""],
            ),
            (
                &[
                    b"a\x0d\x0a91;",
                    b"\x0aZ\x0d26\x0a",
                    b"68Z0Z;F",
                    b"\x0da97;",
                    b"5x030x",
                    b"6",
                    b"\x0a9",
                    b";Z2x",
                    b"0\x0d",
                ],
                &[
                    b"91;",
                    b"\x0aZ\x0d26\x0a",
                    b"68Z0Z;F",
                    b"\x0da97;",
                    b"5x030x",
                    b"6",
                    b"\x0a9",
                    b";Z2x",
                    b"0\x0d",
                ],
            ),
            (
                &[b"0\x0d\x0aT", b": x\x0d\x0a\x0d", b"\x0am", b"o", b"re"],
                &[b"", b"", b"", b"", b""],
            ),
            (
                &[
                    b"4x41",
                    b"1",
                    b"5;91aF",
                    b";1\x0dx\x0d0Z",
                    b"6",
                    b"\x0a;80F",
                    b"47977Z",
                    b"9662",
                ],
                &[
                    b"",
                    b"",
                    b"",
                    b"x\x0d0Z",
                    b"6",
                    b"\x0a;80F",
                    b"47977Z",
                    b"9662",
                ],
            ),
            (
                &[b"0\x0d\x0aT: x\x0d", b"\x0a\x0d\x0am", b"ore"],
                &[b"", b"", b""],
            ),
            (
                &[
                    b"\x0a",
                    b"\x0a\x0a87F0",
                    b"\x0d\x0d36\x0d22",
                    b"29Z\x0axx\x0d",
                    b"F46\x0a",
                    b"3\x0d",
                ],
                &[
                    b"\x0a",
                    b"\x0a\x0a87F0",
                    b"\x0d\x0d36\x0d22",
                    b"29Z\x0axx\x0d",
                    b"F46\x0a",
                    b"3\x0d",
                ],
            ),
            (&[], &[]),
            (
                &[b"0aZ\x0a\x0d\x0a\x0d\x0d", b"x\x0d"],
                &[b"\x0d\x0a\x0d\x0d", b"x\x0d"],
            ),
            (
                &[
                    b"5\x0d",
                    b"\x0aaaaaa",
                    b"\x0d\x0a7\x0d\x0a",
                    b"b",
                    b"bbbbbb\x0d",
                    b"\x0a3",
                    b"\x0d\x0accc",
                    b"\x0d\x0a0\x0d",
                    b"\x0a",
                    b"T: x\x0d\x0a\x0d",
                    b"\x0a",
                    b"mor",
                    b"e",
                ],
                &[
                    b"", b"aaaaa", b"", b"b", b"bbbbbb", b"", b"ccc", b"", b"", b"", b"", b"", b"",
                ],
            ),
            (&[b"\x0a1\x0aa\x0aZ"], &[b"\x0a1\x0aa\x0aZ"]),
            (
                &[
                    b"7\x0d\x0aaaa",
                    b"aaaa",
                    b"\x0d\x0a4;",
                    b"e=1\x0d\x0abb",
                    b"bb\x0d\x0a0\x0d\x0aT",
                    b":",
                    b" x\x0d\x0a\x0d\x0a",
                    b"more",
                ],
                &[b"aaa", b"aaaa", b"", b"bb", b"bb", b"", b"", b""],
            ),
            (
                &[
                    b"7\x0a6",
                    b"\x0d7",
                    b"xx6685F\x0d",
                    b"34\x0a1FZZ",
                    b"x0\x0a4711",
                    b"ax",
                    b"7",
                ],
                &[
                    b"6",
                    b"\x0d7",
                    b"xx6685F\x0d",
                    b"34\x0a1FZZ",
                    b"x0\x0a4711",
                    b"ax",
                    b"7",
                ],
            ),
            (
                &[
                    b"8\x0aaaaa",
                    b"aaaa\x0d\x0ab",
                    b"\x0abbbbbbb",
                    b"b",
                    b"bbb\x0a",
                ],
                &[b"aaaa", b"aaaa", b"bbbbbbb", b"b", b"bbb"],
            ),
            (
                &[
                    b"aa30",
                    b"7\x0d61F",
                    b"F\x0a;5",
                    b";8",
                    b"47",
                    b"447x\x0aZxx",
                    b"6a\x0d;;",
                    b"1;Z\x0a0",
                    b"461",
                ],
                &[
                    b"",
                    b"61F",
                    b"F\x0a;5",
                    b";8",
                    b"47",
                    b"447x\x0aZxx",
                    b"6a\x0d;;",
                    b"1;Z\x0a0",
                    b"461",
                ],
            ),
            (
                &[
                    b"1\x0d\x0aa\x0d",
                    b"\x0a1\x0ab\x0d",
                    b"\x0a5\x0accccc",
                    b"\x0d\x0a",
                ],
                &[b"a", b"b", b"ccccc", b""],
            ),
            (
                &[b"85\x0a78", b"Z1", b"\x0a4", b"\x0d24", b"99F"],
                &[b"78", b"Z1", b"\x0a4", b"\x0d24", b"99F"],
            ),
            (&[b"0\x0d\x0aT: ", b"x", b"Q\x0d\x0amore"], &[b"", b"", b""]),
            (
                &[
                    b"42\x0a",
                    b"8\x0dFF706",
                    b";4F\x0a6\x0a",
                    b"\x0a24\x0a5\x0aF\x0a",
                    b"4\x0d\x0d\x0d2\x0d",
                    b"\x0d3;163",
                ],
                &[
                    b"",
                    b"8\x0dFF706",
                    b";4F\x0a6\x0a",
                    b"\x0a24\x0a5\x0aF\x0a",
                    b"4\x0d\x0d\x0d2\x0d",
                    b"\x0d3;163",
                ],
            ),
            (
                &[
                    b"1",
                    b";e=1\x0d",
                    b"\x0aa\x0a2\x0d\x0ab",
                    b"b\x0d\x0a4;e",
                    b"=1",
                    b"\x0acccc\x0d\x0a",
                ],
                &[b"", b"", b"ab", b"b", b"", b"cccc"],
            ),
            (&[b"84\x0da\x0d", b"8\x0a"], &[b"a\x0d", b"8\x0a"]),
            (
                &[
                    b"a\x0d",
                    b"\x0aa",
                    b"aaaaaaaa",
                    b"a\x0d\x0aa",
                    b"Q",
                    b"aaaaaa",
                    b"a",
                    b"\x0d\x0aa\x0ab",
                    b"bbbbbbb",
                    b"bb\x0d",
                    b"\x0a3\x0d\x0accc\x0d",
                    b"\x0a",
                ],
                &[
                    b"",
                    b"a",
                    b"aaaaaaaa",
                    b"a",
                    b"",
                    b"",
                    b"",
                    b"a\x0ab",
                    b"bbbbbbb",
                    b"bb\x0d",
                    b"\x0a3\x0d\x0accc\x0d",
                    b"\x0a",
                ],
            ),
            (
                &[
                    b"7a\x0a9F",
                    b"0302x8F",
                    b"F4a3",
                    b"x",
                    b"4658\x0a\x0a",
                    b"755\x0aa;",
                    b"\x0a",
                ],
                &[
                    b"9F",
                    b"0302x8F",
                    b"F4a3",
                    b"x",
                    b"4658\x0a\x0a",
                    b"755\x0aa;",
                    b"\x0a",
                ],
            ),
            (&[b"2Qr", b"e"], &[b"", b""]),
            (
                &[
                    b"xZZ5\x0a4",
                    b"80",
                    b"\x0a8aF6",
                    b"\x0dx6\x0dZZ",
                    b"\x0a91\x0d20",
                    b"44x\x0a5a",
                    b"aF\x0d2",
                ],
                &[
                    b"xZZ5\x0a4",
                    b"80",
                    b"\x0a8aF6",
                    b"\x0dx6\x0dZZ",
                    b"\x0a91\x0d20",
                    b"44x\x0a5a",
                    b"aF\x0d2",
                ],
            ),
        ];
    }
}
