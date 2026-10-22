//! lexer.c: the lexer (GetToken), node/attribute construction, doctype
//! and version handling.

use crate::attrs;
use crate::config;
use crate::doc::*;
use crate::entities;
use crate::enums::code::*;
use crate::enums::{AttrId, OptId, TagId};
use crate::istack;
use crate::message;
use crate::parser;
use crate::streamio::{self, InSel};
use crate::tags;
use crate::tmbstr;
use crate::utf8;

// ---------------------------------------------------------------------------
// doctype table

struct W3CDoctype {
    score: u32,
    vers: u32,
    vers_out: u32,
    #[allow(dead_code)]
    xhtml: bool,
    name: &'static str,
    fpi: Option<&'static str>,
    si: Option<&'static str>,
}

const fn dt(
    score: u32,
    vers: u32,
    vers_out: u32,
    xhtml: bool,
    name: &'static str,
    fpi: Option<&'static str>,
    si: Option<&'static str>,
) -> W3CDoctype {
    W3CDoctype {
        score,
        vers,
        vers_out,
        xhtml,
        name,
        fpi,
        si,
    }
}

static W3C_DOCTYPES: [W3CDoctype; 19] = [
    dt(
        2,
        HT20,
        200,
        false,
        "HTML 2.0",
        Some("-//IETF//DTD HTML 2.0//EN"),
        None,
    ),
    dt(
        2,
        HT20,
        200,
        false,
        "HTML 2.0",
        Some("-//IETF//DTD HTML//EN"),
        None,
    ),
    dt(
        2,
        HT20,
        200,
        false,
        "HTML 2.0",
        Some("-//W3C//DTD HTML 2.0//EN"),
        None,
    ),
    dt(
        1,
        HT32,
        320,
        false,
        "HTML 3.2",
        Some("-//W3C//DTD HTML 3.2//EN"),
        None,
    ),
    dt(
        1,
        HT32,
        320,
        false,
        "HTML 3.2",
        Some("-//W3C//DTD HTML 3.2 Final//EN"),
        None,
    ),
    dt(
        1,
        HT32,
        320,
        false,
        "HTML 3.2",
        Some("-//W3C//DTD HTML 3.2 Draft//EN"),
        None,
    ),
    dt(
        6,
        H40S,
        400,
        false,
        "HTML 4.0 Strict",
        Some("-//W3C//DTD HTML 4.0//EN"),
        Some("http://www.w3.org/TR/REC-html40/strict.dtd"),
    ),
    dt(
        8,
        H40T,
        400,
        false,
        "HTML 4.0 Transitional",
        Some("-//W3C//DTD HTML 4.0 Transitional//EN"),
        Some("http://www.w3.org/TR/REC-html40/loose.dtd"),
    ),
    dt(
        7,
        H40F,
        400,
        false,
        "HTML 4.0 Frameset",
        Some("-//W3C//DTD HTML 4.0 Frameset//EN"),
        Some("http://www.w3.org/TR/REC-html40/frameset.dtd"),
    ),
    dt(
        3,
        H41S,
        401,
        false,
        "HTML 4.01 Strict",
        Some("-//W3C//DTD HTML 4.01//EN"),
        Some("http://www.w3.org/TR/html4/strict.dtd"),
    ),
    dt(
        5,
        H41T,
        401,
        false,
        "HTML 4.01 Transitional",
        Some("-//W3C//DTD HTML 4.01 Transitional//EN"),
        Some("http://www.w3.org/TR/html4/loose.dtd"),
    ),
    dt(
        4,
        H41F,
        401,
        false,
        "HTML 4.01 Frameset",
        Some("-//W3C//DTD HTML 4.01 Frameset//EN"),
        Some("http://www.w3.org/TR/html4/frameset.dtd"),
    ),
    dt(
        9,
        X10S,
        100,
        true,
        "XHTML 1.0 Strict",
        Some("-//W3C//DTD XHTML 1.0 Strict//EN"),
        Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd"),
    ),
    dt(
        11,
        X10T,
        100,
        true,
        "XHTML 1.0 Transitional",
        Some("-//W3C//DTD XHTML 1.0 Transitional//EN"),
        Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"),
    ),
    dt(
        10,
        X10F,
        100,
        true,
        "XHTML 1.0 Frameset",
        Some("-//W3C//DTD XHTML 1.0 Frameset//EN"),
        Some("http://www.w3.org/TR/xhtml1/DTD/xhtml1-frameset.dtd"),
    ),
    dt(
        12,
        XH11,
        110,
        true,
        "XHTML 1.1",
        Some("-//W3C//DTD XHTML 1.1//EN"),
        Some("http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd"),
    ),
    dt(
        13,
        XB10,
        100,
        true,
        "XHTML Basic 1.0",
        Some("-//W3C//DTD XHTML Basic 1.0//EN"),
        Some("http://www.w3.org/TR/xhtml-basic/xhtml-basic10.dtd"),
    ),
    dt(20, HT50, 500, false, "HTML5", None, None),
    dt(21, XH50, 500, true, "XHTML5", None, None),
];

/// VERS_HMTL40PX: just HTML 4 plus XHTML (the former VERS_FROM40)
const VERS_HMTL40PX: u32 = VERS_HTML40 | VERS_XHTML11 | VERS_BASIC;

/// HTMLVersion(): choose what version to use for new doctype
pub fn html_version(doc: &mut Doc) -> u32 {
    let mut j = 0usize;
    let mut score = 0u32;
    let vers = doc.lxr().versions;
    let dtver = doc.lxr().doctype;
    let dtmode = doc.cfg(OptId::DoctypeMode);
    let xhtml =
        (doc.cfg_bool(OptId::XmlOut) || doc.lxr().isvoyager) && !doc.cfg_bool(OptId::HtmlOut);
    let html4 = dtmode == TIDY_DOCTYPE_STRICT
        || dtmode == TIDY_DOCTYPE_LOOSE
        || (VERS_HMTL40PX & dtver) != 0;
    let html5 = !html4 && (dtmode == TIDY_DOCTYPE_AUTO || dtmode == TIDY_DOCTYPE_HTML5);

    if xhtml && dtver == VERS_UNKNOWN {
        return XH50;
    }
    if dtver == VERS_UNKNOWN {
        return HT50;
    }
    if !xhtml && dtver == VERS_HTML5 {
        return HT50;
    }
    if xhtml && html5 && (vers & VERS_HTML5) == XH50 {
        return XH50;
    }

    for (i, d) in W3C_DOCTYPES.iter().enumerate() {
        if (xhtml && (VERS_XHTML & d.vers) == 0) || (html4 && (VERS_HMTL40PX & d.vers) == 0) {
            continue;
        }
        if (vers & d.vers) != 0 && (d.score < score || score == 0) {
            score = d.score;
            j = i;
        }
    }
    if score != 0 {
        return W3C_DOCTYPES[j].vers;
    }
    VERS_UNKNOWN
}

fn get_fpi_from_vers(vers: u32) -> Option<&'static str> {
    W3C_DOCTYPES
        .iter()
        .find(|d| d.vers == vers)
        .and_then(|d| d.fpi)
}

fn get_si_from_vers(vers: u32) -> Option<&'static str> {
    W3C_DOCTYPES
        .iter()
        .find(|d| d.vers == vers)
        .and_then(|d| d.si)
}

fn get_name_from_vers(vers: u32) -> Option<&'static str> {
    W3C_DOCTYPES.iter().find(|d| d.vers == vers).map(|d| d.name)
}

fn get_vers_from_fpi(fpi: &[u8]) -> u32 {
    for d in W3C_DOCTYPES.iter() {
        if let Some(f) = d.fpi {
            if tmbstr::tmbstrcasecmp(f.as_bytes(), fpi) == 0 {
                return d.vers;
            }
        }
    }
    0
}

/// ConstrainVersion(): everything is allowed in proprietary version of HTML
pub fn constrain_version(doc: &mut Doc, vers: u32) {
    doc.lx().versions &= vers | VERS_PROPRIETARY;
}

// ---------------------------------------------------------------------------
// character classes

const fn build_lexmap() -> [u32; 128] {
    let mut m = [0u32; 128];
    let mut i = 0;
    while i < 128 {
        let c = i as u8;
        let mut v = 0;
        if c == b'\r' || c == b'\n' || c == 0x0c {
            v |= NEWLINE | WHITE;
        }
        if c == b' ' || c == b'\t' {
            v |= WHITE;
        }
        if c == b'-' || c == b'.' || c == b':' || c == b'_' {
            v |= NAMECHAR;
        }
        if c.is_ascii_digit() {
            v |= DIGIT | DIGITHEX | NAMECHAR;
        }
        if c.is_ascii_lowercase() {
            v |= LOWERCASE | LETTER | NAMECHAR;
        }
        if c.is_ascii_uppercase() {
            v |= UPPERCASE | LETTER | NAMECHAR;
        }
        if (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F') {
            v |= DIGITHEX;
        }
        m[i] = v;
        i += 1;
    }
    m
}

static LEXMAP: [u32; 128] = build_lexmap();

#[inline]
fn map(c: u32) -> u32 {
    if c < 128 { LEXMAP[c as usize] } else { 0 }
}

pub fn is_white(c: u32) -> bool {
    (map(c) & WHITE) != 0
}

pub fn is_newline(c: u32) -> bool {
    (map(c) & NEWLINE) != 0
}

pub fn is_digit(c: u32) -> bool {
    (map(c) & DIGIT) != 0
}

fn is_digit_hex(c: u32) -> bool {
    (map(c) & DIGITHEX) != 0
}

pub fn is_letter(c: u32) -> bool {
    (map(c) & LETTER) != 0
}

pub fn is_html_space(c: u32) -> bool {
    c == 0x020 || c == 0x009 || c == 0x00a || c == 0x00c || c == 0x00d
}

pub fn is_namechar(c: u32) -> bool {
    (map(c) & NAMECHAR) != 0
}

