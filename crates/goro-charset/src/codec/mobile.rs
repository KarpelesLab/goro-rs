//! The Japanese mobile carriers' encodings: SJIS-Mobile#DOCOMO/KDDI/SOFTBANK
//! (CP932 with emoji in the user-defined area) and UTF-8-Mobile#DOCOMO/
//! KDDI-A/KDDI-B/SOFTBANK (UTF-8 with the emoji in each carrier's Private
//! Use Area layout).
//!
//! Emoji: Unicode's EmojiSources.txt, plus the carrier-specific ones of test
//! ext/mbstring/tests/sjis_mobile_encodings.phpt (tools/charset/gen_tables.py).

use super::unicode::{push_utf8, utf8_decode_one};
use super::{Decode, ERR, Encode, Subst, fail};
use crate::Encoding;
use crate::codec::japanese::{index_to_sjis, sjis_to_index};
use crate::tables::{jis as t, mobile as m, rev};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Carrier {
    Docomo,
    Kddi,
    Softbank,
}

fn carrier(enc: Encoding) -> Carrier {
    match enc {
        Encoding::SjisDocomo | Encoding::Utf8Docomo => Carrier::Docomo,
        Encoding::SjisKddi | Encoding::Utf8KddiA | Encoding::Utf8KddiB => Carrier::Kddi,
        _ => Carrier::Softbank,
    }
}

fn emoji_table(c: Carrier) -> &'static [(u16, u32, u32)] {
    match c {
        Carrier::Docomo => &m::DOCOMO,
        Carrier::Kddi => &m::KDDI,
        Carrier::Softbank => &m::SOFTBANK,
    }
}

/// The emoji at a Shift_JIS code: (code point, second code point or 0).
fn emoji_at(c: Carrier, code: u16) -> Option<(u32, u32)> {
    let tab = emoji_table(c);
    tab.binary_search_by_key(&code, |e| e.0)
        .ok()
        .map(|i| (tab[i].1, tab[i].2))
}

fn enc_table(c: Carrier) -> &'static [(u32, u32, u16)] {
    match c {
        Carrier::Docomo => &m::DOCOMO_ENC,
        Carrier::Kddi => &m::KDDI_ENC,
        Carrier::Softbank => &m::SOFTBANK_ENC,
    }
}

/// The Shift_JIS code of an emoji (one or two code points).
fn emoji_code(c: Carrier, a: u32, b: u32) -> Option<u16> {
    let tab = enc_table(c);
    tab.binary_search_by(|e| (e.0, e.1).cmp(&(a, b)))
        .ok()
        .map(|i| tab[i].2)
}

/// Whether `cp` starts a two-code-point emoji (keycaps, flags). Any
/// regional indicator from the lowest to the highest one starting a flag
/// counts (observed on PHP 8.5).
fn starts_pair(c: Carrier, cp: u32) -> bool {
    let tab = enc_table(c);
    if (0x1F1E6..=0x1F1FF).contains(&cp) {
        let mut flags = tab.iter().filter(|e| (0x1F1E6..=0x1F1FF).contains(&e.0) && e.1 != 0);
        let Some(first) = flags.next() else {
            return false;
        };
        let last = flags.last().unwrap_or(first);
        return (first.0..=last.0).contains(&cp);
    }
    let i = tab.partition_point(|e| e.0 < cp);
    tab[i..].iter().take_while(|e| e.0 == cp).any(|e| e.1 != 0)
}

/// Whether `cp` is an emoji by itself.
fn single_emoji(c: Carrier, cp: u32) -> Option<u16> {
    emoji_code(c, cp, 0)
}

/// Whether the carrier has emoji at this Shift_JIS lead byte.
fn emoji_lead(c: Carrier, lead: u8) -> bool {
    match c {
        Carrier::Docomo => (0xF8..=0xF9).contains(&lead),
        Carrier::Kddi => matches!(lead, 0xF3 | 0xF4 | 0xF6 | 0xF7),
        Carrier::Softbank => matches!(lead, 0xF7 | 0xF9 | 0xFB),
    }
}

pub(crate) struct SjisMobileDec {
    c: Carrier,
    /// SoftBank's `ESC $ X` emoji run: the Shift_JIS lead row of X.
    esc_run: Option<u8>,
}

impl SjisMobileDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        SjisMobileDec {
            c: carrier(enc),
            esc_run: None,
        }
    }
}

/// SoftBank's escape codes: `ESC $` and one of G E F O P Q selects a kuten
/// row of emoji (test ext/mbstring/tests/sjis_mobile_encodings.phpt).
fn softbank_esc_row(b: u8) -> Option<(u8, u8)> {
    // (ku, last ten)
    Some(match b {
        b'G' => (0x91, 0x7A),
        b'E' => (0x8D, 0x7A),
        b'F' => (0x8E, 0x7A),
        b'O' => (0x92, 0x6D),
        b'P' => (0x95, 0x6C),
        b'Q' => (0x96, 0x5E),
        _ => return None,
    })
}

