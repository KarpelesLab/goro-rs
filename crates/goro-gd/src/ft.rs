//! gdft.c: FreeType text rendering (gdImageStringFTEx).
//!
//! FreeType itself is replaced by skrifa (a pure Rust font scaler whose
//! TrueType interpreter, CFF hinter and autohinter follow FreeType's) for
//! charmaps, outlines, hinting and metrics, plus the FreeType behaviours
//! libgd depends on that are reproduced here: the size computation
//! (FT_Set_Char_Size, integer ppem for TrueType fonts with head flag 3),
//! the hinter selection of FT_Load_Glyph, rounded hinted advances, kerning
//! (FT_Get_Kerning, 'kern' table), gridfitted control boxes, outline
//! transformation in 16.16 fixed point and an anti-aliasing area coverage
//! rasterizer (the "smooth" renderer).

use std::cell::RefCell;
use std::rc::Rc;

use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, HintingInstance, HintingOptions, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider, Tag};

use crate::*;

/// Number of antialiased colors for indexed bitmaps
const NUMCOLORS: i32 = 8;
/// Number of fonts cached before least recently used is replaced
const FONTCACHESIZE: usize = 6;
/// Line separation as a factor of font height
const LINESPACE: f64 = 1.05;

pub const FTEX_LINESPACE: i32 = 1;
pub const FTEX_CHARMAP: i32 = 2;
pub const FTEX_RESOLUTION: i32 = 4;
pub const FTEX_UNICODE: i32 = 0;
pub const FTEX_SHIFT_JIS: i32 = 1;
pub const FTEX_BIG5: i32 = 2;
pub const FTEX_MACROMAN: i32 = 3;

const DEFAULT_FONTPATH: &str = "/usr/X11R6/lib/X11/fonts/TrueType:/usr/X11R6/lib/X11/fonts/truetype:/usr/X11R6/lib/X11/fonts/TTF:/usr/share/fonts/TrueType:/usr/share/fonts/truetype:/usr/openwin/lib/X11/fonts/TrueType:/usr/X11R6/lib/X11/fonts/Type1:/usr/lib/X11/fonts/Type1:/usr/openwin/lib/X11/fonts/Type1";
const PATHSEPARATOR: char = ':';

/// gdFTStringExtra
#[derive(Clone, Debug, Default)]
pub struct FtStringExtra {
    pub flags: i32,
    pub linespacing: f64,
    pub charmap: i32,
    pub hdpi: i32,
    pub vdpi: i32,
}

/// FT_Encoding of a charmap, as far as libgd looks at it
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    MsSymbol,
    AdobeStandard,
    Other,
}

#[derive(Clone, Debug)]
struct CharMap {
    platform: u16,
    encoding_id: u16,
    encoding: Encoding,
    /// Index of the sfnt cmap encoding record (None: synthesized CFF
    /// encoding charmap)
    record: Option<u16>,
}

/// font_t
struct Font {
    fontlist: String,
    data: Rc<Vec<u8>>,
    charmaps: Vec<CharMap>,
    charmap: usize,
    have_char_map_unicode: bool,
    have_char_map_big5: bool,
    have_char_map_sjis: bool,
    #[allow(dead_code)]
    have_char_map_apple_roman: bool,
    family_name: Option<String>,
}

thread_local! {
    static FONT_CACHE: RefCell<Vec<Rc<Font>>> = const { RefCell::new(Vec::new()) };
}

/// gdFontCacheShutdown()
pub fn font_cache_shutdown() {
    FONT_CACHE.with(|c| c.borrow_mut().clear());
}

fn be16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}

fn be16s(d: &[u8], o: usize) -> Option<i16> {
    be16(d, o).map(|v| v as i16)
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(o)?,
        *d.get(o + 1)?,
        *d.get(o + 2)?,
        *d.get(o + 3)?,
    ]))
}

/// FT_MulFix()
fn mul_fix(a: i64, b: i64) -> i64 {
    let ab = a * b;
    (ab + 0x8000 - (ab < 0) as i64) >> 16
}

/// FT_DivFix()
fn div_fix(a: i64, b: i64) -> i64 {
    if b == 0 {
        return 0x7FFFFFFF;
    }
    let neg = (a < 0) != (b < 0);
    let (a, b) = (a.abs(), b.abs());
    let q = ((a << 16) + (b >> 1)) / b;
    if neg { -q } else { q }
}

/// FT_MulDiv()
fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    if c == 0 {
        return 0x7FFFFFFF;
    }
    let neg = ((a < 0) != (b < 0)) != (c < 0);
    let (a, b, c) = (a.abs(), b.abs(), c.abs());
    let d = (a * b + (c >> 1)) / c;
    if neg { -d } else { d }
}

fn pix_round(v: i64) -> i64 {
    (v + 32) & !63
}

fn pix_floor(v: i64) -> i64 {
    v & !63
}

fn pix_ceil(v: i64) -> i64 {
    (v + 63) & !63
}

/// The family name as FreeType reports it (typographic family first)
fn family_name(font: &FontRef) -> Option<String> {
    let name = font.table_data(Tag::new(b"name"))?;
    let d = name.as_bytes();
    let count = be16(d, 2)? as usize;
    let storage = be16(d, 4)? as usize;
    let mut found: [Option<String>; 2] = [None, None];
    for i in 0..count {
        let r = 6 + i * 12;
        let platform = be16(d, r)?;
        let enc = be16(d, r + 2)?;
        let lang = be16(d, r + 4)?;
        let id = be16(d, r + 6)?;
        let len = be16(d, r + 8)? as usize;
        let off = be16(d, r + 10)? as usize;
        let slot = match id {
            16 => 0,
            1 => 1,
            _ => continue,
        };
        if found[slot].is_some() {
            continue;
        }
        let Some(raw) = d.get(storage + off..storage + off + len) else {
            continue;
        };
        let s = if platform == 3 || platform == 0 {
            if platform == 3 && lang != 0x409 && enc != 0 && enc != 1 {
                continue;
            }
            let u: Vec<u16> = raw
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u)
        } else if platform == 1 && enc == 0 {
            raw.iter().map(|&b| b as char).collect()
        } else {
            continue;
        };
        found[slot] = Some(s);
    }
    let [a, b] = found;
    a.or(b)
}

/// The charmaps of a face in FreeType's order: the sfnt cmap subtables in
/// file order, then for CFF-flavoured fonts the synthesized encoding map.
fn face_charmaps(font: &FontRef) -> Vec<CharMap> {
    let mut v = Vec::new();
    if let Ok(cmap) = font.cmap() {
        for (i, rec) in cmap.encoding_records().iter().enumerate() {
            if rec.subtable(cmap.offset_data()).is_err() {
                continue;
            }
            let platform = rec.platform_id() as u16;
            let encoding_id = rec.encoding_id();
            let encoding = if platform == 3 && encoding_id == 0 {
                Encoding::MsSymbol
            } else {
                Encoding::Other
            };
            v.push(CharMap {
                platform,
                encoding_id,
                encoding,
                record: Some(i as u16),
            });
        }
    }
    if font.table_data(Tag::new(b"CFF ")).is_some() {
        v.push(CharMap {
            platform: 7,
            encoding_id: 0,
            encoding: Encoding::AdobeStandard,
            record: None,
        });
    }
    v
}

fn is_readable(path: &str) -> bool {
    std::fs::File::open(path).is_ok()
}