/// (low, high) inclusive ranges of IsXMLLetter()
static XML_LETTER_RANGES: &[(u32, u32)] = &[
    (0x41, 0x5a),
    (0x61, 0x7a),
    (0xc0, 0xd6),
    (0xd8, 0xf6),
    (0xf8, 0xff),
    (0x100, 0x131),
    (0x134, 0x13e),
    (0x141, 0x148),
    (0x14a, 0x17e),
    (0x180, 0x1c3),
    (0x1cd, 0x1f0),
    (0x1f4, 0x1f5),
    (0x1fa, 0x217),
    (0x250, 0x2a8),
    (0x2bb, 0x2c1),
    (0x386, 0x386),
    (0x388, 0x38a),
    (0x38c, 0x38c),
    (0x38e, 0x3a1),
    (0x3a3, 0x3ce),
    (0x3d0, 0x3d6),
    (0x3da, 0x3da),
    (0x3dc, 0x3dc),
    (0x3de, 0x3de),
    (0x3e0, 0x3e0),
    (0x3e2, 0x3f3),
    (0x401, 0x40c),
    (0x40e, 0x44f),
    (0x451, 0x45c),
    (0x45e, 0x481),
    (0x490, 0x4c4),
    (0x4c7, 0x4c8),
    (0x4cb, 0x4cc),
    (0x4d0, 0x4eb),
    (0x4ee, 0x4f5),
    (0x4f8, 0x4f9),
    (0x531, 0x556),
    (0x559, 0x559),
    (0x561, 0x586),
    (0x5d0, 0x5ea),
    (0x5f0, 0x5f2),
    (0x621, 0x63a),
    (0x641, 0x64a),
    (0x671, 0x6b7),
    (0x6ba, 0x6be),
    (0x6c0, 0x6ce),
    (0x6d0, 0x6d3),
    (0x6d5, 0x6d5),
    (0x6e5, 0x6e6),
    (0x905, 0x939),
    (0x93d, 0x93d),
    (0x958, 0x961),
    (0x985, 0x98c),
    (0x98f, 0x990),
    (0x993, 0x9a8),
    (0x9aa, 0x9b0),
    (0x9b2, 0x9b2),
    (0x9b6, 0x9b9),
    (0x9dc, 0x9dd),
    (0x9df, 0x9e1),
    (0x9f0, 0x9f1),
    (0xa05, 0xa0a),
    (0xa0f, 0xa10),
    (0xa13, 0xa28),
    (0xa2a, 0xa30),
    (0xa32, 0xa33),
    (0xa35, 0xa36),
    (0xa38, 0xa39),
    (0xa59, 0xa5c),
    (0xa5e, 0xa5e),
    (0xa72, 0xa74),
    (0xa85, 0xa8b),
    (0xa8d, 0xa8d),
    (0xa8f, 0xa91),
    (0xa93, 0xaa8),
    (0xaaa, 0xab0),
    (0xab2, 0xab3),
    (0xab5, 0xab9),
    (0xabd, 0xabd),
    (0xae0, 0xae0),
    (0xb05, 0xb0c),
    (0xb0f, 0xb10),
    (0xb13, 0xb28),
    (0xb2a, 0xb30),
    (0xb32, 0xb33),
    (0xb36, 0xb39),
    (0xb3d, 0xb3d),
    (0xb5c, 0xb5d),
    (0xb5f, 0xb61),
    (0xb85, 0xb8a),
    (0xb8e, 0xb90),
    (0xb92, 0xb95),
    (0xb99, 0xb9a),
    (0xb9c, 0xb9c),
    (0xb9e, 0xb9f),
    (0xba3, 0xba4),
    (0xba8, 0xbaa),
    (0xbae, 0xbb5),
    (0xbb7, 0xbb9),
    (0xc05, 0xc0c),
    (0xc0e, 0xc10),
    (0xc12, 0xc28),
    (0xc2a, 0xc33),
    (0xc35, 0xc39),
    (0xc60, 0xc61),
    (0xc85, 0xc8c),
    (0xc8e, 0xc90),
    (0xc92, 0xca8),
    (0xcaa, 0xcb3),
    (0xcb5, 0xcb9),
    (0xcde, 0xcde),
    (0xce0, 0xce1),
    (0xd05, 0xd0c),
    (0xd0e, 0xd10),
    (0xd12, 0xd28),
    (0xd2a, 0xd39),
    (0xd60, 0xd61),
    (0xe01, 0xe2e),
    (0xe30, 0xe30),
    (0xe32, 0xe33),
    (0xe40, 0xe45),
    (0xe81, 0xe82),
    (0xe84, 0xe84),
    (0xe87, 0xe88),
    (0xe8a, 0xe8a),
    (0xe8d, 0xe8d),
    (0xe94, 0xe97),
    (0xe99, 0xe9f),
    (0xea1, 0xea3),
    (0xea5, 0xea5),
    (0xea7, 0xea7),
    (0xeaa, 0xeab),
    (0xead, 0xeae),
    (0xeb0, 0xeb0),
    (0xeb2, 0xeb3),
    (0xebd, 0xebd),
    (0xec0, 0xec4),
    (0xf40, 0xf47),
    (0xf49, 0xf69),
    (0x10a0, 0x10c5),
    (0x10d0, 0x10f6),
    (0x1100, 0x1100),
    (0x1102, 0x1103),
    (0x1105, 0x1107),
    (0x1109, 0x1109),
    (0x110b, 0x110c),
    (0x110e, 0x1112),
    (0x113c, 0x113c),
    (0x113e, 0x113e),
    (0x1140, 0x1140),
    (0x114c, 0x114c),
    (0x114e, 0x114e),
    (0x1150, 0x1150),
    (0x1154, 0x1155),
    (0x1159, 0x1159),
    (0x115f, 0x1161),
    (0x1163, 0x1163),
    (0x1165, 0x1165),
    (0x1167, 0x1167),
    (0x1169, 0x1169),
    (0x116d, 0x116e),
    (0x1172, 0x1173),
    (0x1175, 0x1175),
    (0x119e, 0x119e),
    (0x11a8, 0x11a8),
    (0x11ab, 0x11ab),
    (0x11ae, 0x11af),
    (0x11b7, 0x11b8),
    (0x11ba, 0x11ba),
    (0x11bc, 0x11c2),
    (0x11eb, 0x11eb),
    (0x11f0, 0x11f0),
    (0x11f9, 0x11f9),
    (0x1e00, 0x1e9b),
    (0x1ea0, 0x1ef9),
    (0x1f00, 0x1f15),
    (0x1f18, 0x1f1d),
    (0x1f20, 0x1f45),
    (0x1f48, 0x1f4d),
    (0x1f50, 0x1f57),
    (0x1f59, 0x1f59),
    (0x1f5b, 0x1f5b),
    (0x1f5d, 0x1f5d),
    (0x1f5f, 0x1f7d),
    (0x1f80, 0x1fb4),
    (0x1fb6, 0x1fbc),
    (0x1fbe, 0x1fbe),
    (0x1fc2, 0x1fc4),
    (0x1fc6, 0x1fcc),
    (0x1fd0, 0x1fd3),
    (0x1fd6, 0x1fdb),
    (0x1fe0, 0x1fec),
    (0x1ff2, 0x1ff4),
    (0x1ff6, 0x1ffc),
    (0x2126, 0x2126),
    (0x212a, 0x212b),
    (0x212e, 0x212e),
    (0x2180, 0x2182),
    (0x3041, 0x3094),
    (0x30a1, 0x30fa),
    (0x3105, 0x312c),
    (0xac00, 0xd7a3),
    (0x4e00, 0x9fa5),
    (0x3007, 0x3007),
    (0x3021, 0x3029),
];

/// (low, high) inclusive ranges IsXMLNamechar() adds to IsXMLLetter()
static XML_NAMECHAR_RANGES: &[(u32, u32)] = &[
    (0x2e, 0x2e),
    (0x5f, 0x5f),
    (0x3a, 0x3a),
    (0x2d, 0x2d),
    (0x300, 0x345),
    (0x360, 0x361),
    (0x483, 0x486),
    (0x591, 0x5a1),
    (0x5a3, 0x5b9),
    (0x5bb, 0x5bd),
    (0x5bf, 0x5bf),
    (0x5c1, 0x5c2),
    (0x5c4, 0x5c4),
    (0x64b, 0x652),
    (0x670, 0x670),
    (0x6d6, 0x6dc),
    (0x6dd, 0x6df),
    (0x6e0, 0x6e4),
    (0x6e7, 0x6e8),
    (0x6ea, 0x6ed),
    (0x901, 0x903),
    (0x93c, 0x93c),
    (0x93e, 0x94c),
    (0x94d, 0x94d),
    (0x951, 0x954),
    (0x962, 0x963),
    (0x981, 0x983),
    (0x9bc, 0x9bc),
    (0x9be, 0x9be),
    (0x9bf, 0x9bf),
    (0x9c0, 0x9c4),
    (0x9c7, 0x9c8),
    (0x9cb, 0x9cd),
    (0x9d7, 0x9d7),
    (0x9e2, 0x9e3),
    (0xa02, 0xa02),
    (0xa3c, 0xa3c),
    (0xa3e, 0xa3e),
    (0xa3f, 0xa3f),
    (0xa40, 0xa42),
    (0xa47, 0xa48),
    (0xa4b, 0xa4d),
    (0xa70, 0xa71),
    (0xa81, 0xa83),
    (0xabc, 0xabc),
    (0xabe, 0xac5),
    (0xac7, 0xac9),
    (0xacb, 0xacd),
    (0xb01, 0xb03),
    (0xb3c, 0xb3c),
    (0xb3e, 0xb43),
    (0xb47, 0xb48),
    (0xb4b, 0xb4d),
    (0xb56, 0xb57),
    (0xb82, 0xb83),
    (0xbbe, 0xbc2),
    (0xbc6, 0xbc8),
    (0xbca, 0xbcd),
    (0xbd7, 0xbd7),
    (0xc01, 0xc03),
    (0xc3e, 0xc44),
    (0xc46, 0xc48),
    (0xc4a, 0xc4d),
    (0xc55, 0xc56),
    (0xc82, 0xc83),
    (0xcbe, 0xcc4),
    (0xcc6, 0xcc8),
    (0xcca, 0xccd),
    (0xcd5, 0xcd6),
    (0xd02, 0xd03),
    (0xd3e, 0xd43),
    (0xd46, 0xd48),
    (0xd4a, 0xd4d),
    (0xd57, 0xd57),
    (0xe31, 0xe31),
    (0xe34, 0xe3a),
    (0xe47, 0xe4e),
    (0xeb1, 0xeb1),
    (0xeb4, 0xeb9),
    (0xebb, 0xebc),
    (0xec8, 0xecd),
    (0xf18, 0xf19),
    (0xf35, 0xf35),
    (0xf37, 0xf37),
    (0xf39, 0xf39),
    (0xf3e, 0xf3e),
    (0xf3f, 0xf3f),
    (0xf71, 0xf84),
    (0xf86, 0xf8b),
    (0xf90, 0xf95),
    (0xf97, 0xf97),
    (0xf99, 0xfad),
    (0xfb1, 0xfb7),
    (0xfb9, 0xfb9),
    (0x20d0, 0x20dc),
    (0x20e1, 0x20e1),
    (0x302a, 0x302f),
    (0x3099, 0x3099),
    (0x309a, 0x309a),
    (0x30, 0x39),
    (0x660, 0x669),
    (0x6f0, 0x6f9),
    (0x966, 0x96f),
    (0x9e6, 0x9ef),
    (0xa66, 0xa6f),
    (0xae6, 0xaef),
    (0xb66, 0xb6f),
    (0xbe7, 0xbef),
    (0xc66, 0xc6f),
    (0xce6, 0xcef),
    (0xd66, 0xd6f),
    (0xe50, 0xe59),
    (0xed0, 0xed9),
    (0xf20, 0xf29),
    (0xb7, 0xb7),
    (0x2d0, 0x2d0),
    (0x2d1, 0x2d1),
    (0x387, 0x387),
    (0x640, 0x640),
    (0xe46, 0xe46),
    (0xec6, 0xec6),
    (0x3005, 0x3005),
    (0x3031, 0x3035),
    (0x309d, 0x309e),
    (0x30fc, 0x30fe),
];

pub fn is_xml_letter(c: u32) -> bool {
    XML_LETTER_RANGES.iter().any(|&(lo, hi)| c >= lo && c <= hi)
}

pub fn is_xml_namechar(c: u32) -> bool {
    is_xml_letter(c)
        || XML_NAMECHAR_RANGES
            .iter()
            .any(|&(lo, hi)| c >= lo && c <= hi)
}

pub fn is_upper(c: u32) -> bool {
    (map(c) & UPPERCASE) != 0
}

pub fn to_lower(c: u32) -> u32 {
    if map(c) & UPPERCASE != 0 {
        c + (b'a' - b'A') as u32
    } else {
        c
    }
}

pub fn to_upper(c: u32) -> u32 {
    if map(c) & LOWERCASE != 0 {
        c - (b'a' - b'A') as u32
    } else {
        c
    }
}