fn push_emoji(e: (u32, u32), out: &mut Vec<u32>) {
    out.push(e.0);
    if e.1 != 0 {
        out.push(e.1);
    }
}

impl Decode for SjisMobileDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        let c = self.c;
        while i < n && out.len() < limit {
            let b = s[i];
            if let Some(ku) = self.esc_run {
                if b == 0x0F {
                    self.esc_run = None;
                    i += 1;
                    continue;
                }
                // ten bytes of the selected row; anything else is an error
                // that ends the run (observed on PHP 8.5)
                let (row_ku, max_ten) = softbank_esc_row(ku).unwrap();
                i += 1;
                if (0x21..=max_ten).contains(&b) {
                    let idx = (row_ku - 0x21) as usize * 94 + (b - 0x21) as usize;
                    let (l, tr) = index_to_sjis(idx);
                    if let Some(e) = emoji_at(c, ((l as u16) << 8) | tr as u16) {
                        push_emoji(e, out);
                        continue;
                    }
                }
                self.esc_run = None;
                out.push(ERR);
                continue;
            }
            if b == 0x1B && c == Carrier::Softbank {
                if i + 2 < n && s[i + 1] == b'$' && softbank_esc_row(s[i + 2]).is_some() {
                    self.esc_run = Some(s[i + 2]);
                    i += 3;
                    continue;
                }
                out.push(ERR);
                i = if i + 1 < n && s[i + 1] == b'$' { (i + 3).min(n) } else { i + 2 }.min(n);
                continue;
            }
            if b < 0x80 {
                out.push(b as u32);
                i += 1;
                continue;
            }
            if (0xA1..=0xDF).contains(&b) {
                out.push(0xFF61 + (b - 0xA1) as u32);
                i += 1;
                continue;
            }
            if !((0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)) || i + 1 >= n {
                out.push(ERR);
                i += 1;
                continue;
            }
            let tr = s[i + 1];
            i += 2;
            if !((0x40..=0x7E).contains(&tr) || (0x80..=0xFC).contains(&tr)) {
                out.push(ERR);
                continue;
            }
            if emoji_lead(c, b) {
                if let Some(e) = emoji_at(c, ((b as u16) << 8) | tr as u16) {
                    push_emoji(e, out);
                    continue;
                }
            }
            let cp = t::CP932[sjis_to_index(b, tr)];
            out.push(if cp == 0 { ERR } else { cp as u32 });
        }
        *pos = i;
    }
}

/// Shared emoji encoding state: a held digit, `#` or regional indicator that
/// may start a two-code-point emoji.
#[derive(Default)]
struct PairState {
    pending: Option<u32>,
}

pub(crate) struct SjisMobileEnc {
    c: Carrier,
    st: PairState,
}

impl SjisMobileEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        SjisMobileEnc {
            c: carrier(enc),
            st: PairState::default(),
        }
    }

    fn base(&self, cp: u32, out: &mut Vec<u8>) -> bool {
        if cp < 0x80 {
            out.push(cp as u8);
            return true;
        }
        if (0xFF61..=0xFF9F).contains(&cp) {
            out.push((cp - 0xFF61) as u8 + 0xA1);
            return true;
        }
        if let Some(code) = single_emoji(self.c, cp) {
            out.extend_from_slice(&code.to_be_bytes());
            return true;
        }
        if cp > 0xFFFF {
            return false;
        }
        // test ext/mbstring/tests/sjis_mobile_encodings.phpt
        let c = match cp {
            0x301C => 0xFF5E,
            0x2212 => 0xFF0D,
            0x203E | 0xAF => 0xFFE3,
            0x2016 => 0x2225,
            0xAC => 0xFFE2,
            0xA5 => 0xFFE5,
            0xA3 => 0xFFE1,
            0xA2 => 0xFFE0,
            _ => cp,
        };
        let i = rev(&t::CP932_LOW_REV_IDX, &t::CP932_LOW_REV_DATA, c);
        if i == 0 {
            return false;
        }
        let (l, tr) = index_to_sjis(i as usize - 1);
        out.extend_from_slice(&[l, tr]);
        true
    }
}

