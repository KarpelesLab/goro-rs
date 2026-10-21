//! Base64, Quoted-Printable, UUENCODE and HTML-ENTITIES pseudo-encodings
//! (C: mbfilter_base64.c, mbfilter_qprint.c, mbfilter_uuencode.c,
//! mbfilter_htmlent.c, html_entities.c)

use super::tables::filt_htmlent::htmlentitifieds;
use super::*;

/* ===== Base64 ===== */

static BASE64_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn decode_base64(c: u8) -> i32 {
    match c {
        b'A'..=b'Z' => (c - b'A') as i32,
        b'a'..=b'z' => (c - b'a') as i32 + 26,
        b'0'..=b'9' => (c - b'0') as i32 + 52,
        b'+' => 62,
        b'/' => 63,
        _ => -1,
    }
}

fn mb_base64_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let mut bits = *state & 0xFF;
    let mut cache = *state >> 8;
    for &c in input {
        if c == b'\r' || c == b'\n' || c == b' ' || c == b'\t' || c == b'=' {
            continue;
        }
        let value = decode_base64(c);
        if value == -1 {
            out.push(MBFL_BAD_INPUT);
        } else {
            bits += 6;
            cache = (cache << 6) | (value as u32 & 0x3F);
            if bits == 24 {
                out.push((cache >> 16) & 0xFF);
                out.push((cache >> 8) & 0xFF);
                out.push(cache & 0xFF);
                bits = 0;
                cache = 0;
            }
        }
    }
    // p == e (we always consume all input)
    if bits != 0 {
        if bits == 18 {
            out.push((cache >> 10) & 0xFF);
            out.push((cache >> 2) & 0xFF);
        } else if bits == 12 {
            out.push((cache >> 4) & 0xFF);
        }
    }
}

fn mb_wchar_to_base64(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let mut bits = (buf.state & 0x3) * 8;
    let mut chars_output = ((buf.state >> 2) & 0x3F) * 4;
    let mut cache = buf.state >> 8;

    for &w in input {
        cache = (cache << 8) | (w & 0xFF);
        bits += 8;
        if bits == 24 {
            if chars_output > 72 {
                buf.add2(b'\r', b'\n');
                chars_output = 0;
            }
            buf.add4(
                BASE64_TABLE[((cache >> 18) & 0x3F) as usize],
                BASE64_TABLE[((cache >> 12) & 0x3F) as usize],
                BASE64_TABLE[((cache >> 6) & 0x3F) as usize],
                BASE64_TABLE[(cache & 0x3F) as usize],
            );
            chars_output += 4;
            bits = 0;
            cache = 0;
        }
    }

    if end && bits != 0 {
        if chars_output > 72 {
            buf.add2(b'\r', b'\n');
        }
        if bits == 8 {
            buf.add4(
                BASE64_TABLE[((cache >> 2) & 0x3F) as usize],
                BASE64_TABLE[((cache & 0x3) << 4) as usize],
                b'=',
                b'=',
            );
        } else {
            buf.add4(
                BASE64_TABLE[((cache >> 10) & 0x3F) as usize],
                BASE64_TABLE[((cache >> 4) & 0x3F) as usize],
                BASE64_TABLE[((cache & 0xF) << 2) as usize],
                b'=',
            );
        }
    } else {
        buf.state = (cache << 8) | (((chars_output / 4) & 0x3F) << 2) | ((bits / 8) & 0x3);
    }
}

pub static ENC_BASE64: Encoding = Encoding {
    no: EncNo::Base64,
    name: "BASE64",
    mime_name: Some("BASE64"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_base64_to_wchar),
    from_wchar: Some(mb_wchar_to_base64),
    check: None,
    cut: None,
};

/* ===== Quoted-Printable ===== */

#[inline]
fn hex2code(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => (c - b'0') as i32,
        b'A'..=b'F' => (c - b'A') as i32 + 10,
        b'a'..=b'f' => (c - b'a') as i32 + 10,
        _ => -1,
    }
}