/// fontFetch()
fn font_fetch(fontlist: &str, preferred_map: i32) -> Result<Font, String> {
    let fontsearchpath =
        std::env::var("GDFONTPATH").unwrap_or_else(|_| DEFAULT_FONTPATH.to_string());
    let mut fullname: Option<String> = None;
    'names: for name in fontlist.split(';').filter(|s| !s.is_empty()) {
        if name.starts_with('/') && is_readable(name) {
            fullname = Some(name.to_string());
            break;
        }
        for dir in fontsearchpath
            .split(PATHSEPARATOR)
            .filter(|s| !s.is_empty())
        {
            let dir = if dir == "." {
                match std::env::current_dir() {
                    Ok(d) => d.to_string_lossy().into_owned(),
                    Err(_) => continue,
                }
            } else {
                dir.to_string()
            };
            for ext in ["", ".ttf", ".pfa", ".pfb", ".dfont"] {
                let f = format!("{}/{}{}", dir, name, ext);
                if is_readable(&f) {
                    fullname = Some(f);
                    break 'names;
                }
            }
        }
    }
    let Some(fullname) = fullname else {
        return Err("Could not find/open font".into());
    };
    let data = std::fs::read(&fullname).map_err(|_| "Could not read font".to_string())?;
    let data = Rc::new(data);
    let fr = FontRef::from_index(&data, 0).map_err(|_| "Could not read font".to_string())?;
    if fr.maxp().is_err() {
        return Err("Could not read font".into());
    }
    let charmaps = face_charmaps(&fr);
    let mut a = Font {
        fontlist: fontlist.to_string(),
        data: data.clone(),
        charmaps: charmaps.clone(),
        charmap: 0,
        have_char_map_unicode: false,
        have_char_map_big5: false,
        have_char_map_sjis: false,
        have_char_map_apple_roman: false,
        family_name: family_name(&fr),
    };
    let mut found: Option<usize> = None;
    for (n, cm) in charmaps.iter().enumerate() {
        let (platform, encoding) = (cm.platform, cm.encoding_id);
        // Whatever is the last value is what should be set
        a.have_char_map_unicode = false;
        a.have_char_map_big5 = false;
        a.have_char_map_sjis = false;
        a.have_char_map_apple_roman = false;
        if cm.encoding == Encoding::MsSymbol || cm.encoding == Encoding::AdobeStandard {
            a.have_char_map_unicode = true;
            a.charmap = n;
            return Ok(a);
        }
        if (platform == 3 && encoding == 1)
            || (platform == 3 && encoding == 0)
            || (platform == 2 && encoding == 1)
            || platform == 0
        {
            a.have_char_map_unicode = true;
            found = Some(n);
            if preferred_map == FTEX_UNICODE {
                break;
            }
        } else if platform == 3 && encoding == 4 {
            a.have_char_map_big5 = true;
            found = Some(n);
            if preferred_map == FTEX_BIG5 {
                break;
            }
        } else if platform == 3 && encoding == 2 {
            a.have_char_map_sjis = true;
            found = Some(n);
            if preferred_map == FTEX_SHIFT_JIS {
                break;
            }
        } else if (platform == 1 && encoding == 0) || (platform == 2 && encoding == 0) {
            a.have_char_map_apple_roman = true;
            found = Some(n);
            if preferred_map == FTEX_MACROMAN {
                break;
            }
        }
    }
    let Some(found) = found else {
        return Err("Unable to find a CharMap that I can handle".into());
    };
    a.charmap = found;
    Ok(a)
}

/// gdCacheGet(fontCache, ...) with fontTest()
fn font_get(fontlist: &str, preferred_map: i32) -> Result<Rc<Font>, String> {
    FONT_CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let hit = cache.iter().position(|a| {
            a.fontlist == fontlist
                && match preferred_map {
                    FTEX_UNICODE => a.have_char_map_unicode,
                    FTEX_SHIFT_JIS | FTEX_BIG5 => a.have_char_map_sjis,
                    _ => false,
                }
        });
        if let Some(i) = hit {
            let f = cache.remove(i);
            cache.insert(0, f.clone());
            return Ok(f);
        }
        let f = Rc::new(font_fetch(fontlist, preferred_map)?);
        if cache.len() >= FONTCACHESIZE {
            cache.pop();
        }
        cache.insert(0, f.clone());
        Ok(f)
    })
}

/// gdTcl_UtfToUniChar(): (char, bytes consumed). Reads past the end as NUL.
fn utf_to_uni_char(s: &[u8]) -> (i32, usize) {
    let at = |i: usize| -> i32 { s.get(i).copied().unwrap_or(0) as i32 };
    let mut byte = at(0);
    if byte == b'&' as i32 {
        let mut n: i32 = 0;
        byte = at(1);
        if byte == b'#' as i32 {
            byte = at(2);
            let mut i;
            if byte == b'x' as i32 || byte == b'X' as i32 {
                i = 3;
                while i < 8 {
                    byte = at(i);
                    let v = if (b'A' as i32..=b'F' as i32).contains(&byte) {
                        byte - b'A' as i32 + 10
                    } else if (b'a' as i32..=b'f' as i32).contains(&byte) {
                        byte - b'a' as i32 + 10
                    } else if (b'0' as i32..=b'9' as i32).contains(&byte) {
                        byte - b'0' as i32
                    } else {
                        break;
                    };
                    byte = v;
                    n = n.wrapping_mul(16).wrapping_add(byte);
                    i += 1;
                }
            } else {
                i = 2;
                while i < 8 {
                    byte = at(i);
                    if (b'0' as i32..=b'9' as i32).contains(&byte) {
                        n = n.wrapping_mul(10).wrapping_add(byte - b'0' as i32);
                    } else {
                        break;
                    }
                    i += 1;
                }
            }
            if byte == b';' as i32 {
                return (n, i + 1);
            }
        }
    }
    let byte = at(0);
    let b1 = at(1);
    let b2 = at(2);
    if byte < 0xC0 {
        (byte, 1)
    } else if byte < 0xE0 {
        if (b1 & 0xC0) == 0x80 {
            (((byte & 0x1F) << 6) | (b1 & 0x3F), 2)
        } else {
            (byte, 1)
        }
    } else if byte < 0xF0 {
        if (b1 & 0xC0) == 0x80 && (b2 & 0xC0) == 0x80 {
            (((byte & 0x0F) << 12) | ((b1 & 0x3F) << 6) | (b2 & 0x3F), 3)
        } else {
            (byte, 1)
        }
    } else {
        (byte, 1)
    }
}

/// 2x3 transform in 16.16 (FT_Matrix)
#[derive(Clone, Copy)]
struct Matrix {
    xx: i64,
    xy: i64,
    yx: i64,
    yy: i64,
}

/// FT_Vector_Transform()
fn transform(m: &Matrix, x: i64, y: i64) -> (i64, i64) {
    (
        mul_fix(x, m.xx) + mul_fix(y, m.xy),
        mul_fix(x, m.yx) + mul_fix(y, m.yy),
    )
}

#[derive(Clone, Copy, Debug)]
enum Seg {
    Move(i64, i64),
    Line(i64, i64),
    Quad(i64, i64, i64, i64),
    Cubic(i64, i64, i64, i64, i64, i64),
}

/// A glyph outline in 26.6 (y up)
#[derive(Default, Clone)]
struct Outline {
    segs: Vec<Seg>,
}

/// An OutlinePen collecting into an Outline: in 26.6 (pixel input), or in
/// doubled font units (unscaled input, so implied midpoints stay exact)
struct Collect<'a>(&'a mut Outline, bool);

fn f2d6(v: f32) -> i64 {
    f2d6_mid(v)
}

impl Collect<'_> {
    fn c(&self, v: f32) -> i64 {
        if self.1 {
            (v as f64 * 2.0).round() as i64
        } else {
            f2d6(v)
        }
    }
}