/// LastChar(): last character in string
fn last_char(s: Option<&[u8]>) -> u8 {
    match s {
        Some(s) => {
            let s = tmbstr::cstr(s);
            s.last().copied().unwrap_or(0)
        }
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// lexer construction / buffer

/// NewLexer()
pub fn new_lexer(doc: &mut Doc) -> Box<Lexer> {
    Box::new(Lexer {
        lines: 1,
        columns: 1,
        state: LexerState::LexContent,
        versions: VERS_ALL | VERS_PROPRIETARY,
        doctype: VERS_UNKNOWN,
        root: Some(doc.root),
        ..Default::default()
    })
}

fn end_of_input(doc: &mut Doc) -> bool {
    let pushed = doc.doc_in.as_ref().expect("tidy: no input stream").pushed;
    !pushed && streamio::is_eof(doc, InSel::Doc)
}

/// FreeLexer()
pub fn free_lexer(doc: &mut Doc) {
    if doc.lexer.is_some() {
        // FreeStyles(), the token frees and popping the inline stack only
        // release memory.
        while !doc.lxr().istack.is_empty() {
            istack::pop_inline(doc, None);
        }
        doc.lexer = None;
    }
}

/// Grow the (zero-filled) allocation of lexbuf to at least `n` bytes.
fn lexbuf_reserve(lx: &mut Lexer, n: usize) {
    if lx.lexbuf.len() < n {
        lx.lexbuf.resize(n, 0);
    }
}

/// AddByte(): the byte, then a NUL after it
fn add_byte(doc: &mut Doc, ch: u8) {
    let lx = doc.lx();
    let ls = lx.lexsize as usize;
    lexbuf_reserve(lx, ls + 2);
    lx.lexbuf[ls] = ch;
    lx.lexsize += 1;
    lx.lexbuf[ls + 1] = 0;
}

fn change_char(doc: &mut Doc, c: u8) {
    let lx = doc.lx();
    if lx.lexsize > 0 {
        let i = lx.lexsize as usize - 1;
        lx.lexbuf[i] = c;
    }
}

/// `lexer->lexsize = n`
fn set_lexsize(doc: &mut Doc, n: u32) {
    let lx = doc.lx();
    lx.lexsize = n;
    lexbuf_reserve(lx, n as usize);
}

/// `lexer->lexsize -= n`
fn dec_lexsize(doc: &mut Doc, n: u32) {
    let s = doc.lxr().lexsize();
    set_lexsize(doc, s.wrapping_sub(n));
}

/// `lexer->lexbuf[i] = '\0'`
fn set_nul(doc: &mut Doc, i: u32) {
    set_lexbyte(doc, i, 0);
}

fn lexbyte(doc: &Doc, i: u32) -> u8 {
    doc.lxr().lexbuf.get(i as usize).copied().unwrap_or(0)
}

fn set_lexbyte(doc: &mut Doc, i: u32, c: u8) {
    let lx = doc.lx();
    lexbuf_reserve(lx, i as usize + 1);
    lx.lexbuf[i as usize] = c;
}

/// The C string at `lexbuf + start`.
fn lexstr(doc: &Doc, start: u32) -> Vec<u8> {
    let lb = &doc.lxr().lexbuf;
    let s = (start as usize).min(lb.len());
    tmbstr::cstr(&lb[s..]).to_vec()
}

/// `tmbstrndup(lexbuf + start, len)`
fn lexndup(doc: &Doc, start: u32, len: u32) -> Option<Vec<u8>> {
    let lb = &doc.lxr().lexbuf;
    let s = (start as usize).min(lb.len());
    tmbstr::tmbstrndup(&lb[s..], len as usize)
}

/// AddCharToLexer(): store character c as UTF-8 encoded byte stream
pub fn add_char_to_lexer(doc: &mut Doc, c: u32) {
    let (err, buf, count) = utf8::encode_char_to_utf8_bytes(c);
    if err {
        add_byte(doc, 0xEF);
        add_byte(doc, 0xBF);
        add_byte(doc, 0xBD);
    } else {
        for &b in &buf[..count as usize] {
            add_byte(doc, b);
        }
    }
}

fn add_string_to_lexer(doc: &mut Doc, s: &[u8]) {
    for &c in tmbstr::cstr(s) {
        add_char_to_lexer(doc, c as u32);
    }
}

fn set_lexer_locus(doc: &mut Doc) {
    let (l, c) = {
        let i = doc.doc_in.as_ref().expect("tidy: no input stream");
        (i.curline, i.curcol)
    };
    let lx = doc.lx();
    lx.lines = l as u32;
    lx.columns = c as u32;
}

#[inline]
fn read(doc: &mut Doc) -> u32 {
    streamio::read_char(doc, InSel::Doc)
}

#[inline]
fn unget(doc: &mut Doc, c: u32) {
    streamio::unget_char(doc, InSel::Doc, c);
}

fn curcol_add(doc: &mut Doc, n: i32) {
    if let Some(i) = doc.doc_in.as_mut() {
        i.curcol += n;
    }
}

// ---------------------------------------------------------------------------
// entities

#[derive(PartialEq, Eq)]
enum SpStatus {
    Ok,
    Failed,
    Error,
}

/// GetSurrogatePair(): have detected the first of a surrogate pair; try to
/// find and decode the second (already past the '&').
fn get_surrogate_pair(doc: &mut Doc, is_xml: bool, pch: &mut u32) -> SpStatus {
    let mut c;
    let mut ch: u32 = 0;
    let mut buf: Vec<u8> = Vec::new();
    let mut status = SpStatus::Error;
    let mut hex = false;
    let fch = *pch;
    if doc.lexer.is_none() {
        return status;
    }
    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        if c == ';' as u32 {
            break;
        }
        buf.push(c as u8);
        let offset = buf.len();
        if offset == 1 {
            if c != '#' as u32 {
                break;
            }
        } else if offset == 2 && (c == 'x' as u32 || (!is_xml && c == 'X' as u32)) {
            hex = true;
        } else if hex {
            if !is_digit_hex(c) {
                break;
            }
        } else if !is_digit(c) {
            break;
        }
    }

    if c == ';' as u32 {
        let scanned = if hex {
            entities::scanf_uint(buf.get(2..).unwrap_or(&[]), true)
        } else {
            entities::scanf_uint(buf.get(1..).unwrap_or(&[]), false)
        };
        if let Some(v) = scanned {
            ch = v;
        }
        if scanned.is_some() && utf8::is_high_surrogate(ch) {
            ch = utf8::combine_surrogate_pair(ch, fch);
            if utf8::is_valid_combined_char(ch) {
                *pch = ch;
                status = SpStatus::Ok;
            } else {
                status = SpStatus::Failed;
                *pch = 0xFFFD;
                message::report_surrogate_error(doc, BAD_SURROGATE_PAIR, fch, ch);
            }
        }
    }

    if status == SpStatus::Error {
        if c == ';' as u32 {
            unget(doc, c);
        }
        for &b in buf.iter().rev() {
            unget(doc, b as u32);
        }
    }
    status
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntState {
    Default,
    NumDec,
    NumHex,
}

/// ParseEntity(): handles entities and numeric character references
fn parse_entity(doc: &mut Doc, mode: GetTokenMode) {
    let mut ent_state = EntState::Default;
    let mut char_read = 0u32;
    let mut semicolon = false;
    let found;
    let is_xml = doc.cfg_bool(OptId::XmlTags);
    let mut preserve_entities = doc.cfg_bool(OptId::PreserveEntities);
    let mut c;
    let mut ch: u32;
    let entver: u32;

    let start = doc.lxr().lexsize().wrapping_sub(1);
    let startcol = doc.doc_in.as_ref().map_or(0, |i| i.curcol) - 1;

    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        if c == ';' as u32 {
            semicolon = true;
            break;
        }
        char_read += 1;

        if char_read == 1 && c == '#' as u32 {
            let enc = doc.cfg(OptId::InCharEncoding);
            if !doc.cfg_bool(OptId::NCR) || enc == BIG5 || enc == SHIFTJIS {
                unget(doc, '#' as u32);
                return;
            }
            add_char_to_lexer(doc, c);
            ent_state = EntState::NumDec;
            continue;
        } else if char_read == 2
            && ent_state == EntState::NumDec
            && (c == 'x' as u32 || (!is_xml && c == 'X' as u32))
        {
            add_char_to_lexer(doc, c);
            ent_state = EntState::NumHex;
            continue;
        }

        let ok = match ent_state {
            EntState::Default => is_namechar(c),
            EntState::NumDec => is_digit(c),
            EntState::NumHex => is_digit_hex(c),
        };
        if ok {
            add_char_to_lexer(doc, c);
            continue;
        }

        unget(doc, c);
        break;
    }

    let ent = lexstr(doc, start);

    if tmbstr::tmbstrcmp(&ent, b"&apos") == 0
        && !doc.cfg_bool(OptId::XmlOut)
        && !doc.lxr().isvoyager
        && !doc.cfg_bool(OptId::XhtmlOut)
        && html_version(doc) != HT50
    {
        message::report_entity_error(doc, APOS_UNDEFINED, Some(&ent), 39);
    }

    if mode == GetTokenMode::OtherNamespace && c == ';' as u32 {
        found = true;
        ch = 255;
        entver = XH50 | HT50;
        preserve_entities = true;
    } else {
        let (f, code, v) = entities::entity_info(&ent, is_xml);
        found = f;
        ch = code;
        entver = v;
    }

    if !preserve_entities && found && utf8::is_low_surrogate(ch) {
        let mut c1 = read(doc);
        if c1 == '&' as u32 {
            c1 = ch;
            let status = get_surrogate_pair(doc, is_xml, &mut ch);
            if status == SpStatus::Error {
                message::report_surrogate_error(doc, BAD_SURROGATE_TAIL, c1, 0);
                unget(doc, '&' as u32);
            }
        } else {
            unget(doc, c1);
            message::report_surrogate_error(doc, BAD_SURROGATE_TAIL, ch, 0);
            ch = 0xFFFD;
        }
    } else if !preserve_entities && found && utf8::is_high_surrogate(ch) {
        message::report_surrogate_error(doc, BAD_SURROGATE_LEAD, ch, 0);
        ch = 0xFFFD;
    }

    if !found || (128..=159).contains(&ch) || (ch >= 256 && c != ';' as u32) {
        set_lexer_locus(doc);
        doc.lx().columns = startcol as u32;

        if doc.lxr().lexsize() > start.wrapping_add(1) {
            if (128..=159).contains(&ch) {
                let mut replace_mode = DISCARDED_CHAR;
                let c1 = streamio::decode_win1252(ch);
                if c1 != 0 {
                    replace_mode = REPLACED_CHAR;
                }
                if c != ';' as u32 {
                    let ent = lexstr(doc, start);
                    message::report_entity_error(doc, MISSING_SEMICOLON_NCR, Some(&ent), c as i32);
                }
                message::report_encoding_error(
                    doc,
                    INVALID_NCR,
                    ch,
                    replace_mode == DISCARDED_CHAR,
                );
                set_lexsize(doc, start);
                if c1 != 0 {
                    add_char_to_lexer(doc, c1);
                }
                semicolon = false;
            } else {
                let ent = lexstr(doc, start);
                message::report_entity_error(doc, UNKNOWN_ENTITY, Some(&ent), ch as i32);
            }
            if semicolon {
                add_char_to_lexer(doc, ';' as u32);
            }
        } else if html_version(doc) != HT50 {
            let ent = lexstr(doc, start);
            message::report_entity_error(doc, UNESCAPED_AMPERSAND, Some(&ent), ch as i32);
        }
    } else {
        if c != ';' as u32 {
            set_lexer_locus(doc);
            doc.lx().columns = startcol as u32;
            let ent = lexstr(doc, start);
            message::report_entity_error(doc, MISSING_SEMICOLON, Some(&ent), c as i32);
        }
        if preserve_entities {
            add_char_to_lexer(doc, ';' as u32);
        } else {
            set_lexsize(doc, start);
            if ch == 160 && mode == GetTokenMode::Preformatted {
                ch = ' ' as u32;
            }
            add_char_to_lexer(doc, ch);
            if ch == '&' as u32 && !doc.cfg_bool(OptId::QuoteAmpersand) {
                add_string_to_lexer(doc, b"amp;");
            }
        }
        constrain_version(doc, entver);
    }
}

/// ParseTagName(): returns the terminating char as `(tmbchar) c`
fn parse_tag_name(doc: &mut Doc) -> u32 {
    let txtstart = doc.lxr().txtstart;
    let mut c = lexbyte(doc, txtstart) as u32;
    let xml = doc.cfg_bool(OptId::XmlTags);

    if !xml && is_upper(c) {
        set_lexbyte(doc, txtstart, to_lower(c) as u8);
    }
    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        if (!xml && !is_namechar(c)) || (xml && !is_xml_namechar(c)) {
            break;
        }
        if !xml && is_upper(c) {
            c = to_lower(c);
        }
        add_char_to_lexer(doc, c);
    }
    let ls = doc.lxr().lexsize();
    doc.lx().txtend = ls;
    // (tmbchar) c: a signed char converted back to uint
    (c as u8 as i8) as i32 as u32
}

// ---------------------------------------------------------------------------
// nodes

/// `NewNode(allocator, lexer)` with or without a lexer
fn new_node_with(doc: &mut Doc, use_lexer: bool) -> NodeId {
    let id = doc.alloc_node();
    if use_lexer {
        if let Some(lx) = doc.lexer.as_ref() {
            let (l, c) = (lx.lines, lx.columns);
            doc[id].line = l;
            doc[id].column = c;
        }
    }
    doc[id].type_ = NodeType::TextNode;
    id
}

/// NewNode(): a TextNode at the lexer's position (if there is a lexer)
pub fn new_node(doc: &mut Doc) -> NodeId {
    new_node_with(doc, true)
}

/// CloneNode(): used to clone heading nodes when split by an <HR>
pub fn clone_node(doc: &mut Doc, element: Option<NodeId>) -> NodeId {
    let node = new_node(doc);
    let ls = doc.lxr().lexsize();
    doc[node].start = ls;
    doc[node].end = ls;
    if let Some(e) = element {
        let src = doc[e].clone();
        let attrs = istack::dup_attrs(doc, src.attributes);
        let n = &mut doc[node];
        n.parent = src.parent;
        n.type_ = src.type_;
        n.closed = src.closed;
        n.implicit = src.implicit;
        n.tag = src.tag;
        n.element = src.element;
        n.attributes = attrs;
    }
    node
}

/// FreeAttrs()
pub fn free_attrs(doc: &mut Doc, node: NodeId) {
    while let Some(av) = doc[node].attributes {
        if doc[av].attribute.is_some()
            && (doc.attr_is(Some(av), AttrId::ID) || doc.attr_is(Some(av), AttrId::NAME))
            && attrs::is_anchor_element(doc, node)
        {
            let v = doc[av].value.clone().unwrap_or_default();
            attrs::remove_anchor_by_node(doc, &v, node);
        }
        doc[node].attributes = doc[av].next;
        free_attribute(doc, av);
    }
}

/// FreeAttribute(): doesn't repair attribute list linkage
pub fn free_attribute(doc: &mut Doc, av: AttId) {
    let (asp, php) = (doc[av].asp, doc[av].php);
    free_node(doc, asp);
    free_node(doc, php);
}

/// DetachAttribute(): detach attribute from node
pub fn detach_attribute(doc: &mut Doc, node: NodeId, attr: AttId) {
    let mut prev: Option<AttId> = None;
    let mut av = doc[node].attributes;
    while let Some(a) = av {
        if a == attr {
            let next = doc[attr].next;
            match prev {
                Some(p) => doc[p].next = next,
                None => doc[node].attributes = next,
            }
            break;
        }
        prev = Some(a);
        av = doc[a].next;
    }
}

/// RemoveAttribute(): detach attribute from node then free it
pub fn remove_attribute(doc: &mut Doc, node: NodeId, attr: AttId) {
    detach_attribute(doc, node, attr);
    free_attribute(doc, attr);
}

/// FreeNode(): frees the node, its children and its following siblings.
/// The arena keeps the nodes; the observable effects are the anchor list
/// updates of FreeAttrs() and the root's content being cleared.
pub fn free_node(doc: &mut Doc, node: Option<NodeId>) {
    let mut node = node;
    while let Some(n) = node {
        let next = doc[n].next;
        let content = doc[n].content;
        // detach the attribute list first (FreeAttrs consumes it)
        free_attrs_keep(doc, n);
        free_node(doc, content);
        if doc[n].type_ == NodeType::RootNode {
            doc[n].content = None;
        }
        node = next;
    }
}

/// FreeAttrs() as called by FreeNode(): the anchors are removed, but the
/// attribute list stays readable (the node is dead anyway, and stale
/// references to it keep working like they would on unreused C memory).
fn free_attrs_keep(doc: &mut Doc, node: NodeId) {
    let mut a = doc[node].attributes;
    while let Some(av) = a {
        if doc[av].attribute.is_some()
            && (doc.attr_is(Some(av), AttrId::ID) || doc.attr_is(Some(av), AttrId::NAME))
            && attrs::is_anchor_element(doc, node)
        {
            let v = doc[av].value.clone().unwrap_or_default();
            attrs::remove_anchor_by_node(doc, &v, node);
        }
        a = doc[av].next;
        free_attribute(doc, av);
    }
}