fn mb_qprint_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    while p < e {
        let c = input[p];
        p += 1;
        if c == b'=' && p < e {
            let c2 = input[p];
            p += 1;
            if hex2code(c2) >= 0 && p < e {
                let c3 = input[p];
                p += 1;
                if hex2code(c3) >= 0 {
                    out.push(((hex2code(c2) << 4) | hex2code(c3)) as u32);
                } else {
                    out.push('=' as u32);
                    out.push(c2 as u32);
                    out.push(c3 as u32);
                }
            } else if c2 == b'\r' && p < e {
                let c3 = input[p];
                p += 1;
                if c3 != b'\n' {
                    out.push(c3 as u32);
                }
            } else if c2 != b'\n' {
                out.push('=' as u32);
                out.push(c2 as u32);
            }
        } else {
            out.push(c as u32);
        }
    }
}

#[inline]
fn qprint_enc_nibble(nibble: u8) -> u8 {
    if nibble < 10 {
        nibble + b'0'
    } else {
        nibble - 10 + b'A'
    }
}

fn mb_wchar_to_qprint(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    let mut chars_output = buf.state;
    for &w in input {
        if w == 0 {
            buf.add(0);
            chars_output = 0;
            continue;
        } else if w == '\n' as u32 {
            buf.add2(b'\r', b'\n');
            chars_output = 0;
            continue;
        } else if w == '\r' as u32 {
            continue;
        }
        if chars_output >= 72 {
            buf.add3(b'=', b'\r', b'\n');
            chars_output = 0;
        }
        if w >= 0x80 || w == '=' as u32 {
            buf.add3(
                b'=',
                qprint_enc_nibble(((w >> 4) & 0xF) as u8),
                qprint_enc_nibble((w & 0xF) as u8),
            );
            chars_output += 3;
        } else {
            buf.add(w as u8);
            chars_output += 1;
        }
    }
    buf.state = chars_output;
}

pub static ENC_QPRINT: Encoding = Encoding {
    no: EncNo::Qprint,
    name: "Quoted-Printable",
    mime_name: Some("Quoted-Printable"),
    aliases: &["qprint"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_qprint_to_wchar),
    from_wchar: Some(mb_wchar_to_qprint),
    check: None,
    cut: None,
};

/* ===== UUENCODE ===== */

const UUDEC_STATE_GROUND: u32 = 0;
const UUDEC_STATE_SIZE: u32 = 3;
const UUDEC_STATE_A: u32 = 4;
const UUDEC_STATE_SKIP_NEWLINE: u32 = 8;

/// C: `#define UUDEC(c) (char)(((c)-' ') & 077)`
#[inline]
fn uudec(c: u8) -> u32 {
    (c.wrapping_sub(b' ') & 0o77) as u32
}

fn mb_uuencode_to_wchar(input: &[u8], out: &mut Vec<u32>, state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    let mut st = *state & 0xFF;
    let mut size = *state >> 8;

    while p < e {
        let c = input[p];
        p += 1;
        match st {
            UUDEC_STATE_GROUND => {
                if c == b'b' && e - p >= 5 && &input[p..p + 5] == b"egin " {
                    p += 5;
                    while p < e {
                        let ch = input[p];
                        p += 1;
                        if ch == b'\n' {
                            break;
                        }
                    }
                    st = UUDEC_STATE_SIZE;
                }
            }
            UUDEC_STATE_SIZE => {
                size = uudec(c);
                st = UUDEC_STATE_A;
            }
            UUDEC_STATE_A => {
                if e - p < 4 {
                    p = e;
                    continue;
                }
                let a = uudec(c);
                let b = uudec(input[p]);
                let cc = uudec(input[p + 1]);
                let d = uudec(input[p + 2]);
                p += 3;
                if size > 0 {
                    out.push(((a << 2) | (b >> 4)) & 0xFF);
                    size -= 1;
                }
                if size > 0 {
                    out.push(((b << 4) | (cc >> 2)) & 0xFF);
                    size -= 1;
                }
                if size > 0 {
                    out.push(((cc << 6) | d) & 0xFF);
                    size -= 1;
                }
                st = if size != 0 {
                    UUDEC_STATE_A
                } else {
                    UUDEC_STATE_SKIP_NEWLINE
                };
            }
            UUDEC_STATE_SKIP_NEWLINE => {
                st = UUDEC_STATE_SIZE;
            }
            _ => {}
        }
    }
    *state = (size << 8) | st;
}