impl OutlinePen for Collect<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let s = Seg::Move(self.c(x), self.c(y));
        self.0.segs.push(s);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let s = Seg::Line(self.c(x), self.c(y));
        self.0.segs.push(s);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let s = Seg::Quad(self.c(cx0), self.c(cy0), self.c(x), self.c(y));
        self.0.segs.push(s);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let s = Seg::Cubic(
            self.c(cx0),
            self.c(cy0),
            self.c(cx1),
            self.c(cy1),
            self.c(x),
            self.c(y),
        );
        self.0.segs.push(s);
    }
    fn close(&mut self) {}
}

impl Outline {
    fn points(&self) -> impl Iterator<Item = (i64, i64)> + '_ {
        self.segs.iter().flat_map(|s| {
            let v: Vec<(i64, i64)> = match *s {
                Seg::Move(x, y) | Seg::Line(x, y) => vec![(x, y)],
                Seg::Quad(a, b, x, y) => vec![(a, b), (x, y)],
                Seg::Cubic(a, b, c, d, x, y) => vec![(a, b), (c, d), (x, y)],
            };
            v.into_iter()
        })
    }

    /// FT_Outline_Get_CBox()
    fn cbox(&self) -> (i64, i64, i64, i64) {
        let mut it = self.points();
        let Some((x0, y0)) = it.next() else {
            return (0, 0, 0, 0);
        };
        let (mut xmin, mut ymin, mut xmax, mut ymax) = (x0, y0, x0, y0);
        for (x, y) in it {
            xmin = xmin.min(x);
            ymin = ymin.min(y);
            xmax = xmax.max(x);
            ymax = ymax.max(y);
        }
        (xmin, ymin, xmax, ymax)
    }

    /// FT_Outline_Transform()
    fn transform(&mut self, m: &Matrix) {
        self.map(|x, y| transform(m, x, y));
    }

    /// Map the stored points through `t`, recomputing the implied conic
    /// on-curve points from the mapped controls.
    fn map(&mut self, t: impl Fn(i64, i64) -> (i64, i64)) {
        // FreeType transforms the stored points; on-curve points implied
        // between two conic controls are recomputed by
        // FT_Outline_Decompose() afterwards: find them first.
        let n = self.segs.len();
        let mid = |a: i64, b: i64| (a + b) / 2;
        let ctrl = |s: &Seg| match *s {
            Seg::Quad(a, b, _, _) => Some((a, b)),
            _ => None,
        };
        let end = |s: &Seg| match *s {
            Seg::Move(x, y)
            | Seg::Line(x, y)
            | Seg::Quad(_, _, x, y)
            | Seg::Cubic(_, _, _, _, x, y) => (x, y),
        };
        // (segment index, control before, control after)
        let mut implied: Vec<(usize, usize, usize)> = Vec::new();
        let mut i = 0;
        while i < n {
            // a contour: Move at i, segments until the next Move
            let start = i;
            let mut j = i + 1;
            while j < n && !matches!(self.segs[j], Seg::Move(..)) {
                j += 1;
            }
            for k in start + 1..j {
                if let (Some(c1), Some(c2)) = (
                    ctrl(&self.segs[k]),
                    self.segs.get(k + 1).filter(|_| k + 1 < j).and_then(ctrl),
                ) {
                    let p = end(&self.segs[k]);
                    if p == (mid(c1.0, c2.0), mid(c1.1, c2.1)) {
                        implied.push((k, k, k + 1));
                    }
                }
            }
            // implied contour start
            if j > start + 1 {
                if let (Some(cf), Some(cl)) = (ctrl(&self.segs[start + 1]), ctrl(&self.segs[j - 1]))
                {
                    let p = end(&self.segs[start]);
                    if p == (mid(cl.0, cf.0), mid(cl.1, cf.1)) && end(&self.segs[j - 1]) == p {
                        implied.push((start, j - 1, start + 1));
                        implied.push((j - 1, j - 1, start + 1));
                    }
                }
            }
            i = j;
        }
        for s in &mut self.segs {
            *s = match *s {
                Seg::Move(x, y) => {
                    let (x, y) = t(x, y);
                    Seg::Move(x, y)
                }
                Seg::Line(x, y) => {
                    let (x, y) = t(x, y);
                    Seg::Line(x, y)
                }
                Seg::Quad(a, b, x, y) => {
                    let (a, b) = t(a, b);
                    let (x, y) = t(x, y);
                    Seg::Quad(a, b, x, y)
                }
                Seg::Cubic(a, b, c, d, x, y) => {
                    let (a, b) = t(a, b);
                    let (c, d) = t(c, d);
                    let (x, y) = t(x, y);
                    Seg::Cubic(a, b, c, d, x, y)
                }
            };
        }
        for (k, c1, c2) in implied {
            let (Some(a), Some(b)) = (ctrl(&self.segs[c1]), ctrl(&self.segs[c2])) else {
                continue;
            };
            let (x, y) = (mid(a.0, b.0), mid(a.1, b.1));
            self.segs[k] = match self.segs[k] {
                Seg::Move(..) => Seg::Move(x, y),
                Seg::Quad(p, q, _, _) => Seg::Quad(p, q, x, y),
                s => s,
            };
        }
    }
}

/// FT_Bitmap (8-bit grays)
struct Bitmap {
    rows: i32,
    width: i32,
    left: i32,
    top: i32,
    buffer: Vec<u8>,
}

/// ftgrays.c: FreeType's anti-aliasing rasterizer, in 24.8 subpixels with
/// per-cell cover/area accumulation, the conic forward differencing and
/// the cubic bisection of FreeType 2.11+.
const PIXEL_BITS: i64 = 8;
const ONE_PIXEL: i64 = 1 << PIXEL_BITS;

fn trunc(x: i64) -> i64 {
    x >> PIXEL_BITS
}

fn fract(x: i64) -> i64 {
    x & (ONE_PIXEL - 1)
}

fn upscale(x: i64) -> i64 {
    x * (ONE_PIXEL >> 6)
}

/// FT_UDIVPREP(): the reciprocal used by FT_UDIV()
fn udiv_prep(c: bool, b: i64) -> i64 {
    if c {
        ((u64::MAX >> PIXEL_BITS) as i64) / b
    } else {
        0
    }
}

/// FT_UDIV(a, b) with `br` the (possibly negated) reciprocal of b
fn udiv(a: i64, br: i64) -> i64 {
    ((a as u64).wrapping_mul(br as u64) >> (64 - PIXEL_BITS)) as i64
}

struct Grays {
    min_ex: i64,
    max_ex: i64,
    min_ey: i64,
    max_ey: i64,
    /// per row: the cells (x, cover, area) in visiting order; x = min_ex - 1
    /// collects everything left of the bitmap
    cells: Vec<Vec<(i64, i64, i64)>>,
    ex: i64,
    ey: i64,
    cover: i64,
    area: i64,
    x: i64,
    y: i64,
    valid: bool,
}

impl Grays {
    fn new(w: i64, h: i64) -> Grays {
        Grays {
            min_ex: 0,
            max_ex: w,
            min_ey: 0,
            max_ey: h,
            cells: vec![Vec::new(); h as usize],
            ex: 0,
            ey: 0,
            cover: 0,
            area: 0,
            x: 0,
            y: 0,
            valid: false,
        }
    }

    fn record(&mut self) {
        if self.valid && (self.area != 0 || self.cover != 0) {
            let row = &mut self.cells[(self.ey - self.min_ey) as usize];
            match row.last_mut() {
                Some(c) if c.0 == self.ex => {
                    c.1 += self.cover;
                    c.2 += self.area;
                }
                _ => row.push((self.ex, self.cover, self.area)),
            }
        }
    }