/// Runs the pair logic: returns `Some(result)` when `cp` was consumed.
fn pair_step<F: FnMut(u32, &mut Vec<u8>) -> bool>(
    st: &mut PairState,
    c: Carrier,
    cp: u32,
    out: &mut Vec<u8>,
    emit_code: &mut dyn FnMut(u16, &mut Vec<u8>),
    mut base: F,
) -> PairResult {
    if let Some(p) = st.pending.take() {
        if let Some(code) = emoji_code(c, p, cp) {
            emit_code(code, out);
            return PairResult::Done(true);
        }
        // The held character goes out by itself first; a regional indicator
        // without its pair is an error (test
        // ext/mbstring/tests/sjis_mobile_encodings.phpt).
        if (0x1F1E6..=0x1F1FF).contains(&p) || !base(p, out) {
            return PairResult::Failed(p);
        }
    }
    if starts_pair(c, cp) {
        st.pending = Some(cp);
        return PairResult::Done(true);
    }
    PairResult::Done(base(cp, out))
}

enum PairResult {
    Done(bool),
    /// The held code point failed; the current one is still to process.
    Failed(u32),
}

impl Encode for SjisMobileEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        // Only used for substitutes (no pair logic needed).
        if let Some(p) = self.st.pending.take() {
            self.base(p, out);
        }
        self.base(cp, out)
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        for &cp in cps {
            loop {
                let c = self.c;
                let mut st = std::mem::take(&mut self.st);
                let r = {
                    let me = &*self;
                    pair_step(
                        &mut st,
                        c,
                        cp,
                        out,
                        &mut |code, o| o.extend_from_slice(&code.to_be_bytes()),
                        |x, o| me.base(x, o),
                    )
                };
                self.st = st;
                match r {
                    PairResult::Done(true) => break,
                    PairResult::Done(false) => {
                        fail(self, cp, out, sub, errors);
                        break;
                    }
                    PairResult::Failed(p) => {
                        fail(self, p, out, sub, errors);
                        // retry cp with nothing pending
                    }
                }
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        if let Some(p) = self.st.pending.take()
            && ((0x1F1E6..=0x1F1FF).contains(&p) || !self.base(p, out))
        {
            fail(self, p, out, sub, errors);
        }
    }
}

/// The carrier PUA layout of a UTF-8-Mobile encoding.
fn pua_runs(enc: Encoding) -> &'static [(u32, u32, u16)] {
    match enc {
        Encoding::Utf8Docomo => &m::PUA_DOCOMO,
        Encoding::Utf8KddiA => &m::PUA_KDDI_A,
        Encoding::Utf8KddiB => &m::PUA_KDDI_B,
        _ => &m::PUA_SOFTBANK,
    }
}

fn pua_to_sjis(runs: &[(u32, u32, u16)], cp: u32) -> Option<u16> {
    let r = runs.iter().find(|r| (r.0..=r.1).contains(&cp))?;
    let start = sjis_to_index((r.2 >> 8) as u8, r.2 as u8);
    let (l, tr) = index_to_sjis(start + (cp - r.0) as usize);
    Some(((l as u16) << 8) | tr as u16)
}

fn sjis_to_pua(runs: &[(u32, u32, u16)], code: u16) -> Option<u32> {
    let idx = sjis_to_index((code >> 8) as u8, code as u8);
    runs.iter().find_map(|r| {
        let start = sjis_to_index((r.2 >> 8) as u8, r.2 as u8);
        let len = (r.1 - r.0) as usize;
        (idx >= start && idx <= start + len).then(|| r.0 + (idx - start) as u32)
    })
}

pub(crate) struct Utf8MobileDec {
    c: Carrier,
    runs: &'static [(u32, u32, u16)],
}

impl Utf8MobileDec {
    pub(crate) fn new(enc: Encoding) -> Self {
        Utf8MobileDec {
            c: carrier(enc),
            runs: pua_runs(enc),
        }
    }
}

impl Decode for Utf8MobileDec {
    fn decode(&mut self, s: &[u8], pos: &mut usize, out: &mut Vec<u32>, limit: usize) {
        let mut i = *pos;
        let n = s.len();
        while i < n && out.len() < limit {
            let (cp, len) = utf8_decode_one(s, i);
            i += len;
            if (0xE000..=0xF8FF).contains(&cp)
                && let Some(code) = pua_to_sjis(self.runs, cp)
            {
                match emoji_at(self.c, code) {
                    Some(e) => push_emoji(e, out),
                    None => out.push(ERR),
                }
                continue;
            }
            out.push(cp);
        }
        *pos = i;
    }
}

pub(crate) struct Utf8MobileEnc {
    c: Carrier,
    runs: &'static [(u32, u32, u16)],
    st: PairState,
}

impl Utf8MobileEnc {
    pub(crate) fn new(enc: Encoding) -> Self {
        Utf8MobileEnc {
            c: carrier(enc),
            runs: pua_runs(enc),
            st: PairState::default(),
        }
    }