#[inline]
fn uuencode_six_bits(bits: u32) -> u8 {
    if bits == 0 { b'`' } else { (bits + 32) as u8 }
}

fn mb_wchar_to_uuencode(input: &[u32], buf: &mut ConvBuf, end: bool) {
    let mut bytes_encoded = (buf.state >> 1) & 0x7F;
    let mut n_cached_bits = (buf.state >> 8) & 0xFF;
    let mut cached_bits = buf.state >> 16;
    let mut len = input.len();
    let mut i = 0usize;
    let mut jump_to_line_break = false;

    if buf.state == 0 {
        buf.appends(b"begin 0644 filename\n");
        buf.add((len.min(45) + 32) as u8);
        buf.state |= 1;
    } else if len == 0 && end && bytes_encoded == 0 && n_cached_bits == 0 {
        buf.out.pop();
        return;
    } else {
        let mut len_byte = buf.out.len() as isize - (bytes_encoded * 4 / 3) as isize - 1;
        if n_cached_bits != 0 {
            len_byte -= if n_cached_bits == 2 { 1 } else { 2 };
        }
        let v = (bytes_encoded as usize
            + len
            + if n_cached_bits != 0 {
                if n_cached_bits == 2 { 1 } else { 2 }
            } else {
                0
            })
        .min(45)
            + 32;
        if len_byte >= 0 && (len_byte as usize) < buf.out.len() {
            buf.out[len_byte as usize] = v as u8;
        }

        if n_cached_bits != 0 {
            if n_cached_bits == 2 {
                let w = cached_bits;
                let mut w2 = 0;
                let mut w3 = 0;
                if len > 0 {
                    w2 = input[i];
                    i += 1;
                    len -= 1;
                }
                if len > 0 {
                    w3 = input[i];
                    i += 1;
                    len -= 1;
                }
                buf.add3(
                    uuencode_six_bits((w << 4).wrapping_add((w2 >> 4) & 0xF)),
                    uuencode_six_bits(((w2 & 0xF) << 2) + ((w3 >> 6) & 0x3)),
                    uuencode_six_bits(w3 & 0x3F),
                );
            } else {
                let w2 = cached_bits;
                let mut w3 = 0;
                if len > 0 {
                    w3 = input[i];
                    i += 1;
                    len -= 1;
                }
                buf.add2(
                    uuencode_six_bits((w2 << 2).wrapping_add((w3 >> 6) & 0x3)),
                    uuencode_six_bits(w3 & 0x3F),
                );
            }
            n_cached_bits = 0;
            cached_bits = 0;
            jump_to_line_break = true;
        }
    }

    loop {
        if !jump_to_line_break {
            if len == 0 {
                break;
            }
            len -= 1;
            let w = input[i];
            i += 1;
            let mut w2 = 0;
            let mut w3 = 0;

            if len == 0 {
                if !end {
                    buf.add(uuencode_six_bits((w >> 2) & 0x3F));
                    cached_bits = w & 0x3;
                    n_cached_bits = 2;
                    break;
                }
            } else {
                w2 = input[i];
                i += 1;
                len -= 1;
            }

            if len == 0 {
                if !end {
                    buf.add2(
                        uuencode_six_bits((w >> 2) & 0x3F),
                        uuencode_six_bits(((w & 0x3) << 4) + ((w2 >> 4) & 0xF)),
                    );
                    cached_bits = w2 & 0xF;
                    n_cached_bits = 4;
                    break;
                }
            } else {
                w3 = input[i];
                i += 1;
                len -= 1;
            }

            buf.add4(
                uuencode_six_bits((w >> 2) & 0x3F),
                uuencode_six_bits(((w & 0x3) << 4) + ((w2 >> 4) & 0xF)),
                uuencode_six_bits(((w2 & 0xF) << 2) + ((w3 >> 6) & 0x3)),
                uuencode_six_bits(w3 & 0x3F),
            );
        }
        jump_to_line_break = false;

        // possible_line_break:
        bytes_encoded += 3;
        if bytes_encoded >= 45 {
            buf.add(b'\n');
            if len > 0 || !end {
                buf.add((len.min(45) + 32) as u8);
            }
            bytes_encoded = 0;
        }
    }

    if bytes_encoded != 0 && end {
        buf.add(b'\n');
    }

    buf.state = ((cached_bits & 0xFF) << 16)
        | ((n_cached_bits & 0xFF) << 8)
        | ((bytes_encoded & 0x7F) << 1)
        | (buf.state & 1);
}