/// TextToken()
pub fn text_token(doc: &mut Doc) -> NodeId {
    let node = new_node(doc);
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    doc[node].start = s;
    doc[node].end = e;
    node
}

/// NewLineNode(): used for creating preformatted text from Word2000
pub fn new_line_node(doc: &mut Doc) -> NodeId {
    let node = new_node(doc);
    doc[node].start = doc.lxr().lexsize();
    add_char_to_lexer(doc, '\n' as u32);
    doc[node].end = doc.lxr().lexsize();
    node
}

/// NewLiteralTextNode(): used for adding a &nbsp; for Word2000
pub fn new_literal_text_node(doc: &mut Doc, txt: &[u8]) -> NodeId {
    let node = new_node(doc);
    doc[node].start = doc.lxr().lexsize();
    add_string_to_lexer(doc, txt);
    doc[node].end = doc.lxr().lexsize();
    node
}

fn tag_token(doc: &mut Doc, type_: NodeType) -> NodeId {
    let node = new_node(doc);
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    let el = lexndup(doc, s, e.wrapping_sub(s));
    doc[node].type_ = type_;
    doc[node].element = el;
    doc[node].start = s;
    doc[node].end = s;
    if matches!(
        type_,
        NodeType::StartTag | NodeType::StartEndTag | NodeType::EndTag
    ) {
        tags::find_tag(doc, node);
    }
    node
}

fn new_token(doc: &mut Doc, type_: NodeType) -> NodeId {
    let node = new_node(doc);
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    doc[node].type_ = type_;
    doc[node].start = s;
    doc[node].end = e;
    node
}

/// AddStringLiteral()
pub fn add_string_literal(doc: &mut Doc, s: &[u8]) {
    for &c in tmbstr::cstr(s) {
        add_byte(doc, c);
    }
}

fn root_content(doc: &Doc) -> Option<NodeId> {
    doc[doc.root].content
}

/// FindDocType()
pub fn find_doc_type(doc: &mut Doc) -> Option<NodeId> {
    let mut node = root_content(doc);
    while let Some(n) = node {
        if doc[n].type_ == NodeType::DocTypeTag {
            break;
        }
        node = doc[n].next;
    }
    node
}

/// FindContainer(): returns containing block element, if any
pub fn find_container(doc: &mut Doc, node: Option<NodeId>) -> Option<NodeId> {
    let mut node = node.and_then(|n| doc[n].parent);
    while let Some(n) = node {
        if !tags::node_has_cm(doc, Some(n), CM_INLINE) {
            break;
        }
        node = doc[n].parent;
    }
    node
}

/// FindHTML()
pub fn find_html(doc: &mut Doc) -> Option<NodeId> {
    let mut node = root_content(doc);
    while let Some(n) = node {
        if doc.is(n, TagId::HTML) {
            break;
        }
        node = doc[n].next;
    }
    node
}

/// FindXmlDecl()
pub fn find_xml_decl(doc: &mut Doc) -> Option<NodeId> {
    let mut node = root_content(doc);
    while let Some(n) = node {
        if doc[n].type_ == NodeType::XmlDecl {
            break;
        }
        node = doc[n].next;
    }
    node
}

/// FindHEAD()
pub fn find_head(doc: &mut Doc) -> Option<NodeId> {
    let mut node = find_html(doc);
    if let Some(h) = node {
        node = doc[h].content;
        while let Some(n) = node {
            if doc.is(n, TagId::HEAD) {
                break;
            }
            node = doc[n].next;
        }
    }
    node
}

/// FindTITLE()
pub fn find_title(doc: &mut Doc) -> Option<NodeId> {
    let mut node = find_head(doc);
    if let Some(h) = node {
        node = doc[h].content;
        while let Some(n) = node {
            if doc.is(n, TagId::TITLE) {
                break;
            }
            node = doc[n].next;
        }
    }
    node
}

/// FindBody()
pub fn find_body(doc: &mut Doc) -> Option<NodeId> {
    let mut node = root_content(doc);
    while let Some(n) = node {
        if doc.is(n, TagId::HTML) {
            break;
        }
        node = doc[n].next;
    }
    let html = node?;
    node = doc[html].content;
    while let Some(n) = node {
        if doc.is(n, TagId::BODY) || doc.is(n, TagId::FRAMESET) {
            break;
        }
        node = doc[n].next;
    }
    if let Some(fs) = node {
        if doc.is(fs, TagId::FRAMESET) {
            node = doc[fs].content;
            while let Some(n) = node {
                if doc.is(n, TagId::NOFRAMES) {
                    break;
                }
                node = doc[n].next;
            }
            if let Some(nf) = node {
                node = doc[nf].content;
                while let Some(n) = node {
                    if doc.is(n, TagId::BODY) {
                        break;
                    }
                    node = doc[n].next;
                }
            }
        }
    }
    node
}

/// AddGenerator(): add meta element for Tidy
pub fn add_generator(doc: &mut Doc) -> bool {
    let Some(head) = find_head(doc) else {
        return false;
    };
    let buf = format!(
        "HTML Tidy for HTML5 for Linux version {}",
        message::library_version()
    )
    .into_bytes();

    let mut node = doc[head].content;
    while let Some(n) = node {
        if doc.is(n, TagId::META) {
            let attval = attrs::attr_get_by_id(doc, n, AttrId::NAME);
            if doc.attr_value_is(attval, b"generator") {
                let attval = attrs::attr_get_by_id(doc, n, AttrId::CONTENT);
                if doc.attr_has_value(attval) {
                    let a = attval.expect("attval");
                    let v = doc[a].value.clone().unwrap_or_default();
                    if tmbstr::tmbstrncasecmp(&v, b"HTML Tidy", 9) == 0 {
                        doc[a].value = Some(buf);
                        return false;
                    }
                }
            }
        }
        node = doc[n].next;
    }

    if doc.cfg(OptId::AccessibilityCheckLevel) == 0 {
        let node = inferred_tag(doc, TagId::META);
        attrs::add_attribute(doc, node, b"name", b"generator");
        attrs::add_attribute(doc, node, b"content", &buf);
        parser::insert_node_at_start(doc, head, node);
        return true;
    }
    false
}

/// FindGivenVersion(): examine <!DOCTYPE ...> to identify version
fn find_given_version(doc: &mut Doc, doctype: NodeId) -> u32 {
    let fpi = attrs::get_attr_by_name(doc, doctype, b"PUBLIC");
    let fpi_val = fpi.and_then(|a| doc[a].value.clone());
    let Some(fv) = fpi_val else {
        if let Some(el) = doc[doctype].element.as_deref() {
            if tmbstr::tmbstrcasecmp(el, b"html") == 0 {
                return VERS_HTML5;
            }
        }
        return VERS_UNKNOWN;
    };
    let vers = get_vers_from_fpi(&fv);
    if VERS_XHTML & vers != 0 {
        config::set_option_bool(doc, OptId::XmlOut, true);
        config::set_option_bool(doc, OptId::XhtmlOut, true);
        doc.lx().isvoyager = true;
    }
    let a = fpi.expect("fpi");
    doc[a].value = get_fpi_from_vers(vers).map(|s| s.as_bytes().to_vec());
    vers
}

/// ApparentVersion(): return guessed version
pub fn apparent_version(doc: &mut Doc) -> u32 {
    let lx = doc.lxr();
    if (lx.doctype == XH11 || lx.doctype == XB10) && (lx.versions & lx.doctype) != 0 {
        lx.doctype
    } else {
        html_version(doc)
    }
}

pub fn html_version_name_from_code(vers: u32, _is_xhtml: bool) -> Option<&'static str> {
    get_name_from_vers(vers)
}

pub fn html_version_number_from_code(vers: u32) -> u32 {
    W3C_DOCTYPES
        .iter()
        .find(|d| d.vers == vers)
        .map_or(VERS_UNKNOWN, |d| d.vers_out)
}

pub fn warn_missing_si_in_emitted_doc_type(doc: &mut Doc) -> bool {
    let is_xhtml = doc.lxr().isvoyager;
    if is_xhtml {
        return false;
    }
    let ve = doc.lxr().version_emitted;
    if html_version_name_from_code(ve, is_xhtml).is_none() {
        return false;
    }
    if get_si_from_vers(ve).is_none() {
        return false;
    }
    if let Some(doctype) = find_doc_type(doc) {
        if attrs::get_attr_by_name(doc, doctype, b"SYSTEM").is_none() {
            return true;
        }
    }
    false
}

/// NewDocTypeNode(): a doctype before the <html> element
fn new_doc_type_node(doc: &mut Doc) -> Option<NodeId> {
    let html = find_html(doc)?;
    let doctype = new_node_with(doc, false);
    doc[doctype].type_ = NodeType::DocTypeTag;
    parser::insert_node_before_element(doc, html, doctype);
    Some(doctype)
}

fn opt_bytes(s: Option<&'static str>) -> Option<&'static [u8]> {
    s.map(|s| s.as_bytes())
}

fn lower_element(doc: &mut Doc, node: NodeId) {
    if let Some(e) = doc[node].element.as_mut() {
        tmbstr::tmbstrtolower(e);
    }
}

pub fn set_xhtml_doc_type(doc: &mut Doc) -> bool {
    let doctype = find_doc_type(doc);
    let dtmode = doc.cfg(OptId::DoctypeMode);
    let pub_ = b"PUBLIC";
    let sys = b"SYSTEM";

    let av = apparent_version(doc);
    doc.lx().version_emitted = av;

    if dtmode == TIDY_DOCTYPE_OMIT {
        if let Some(d) = doctype {
            parser::discard_element(doc, d);
        }
        return true;
    }
    if dtmode == TIDY_DOCTYPE_USER && doc.cfg_str(OptId::Doctype).is_none() {
        return false;
    }

    let dt = match doctype {
        None => {
            let d = new_doc_type_node(doc).expect("tidy: no html element for doctype");
            doc[d].element = Some(b"html".to_vec());
            d
        }
        Some(d) => {
            lower_element(doc, d);
            d
        }
    };

    match dtmode {
        TIDY_DOCTYPE_HTML5 => {
            attrs::repair_attr_value(doc, dt, pub_, None);
            attrs::repair_attr_value(doc, dt, sys, None);
            doc.lx().version_emitted = XH50;
        }
        TIDY_DOCTYPE_STRICT => {
            attrs::repair_attr_value(doc, dt, pub_, opt_bytes(get_fpi_from_vers(X10S)));
            attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(X10S)));
            doc.lx().version_emitted = X10S;
        }
        TIDY_DOCTYPE_LOOSE => {
            attrs::repair_attr_value(doc, dt, pub_, opt_bytes(get_fpi_from_vers(X10T)));
            attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(X10T)));
            doc.lx().version_emitted = X10T;
        }
        TIDY_DOCTYPE_USER => {
            let v = doc.cfg_str(OptId::Doctype).map(|s| s.to_vec());
            attrs::repair_attr_value(doc, dt, pub_, v.as_deref());
            attrs::repair_attr_value(doc, dt, sys, Some(b""));
        }
        TIDY_DOCTYPE_AUTO => {
            let (ldt, lv) = (doc.lxr().doctype, doc.lxr().versions);
            if ldt == VERS_UNKNOWN || ldt == VERS_HTML5 {
                doc.lx().version_emitted = XH50;
                return true;
            } else if lv & XH11 != 0 && ldt == XH11 {
                if attrs::get_attr_by_name(doc, dt, sys).is_none() {
                    attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(XH11)));
                }
                doc.lx().version_emitted = XH11;
                return true;
            } else if lv & XH11 != 0 && (lv & VERS_HTML40) == 0 {
                attrs::repair_attr_value(doc, dt, pub_, opt_bytes(get_fpi_from_vers(XH11)));
                attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(XH11)));
                doc.lx().version_emitted = XH11;
            } else if lv & XB10 != 0 && ldt == XB10 {
                if attrs::get_attr_by_name(doc, dt, sys).is_none() {
                    attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(XB10)));
                }
                doc.lx().version_emitted = XB10;
                return true;
            } else if lv & VERS_HTML40_STRICT != 0 {
                attrs::repair_attr_value(doc, dt, pub_, opt_bytes(get_fpi_from_vers(X10S)));
                attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(X10S)));
                doc.lx().version_emitted = X10S;
            } else if lv & VERS_FRAMESET != 0 {
                attrs::repair_attr_value(doc, dt, pub_, opt_bytes(get_fpi_from_vers(X10F)));
                attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(X10F)));
                doc.lx().version_emitted = X10F;
            } else if lv & VERS_LOOSE != 0 {
                attrs::repair_attr_value(doc, dt, pub_, opt_bytes(get_fpi_from_vers(X10T)));
                attrs::repair_attr_value(doc, dt, sys, opt_bytes(get_si_from_vers(X10T)));
                doc.lx().version_emitted = X10T;
            } else if lv & VERS_HTML5 != 0 {
                // Issue #273: nothing to do
            } else {
                parser::discard_element(doc, dt);
                return false;
            }
        }
        _ => {}
    }
    false
}