    /// gray_set_cell()
    fn set_cell(&mut self, mut ex: i64, ey: i64) {
        self.record();
        self.area = 0;
        self.cover = 0;
        if ey >= self.max_ey || ey < self.min_ey || ex >= self.max_ex {
            self.valid = false;
            return;
        }
        if ex < self.min_ex {
            ex = self.min_ex - 1;
        }
        self.ex = ex;
        self.ey = ey;
        self.valid = true;
    }

    fn move_to(&mut self, x: i64, y: i64) {
        let (x, y) = (upscale(x), upscale(y));
        self.set_cell(trunc(x), trunc(y));
        self.x = x;
        self.y = y;
    }

    /// gray_render_line() (coordinates in 24.8)
    fn render_line(&mut self, to_x: i64, to_y: i64) {
        let mut ey1 = trunc(self.y);
        let ey2 = trunc(to_y);
        if (ey1 >= self.max_ey && ey2 >= self.max_ey) || (ey1 < self.min_ey && ey2 < self.min_ey) {
            self.x = to_x;
            self.y = to_y;
            return;
        }
        let mut ex1 = trunc(self.x);
        let ex2 = trunc(to_x);
        let mut fx1 = fract(self.x);
        let mut fy1 = fract(self.y);
        let dx = to_x - self.x;
        let dy = to_y - self.y;
        if ex1 == ex2 && ey1 == ey2 {
            // inside one cell
        } else if dy == 0 {
            self.set_cell(ex2, ey2);
            self.x = to_x;
            self.y = to_y;
            return;
        } else if dx == 0 {
            if dy > 0 {
                loop {
                    let fy2 = ONE_PIXEL;
                    self.cover += fy2 - fy1;
                    self.area += (fy2 - fy1) * fx1 * 2;
                    fy1 = 0;
                    ey1 += 1;
                    self.set_cell(ex1, ey1);
                    if ey1 == ey2 {
                        break;
                    }
                }
            } else {
                loop {
                    let fy2 = 0;
                    self.cover += fy2 - fy1;
                    self.area += (fy2 - fy1) * fx1 * 2;
                    fy1 = ONE_PIXEL;
                    ey1 -= 1;
                    self.set_cell(ex1, ey1);
                    if ey1 == ey2 {
                        break;
                    }
                }
            }
        } else {
            let mut prod = dx * fy1 - dy * fx1;
            let dx_r = udiv_prep(ex1 != ex2, dx);
            let dy_r = udiv_prep(ey1 != ey2, dy);
            loop {
                let (fx2, fy2);
                if prod - dx * ONE_PIXEL > 0 && prod <= 0 {
                    // left
                    fx2 = 0;
                    fy2 = udiv(-prod, -dx_r);
                    prod -= dy * ONE_PIXEL;
                    self.cover += fy2 - fy1;
                    self.area += (fy2 - fy1) * (fx1 + fx2);
                    fx1 = ONE_PIXEL;
                    fy1 = fy2;
                    ex1 -= 1;
                } else if prod - dx * ONE_PIXEL + dy * ONE_PIXEL > 0 && prod - dx * ONE_PIXEL <= 0 {
                    // up
                    prod -= dx * ONE_PIXEL;
                    fx2 = udiv(-prod, dy_r);
                    fy2 = ONE_PIXEL;
                    self.cover += fy2 - fy1;
                    self.area += (fy2 - fy1) * (fx1 + fx2);
                    fx1 = fx2;
                    fy1 = 0;
                    ey1 += 1;
                } else if prod + dy * ONE_PIXEL >= 0 && prod - dx * ONE_PIXEL + dy * ONE_PIXEL <= 0
                {
                    // right
                    prod += dy * ONE_PIXEL;
                    fx2 = ONE_PIXEL;
                    fy2 = udiv(prod, dx_r);
                    self.cover += fy2 - fy1;
                    self.area += (fy2 - fy1) * (fx1 + fx2);
                    fx1 = 0;
                    fy1 = fy2;
                    ex1 += 1;
                } else {
                    // down
                    fy2 = 0;
                    fx2 = udiv(prod, -dy_r);
                    prod += dx * ONE_PIXEL;
                    self.cover += fy2 - fy1;
                    self.area += (fy2 - fy1) * (fx1 + fx2);
                    fx1 = fx2;
                    fy1 = ONE_PIXEL;
                    ey1 -= 1;
                }
                self.set_cell(ex1, ey1);
                if ex1 == ex2 && ey1 == ey2 {
                    break;
                }
            }
        }
        let fx2 = fract(to_x);
        let fy2 = fract(to_y);
        self.cover += fy2 - fy1;
        self.area += (fy2 - fy1) * (fx1 + fx2);
        self.x = to_x;
        self.y = to_y;
    }

    /// gray_render_conic() (control and end point in 26.6)
    fn render_conic(&mut self, cx: i64, cy: i64, tx: i64, ty: i64) {
        let (p0x, p0y) = (self.x, self.y);
        let (p1x, p1y) = (upscale(cx), upscale(cy));
        let (p2x, p2y) = (upscale(tx), upscale(ty));
        if (trunc(p0y) >= self.max_ey && trunc(p1y) >= self.max_ey && trunc(p2y) >= self.max_ey)
            || (trunc(p0y) < self.min_ey && trunc(p1y) < self.min_ey && trunc(p2y) < self.min_ey)
        {
            self.x = p2x;
            self.y = p2y;
            return;
        }
        let bx = p1x - p0x;
        let by = p1y - p0y;
        let ax = p2x - p1x - bx;
        let ay = p2y - p1y - by;
        let mut dx = ax.abs().max(ay.abs());
        if dx <= ONE_PIXEL / 4 {
            self.render_line(p2x, p2y);
            return;
        }
        let mut shift = 0;
        loop {
            dx >>= 2;
            shift += 1;
            if dx <= ONE_PIXEL / 4 {
                break;
            }
        }
        let rx = ax << (33 - 2 * shift);
        let ry = ay << (33 - 2 * shift);
        let mut qx = (bx << (33 - shift)) + (ax << (32 - 2 * shift));
        let mut qy = (by << (33 - shift)) + (ay << (32 - 2 * shift));
        let mut px = p0x << 32;
        let mut py = p0y << 32;
        for _ in 0..(1u32 << shift) {
            px += qx;
            py += qy;
            qx += rx;
            qy += ry;
            self.render_line(px >> 32, py >> 32);
        }
    }

    /// gray_render_cubic() (points in 26.6)
    fn render_cubic(&mut self, c1x: i64, c1y: i64, c2x: i64, c2y: i64, tx: i64, ty: i64) {
        let mut stack = [(0i64, 0i64); 16 * 3 + 1];
        stack[0] = (upscale(tx), upscale(ty));
        stack[1] = (upscale(c2x), upscale(c2y));
        stack[2] = (upscale(c1x), upscale(c1y));
        stack[3] = (self.x, self.y);
        let (me, mi) = (self.max_ey, self.min_ey);
        if stack[..4].iter().all(|p| trunc(p.1) >= me) || stack[..4].iter().all(|p| trunc(p.1) < mi)
        {
            self.x = stack[0].0;
            self.y = stack[0].1;
            return;
        }
        let mut arc = 0usize;
        loop {
            let a = &stack[arc..arc + 4];
            let flat = (2 * a[0].0 - 3 * a[1].0 + a[3].0).abs() <= ONE_PIXEL / 2
                && (2 * a[0].1 - 3 * a[1].1 + a[3].1).abs() <= ONE_PIXEL / 2
                && (a[0].0 - 3 * a[2].0 + 2 * a[3].0).abs() <= ONE_PIXEL / 2
                && (a[0].1 - 3 * a[2].1 + 2 * a[3].1).abs() <= ONE_PIXEL / 2;
            if flat || arc + 7 > stack.len() {
                let (x, y) = stack[arc];
                self.render_line(x, y);
                if arc == 0 {
                    return;
                }
                arc -= 3;
                continue;
            }
            // gray_split_cubic()
            let b = &mut stack[arc..arc + 7];
            for k in 0..2 {
                let g = |p: (i64, i64)| if k == 0 { p.0 } else { p.1 };
                let (v0, v1, v2, v3) = (g(b[0]), g(b[1]), g(b[2]), g(b[3]));
                let mut a_ = v0 + v1;
                let b_ = v1 + v2;
                let mut c = v2 + v3;
                let n6 = v3;
                let n5 = c >> 1;
                c += b_;
                let n4 = c >> 2;
                let n1 = a_ >> 1;
                a_ += b_;
                let n2 = a_ >> 2;
                let n3 = (a_ + c) >> 3;
                let set = |p: &mut (i64, i64), v: i64| {
                    if k == 0 { p.0 = v } else { p.1 = v }
                };
                set(&mut b[6], n6);
                set(&mut b[5], n5);
                set(&mut b[4], n4);
                set(&mut b[1], n1);
                set(&mut b[2], n2);
                set(&mut b[3], n3);
            }
            arc += 3;
        }
    }