pub static ENC_UUENCODE: Encoding = Encoding {
    no: EncNo::Uuencode,
    name: "UUENCODE",
    mime_name: Some("x-uuencode"),
    aliases: &[],
    mblen_table: None,
    flag: MBFL_ENCTYPE_SBCS,
    to_wchar: Some(mb_uuencode_to_wchar),
    from_wchar: Some(mb_wchar_to_uuencode),
    check: None,
    cut: None,
};

/* ===== HTML-ENTITIES ===== */

#[inline]
fn is_html_entity_char(c: u8) -> bool {
    c.is_ascii_digit() || c.is_ascii_uppercase() || c.is_ascii_lowercase() || c == b'#'
}

fn mb_htmlent_to_wchar(input: &[u8], out: &mut Vec<u32>, _state: &mut u32) {
    let e = input.len();
    let mut p = 0usize;
    'outer: while p < e {
        let c = input[p];
        p += 1;
        if c != b'&' {
            out.push(c as u32);
            continue;
        }
        let mut terminator = p;
        while terminator < e && is_html_entity_char(input[terminator]) {
            terminator += 1;
        }
        if terminator < e && input[terminator] == b';' {
            if input[p] == b'#' && e - p >= 2 {
                /* Numeric entity */
                let mut value: u32 = 0;
                let mut digits = p + 1;
                let mut bad = false;
                if input[digits] == b'x' || input[digits] == b'X' {
                    digits += 1;
                    if digits == terminator {
                        bad = true;
                    }
                    while !bad && digits < terminator {
                        let d = input[digits];
                        digits += 1;
                        let v = match d {
                            b'0'..=b'9' => d - b'0',
                            b'A'..=b'F' => d - b'A' + 10,
                            b'a'..=b'f' => d - b'a' + 10,
                            _ => {
                                bad = true;
                                break;
                            }
                        };
                        value = value.wrapping_mul(16).wrapping_add(v as u32);
                    }
                } else {
                    if digits == terminator {
                        bad = true;
                    }
                    while !bad && digits < terminator {
                        let d = input[digits];
                        digits += 1;
                        if d.is_ascii_digit() {
                            value = value.wrapping_mul(10).wrapping_add((d - b'0') as u32);
                        } else {
                            bad = true;
                        }
                    }
                }
                if !bad && value <= 0x10FFFF {
                    out.push(value);
                    p = terminator + 1;
                    continue 'outer;
                }
            } else if terminator > p {
                /* Named entity */
                let name = &input[p..terminator];
                for (ename, code) in HTML_ENTITY_LIST {
                    if ename.as_bytes() == name {
                        out.push(*code);
                        p = terminator + 1;
                        continue 'outer;
                    }
                }
            }
        }
        /* bad_entity */
        out.push('&' as u32);
        while p < terminator {
            out.push(input[p] as u32);
            p += 1;
        }
        if terminator < e && input[terminator] == b';' {
            out.push(input[p] as u32);
            p += 1;
        }
    }
}