/// FixDocType(): fixup doctype if missing
pub fn fix_doc_type(doc: &mut Doc) -> bool {
    let mut doctype = find_doc_type(doc);
    let dtmode = doc.cfg(OptId::DoctypeMode);
    let mut had_si = false;

    if doctype.is_some() && dtmode == TIDY_DOCTYPE_AUTO && doc.lxr().doctype == VERS_HTML5 {
        doc.lx().version_emitted = HT50;
        return true;
    }
    {
        let lx = doc.lxr();
        let (lv, ldt, voy) = (lx.versions, lx.doctype, lx.isvoyager);
        if dtmode == TIDY_DOCTYPE_AUTO
            && (lv & ldt) != 0
            && !((VERS_XHTML & ldt) != 0 && !voy)
            && find_doc_type(doc).is_some()
        {
            doc.lx().version_emitted = ldt;
            return true;
        }
    }
    if dtmode == TIDY_DOCTYPE_OMIT {
        if let Some(d) = doctype {
            parser::discard_element(doc, d);
        }
        let av = apparent_version(doc);
        doc.lx().version_emitted = av;
        return true;
    }
    if doc.cfg_bool(OptId::XmlOut) {
        return true;
    }
    if let Some(d) = doctype {
        had_si = attrs::get_attr_by_name(doc, d, b"SYSTEM").is_some();
    }
    if (dtmode == TIDY_DOCTYPE_STRICT || dtmode == TIDY_DOCTYPE_LOOSE) && doctype.is_some() {
        parser::discard_element(doc, doctype.expect("doctype"));
        doctype = None;
    }
    let guessed = match dtmode {
        TIDY_DOCTYPE_HTML5 => HT50,
        TIDY_DOCTYPE_STRICT => H41S,
        TIDY_DOCTYPE_LOOSE => H41T,
        TIDY_DOCTYPE_AUTO => html_version(doc),
        _ => VERS_UNKNOWN,
    };
    doc.lx().version_emitted = guessed;
    if guessed == VERS_UNKNOWN {
        return false;
    }
    let dt = match doctype {
        Some(d) => {
            lower_element(doc, d);
            d
        }
        None => {
            let d = new_doc_type_node(doc).expect("tidy: no html element for doctype");
            doc[d].element = Some(b"html".to_vec());
            d
        }
    };
    attrs::repair_attr_value(doc, dt, b"PUBLIC", opt_bytes(get_fpi_from_vers(guessed)));
    if had_si {
        attrs::repair_attr_value(doc, dt, b"SYSTEM", opt_bytes(get_si_from_vers(guessed)));
    }
    true
}

/// FixXmlDecl(): ensure XML document starts with <?xml version="1.0"?>
pub fn fix_xml_decl(doc: &mut Doc) -> bool {
    let root = doc.root;
    let xml = match doc[root].content {
        Some(c) if doc[c].type_ == NodeType::XmlDecl => c,
        rc => {
            let x = new_node(doc);
            doc[x].type_ = NodeType::XmlDecl;
            match rc {
                Some(c) => parser::insert_node_before_element(doc, c, x),
                None => doc[root].content = Some(x),
            }
            x
        }
    };
    let version = attrs::get_attr_by_name(doc, xml, b"version");
    let encoding = attrs::get_attr_by_name(doc, xml, b"encoding");
    if encoding.is_none() && doc.cfg(OptId::OutCharEncoding) != UTF8 {
        if let Some(enc) = streamio::get_encoding_name_from_tidy_id(doc.cfg(OptId::OutCharEncoding))
        {
            attrs::add_attribute(doc, xml, b"encoding", enc.as_bytes());
        }
    }
    if version.is_none() {
        attrs::add_attribute(doc, xml, b"version", b"1.0");
    }
    true
}

/// InferredTag()
pub fn inferred_tag(doc: &mut Doc, id: TagId) -> NodeId {
    let node = new_node(doc);
    let dict = tags::lookup_tag_def(id).expect("tidy: InferredTag of unknown tag");
    let name = doc.dict_name(dict).map(|s| s.to_vec());
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    let n = &mut doc[node];
    n.type_ = NodeType::StartTag;
    n.implicit = true;
    n.element = name;
    n.tag = Some(dict);
    n.start = s;
    n.end = e;
    node
}