    /// gray_sweep(): the 8-bit coverage, rows top down
    fn sweep(mut self) -> Vec<u8> {
        self.record();
        self.valid = false;
        let w = self.max_ex - self.min_ex;
        let h = self.max_ey - self.min_ey;
        let mut out = vec![0u8; (w * h) as usize];
        let fill = |area: i64| -> u8 {
            let mut coverage = (area >> (PIXEL_BITS * 2 + 1 - 8)) as i32;
            if coverage < 0 {
                coverage = !coverage;
            }
            coverage.min(255) as u8
        };
        for y in 0..h {
            let line = ((h - 1 - y) * w) as usize;
            let row = &mut self.cells[y as usize];
            row.sort_by_key(|c| c.0);
            let mut x = self.min_ex;
            let mut cover: i64 = 0;
            let mut i = 0;
            while i < row.len() {
                // merge the visits of one cell
                let cx = row[i].0;
                let (mut c, mut a) = (0, 0);
                while i < row.len() && row[i].0 == cx {
                    c += row[i].1;
                    a += row[i].2;
                    i += 1;
                }
                if cover != 0 && cx > x {
                    let v = fill(cover);
                    out[line + x as usize..line + cx as usize].fill(v);
                }
                cover += c * (ONE_PIXEL * 2);
                let area = cover - a;
                if area != 0 && cx >= self.min_ex {
                    out[line + cx as usize] = fill(area);
                }
                x = cx + 1;
            }
            if cover != 0 && x < self.max_ex {
                let v = fill(cover);
                out[line + x as usize..line + w as usize].fill(v);
            }
        }
        out
    }
}

/// FT_Outline_Decompose() rounding of a coordinate given by skrifa: implied
/// conic on-points are computed as (a + b) / 2 in 26.6 (truncating)
fn f2d6_mid(v: f32) -> i64 {
    let v = v as f64 * 64.0;
    if (v - v.trunc()).abs() == 0.5 {
        v.trunc() as i64
    } else {
        v.round() as i64
    }
}

/// Render an outline (26.6, y up) into a gray bitmap: FT_Glyph_To_Bitmap
/// with FT_RENDER_MODE_NORMAL. None: raster overflow.
fn render_outline(outline: &Outline) -> Option<Bitmap> {
    let (xmin, ymin, xmax, ymax) = outline.cbox();
    let (bx0, by0, bx1, by1) = (
        pix_floor(xmin) >> 6,
        pix_floor(ymin) >> 6,
        pix_ceil(xmax) >> 6,
        pix_ceil(ymax) >> 6,
    );
    if bx0 < -0x8000 || bx1 > 0x7FFF || by0 < -0x8000 || by1 > 0x7FFF {
        return None;
    }
    let width = (bx1 - bx0) as i32;
    let rows = (by1 - by0) as i32;
    let mut bm = Bitmap {
        rows,
        width,
        left: bx0 as i32,
        top: by1 as i32,
        buffer: Vec::new(),
    };
    if rows <= 0 || width <= 0 || outline.segs.is_empty() {
        bm.rows = 0;
        bm.width = 0;
        return Some(bm);
    }
    let mut g = Grays::new(width as i64, rows as i64);
    let (ox, oy) = (bx0 << 6, by0 << 6);
    let mut start: Option<(i64, i64)> = None;
    for s in &outline.segs {
        match *s {
            Seg::Move(x, y) => {
                if let Some((sx, sy)) = start {
                    g.render_line(upscale(sx), upscale(sy));
                }
                start = Some((x - ox, y - oy));
                g.move_to(x - ox, y - oy);
            }
            Seg::Line(x, y) => g.render_line(upscale(x - ox), upscale(y - oy)),
            Seg::Quad(a, b, x, y) => g.render_conic(a - ox, b - oy, x - ox, y - oy),
            Seg::Cubic(a, b, c, d, x, y) => {
                g.render_cubic(a - ox, b - oy, c - ox, d - oy, x - ox, y - oy)
            }
        }
    }
    if let Some((sx, sy)) = start {
        g.render_line(upscale(sx), upscale(sy));
    }
    bm.buffer = g.sweep();
    Some(bm)
}

/// Which hinter FT_Load_Glyph() would run
#[derive(Clone, Copy, PartialEq, Eq)]
enum HintKind {
    /// The native hinter (TrueType bytecode / CFF) or the autohinter as
    /// selected by skrifa's AutoFallback (FreeType's rules)
    Default,
    /// TrueType without instructions loaded with a non axis-aligned
    /// transform: no autohinting; the scaled outline, rounded advance
    Unhinted,
}

/// A loaded glyph: the slot outline and metrics.horiAdvance (26.6)
struct Glyph {
    outline: Outline,
    advance: i64,
}

/// FT_Size with the scales needed here
struct Sizing {
    /// y ppem as skrifa expects it (pixels)
    ppem: f32,
    x_scale: i64,
    /// TrueType driver (non-autohinted) size
    tt_ppem: f32,
    tt_x_scale: i64,
    tt_y_scale: i64,
    x_ppem: i64,
    height: i64,
}

struct Face<'a> {
    font: FontRef<'a>,
    outlines: skrifa::OutlineGlyphCollection<'a>,
    hinter: Option<HintingInstance>,
    sizing: Sizing,
    auto_hinted: bool,
    is_tt: bool,
    num_glyphs: u32,
    advances: Vec<u16>,
    kern: Option<Vec<u8>>,
    hdpi: i32,
    vdpi: i32,
}

/// The unscaled advance widths (hmtx)
fn read_advances(font: &FontRef) -> Vec<u16> {
    let (Some(hhea), Some(hmtx)) = (
        font.table_data(Tag::new(b"hhea")),
        font.table_data(Tag::new(b"hmtx")),
    ) else {
        return Vec::new();
    };
    let n = be16(hhea.as_bytes(), 34).unwrap_or(0) as usize;
    let h = hmtx.as_bytes();
    (0..n).filter_map(|i| be16(h, i * 4)).collect()
}