    fn emit_code(&self, code: u16, out: &mut Vec<u8>) -> bool {
        match sjis_to_pua(self.runs, code) {
            Some(p) => {
                push_utf8(p, out);
                true
            }
            None => false,
        }
    }

    fn base(&self, cp: u32, out: &mut Vec<u8>) -> bool {
        if let Some(code) = single_emoji(self.c, cp)
            && self.emit_code(code, out)
        {
            return true;
        }
        if cp > 0x10FFFF {
            return false;
        }
        push_utf8(cp, out);
        true
    }
}

impl Encode for Utf8MobileEnc {
    fn put(&mut self, cp: u32, out: &mut Vec<u8>) -> bool {
        if let Some(p) = self.st.pending.take() {
            self.base(p, out);
        }
        self.base(cp, out)
    }

    fn encode(&mut self, cps: &[u32], out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        for &cp in cps {
            loop {
                let c = self.c;
                let mut st = std::mem::take(&mut self.st);
                let r = {
                    let me = &*self;
                    pair_step(
                        &mut st,
                        c,
                        cp,
                        out,
                        &mut |code, o| {
                            me.emit_code(code, o);
                        },
                        |x, o| me.base(x, o),
                    )
                };
                self.st = st;
                match r {
                    PairResult::Done(true) => break,
                    PairResult::Done(false) => {
                        fail(self, cp, out, sub, errors);
                        break;
                    }
                    PairResult::Failed(p) => {
                        fail(self, p, out, sub, errors);
                    }
                }
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>, sub: &Subst, errors: &mut usize) {
        if let Some(p) = self.st.pending.take()
            && ((0x1F1E6..=0x1F1FF).contains(&p) || !self.base(p, out))
        {
            fail(self, p, out, sub, errors);
        }
    }
}

/// ISO-2022-JP-MOBILE#KDDI reaches the KDDI emoji of Shift_JIS rows 0xF6
/// and 0xF7 as JIS rows 0x75..0x78 (observed on PHP 8.5). Pushes the emoji
/// and returns `Some(true)`, `Some(false)` for an empty cell of those rows,
/// `None` for other rows.
pub(crate) fn kddi_jis_emoji(lead: u8, trail: u8, out: &mut Vec<u32>) -> Option<bool> {
    if !(0x75..=0x78).contains(&lead) {
        return None;
    }
    let idx = (lead - 0x21) as usize * 94 + (trail - 0x21) as usize + 22 * 94;
    let (l, tr) = index_to_sjis(idx);
    match emoji_at(Carrier::Kddi, ((l as u16) << 8) | tr as u16) {
        Some(e) => {
            push_emoji(e, out);
            Some(true)
        }
        None => Some(false),
    }
}

/// The JIS bytes of a single-code-point KDDI emoji reachable from
/// ISO-2022-JP-MOBILE#KDDI.
pub(crate) fn kddi_jis_emoji_rev(cp: u32) -> Option<(u8, u8)> {
    // The encoder also writes the emoji of rows 0xF3 and 0xF4, as JIS rows
    // 0x6F..0x72, which the decoder reads as JIS X 0208 (observed on PHP
    // 8.5).
    // A regional indicator alone takes the code of the flag it starts.
    let code = single_emoji(Carrier::Kddi, cp).or_else(|| {
        if !(0x1F1E6..=0x1F1FF).contains(&cp) {
            return None;
        }
        m::KDDI_ENC.iter().find(|e| e.0 == cp && e.1 != 0).map(|e| e.2)
    })?;
    let (l, tr) = ((code >> 8) as u8, code as u8);
    if !matches!(l, 0xF3 | 0xF4 | 0xF6 | 0xF7) {
        return None;
    }
    let idx = sjis_to_index(l, tr) - 22 * 94;
    Some(((idx / 94) as u8 + 0x21, (idx % 94) as u8 + 0x21))
}

/// ISO-2022-JP-MOBILE#KDDI: whether `cp` may start a two-code-point emoji.
pub(crate) fn kddi_starts_pair(cp: u32) -> bool {
    starts_pair(Carrier::Kddi, cp)
}

/// ISO-2022-JP-MOBILE#KDDI: the JIS bytes of a two-code-point emoji.
pub(crate) fn kddi_jis_pair(a: u32, b: u32) -> Option<(u8, u8)> {
    let code = emoji_code(Carrier::Kddi, a, b)?;
    let (l, tr) = ((code >> 8) as u8, code as u8);
    if !matches!(l, 0xF3 | 0xF4 | 0xF6 | 0xF7) {
        return None;
    }
    let idx = sjis_to_index(l, tr) - 22 * 94;
    Some(((idx / 94) as u8 + 0x21, (idx % 94) as u8 + 0x21))
}