fn expects_content(doc: &Doc, node: NodeId) -> bool {
    if doc[node].type_ != NodeType::StartTag {
        return false;
    }
    match doc[node].tag {
        None => true,
        Some(t) => doc.dict_model(t) & CM_EMPTY == 0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CdataState {
    Intermediate,
    StartTag,
    EndTag,
}

/// GetCDATA(): a text node for the contents of a CDATA element like style
/// or script which ends with </foo> for some foo.
fn get_cdata(doc: &mut Doc, container: NodeId) -> Option<NodeId> {
    let mut start: u32 = 0;
    let mut nested: i32 = 0;
    let mut state = CdataState::Intermediate;
    let mut is_empty = true;
    let mut c;
    let has_src = attrs::attr_get_by_id(doc, container, AttrId::SRC).is_some();
    let nonested = (doc.is(container, TagId::SCRIPT) || doc.is(container, TagId::STYLE))
        && doc.cfg_bool(OptId::SkipNested);
    let cel = doc[container].element.clone().unwrap_or_default();
    let cel_len = tmbstr::tmbstrlen(&cel);

    set_lexer_locus(doc);
    {
        let lx = doc.lx();
        lx.waswhite = false;
        let ls = lx.lexsize();
        lx.txtstart = ls;
        lx.txtend = ls;
    }

    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        add_char_to_lexer(doc, c);
        let ls = doc.lxr().lexsize();
        doc.lx().txtend = ls;

        match state {
            CdataState::Intermediate => {
                if c != '<' as u32 {
                    if is_empty && !is_white(c) {
                        is_empty = false;
                    }
                    continue;
                }
                c = read(doc);
                if is_letter(c) {
                    if has_src && is_empty && doc.is(container, TagId::SCRIPT) {
                        let ts = doc.lxr().txtstart;
                        set_lexsize(doc, ts);
                        unget(doc, c);
                        unget(doc, '<' as u32);
                        return None;
                    }
                    add_char_to_lexer(doc, c);
                    start = doc.lxr().lexsize() - 1;
                    state = CdataState::StartTag;
                } else if c == '/' as u32 {
                    add_char_to_lexer(doc, c);
                    c = read(doc);
                    if !is_letter(c) {
                        unget(doc, c);
                        continue;
                    }
                    unget(doc, c);
                    start = doc.lxr().lexsize();
                    state = CdataState::EndTag;
                } else if c == '\\' as u32 {
                    add_char_to_lexer(doc, c);
                    c = read(doc);
                    if c != '/' as u32 {
                        unget(doc, c);
                        continue;
                    }
                    add_char_to_lexer(doc, c);
                    if nonested {
                        continue;
                    }
                    c = read(doc);
                    if !is_letter(c) {
                        unget(doc, c);
                        continue;
                    }
                    unget(doc, c);
                    start = doc.lxr().lexsize();
                    state = CdataState::EndTag;
                } else {
                    unget(doc, c);
                }
            }
            CdataState::StartTag => {
                if is_letter(c) {
                    continue;
                }
                let m = {
                    let s = lexstr_raw(doc, start);
                    tmbstr::tmbstrncasecmp(&cel, &s, cel_len) == 0
                };
                if m && !nonested {
                    nested += 1;
                }
                state = CdataState::Intermediate;
            }
            CdataState::EndTag => {
                if is_letter(c) {
                    continue;
                }
                let matches = {
                    let s = lexstr_raw(doc, start);
                    tmbstr::tmbstrncasecmp(&cel, &s, cel_len) == 0
                };
                if is_empty && !matches {
                    unget_lexbuf_from(doc, start);
                    unget(doc, '/' as u32);
                    unget(doc, '<' as u32);
                    break;
                }
                let close = if matches {
                    let was = nested;
                    nested -= 1;
                    was <= 0
                } else {
                    false
                };
                if close {
                    unget_lexbuf_from(doc, start);
                    unget(doc, '/' as u32);
                    unget(doc, '<' as u32);
                    let ls = doc.lxr().lexsize();
                    let n = ls.wrapping_sub(ls.wrapping_sub(start).wrapping_add(2));
                    set_lexsize(doc, n);
                    break;
                } else if lexbyte(doc, start.wrapping_sub(2)) != b'\\' {
                    set_lexer_locus(doc);
                    let col = doc.lxr().columns;
                    doc.lx().columns = col.wrapping_sub(3);
                    if parser::is_javascript(doc, container)
                        && doc.cfg_bool(OptId::EscapeScripts)
                        && !tags::is_html5_mode(doc)
                    {
                        message::report(doc, None, None, BAD_CDATA_CONTENT, &[]);
                        // shift lexbuf[start-1 .. lexsize] up by one
                        let at = (start as usize).saturating_sub(1);
                        let lx = doc.lx();
                        let ls = lx.lexsize as usize;
                        lexbuf_reserve(lx, ls + 1);
                        let mut i = ls;
                        while i > at {
                            lx.lexbuf[i] = lx.lexbuf[i - 1];
                            i -= 1;
                        }
                        lx.lexbuf[at] = b'\\';
                        lx.lexsize += 1;
                    }
                }
                state = CdataState::Intermediate;
            }
        }
    }
    if is_empty {
        let te = doc.lxr().txtend;
        set_lexsize(doc, te);
        doc.lx().txtstart = te;
    } else {
        let ls = doc.lxr().lexsize();
        doc.lx().txtend = ls;
    }
    if c == END_OF_STREAM {
        message::report(doc, Some(container), None, MISSING_ENDTAG_FOR, &[]);
    }
    Some(text_token(doc))
}

/// `lexbuf + start` up to lexsize (for tmbstrncasecmp, NUL-terminated)
fn lexstr_raw(doc: &Doc, start: u32) -> Vec<u8> {
    let lb = &doc.lxr().lexbuf;
    let s = (start as usize).min(lb.len());
    lb[s..].to_vec()
}

/// `for (i = lexsize - 1; i >= start; --i) UngetChar(lexbuf[i])`
fn unget_lexbuf_from(doc: &mut Doc, start: u32) {
    let ls = doc.lxr().lexsize() as i64;
    let mut i = ls - 1;
    while i >= start as i64 {
        let b = lexbyte(doc, i as u32);
        unget(doc, b as u32);
        i -= 1;
    }
}

pub fn unget_token(doc: &mut Doc) {
    doc.lx().pushed = true;
}

/// GetToken()
pub fn get_token(doc: &mut Doc, mode: GetTokenMode) -> Option<NodeId> {
    let (pushed, itoken) = (doc.lxr().pushed, doc.lxr().itoken);
    if pushed || itoken.is_some() {
        if itoken.is_some() {
            if pushed {
                doc.lx().pushed = false;
                return itoken;
            }
            doc.lx().itoken = None;
        }
        doc.lx().pushed = false;
        let token = doc.lxr().token;
        let is_text = token.is_some_and(|t| doc[t].type_ == NodeType::TextNode);
        let (ins, inode) = (doc.lxr().insert, doc.lxr().inode);
        if !is_text || !(ins.is_some() || inode.is_some()) {
            return token;
        }
        let it = istack::inserted_token(doc);
        doc.lx().itoken = Some(it);
        return Some(it);
    }

    let (ins, inode, isz) = {
        let lx = doc.lxr();
        (lx.insert, lx.inode, lx.istack.len())
    };
    if (ins.is_some() || inode.is_some()) && isz > 0 {
        let t = istack::inserted_token(doc);
        doc.lx().token = Some(t);
        return Some(t);
    }

    if mode == GetTokenMode::CdataContent {
        let parent = doc.lxr().parent.expect("tidy: CDATA without parent");
        return get_cdata(doc, parent);
    }

    get_token_from_stream(doc, mode)
}

/// CondReturnTextNode
fn cond_text_node(doc: &mut Doc) -> Option<NodeId> {
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    if e > s {
        let t = text_token(doc);
        doc.lx().token = Some(t);
        return Some(t);
    }
    None
}

fn set_state(doc: &mut Doc, st: LexerState) {
    doc.lx().state = st;
}

fn txtend_to_lexsize(doc: &mut Doc) {
    let ls = doc.lxr().lexsize();
    doc.lx().txtend = ls;
}

fn txtstart_to_lexsize(doc: &mut Doc) {
    let ls = doc.lxr().lexsize();
    doc.lx().txtstart = ls;
}

fn finish_token(doc: &mut Doc, t: NodeId) -> Option<NodeId> {
    let lx = doc.lx();
    lx.state = LexerState::LexContent;
    lx.waswhite = false;
    lx.token = Some(t);
    Some(t)
}

fn get_token_from_stream(doc: &mut Doc, mode: GetTokenMode) -> Option<NodeId> {
    let mut mode = mode;
    let mut c;
    let mut badcomment = 0u32;
    let mut isempty;
    let mut attributes: Option<AttId> = None;

    let fix_comments = match doc.cfg_auto_bool(OptId::FixComments) {
        TIDY_YES_STATE => true,
        TIDY_NO_STATE => false,
        _ => (html_version(doc) & HT50) == 0,
    };

    doc.lx().token = None;
    set_lexer_locus(doc);
    doc.lx().waswhite = false;
    {
        let lx = doc.lx();
        let ls = lx.lexsize();
        lx.txtstart = ls;
        lx.txtend = ls;
    }

    'outer: loop {
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        if doc.lxr().insertspace {
            add_char_to_lexer(doc, ' ' as u32);
            let lx = doc.lx();
            lx.waswhite = true;
            lx.insertspace = false;
        }
        if c == 160 && mode == GetTokenMode::Preformatted {
            c = ' ' as u32;
        }
        add_char_to_lexer(doc, c);

        match doc.lxr().state {
            LexerState::LexContent => {
                if is_white(c)
                    && mode == GetTokenMode::IgnoreWhitespace
                    && doc.lxr().lexsize() == doc.lxr().txtstart.wrapping_add(1)
                {
                    dec_lexsize(doc, 1);
                    doc.lx().waswhite = false;
                    set_lexer_locus(doc);
                    continue;
                }
                if c == '<' as u32 {
                    set_state(doc, LexerState::LexGt);
                    continue;
                }
                if is_white(c) {
                    if doc.lxr().waswhite {
                        if mode != GetTokenMode::Preformatted && mode != GetTokenMode::IgnoreMarkup
                        {
                            dec_lexsize(doc, 1);
                            set_lexer_locus(doc);
                        }
                    } else {
                        doc.lx().waswhite = true;
                        if mode != GetTokenMode::Preformatted
                            && mode != GetTokenMode::IgnoreMarkup
                            && c != ' ' as u32
                        {
                            change_char(doc, b' ');
                        }
                    }
                    continue;
                } else if c == '&' as u32 && mode != GetTokenMode::IgnoreMarkup {
                    parse_entity(doc, mode);
                }
                if mode == GetTokenMode::IgnoreWhitespace {
                    mode = GetTokenMode::MixedContent;
                }
                doc.lx().waswhite = false;
                continue;
            }

            LexerState::LexGt => {
                if c == '/' as u32 {
                    c = read(doc);
                    if c == END_OF_STREAM {
                        unget(doc, c);
                        continue;
                    }
                    add_char_to_lexer(doc, c);
                    if is_letter(c) || (doc.cfg_bool(OptId::XmlTags) && is_xml_namechar(c)) {
                        dec_lexsize(doc, 3);
                        txtend_to_lexsize(doc);
                        unget(doc, c);
                        set_state(doc, LexerState::LexEndTag);
                        curcol_add(doc, -2);

                        let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
                        if e > s {
                            let ls = doc.lxr().lexsize();
                            if mode == GetTokenMode::IgnoreWhitespace
                                && lexbyte(doc, ls.wrapping_sub(1)) == b' '
                            {
                                dec_lexsize(doc, 1);
                                txtend_to_lexsize(doc);
                            }
                            let t = text_token(doc);
                            doc.lx().token = Some(t);
                            return Some(t);
                        }
                        continue;
                    }
                    doc.lx().waswhite = false;
                    set_state(doc, LexerState::LexContent);
                    continue;
                }

                if mode == GetTokenMode::IgnoreMarkup {
                    doc.lx().waswhite = false;
                    set_state(doc, LexerState::LexContent);
                    continue;
                }

                if c == '!' as u32 {
                    c = read(doc);
                    if c == '-' as u32 {
                        c = read(doc);
                        if c == '-' as u32 {
                            set_state(doc, LexerState::LexComment);
                            dec_lexsize(doc, 2);
                            txtend_to_lexsize(doc);
                            if let Some(t) = cond_text_node(doc) {
                                return Some(t);
                            }
                            txtstart_to_lexsize(doc);
                            continue;
                        }
                    } else if c == 'd' as u32 || c == 'D' as u32 {
                        set_state(doc, LexerState::LexDocType);
                        dec_lexsize(doc, 2);
                        txtend_to_lexsize(doc);
                        mode = GetTokenMode::IgnoreWhitespace;

                        loop {
                            c = read(doc);
                            if c == END_OF_STREAM || c == '>' as u32 {
                                unget(doc, c);
                                break;
                            }
                            if !is_white(c) {
                                continue;
                            }
                            loop {
                                c = read(doc);
                                if c == END_OF_STREAM || c == '>' as u32 {
                                    unget(doc, c);
                                    break;
                                }
                                if is_white(c) {
                                    continue;
                                }
                                unget(doc, c);
                                break;
                            }
                            break;
                        }
                        if let Some(t) = cond_text_node(doc) {
                            return Some(t);
                        }
                        txtstart_to_lexsize(doc);
                        continue;
                    } else if c == '[' as u32 {
                        dec_lexsize(doc, 2);
                        set_state(doc, LexerState::LexSection);
                        txtend_to_lexsize(doc);
                        if let Some(t) = cond_text_node(doc) {
                            return Some(t);
                        }
                        txtstart_to_lexsize(doc);
                        continue;
                    }

                    message::report(doc, None, None, MALFORMED_COMMENT_DROPPING, &[]);
                    loop {
                        c = read(doc);
                        if c == '>' as u32 {
                            break;
                        }
                        if c == END_OF_STREAM {
                            unget(doc, c);
                            break;
                        }
                    }
                    dec_lexsize(doc, 2);
                    set_state(doc, LexerState::LexContent);
                    continue;
                }

                if c == '?' as u32 {
                    dec_lexsize(doc, 2);
                    set_state(doc, LexerState::LexProcInstr);
                    txtend_to_lexsize(doc);
                    if let Some(t) = cond_text_node(doc) {
                        return Some(t);
                    }
                    txtstart_to_lexsize(doc);
                    continue;
                }

                if c == '%' as u32 {
                    dec_lexsize(doc, 2);
                    set_state(doc, LexerState::LexAsp);
                    txtend_to_lexsize(doc);
                    if let Some(t) = cond_text_node(doc) {
                        return Some(t);
                    }
                    txtstart_to_lexsize(doc);
                    continue;
                }

                if c == '#' as u32 {
                    dec_lexsize(doc, 2);
                    set_state(doc, LexerState::LexJste);
                    txtend_to_lexsize(doc);
                    if let Some(t) = cond_text_node(doc) {
                        return Some(t);
                    }
                    txtstart_to_lexsize(doc);
                    continue;
                }

                if is_letter(c) || (doc.cfg_bool(OptId::XmlTags) && is_xml_namechar(c)) {
                    unget(doc, c);
                    unget(doc, '<' as u32);
                    dec_lexsize(doc, 2);
                    txtend_to_lexsize(doc);
                    set_state(doc, LexerState::LexStartTag);
                    if let Some(t) = cond_text_node(doc) {
                        return Some(t);
                    }
                    continue;
                }

                unget(doc, c);
                dec_lexsize(doc, 1);
                set_state(doc, LexerState::LexContent);
                doc.lx().waswhite = false;
                continue;
            }

            LexerState::LexEndTag => {
                let ts = doc.lxr().lexsize().wrapping_sub(1);
                doc.lx().txtstart = ts;
                curcol_add(doc, 2);
                c = parse_tag_name(doc);
                let t = tag_token(doc, NodeType::EndTag);
                doc.lx().token = Some(t);
                let ts = doc.lxr().txtstart;
                set_lexsize(doc, ts);
                doc.lx().txtend = ts;

                while c != '>' as u32 && c != END_OF_STREAM {
                    c = read(doc);
                }
                if c == END_OF_STREAM {
                    free_node(doc, Some(t));
                    continue;
                }
                return finish_token(doc, t);
            }

            LexerState::LexStartTag => {
                c = read(doc);
                change_char(doc, c as u8);
                let ts = doc.lxr().lexsize().wrapping_sub(1);
                doc.lx().txtstart = ts;
                c = parse_tag_name(doc);
                isempty = false;
                attributes = None;
                let t = tag_token(doc, NodeType::StartTag);
                doc.lx().token = Some(t);

                if c != '>' as u32 {
                    if c == '/' as u32 {
                        unget(doc, c);
                    }
                    attributes = parse_attrs(doc, &mut isempty);
                }
                if isempty {
                    doc[t].type_ = NodeType::StartEndTag;
                }
                doc[t].attributes = attributes;
                let ts = doc.lxr().txtstart;
                set_lexsize(doc, ts);
                doc.lx().txtend = ts;

                if doc.is(t, TagId::PRE) {
                    mode = GetTokenMode::Preformatted;
                }
                if (mode != GetTokenMode::Preformatted && expects_content(doc, t))
                    || doc.is(t, TagId::BR)
                    || doc.is(t, TagId::HR)
                {
                    c = read(doc);
                    // Issue #329: keep the newline unless ignoring white space
                    if (c == '\n' as u32 && mode != GetTokenMode::IgnoreWhitespace)
                        || (c != '\n' as u32 && c != 0x0c)
                    {
                        unget(doc, c);
                    }
                    doc.lx().waswhite = true;
                } else {
                    doc.lx().waswhite = false;
                }

                set_state(doc, LexerState::LexContent);
                match doc[t].tag {
                    None => {
                        if mode != GetTokenMode::OtherNamespace {
                            if (doc.lxr().doctype & VERS_HTML5) > 0
                                && tags::element_is_autonomous_custom_format(
                                    doc[t].element.as_deref(),
                                )
                            {
                                message::report(
                                    doc,
                                    None,
                                    Some(t),
                                    UNKNOWN_ELEMENT_LOOKS_CUSTOM,
                                    &[],
                                );
                            } else {
                                message::report(doc, None, Some(t), UNKNOWN_ELEMENT, &[]);
                            }
                        }
                    }
                    Some(tag) => {
                        if !doc.cfg_bool(OptId::XmlTags) {
                            let v = doc.dict_versions(tag);
                            constrain_version(doc, v);
                            attrs::repair_duplicate_attributes(doc, t, false);
                        } else {
                            attrs::repair_duplicate_attributes(doc, t, true);
                        }
                    }
                }
                return Some(t);
            }

            LexerState::LexComment => {
                if c != '-' as u32 {
                    continue;
                }
                c = read(doc);
                if c != '-' as u32
                    && fix_comments
                    && doc.lxr().lexsize().wrapping_sub(doc.lxr().txtstart) == 1
                {
                    let ls = doc.lxr().lexsize();
                    set_lexbyte(doc, ls - 1, b'=');
                }
                add_char_to_lexer(doc, c);
                if c != '-' as u32 {
                    continue;
                }
                // end_comment:
                loop {
                    c = read(doc);
                    if c == '>' as u32 {
                        if badcomment != 0 {
                            if html_version(doc) & HT50 != 0 {
                                if fix_comments {
                                    message::report(doc, None, None, MALFORMED_COMMENT, &[]);
                                }
                            } else if fix_comments {
                                message::report(doc, None, None, MALFORMED_COMMENT, &[]);
                            } else {
                                message::report(doc, None, None, MALFORMED_COMMENT_WARN, &[]);
                            }
                        }
                        dec_lexsize(doc, 2);
                        txtend_to_lexsize(doc);
                        let t = new_token(doc, NodeType::CommentTag);
                        let r = finish_token(doc, t);
                        c = read(doc);
                        if c == '\n' as u32 {
                            doc[t].linebreak = true;
                        } else {
                            unget(doc, c);
                        }
                        return r;
                    }
                    if badcomment == 0 {
                        set_lexer_locus(doc);
                        let col = doc.lxr().columns;
                        doc.lx().columns = col.wrapping_sub(3);
                    }
                    badcomment += 1;
                    if fix_comments {
                        let ls = doc.lxr().lexsize();
                        set_lexbyte(doc, ls.wrapping_sub(2), b'=');
                    }
                    if c == '-' as u32 {
                        add_char_to_lexer(doc, c);
                        continue;
                    }
                    if fix_comments {
                        let ls = doc.lxr().lexsize();
                        set_lexbyte(doc, ls.wrapping_sub(1), b'=');
                    }
                    add_char_to_lexer(doc, c);
                    continue 'outer;
                }
            }

            LexerState::LexDocType => {
                unget(doc, c);
                dec_lexsize(doc, 1);
                let tok = parse_doc_type_decl(doc);
                doc.lx().token = tok;
                txtend_to_lexsize(doc);
                set_state(doc, LexerState::LexContent);
                doc.lx().waswhite = false;

                if doc.lxr().doctype == VERS_UNKNOWN && !doc.cfg_bool(OptId::XmlTags) {
                    if let Some(t) = tok {
                        let v = find_given_version(doc, t);
                        doc.lx().doctype = v;
                        if v != VERS_HTML5 {
                            tags::adjust_tags(doc);
                        }
                    }
                }
                return tok;
            }

            LexerState::LexProcInstr => {
                let (ls, ts) = (doc.lxr().lexsize(), doc.lxr().txtstart);
                if ls.wrapping_sub(ts) == 3
                    && tmbstr::tmbstrncmp(&lexstr_raw(doc, ts), b"php", 3) == 0
                {
                    set_state(doc, LexerState::LexPhp);
                    continue;
                }
                if ls.wrapping_sub(ts) == 4
                    && tmbstr::tmbstrncmp(&lexstr_raw(doc, ts), b"xml", 3) == 0
                    && is_white(lexbyte(doc, ts + 3) as u32)
                {
                    set_state(doc, LexerState::LexXmlDecl);
                    attributes = None;
                    continue;
                }

                if doc.cfg_bool(OptId::XmlPIs) || doc.lxr().isvoyager {
                    if c != '?' as u32 {
                        continue;
                    }
                    c = read(doc);
                    if c == END_OF_STREAM {
                        message::report(doc, None, None, UNEXPECTED_END_OF_FILE, &[]);
                        unget(doc, c);
                        continue;
                    }
                    add_char_to_lexer(doc, c);
                }

                if c != '>' as u32 {
                    continue;
                }
                dec_lexsize(doc, 1);

                let t;
                if doc.lxr().lexsize() != 0 {
                    let (ls, ts) = (doc.lxr().lexsize(), doc.lxr().txtstart);
                    let mut i = 0u32;
                    while i < ls.wrapping_sub(ts) && !is_white(lexbyte(doc, i + ts) as u32) {
                        i += 1;
                    }
                    let closed = lexbyte(doc, ls - 1) == b'?';
                    if closed {
                        dec_lexsize(doc, 1);
                    }
                    doc.lx().txtstart = ts + i;
                    txtend_to_lexsize(doc);
                    t = new_token(doc, NodeType::ProcInsTag);
                    doc[t].closed = closed;
                    let el = lexndup(doc, ts, i);
                    doc[t].element = el;
                } else {
                    txtend_to_lexsize(doc);
                    t = new_token(doc, NodeType::ProcInsTag);
                }
                return finish_token(doc, t);
            }

            LexerState::LexAsp => {
                if c != '%' as u32 {
                    continue;
                }
                c = read(doc);
                if c != '>' as u32 {
                    unget(doc, c);
                    continue;
                }
                dec_lexsize(doc, 1);
                txtend_to_lexsize(doc);
                let t = new_token(doc, NodeType::AspTag);
                return finish_token(doc, t);
            }

            LexerState::LexJste => {
                if c != '#' as u32 {
                    continue;
                }
                c = read(doc);
                if c != '>' as u32 {
                    unget(doc, c);
                    continue;
                }
                dec_lexsize(doc, 1);
                txtend_to_lexsize(doc);
                let t = new_token(doc, NodeType::JsteTag);
                return finish_token(doc, t);
            }

            LexerState::LexPhp => {
                if c != '?' as u32 {
                    continue;
                }
                c = read(doc);
                if c != '>' as u32 {
                    unget(doc, c);
                    continue;
                }
                dec_lexsize(doc, 1);
                txtend_to_lexsize(doc);
                let t = new_token(doc, NodeType::PhpTag);
                return finish_token(doc, t);
            }

            LexerState::LexXmlDecl => {
                if is_white(c) && c != '?' as u32 {
                    continue;
                }
                if c != '?' as u32 {
                    let mut pdelim: u32 = 0;
                    isempty = false;
                    unget(doc, c);
                    let mut asp = None;
                    let mut php = None;
                    let name = parse_attribute(doc, &mut isempty, &mut asp, &mut php);
                    let Some(name) = name else {
                        if let Some(a) = asp {
                            let av = new_attribute(doc);
                            doc[av].asp = Some(a);
                            add_attr_to_list(doc, &mut attributes, av);
                        }
                        if let Some(p) = php {
                            let av = new_attribute(doc);
                            doc[av].php = Some(p);
                            add_attr_to_list(doc, &mut attributes, av);
                        }
                        dec_lexsize(doc, 1);
                        let ts = doc.lxr().txtstart;
                        doc.lx().txtend = ts;
                        set_nul(doc, ts);
                        let t = new_token(doc, NodeType::XmlDecl);
                        doc[t].attributes = attributes;
                        return finish_token(doc, t);
                    };
                    let av = new_attribute(doc);
                    let value = parse_value(doc, &name, true, &mut isempty, &mut pdelim);
                    doc[av].attribute = Some(name);
                    doc[av].value = value;
                    doc[av].delim = pdelim;
                    let d = attrs::find_attribute(doc, av);
                    doc[av].dict = d;
                    add_attr_to_list(doc, &mut attributes, av);
                }
                c = read(doc);
                if c != '>' as u32 {
                    unget(doc, c);
                    continue;
                }
                dec_lexsize(doc, 1);
                let ts = doc.lxr().txtstart;
                doc.lx().txtend = ts;
                set_nul(doc, ts);
                let t = new_token(doc, NodeType::XmlDecl);
                doc[t].attributes = attributes;
                return finish_token(doc, t);
            }

            LexerState::LexSection => {
                if c == '[' as u32 {
                    let (ls, ts) = (doc.lxr().lexsize(), doc.lxr().txtstart);
                    if ls == ts.wrapping_add(6)
                        && tmbstr::tmbstrncmp(&lexstr_raw(doc, ts), b"CDATA[", 6) == 0
                    {
                        set_state(doc, LexerState::LexCdata);
                        dec_lexsize(doc, 6);
                        continue;
                    }
                }
                if c == '>' as u32 {
                    unget(doc, c);
                } else if c != ']' as u32 {
                    continue;
                }
                c = read(doc);
                let lexdump = 1;
                if c != '>' as u32 {
                    if c == '-' as u32 {
                        c = read(doc);
                        if c == '-' as u32 {
                            c = read(doc);
                            if c != '>' as u32 {
                                unget(doc, c);
                                unget(doc, '-' as u32);
                                unget(doc, '-' as u32);
                                continue;
                            }
                        } else {
                            unget(doc, c);
                            unget(doc, '-' as u32);
                            continue;
                        }
                    } else {
                        unget(doc, c);
                        continue;
                    }
                }
                dec_lexsize(doc, lexdump);
                txtend_to_lexsize(doc);
                let t = new_token(doc, NodeType::SectionTag);
                return finish_token(doc, t);
            }

            LexerState::LexCdata => {
                if c != ']' as u32 {
                    continue;
                }
                c = read(doc);
                if c != ']' as u32 {
                    unget(doc, c);
                    continue;
                }
                c = read(doc);
                if c != '>' as u32 {
                    unget(doc, c);
                    unget(doc, ']' as u32);
                    continue;
                }
                dec_lexsize(doc, 1);
                txtend_to_lexsize(doc);
                let t = new_token(doc, NodeType::CDATATag);
                return finish_token(doc, t);
            }
        }
    }

    let state = doc.lxr().state;
    if state == LexerState::LexContent {
        txtend_to_lexsize(doc);
        let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
        if e > s {
            unget(doc, c);
            let ls = doc.lxr().lexsize();
            if lexbyte(doc, ls.wrapping_sub(1)) == b' ' {
                dec_lexsize(doc, 1);
                txtend_to_lexsize(doc);
            }
            let t = text_token(doc);
            doc.lx().token = Some(t);
            return Some(t);
        }
    } else if state == LexerState::LexComment {
        if c == END_OF_STREAM {
            message::report(doc, None, None, MALFORMED_COMMENT_EOS, &[]);
        }
        txtend_to_lexsize(doc);
        let t = new_token(doc, NodeType::CommentTag);
        return finish_token(doc, t);
    }
    None
}