/// face->height (sfobjs.c)
fn face_height(font: &FontRef) -> i64 {
    let hhea = font.table_data(Tag::new(b"hhea"));
    let os2 = font.table_data(Tag::new(b"OS/2"));
    let h = hhea.as_ref().map(|d| d.as_bytes()).unwrap_or(&[]);
    let o = os2.as_ref().map(|d| d.as_bytes());
    let typo = o.and_then(|o| {
        Some((
            be16s(o, 68)? as i64,
            be16s(o, 70)? as i64,
            be16s(o, 72)? as i64,
            be16(o, 62)?,
        ))
    });
    if let Some((asc, desc, gap, fs_sel)) = typo {
        if fs_sel & 128 != 0 {
            return asc - desc + gap;
        }
    }
    let asc = be16s(h, 4).unwrap_or(0) as i64;
    let desc = be16s(h, 6).unwrap_or(0) as i64;
    let gap = be16s(h, 8).unwrap_or(0) as i64;
    if asc == 0 && desc == 0 {
        if let Some((a, d, g, _)) = typo {
            if a != 0 || d != 0 {
                return a - d + g;
            }
        }
        if let Some(o) = o {
            let wa = be16(o, 74).unwrap_or(0) as i64;
            let wd = be16(o, 76).unwrap_or(0) as i64;
            return wa + wd;
        }
    }
    asc - desc + gap
}