fn mb_wchar_to_htmlent(input: &[u32], buf: &mut ConvBuf, _end: bool) {
    'outer: for &w in input {
        if (w as usize) < htmlentitifieds.len() && htmlentitifieds[w as usize] != 1 {
            buf.add(w as u8);
            continue;
        }
        buf.add(b'&');
        for (name, code) in HTML_ENTITY_LIST {
            if w == *code {
                buf.appends(name.as_bytes());
                buf.add(b';');
                continue 'outer;
            }
        }
        buf.add(b'#');
        buf.appends(w.to_string().as_bytes());
        buf.add(b';');
    }
}

pub static ENC_HTMLENT: Encoding = Encoding {
    no: EncNo::HtmlEnt,
    name: "HTML-ENTITIES",
    mime_name: Some("HTML-ENTITIES"),
    aliases: &["HTML", "html"],
    mblen_table: None,
    flag: MBFL_ENCTYPE_GL_UNSAFE,
    to_wchar: Some(mb_htmlent_to_wchar),
    from_wchar: Some(mb_wchar_to_htmlent),
    check: None,
    cut: None,
};

pub static ENCODINGS: &[&Encoding] = &[&ENC_BASE64, &ENC_UUENCODE, &ENC_HTMLENT, &ENC_QPRINT];