// ---------------------------------------------------------------------------
// attributes

/// ParseAsp(): parser for ASP within start tags
fn parse_asp(doc: &mut Doc) -> Option<NodeId> {
    let mut asp = None;
    txtstart_to_lexsize(doc);
    loop {
        let mut c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        add_char_to_lexer(doc, c);
        if c != '%' as u32 {
            continue;
        }
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        add_char_to_lexer(doc, c);
        if c == '>' as u32 {
            dec_lexsize(doc, 2);
            break;
        }
    }
    txtend_to_lexsize(doc);
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    if e > s {
        asp = Some(new_token(doc, NodeType::AspTag));
    }
    doc.lx().txtstart = e;
    asp
}

/// ParsePhp(): PHP is like ASP but is based upon XML processing
/// instructions, e.g. <?php ... ?>
fn parse_php(doc: &mut Doc) -> Option<NodeId> {
    let mut php = None;
    txtstart_to_lexsize(doc);
    loop {
        let mut c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        add_char_to_lexer(doc, c);
        if c != '?' as u32 {
            continue;
        }
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        add_char_to_lexer(doc, c);
        if c == '>' as u32 {
            dec_lexsize(doc, 2);
            break;
        }
    }
    txtend_to_lexsize(doc);
    let (s, e) = (doc.lxr().txtstart, doc.lxr().txtend);
    if e > s {
        php = Some(new_token(doc, NodeType::PhpTag));
    }
    doc.lx().txtstart = e;
    php
}

fn token_node(doc: &Doc) -> NodeId {
    doc.lxr().token.expect("tidy: no current token")
}

/// ParseAttribute(): consumes the '>' terminating start tags
fn parse_attribute(
    doc: &mut Doc,
    isempty: &mut bool,
    asp: &mut Option<NodeId>,
    php: &mut Option<NodeId>,
) -> Option<Vec<u8>> {
    let mut c;
    *asp = None;
    *php = None;

    loop {
        c = read(doc);
        if c == '/' as u32 {
            c = read(doc);
            if c == '>' as u32 {
                *isempty = true;
                return None;
            }
            unget(doc, c);
            c = '/' as u32;
            break;
        }
        if c == '>' as u32 {
            return None;
        }
        if c == '<' as u32 {
            c = read(doc);
            if c == '%' as u32 {
                *asp = parse_asp(doc);
                return None;
            } else if c == '?' as u32 {
                *php = parse_php(doc);
                return None;
            }
            unget(doc, c);
            unget(doc, '<' as u32);
            let t = token_node(doc);
            message::report_attr_error(doc, t, None, UNEXPECTED_GT);
            return None;
        }
        if c == '=' as u32 {
            let t = token_node(doc);
            message::report_attr_error(doc, t, None, UNEXPECTED_EQUALSIGN);
            continue;
        }
        if c == '"' as u32 || c == '\'' as u32 {
            let t = token_node(doc);
            message::report_attr_error(doc, t, None, UNEXPECTED_QUOTEMARK);
            continue;
        }
        if c == END_OF_STREAM {
            let t = token_node(doc);
            message::report_attr_error(doc, t, None, UNEXPECTED_END_OF_FILE_ATTR);
            unget(doc, c);
            return None;
        }
        if !is_white(c) {
            break;
        }
    }

    let start = doc.lxr().lexsize();
    let mut lastc = c;

    loop {
        if c == '=' as u32 || c == '>' as u32 {
            unget(doc, c);
            break;
        }
        if c == '<' as u32 || c == END_OF_STREAM {
            unget(doc, c);
            break;
        }
        if lastc == '-' as u32 && (c == '"' as u32 || c == '\'' as u32) {
            dec_lexsize(doc, 1);
            unget(doc, c);
            break;
        }
        if is_white(c) {
            break;
        }
        if c == '/' as u32 {
            c = read(doc);
            if c == '>' as u32 {
                unget(doc, c);
                break;
            } else {
                unget(doc, c);
                c = '/' as u32;
            }
        }
        if doc.cfg(OptId::UpperCaseAttrs) != TIDY_UPPERCASE_PRESERVE
            && !doc.cfg_bool(OptId::XmlTags)
            && is_upper(c)
        {
            c = to_lower(c);
        }
        add_char_to_lexer(doc, c);
        lastc = c;
        c = read(doc);
    }

    let len = doc.lxr().lexsize().wrapping_sub(start) as i32;
    let attr = if len > 0 {
        lexndup(doc, start, len as u32)
    } else {
        None
    };
    set_lexsize(doc, start);
    attr
}

/// ParseServerInstruction(): invoked when < is seen in place of attribute
/// value
fn parse_server_instruction(doc: &mut Doc) -> u32 {
    let mut delim = '"' as u32;
    let mut isrule = false;
    let mut c = read(doc);
    add_char_to_lexer(doc, c);
    if c == '%' as u32 || c == '?' as u32 || c == '@' as u32 {
        isrule = true;
    }
    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        if c == '>' as u32 {
            if isrule {
                add_char_to_lexer(doc, c);
            } else {
                unget(doc, c);
            }
            break;
        }
        if !isrule && is_white(c) {
            break;
        }
        add_char_to_lexer(doc, c);
        if c == '"' as u32 {
            loop {
                c = read(doc);
                if c == END_OF_STREAM {
                    let t = token_node(doc);
                    message::report_attr_error(doc, t, None, UNEXPECTED_END_OF_FILE_ATTR);
                    unget(doc, c);
                    return 0;
                }
                if c == '>' as u32 {
                    unget(doc, c);
                    let t = token_node(doc);
                    message::report_attr_error(doc, t, None, UNEXPECTED_GT);
                    return 0;
                }
                add_char_to_lexer(doc, c);
                if c == '"' as u32 {
                    break;
                }
            }
            delim = '\'' as u32;
            continue;
        }
        if c == '\'' as u32 {
            loop {
                c = read(doc);
                if c == END_OF_STREAM {
                    let t = token_node(doc);
                    message::report_attr_error(doc, t, None, UNEXPECTED_END_OF_FILE_ATTR);
                    unget(doc, c);
                    return 0;
                }
                if c == '>' as u32 {
                    unget(doc, c);
                    let t = token_node(doc);
                    message::report_attr_error(doc, t, None, UNEXPECTED_GT);
                    return 0;
                }
                add_char_to_lexer(doc, c);
                if c == '\'' as u32 {
                    break;
                }
            }
        }
    }
    delim
}