impl<'a> Face<'a> {
    /// FT_Set_Char_Size(face, 0, ptsize * 64, hdpi, vdpi)
    fn new(data: &'a [u8], ptsize: f64, mut hdpi: i32, mut vdpi: i32) -> Result<Face<'a>, String> {
        let font = FontRef::from_index(data, 0).map_err(|_| "Could not read font".to_string())?;
        let outlines = font.outline_glyphs();
        let upem = font
            .head()
            .map(|h| h.units_per_em() as i64)
            .unwrap_or(1000)
            .max(1);
        let head_flags = font.head().map(|h| h.flags().bits()).unwrap_or(0);
        let mut ch = (ptsize * 64.0) as i64;
        let mut cw = 0i64;
        if cw == 0 {
            cw = ch;
        } else if ch == 0 {
            ch = cw;
        }
        if hdpi == 0 {
            hdpi = vdpi;
        } else if vdpi == 0 {
            vdpi = hdpi;
        }
        if cw < 64 {
            cw = 64;
        }
        if ch < 64 {
            ch = 64;
        }
        if hdpi == 0 {
            hdpi = 72;
            vdpi = 72;
        }
        if hdpi < 0 || vdpi < 0 {
            return Err("Could not set character size".into());
        }
        // FT_REQUEST_WIDTH/HEIGHT
        let scaled_w = (cw as i128 * hdpi as i128 + 36) / 72;
        let scaled_h = (ch as i128 * vdpi as i128 + 36) / 72;
        if scaled_w > 0xFFFF * 64 || scaled_h > 0xFFFF * 64 {
            return Err("Could not set character size".into());
        }
        let (scaled_w, scaled_h) = (scaled_w as i64, scaled_h as i64);
        let x_scale = div_fix(scaled_w, upem);
        let y_scale = div_fix(scaled_h, upem);
        let x_ppem = (scaled_w + 32) >> 6;
        let y_ppem = (scaled_h + 32) >> 6;
        let is_tt = font.table_data(Tag::new(b"glyf")).is_some();
        let ppem = scaled_h as f32 / 64.0;
        // metrics.height, as FreeType computes it from the requested size
        let height = pix_round(mul_fix(face_height(&font), y_scale));
        // tt_size_reset(): the TrueType driver scales with an integer ppem
        // when head flag 3 is set (the autohinter keeps the requested
        // scale, size_internal->autohint_metrics)
        let (mut tt_ppem, mut tt_x_scale, mut tt_y_scale) = (ppem, x_scale, y_scale);
        if is_tt {
            if x_ppem < 1 || y_ppem < 1 {
                return Err("Could not set character size".into());
            }
            if head_flags & 8 != 0 {
                tt_x_scale = div_fix(x_ppem << 6, upem);
                tt_y_scale = div_fix(y_ppem << 6, upem);
                tt_ppem = y_ppem as f32;
            }
        }
        let hinter = HintingInstance::new(
            &outlines,
            Size::new(ppem),
            LocationRef::default(),
            HintingOptions::default(),
        )
        .ok();
        // FreeType autohints TrueType glyphs without bytecode
        let auto_hinted = is_tt && !outlines.prefer_interpreter();
        let num_glyphs = font.maxp().map(|m| m.num_glyphs() as u32).unwrap_or(0);
        let advances = read_advances(&font);
        let kern = font
            .table_data(Tag::new(b"kern"))
            .map(|d| d.as_bytes().to_vec());
        Ok(Face {
            font,
            outlines,
            hinter,
            sizing: Sizing {
                ppem,
                x_scale,
                tt_ppem,
                tt_x_scale,
                tt_y_scale,
                x_ppem,
                height,
            },
            auto_hinted,
            is_tt,
            num_glyphs,
            advances,
            kern,
            hdpi,
            vdpi,
        })
    }

    fn unscaled_advance(&self, gid: u32) -> i64 {
        if self.advances.is_empty() {
            return 0;
        }
        let i = (gid as usize).min(self.advances.len() - 1);
        self.advances[i] as i64
    }

    /// FT_Load_Glyph(): the outline (untransformed) and hinted advance
    fn load(&self, gid: u32, kind: HintKind) -> Result<Glyph, String> {
        if gid >= self.num_glyphs.max(1) {
            return Err("Problem loading glyph".into());
        }
        let mut outline = Outline::default();
        let (ppem, scale) = if kind == HintKind::Unhinted {
            (self.sizing.tt_ppem, self.sizing.tt_x_scale)
        } else {
            (self.sizing.ppem, self.sizing.x_scale)
        };
        let mut advance = pix_round(mul_fix(self.unscaled_advance(gid), scale));
        if let Some(g) = self.outlines.get(GlyphId::new(gid)) {
            let r = match (&self.hinter, kind) {
                (Some(h), HintKind::Default) => g.draw(
                    DrawSettings::hinted(h, false),
                    &mut Collect(&mut outline, false),
                ),
                _ if self.is_tt => {
                    // the TrueType loader scales the font units with
                    // FT_MulFix() before decomposing the outline
                    let r = g.draw(
                        DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                        &mut Collect(&mut outline, true),
                    );
                    let ys = self.sizing.tt_y_scale;
                    outline.map(|x, y| (mul_fix(x / 2, scale), mul_fix(y / 2, ys)));
                    r
                }
                _ => g.draw(
                    DrawSettings::unhinted(Size::new(ppem), LocationRef::default()),
                    &mut Collect(&mut outline, false),
                ),
            };
            match r {
                Ok(m) => {
                    if kind == HintKind::Default {
                        if let Some(a) = m.advance_width {
                            advance = f2d6(a);
                        }
                    }
                }
                Err(_) => return Err("Problem loading glyph".into()),
            }
        }
        if self.hdpi != self.vdpi && self.vdpi != 0 {
            // non-square pixels: stretch horizontally
            let m = Matrix {
                xx: div_fix(self.hdpi as i64, self.vdpi as i64),
                xy: 0,
                yx: 0,
                yy: 0x10000,
            };
            outline.transform(&m);
        }
        Ok(Glyph { outline, advance })
    }

    /// FT_Get_Kerning(..., FT_KERNING_DEFAULT): x delta in 26.6
    fn kerning(&self, left: u32, right: u32) -> i64 {
        let Some(k) = &self.kern else {
            return 0;
        };
        let mut value: i64 = 0;
        let version = be16(k, 0).unwrap_or(0);
        if version != 0 {
            return 0;
        }
        let ntables = be16(k, 2).unwrap_or(0) as usize;
        let mut off = 4usize;
        for _ in 0..ntables {
            let Some(len) = be16(k, off + 2) else { break };
            let Some(coverage) = be16(k, off + 4) else {
                break;
            };
            let len = len as usize;
            // format 0, horizontal, not cross-stream
            if coverage >> 8 == 0 && coverage & 1 != 0 && coverage & 4 == 0 {
                let n = be16(k, off + 6).unwrap_or(0) as usize;
                let key = (left << 16) | right;
                let (mut lo, mut hi) = (0usize, n);
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    let p = off + 14 + mid * 6;
                    let Some(pk) = be32(k, p) else { break };
                    if pk == key {
                        let v = be16s(k, p + 4).unwrap_or(0) as i64;
                        if coverage & 8 != 0 {
                            value = v;
                        } else {
                            value += v;
                        }
                        break;
                    } else if pk < key {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
            }
            if len < 6 {
                break;
            }
            off += len;
        }
        if value == 0 {
            return 0;
        }
        let mut x = mul_fix(value, self.sizing.tt_x_scale);
        if self.sizing.x_ppem < 25 {
            x = mul_div(x, self.sizing.x_ppem, 25);
        }
        pix_round(x)
    }

    fn has_kerning(&self) -> bool {
        self.kern.is_some()
    }

    fn char_index(&self, cm: &CharMap, ch: i32) -> u32 {
        if ch < 0 {
            return 0;
        }
        let Some(rec) = cm.record else {
            // synthesized CFF encoding map: map through the Unicode cmap
            return self
                .font
                .charmap()
                .map(ch as u32)
                .map(|g| g.to_u32())
                .unwrap_or(0);
        };
        let Ok(cmap) = self.font.cmap() else {
            return 0;
        };
        match cmap.subtable(rec) {
            Ok(st) => st.map_codepoint(ch as u32).map(|g| g.to_u32()).unwrap_or(0),
            Err(_) => 0,
        }
    }
}

/// tweenColorFetch() for palette images
fn tween_color(im: &mut Image, pixel: i32, bg: i32, fg: i32) -> i32 {
    if fg < 0 {
        if pixel + pixel >= NUMCOLORS { -fg } else { bg }
    } else {
        let npixel = NUMCOLORS - pixel;
        let (f, b) = ((fg as usize) & 0xff, (bg as usize) & 0xff);
        let r = (pixel * im.red[f] + npixel * im.red[b]) / NUMCOLORS;
        let g = (pixel * im.green[f] + npixel * im.green[b]) / NUMCOLORS;
        let bl = (pixel * im.blue[f] + npixel * im.blue[b]) / NUMCOLORS;
        im.color_resolve(r, g, bl)
    }
}

/// gdft_draw_bitmap()
fn draw_bitmap(im: &mut Image, fg: i32, bm: &Bitmap, pen_x: i32, pen_y: i32) {
    let num_grays = 256;
    if im.true_color {
        for row in 0..bm.rows {
            let y = pen_y.wrapping_add(row);
            if y > im.cy2 || y < im.cy1 {
                continue;
            }
            for col in 0..bm.width {
                let v = bm.buffer[(row * bm.width + col) as usize] as i32;
                let mut level = v * ALPHA_MAX / (num_grays - 1);
                if level == 0 {
                    continue;
                }
                if fg >= 0 {
                    level = level * (ALPHA_MAX - true_color_get_alpha(fg)) / ALPHA_MAX;
                }
                level = ALPHA_MAX - level;
                let x = pen_x.wrapping_add(col);
                if x > im.cx2 || x < im.cx1 {
                    continue;
                }
                let tp = &mut im.tpixels[y as usize][x as usize];
                if fg < 0 {
                    if level < ALPHA_MAX / 2 {
                        *tp = fg.wrapping_neg();
                    }
                } else if im.alpha_blending_flag != 0 {
                    let opixel = *tp;
                    if true_color_get_alpha(opixel) != ALPHA_TRANSPARENT {
                        *tp = gd::alpha_blend(opixel, (level << 24) + (fg & 0xFFFFFF));
                    } else {
                        *tp = (level << 24) + (fg & 0xFFFFFF);
                    }
                } else {
                    *tp = (level << 24) + (fg & 0xFFFFFF);
                }
            }
        }
        return;
    }
    for row in 0..bm.rows {
        let y = pen_y.wrapping_add(row);
        if y > im.cy2 || y < im.cy1 {
            continue;
        }
        for col in 0..bm.width {
            let v = bm.buffer[(row * bm.width + col) as usize] as i32;
            let pixel = ((v * NUMCOLORS) + num_grays / 2) / (num_grays - 1);
            if pixel > 0 {
                let x = pen_x.wrapping_add(col);
                if x > im.cx2 || x < im.cx1 {
                    continue;
                }
                let cur = im.pixels[y as usize][x as usize] as i32;
                let nc = if pixel == NUMCOLORS {
                    if fg < 0 { fg.wrapping_neg() } else { fg }
                } else {
                    tween_color(im, pixel, cur, fg)
                };
                im.pixels[y as usize][x as usize] = nc as u8;
            }
        }
    }
}

/// gdroundupdown()
fn roundupdown(v: i64, roundup: bool) -> i32 {
    (if !roundup { v >> 6 } else { (v + 63) >> 6 }) as i32
}

/// gdImageStringFT()
#[allow(clippy::too_many_arguments)]
pub fn image_string_ft(
    im: Option<&mut Image>,
    brect: &mut [i32; 8],
    fg: i32,
    fontlist: &str,
    ptsize: f64,
    angle: f64,
    x: i32,
    y: i32,
    string: &[u8],
) -> Result<(), String> {
    image_string_ft_ex(im, brect, fg, fontlist, ptsize, angle, x, y, string, None)
}

/// gdImageStringFTEx(): Err carries libgd's error string
#[allow(clippy::too_many_arguments)]
pub fn image_string_ft_ex(
    mut im: Option<&mut Image>,
    brect: &mut [i32; 8],
    fg: i32,
    fontlist: &str,
    ptsize: f64,
    angle: f64,
    x: i32,
    y: i32,
    string: &[u8],
    strex: Option<&mut FtStringExtra>,
) -> Result<(), String> {
    let sin_a = angle.sin();
    let cos_a = angle.cos();
    let render = im
        .as_ref()
        .is_some_and(|im| im.true_color || (-255..=255).contains(&fg));
    let mut linespace = LINESPACE;
    let strex = strex.map(|s| s.clone());
    if let Some(s) = &strex {
        if s.flags & FTEX_LINESPACE == FTEX_LINESPACE {
            linespace = s.linespacing;
        }
    }
    let mut m = FTEX_UNICODE;
    if let Some(s) = &strex {
        if s.flags & FTEX_CHARMAP != 0 {
            m = s.charmap;
        }
    }
    let font = font_get(fontlist, m)?;
    let (mut hdpi, mut vdpi) = (RESOLUTION as i32, RESOLUTION as i32);
    if let Some(s) = &strex {
        if s.flags & FTEX_RESOLUTION != 0 {
            hdpi = s.hdpi;
            vdpi = s.vdpi;
        }
    }
    let face = Face::new(&font.data, ptsize, hdpi, vdpi)?;
    let matrix = Matrix {
        xx: (cos_a * 65536.0) as i64,
        yx: (sin_a * 65536.0) as i64,
        xy: -((sin_a * 65536.0) as i64),
        yy: (cos_a * 65536.0) as i64,
    };
    // FT_Load_Glyph() autohints only axis-aligned transforms
    let identity_like = (matrix.yx == 0 && matrix.xx != 0) || (matrix.xx == 0 && matrix.yx != 0);

    let (mut penf_x, mut penf_y) = (0i64, 0i64);
    let (mut pen_x, mut pen_y) = (0i64, 0i64);
    let (mut bxmin, mut bymin, mut bxmax, mut bymax) = (0i64, 0i64, 0i64, 0i64);
    let use_kerning = face.has_kerning();
    let mut previous: u32 = 0;
    let (mut x1, mut y1) = (0i32, 0i32);

    // Try all three types of maps, but start with the specified one
    let mut mfound = false;
    for _ in 0..3 {
        match m {
            FTEX_UNICODE => mfound = font.have_char_map_unicode,
            FTEX_SHIFT_JIS => mfound = font.have_char_map_sjis,
            FTEX_BIG5 => mfound = true,
            _ => {}
        }
        if mfound {
            break;
        }
        m += 1;
        m %= 3;
    }
    if !mfound {
        return Err("No character set found".into());
    }
    let charmap = font.charmaps[font.charmap].clone();
    let symbol =
        charmap.encoding == Encoding::MsSymbol && font.family_name.as_deref() == Some("Symbol");

    let mut next = 0usize;
    let at = |i: usize| -> u8 { string.get(i).copied().unwrap_or(0) };
    let mut i = 0;
    while at(next) != 0 {
        let c = at(next);
        if c == b'\r' {
            penf_x = 0;
            x1 = ((-penf_y as f64 * sin_a + 32.0) as i32) / 64;
            y1 = ((-penf_y as f64 * cos_a + 32.0) as i32) / 64;
            pen_x = 0;
            pen_y = 0;
            previous = 0;
            next += 1;
            continue;
        }
        if c == b'\n' {
            next += 1;
            if at(next) == 0 {
                break;
            }
            penf_x = 0;
            penf_y -= (face.sizing.height as f64 * linespace) as i64;
            penf_y = (penf_y - 32) & -64;
            x1 = ((-penf_y as f64 * sin_a + 32.0) as i32) / 64;
            y1 = ((-penf_y as f64 * cos_a + 32.0) as i32) / 64;
            pen_x = 0;
            pen_y = 0;
            previous = 0;
            continue;
        }
        let mut ch = c as i32;
        if symbol {
            let (u, len) = utf_to_uni_char(&string[next..]);
            ch = u | 0xf000;
            next += len;
        } else {
            match m {
                FTEX_UNICODE => {
                    if font.have_char_map_unicode {
                        let (u, len) = utf_to_uni_char(&string[next..]);
                        ch = u;
                        next += len;
                    }
                }
                FTEX_SHIFT_JIS => {
                    if font.have_char_map_sjis {
                        let c = at(next);
                        if (0xA1..=0xFE).contains(&c) {
                            next += 1;
                            let mut jiscode = 0x100 * (c as i32 & 0x7F) + (at(next) as i32 & 0x7F);
                            let mut h = (jiscode >> 8) & 0xFF;
                            jiscode &= 0xFF;
                            if h & 1 != 0 {
                                jiscode += 0x40 - 0x21;
                            } else {
                                jiscode += 0x9E - 0x21;
                            }
                            if jiscode >= 0x7F {
                                jiscode += 1;
                            }
                            h = (h - 0x21) / 2 + 0x81;
                            if h >= 0xA0 {
                                h += 0x40;
                            }
                            ch = (h << 8) + jiscode;
                        } else {
                            ch = c as i32;
                        }
                        if at(next) != 0 {
                            next += 1;
                        }
                    }
                }
                _ => {
                    ch = at(next) as i32;
                    next += 1;
                    if ch >= 161 && at(next) != 0 {
                        ch = ch * 256 + at(next) as i32;
                        next += 1;
                    }
                }
            }
        }
        let glyph_index = face.char_index(&charmap, ch);
        if use_kerning && previous != 0 && glyph_index != 0 {
            let dx = face.kerning(previous, glyph_index);
            pen_x += (dx as f64 * cos_a) as i32 as i64;
            pen_y -= (dx as f64 * sin_a) as i32 as i64;
            penf_x += dx;
        }

        // bbox: load with FT_LOAD_IGNORE_TRANSFORM
        let g = face.load(glyph_index, HintKind::Default)?;
        let (cx0, cy0, cx1, cy1) = g.outline.cbox();
        let (gx0, gy0) = (pix_floor(cx0) + penf_x, pix_floor(cy0) + penf_y);
        let (mut gx1, gy1) = (pix_ceil(cx1) + penf_x, pix_ceil(cy1) + penf_y);
        if ch == b' ' as i32 {
            gx1 += g.advance;
        }
        if i == 0 {
            bxmin = gx0;
            bymin = gy0;
            bxmax = gx1;
            bymax = gy1;
        } else {
            bxmin = bxmin.min(gx0);
            bymin = bymin.min(gy0);
            bxmax = bxmax.max(gx1);
            bymax = bymax.max(gy1);
        }
        i += 1;
        penf_x += g.advance;

        // the glyph as loaded with the rotation transform
        let mut image_advance = (g.advance, 0i64);
        let mut image_outline: Option<Outline> = None;
        if render || angle != 0.0 {
            let rg = if angle != 0.0 {
                let kind = if face.auto_hinted && !identity_like {
                    HintKind::Unhinted
                } else {
                    HintKind::Default
                };
                if kind == HintKind::Default {
                    Glyph {
                        outline: g.outline.clone(),
                        advance: g.advance,
                    }
                } else {
                    face.load(glyph_index, kind)?
                }
            } else {
                g
            };
            let mut o = rg.outline;
            o.transform(&matrix);
            image_advance = transform(&matrix, rg.advance, 0);
            image_outline = Some(o);
        }
        if render {
            if let (Some(im), Some(o)) = (im.as_deref_mut(), &image_outline) {
                let Some(bm) = render_outline(o) else {
                    return Err("Problem rendering glyph".into());
                };
                let px = x
                    .wrapping_add(x1)
                    .wrapping_add(((pen_x + 31) >> 6) as i32)
                    .wrapping_add(bm.left);
                let py = y
                    .wrapping_add(y1)
                    .wrapping_add(((pen_y + 31) >> 6) as i32)
                    .wrapping_sub(bm.top);
                draw_bitmap(im, fg, &bm, px, py);
            }
        }
        previous = glyph_index;
        // image->advance (16.16) >> 10
        pen_x += (image_advance.0 << 10) >> 10;
        pen_y -= (image_advance.1 << 10) >> 10;
    }

    let d1 = (angle + std::f64::consts::FRAC_PI_4).sin();
    let d2 = (angle - std::f64::consts::FRAC_PI_4).sin();
    let (xmin, ymin, xmax, ymax) = (bxmin as f64, bymin as f64, bxmax as f64, bymax as f64);
    let r = [
        (xmin * cos_a - ymin * sin_a) as i32 as i64,
        (xmin * sin_a + ymin * cos_a) as i32 as i64,
        (xmax * cos_a - ymin * sin_a) as i32 as i64,
        (xmax * sin_a + ymin * cos_a) as i32 as i64,
        (xmax * cos_a - ymax * sin_a) as i32 as i64,
        (xmax * sin_a + ymax * cos_a) as i32 as i64,
        (xmin * cos_a - ymax * sin_a) as i32 as i64,
        (xmin * sin_a + ymax * cos_a) as i32 as i64,
    ];
    brect[0] = x.wrapping_add(roundupdown(r[0], d2 > 0.0));
    brect[1] = y.wrapping_sub(roundupdown(r[1], d1 < 0.0));
    brect[2] = x.wrapping_add(roundupdown(r[2], d1 > 0.0));
    brect[3] = y.wrapping_sub(roundupdown(r[3], d2 > 0.0));
    brect[4] = x.wrapping_add(roundupdown(r[4], d2 < 0.0));
    brect[5] = y.wrapping_sub(roundupdown(r[5], d1 > 0.0));
    brect[6] = x.wrapping_add(roundupdown(r[6], d1 < 0.0));
    brect[7] = y.wrapping_sub(roundupdown(r[7], d2 < 0.0));
    Ok(())
}