/// C: `mbfl_html_entity_list` (html_entities.c)
pub static HTML_ENTITY_LIST: &[(&str, u32)] = &[
    ("quot", 34),
    ("amp", 38),
    ("lt", 60),
    ("gt", 62),
    ("nbsp", 160),
    ("iexcl", 161),
    ("cent", 162),
    ("pound", 163),
    ("curren", 164),
    ("yen", 165),
    ("brvbar", 166),
    ("sect", 167),
    ("uml", 168),
    ("copy", 169),
    ("ordf", 170),
    ("laquo", 171),
    ("not", 172),
    ("shy", 173),
    ("reg", 174),
    ("macr", 175),
    ("deg", 176),
    ("plusmn", 177),
    ("sup2", 178),
    ("sup3", 179),
    ("acute", 180),
    ("micro", 181),
    ("para", 182),
    ("middot", 183),
    ("cedil", 184),
    ("sup1", 185),
    ("ordm", 186),
    ("raquo", 187),
    ("frac14", 188),
    ("frac12", 189),
    ("frac34", 190),
    ("iquest", 191),
    ("Agrave", 192),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Atilde", 195),
    ("Auml", 196),
    ("Aring", 197),
    ("AElig", 198),
    ("Ccedil", 199),
    ("Egrave", 200),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Euml", 203),
    ("Igrave", 204),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Iuml", 207),
    ("ETH", 208),
    ("Ntilde", 209),
    ("Ograve", 210),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Otilde", 213),
    ("Ouml", 214),
    ("times", 215),
    ("Oslash", 216),
    ("Ugrave", 217),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Uuml", 220),
    ("Yacute", 221),
    ("THORN", 222),
    ("szlig", 223),
    ("agrave", 224),
    ("aacute", 225),
    ("acirc", 226),
    ("atilde", 227),
    ("auml", 228),
    ("aring", 229),
    ("aelig", 230),
    ("ccedil", 231),
    ("egrave", 232),
    ("eacute", 233),
    ("ecirc", 234),
    ("euml", 235),
    ("igrave", 236),
    ("iacute", 237),
    ("icirc", 238),
    ("iuml", 239),
    ("eth", 240),
    ("ntilde", 241),
    ("ograve", 242),
    ("oacute", 243),
    ("ocirc", 244),
    ("otilde", 245),
    ("ouml", 246),
    ("divide", 247),
    ("oslash", 248),
    ("ugrave", 249),
    ("uacute", 250),
    ("ucirc", 251),
    ("uuml", 252),
    ("yacute", 253),
    ("thorn", 254),
    ("yuml", 255),
    ("OElig", 338),
    ("oelig", 339),
    ("Scaron", 352),
    ("scaron", 353),
    ("Yuml", 376),
    ("fnof", 402),
    ("circ", 710),
    ("tilde", 732),
    ("Alpha", 913),
    ("Beta", 914),
    ("Gamma", 915),
    ("Delta", 916),
    ("Epsilon", 917),
    ("Zeta", 918),
    ("Eta", 919),
    ("Theta", 920),
    ("Iota", 921),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Nu", 925),
    ("Xi", 926),
    ("Omicron", 927),
    ("Pi", 928),
    ("Rho", 929),
    ("Sigma", 931),
    ("Tau", 932),
    ("Upsilon", 933),
    ("Phi", 934),
    ("Chi", 935),
    ("Psi", 936),
    ("Omega", 937),
    ("alpha", 945),
    ("beta", 946),
    ("gamma", 947),
    ("delta", 948),
    ("epsilon", 949),
    ("zeta", 950),
    ("eta", 951),
    ("theta", 952),
    ("iota", 953),
    ("kappa", 954),
    ("lambda", 955),
    ("mu", 956),
    ("nu", 957),
    ("xi", 958),
    ("omicron", 959),
    ("pi", 960),
    ("rho", 961),
    ("sigmaf", 962),
    ("sigma", 963),
    ("tau", 964),
    ("upsilon", 965),
    ("phi", 966),
    ("chi", 967),
    ("psi", 968),
    ("omega", 969),
    ("thetasym", 977),
    ("upsih", 978),
    ("piv", 982),
    ("ensp", 8194),
    ("emsp", 8195),
    ("thinsp", 8201),
    ("zwnj", 8204),
    ("zwj", 8205),
    ("lrm", 8206),
    ("rlm", 8207),
    ("ndash", 8211),
    ("mdash", 8212),
    ("lsquo", 8216),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("ldquo", 8220),
    ("rdquo", 8221),
    ("bdquo", 8222),
    ("dagger", 8224),
    ("Dagger", 8225),
    ("bull", 8226),
    ("hellip", 8230),
    ("permil", 8240),
    ("prime", 8242),
    ("Prime", 8243),
    ("lsaquo", 8249),
    ("rsaquo", 8250),
    ("oline", 8254),
    ("frasl", 8260),
    ("euro", 8364),
    ("weierp", 8472),
    ("image", 8465),
    ("real", 8476),
    ("trade", 8482),
    ("alefsym", 8501),
    ("larr", 8592),
    ("uarr", 8593),
    ("rarr", 8594),
    ("darr", 8595),
    ("harr", 8596),
    ("crarr", 8629),
    ("lArr", 8656),
    ("uArr", 8657),
    ("rArr", 8658),
    ("dArr", 8659),
    ("hArr", 8660),
    ("forall", 8704),
    ("part", 8706),
    ("exist", 8707),
    ("empty", 8709),
    ("nabla", 8711),
    ("isin", 8712),
    ("notin", 8713),
    ("ni", 8715),
    ("prod", 8719),
    ("sum", 8721),
    ("minus", 8722),
    ("lowast", 8727),
    ("radic", 8730),
    ("prop", 8733),
    ("infin", 8734),
    ("ang", 8736),
    ("and", 8743),
    ("or", 8744),
    ("cap", 8745),
    ("cup", 8746),
    ("int", 8747),
    ("there4", 8756),
    ("sim", 8764),
    ("cong", 8773),
    ("asymp", 8776),
    ("ne", 8800),
    ("equiv", 8801),
    ("le", 8804),
    ("ge", 8805),
    ("sub", 8834),
    ("sup", 8835),
    ("nsub", 8836),
    ("sube", 8838),
    ("supe", 8839),
    ("oplus", 8853),
    ("otimes", 8855),
    ("perp", 8869),
    ("sdot", 8901),
    ("lceil", 8968),
    ("rceil", 8969),
    ("lfloor", 8970),
    ("rfloor", 8971),
    ("lang", 9001),
    ("rang", 9002),
    ("loz", 9674),
    ("spades", 9824),
    ("clubs", 9827),
    ("hearts", 9829),
    ("diams", 9830),
];