/// ParseValue(): values start with "=" or " = " etc.; doesn't consume the
/// ">" at end of start tag
fn parse_value(
    doc: &mut Doc,
    name: &[u8],
    fold_case: bool,
    isempty: &mut bool,
    pdelim: &mut u32,
) -> Option<Vec<u8>> {
    let mut seen_gt = false;
    let mut munge = true;
    let mut c;
    let mut lastc;
    let mut delim: u32 = 0;
    let mut quotewarning = 0u32;
    *pdelim = '"' as u32;

    if doc.cfg_bool(OptId::LiteralAttribs) {
        munge = false;
    }

    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            unget(doc, c);
            break;
        }
        if !is_white(c) {
            break;
        }
    }

    if c != '=' as u32 && c != '"' as u32 && c != '\'' as u32 {
        unget(doc, c);
        return None;
    }

    loop {
        c = read(doc);
        if c == END_OF_STREAM {
            unget(doc, c);
            break;
        }
        if !is_white(c) {
            break;
        }
    }

    if c == '"' as u32 || c == '\'' as u32 {
        delim = c;
    } else if c == '<' as u32 {
        let start = doc.lxr().lexsize();
        add_char_to_lexer(doc, c);
        *pdelim = parse_server_instruction(doc);
        let len = doc.lxr().lexsize().wrapping_sub(start) as i32;
        let v = if len > 0 {
            lexndup(doc, start, len as u32)
        } else {
            None
        };
        set_lexsize(doc, start);
        return v;
    } else {
        unget(doc, c);
    }

    let start = doc.lxr().lexsize();
    c = 0;

    loop {
        lastc = c;
        c = read(doc);
        if c == END_OF_STREAM {
            let t = token_node(doc);
            message::report_attr_error(doc, t, None, UNEXPECTED_END_OF_FILE_ATTR);
            unget(doc, c);
            break;
        }
        if delim == 0 {
            if c == '>' as u32 {
                unget(doc, c);
                break;
            }
            if c == '"' as u32 || c == '\'' as u32 {
                let q = c;
                c = read(doc);
                if c == '>' as u32 {
                    add_char_to_lexer(doc, q);
                    unget(doc, c);
                    break;
                } else {
                    unget(doc, c);
                    c = q;
                }
            }
            if c == '<' as u32 {
                unget(doc, c);
                c = '>' as u32;
                unget(doc, c);
                let t = token_node(doc);
                message::report_attr_error(doc, t, None, UNEXPECTED_GT);
                break;
            }
            if c == '/' as u32 {
                c = read(doc);
                if c == '>' as u32 && !attrs::is_url(doc, name) {
                    *isempty = true;
                    unget(doc, c);
                    break;
                }
                unget(doc, c);
                c = '/' as u32;
            }
        } else {
            if c == delim {
                break;
            }
            if c == '\n' as u32 || c == '<' as u32 || c == '>' as u32 {
                quotewarning += 1;
            }
            if c == '>' as u32 {
                seen_gt = true;
            }
        }

        if c == '&' as u32 {
            add_char_to_lexer(doc, c);
            parse_entity(doc, GetTokenMode::IgnoreWhitespace);
            let ls = doc.lxr().lexsize();
            if lexbyte(doc, ls.wrapping_sub(1)) == b'\n' && munge {
                change_char(doc, b' ');
            }
            continue;
        }

        if c == '\\' as u32 {
            c = read(doc);
            if c != '\n' as u32 {
                unget(doc, c);
                c = '\\' as u32;
            }
        }

        if is_white(c) {
            if delim == 0 {
                break;
            }
            if munge {
                if c == '\n' as u32 && attrs::is_url(doc, name) {
                    let t = token_node(doc);
                    message::report_attr_error(doc, t, None, NEWLINE_IN_URI);
                    continue;
                }
                c = ' ' as u32;
                if lastc == ' ' as u32 {
                    if attrs::is_url(doc, name) {
                        let t = token_node(doc);
                        message::report_attr_error(doc, t, None, WHITE_IN_URI);
                    }
                    continue;
                }
            }
        } else if fold_case && is_upper(c) {
            c = to_lower(c);
        }
        add_char_to_lexer(doc, c);
    }

    if quotewarning > 10 && seen_gt && munge {
        let s = lexstr_raw(doc, start);
        if !attrs::is_script(doc, name)
            && !(attrs::is_url(doc, name) && tmbstr::tmbstrncmp(&s, b"javascript:", 11) == 0)
            && tmbstr::tmbstrncmp(&s, b"<xml ", 5) != 0
        {
            message::report(doc, None, None, SUSPECTED_MISSING_QUOTE, &[]);
        }
    }

    let mut len = doc.lxr().lexsize().wrapping_sub(start) as i32;
    // the value bytes (read before truncating, like C reads the stale buffer)
    let buf = lexstr_raw(doc, start);
    set_lexsize(doc, start);

    let value = if len > 0 || delim != 0 {
        let mut s = 0usize;
        let at = |i: usize| buf.get(i).copied().unwrap_or(0) as u32;
        if len > 0
            && munge
            && tmbstr::tmbstrcasecmp(name, b"alt") != 0
            && tmbstr::tmbstrcasecmp(name, b"title") != 0
            && tmbstr::tmbstrcasecmp(name, b"value") != 0
            && tmbstr::tmbstrcasecmp(name, b"prompt") != 0
        {
            while len > 0 && is_white(at(s + len as usize - 1)) {
                len -= 1;
            }
            while len > 0 && is_white(at(s)) {
                s += 1;
                len -= 1;
            }
        }
        let b = buf.get(s..).unwrap_or(&[]);
        tmbstr::tmbstrndup(b, len.max(0) as usize)
    } else {
        None
    };
    *pdelim = delim;
    value
}

/// IsValidAttrName(): attr must be non-NULL
fn is_valid_attr_name(attr: &[u8]) -> bool {
    let attr = tmbstr::cstr(attr);
    let c = attr.first().copied().unwrap_or(0) as u32;
    if !is_letter(c) {
        return false;
    }
    attr[1..].iter().all(|&c| is_namechar(c as u32))
}

/// NewAttribute(): create a new attribute
pub fn new_attribute(doc: &mut Doc) -> AttId {
    doc.alloc_att()
}

/// NewAttributeEx(): create a new attribute with given name and value
pub fn new_attribute_ex(doc: &mut Doc, name: &[u8], value: Option<&[u8]>, delim: u32) -> AttId {
    let av = new_attribute(doc);
    doc[av].attribute = Some(tmbstr::cstr(name).to_vec());
    doc[av].value = value.map(|v| tmbstr::cstr(v).to_vec());
    doc[av].delim = delim;
    let d = attrs::find_attribute(doc, av);
    doc[av].dict = d;
    av
}

fn add_attr_to_list(doc: &mut Doc, list: &mut Option<AttId>, av: AttId) {
    match *list {
        None => *list = Some(av),
        Some(mut here) => {
            while let Some(n) = doc[here].next {
                here = n;
            }
            doc[here].next = Some(av);
        }
    }
}

/// InsertAttributeAtEnd()
pub fn insert_attribute_at_end(doc: &mut Doc, node: NodeId, av: AttId) {
    let mut list = doc[node].attributes;
    add_attr_to_list(doc, &mut list, av);
    doc[node].attributes = list;
}

/// InsertAttributeAtStart()
pub fn insert_attribute_at_start(doc: &mut Doc, node: NodeId, av: AttId) {
    doc[av].next = doc[node].attributes;
    doc[node].attributes = Some(av);
}

/// ParseAttrs(): swallows closing '>'
fn parse_attrs(doc: &mut Doc, isempty: &mut bool) -> Option<AttId> {
    let mut list: Option<AttId> = None;
    while !end_of_input(doc) {
        let mut asp = None;
        let mut php = None;
        let attribute = parse_attribute(doc, isempty, &mut asp, &mut php);
        let Some(attribute) = attribute else {
            if let Some(a) = asp {
                let av = new_attribute(doc);
                doc[av].asp = Some(a);
                add_attr_to_list(doc, &mut list, av);
                continue;
            }
            if let Some(p) = php {
                let av = new_attribute(doc);
                doc[av].php = Some(p);
                add_attr_to_list(doc, &mut list, av);
                continue;
            }
            break;
        };
        let mut delim = 0u32;
        let value = parse_value(doc, &attribute, false, isempty, &mut delim);

        if is_valid_attr_name(&attribute)
            || (doc.cfg_bool(OptId::XmlTags) && attrs::is_valid_xml_id(&attribute))
        {
            let av = new_attribute(doc);
            let has_value = value.is_some();
            doc[av].delim = if delim != 0 { delim } else { '"' as u32 };
            doc[av].attribute = Some(attribute);
            doc[av].value = value;
            let d = attrs::find_attribute(doc, av);
            doc[av].dict = d;
            add_attr_to_list(doc, &mut list, av);
            if delim == 0 && has_value {
                let t = token_node(doc);
                message::report_attr_error(doc, t, Some(av), MISSING_QUOTEMARK_OPEN);
            }
        } else {
            let av = new_attribute(doc);
            let lc = last_char(Some(&attribute));
            let no_value = value.is_none();
            doc[av].attribute = Some(attribute);
            doc[av].value = value;
            let t = token_node(doc);
            if lc == b'"' {
                message::report_attr_error(doc, t, Some(av), MISSING_QUOTEMARK);
            } else if no_value {
                message::report_attr_error(doc, t, Some(av), MISSING_ATTR_VALUE);
            } else {
                message::report_attr_error(doc, t, Some(av), INVALID_ATTRIBUTE);
            }
            free_attribute(doc, av);
        }
    }
    list
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DtState {
    Intermediate,
    DoctypeName,
    PublicSystem,
    QuotedString,
    IntSubset,
}

/// ParseDocTypeDecl(): <!DOCTYPE foo PUBLIC "fpi" "sysid"> as
/// <foo PUBLIC="fpi" SYSTEM="sysid" />
fn parse_doc_type_decl(doc: &mut Doc) -> Option<NodeId> {
    let mut start = doc.lxr().lexsize();
    let mut state = DtState::DoctypeName;
    let mut delim: u32 = 0;
    let mut hasfpi = true;

    let node = new_node(doc);
    let (ts, te) = (doc.lxr().txtstart, doc.lxr().txtend);
    doc[node].type_ = NodeType::DocTypeTag;
    doc[node].start = ts;
    doc[node].end = te;
    doc.lx().waswhite = false;

    loop {
        let mut c = read(doc);
        if c == END_OF_STREAM {
            break;
        }
        if state != DtState::IntSubset && c == '\n' as u32 {
            c = ' ' as u32;
        }
        if is_white(c) && state != DtState::IntSubset {
            if !doc.lxr().waswhite {
                add_char_to_lexer(doc, c);
                doc.lx().waswhite = true;
            } else {
                continue;
            }
        } else {
            add_char_to_lexer(doc, c);
            doc.lx().waswhite = false;
        }

        match state {
            DtState::Intermediate => {
                if to_upper(c) == 'P' as u32 || to_upper(c) == 'S' as u32 {
                    start = doc.lxr().lexsize() - 1;
                    state = DtState::PublicSystem;
                    continue;
                } else if c == '[' as u32 {
                    start = doc.lxr().lexsize();
                    state = DtState::IntSubset;
                    continue;
                } else if c == '\'' as u32 || c == '"' as u32 {
                    start = doc.lxr().lexsize();
                    delim = c;
                    state = DtState::QuotedString;
                    continue;
                } else if c == '>' as u32 {
                    dec_lexsize(doc, 1);
                    doc[node].end = doc.lxr().lexsize();
                    if let Some(si) = attrs::get_attr_by_name(doc, node, b"SYSTEM") {
                        attrs::check_url(doc, node, si);
                    }
                    let valid = doc[node]
                        .element
                        .as_deref()
                        .is_some_and(attrs::is_valid_xml_id);
                    if !valid {
                        message::report(doc, None, None, MALFORMED_DOCTYPE, &[]);
                        free_node(doc, Some(node));
                        return None;
                    }
                    return Some(node);
                }
            }
            DtState::DoctypeName => {
                if is_white(c) || c == '>' as u32 || c == '[' as u32 {
                    let ls = doc.lxr().lexsize();
                    let el = lexndup(doc, start, ls.wrapping_sub(start).wrapping_sub(1));
                    doc[node].element = el;
                    if c == '>' as u32 || c == '[' as u32 {
                        dec_lexsize(doc, 1);
                        unget(doc, c);
                    }
                    state = DtState::Intermediate;
                    continue;
                }
            }
            DtState::PublicSystem => {
                if is_white(c) || c == '>' as u32 {
                    let ls = doc.lxr().lexsize();
                    let attname = lexndup(doc, start, ls.wrapping_sub(start).wrapping_sub(1))
                        .unwrap_or_default();
                    hasfpi = tmbstr::tmbstrcasecmp(&attname, b"SYSTEM") != 0;
                    if c == '>' as u32 {
                        dec_lexsize(doc, 1);
                        unget(doc, c);
                    }
                    state = DtState::Intermediate;
                    continue;
                }
            }
            DtState::QuotedString => {
                if c == delim {
                    let ls = doc.lxr().lexsize();
                    let value = lexndup(doc, start, ls.wrapping_sub(start).wrapping_sub(1))
                        .unwrap_or_default();
                    let name: &[u8] = if hasfpi { b"PUBLIC" } else { b"SYSTEM" };
                    let att = attrs::add_attribute(doc, node, name, &value);
                    doc[att].delim = delim;
                    hasfpi = false;
                    state = DtState::Intermediate;
                    delim = 0;
                    continue;
                }
            }
            DtState::IntSubset => {
                if c == ']' as u32 {
                    let ls = doc.lxr().lexsize();
                    doc.lx().txtstart = start;
                    doc.lx().txtend = ls - 1;
                    let subset = text_token(doc);
                    parser::insert_node_at_end(doc, node, subset);
                    state = DtState::Intermediate;
                }
            }
        }
    }

    message::report(doc, None, None, MALFORMED_DOCTYPE, &[]);
    free_node(doc, Some(node));
    None
}
