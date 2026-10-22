//! gd_xbm.c: XBM reading (the fgets()/sscanf() based parser) and writing.

use crate::*;

const MAX_XBM_LINE_SIZE: usize = 255;

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// A tiny sscanf(): literal words (a format space matches any amount of
/// whitespace), `%s` and `%d`.
enum Tok<'a> {
    Lit(&'a [u8]),
    Ws,
    Str,
    Int,
}

/// Returns the number of conversions done (like sscanf, -1 for an input
/// failure before the first conversion) and the converted values.
fn sscanf(s: &[u8], fmt: &[Tok]) -> (i32, Vec<Vec<u8>>, Vec<i64>) {
    let mut p = 0usize;
    let mut n = 0;
    let mut strs = Vec::new();
    let mut ints = Vec::new();
    for t in fmt {
        match t {
            Tok::Ws => {
                while p < s.len() && is_space(s[p]) {
                    p += 1;
                }
            }
            Tok::Lit(l) => {
                for &c in l.iter() {
                    if p >= s.len() {
                        return (if n == 0 { -1 } else { n }, strs, ints);
                    }
                    if s[p] != c {
                        return (n, strs, ints);
                    }
                    p += 1;
                }
            }
            Tok::Str => {
                while p < s.len() && is_space(s[p]) {
                    p += 1;
                }
                if p >= s.len() {
                    return (if n == 0 { -1 } else { n }, strs, ints);
                }
                let st = p;
                while p < s.len() && !is_space(s[p]) {
                    p += 1;
                }
                strs.push(s[st..p].to_vec());
                n += 1;
            }
            Tok::Int => {
                while p < s.len() && is_space(s[p]) {
                    p += 1;
                }
                if p >= s.len() {
                    return (if n == 0 { -1 } else { n }, strs, ints);
                }
                let mut q = p;
                let neg = s[q] == b'-';
                if s[q] == b'-' || s[q] == b'+' {
                    q += 1;
                }
                let st = q;
                let mut v: i64 = 0;
                let mut overflow = false;
                while q < s.len() && s[q].is_ascii_digit() {
                    match v
                        .checked_mul(10)
                        .and_then(|v| v.checked_add((s[q] - b'0') as i64))
                    {
                        Some(nv) => v = nv,
                        None => overflow = true,
                    }
                    q += 1;
                }
                if q == st {
                    return (n, strs, ints);
                }
                if overflow {
                    v = if neg { i64::MIN } else { i64::MAX };
                } else if neg {
                    v = -v;
                }
                p = q;
                ints.push(v);
                n += 1;
            }
        }
    }
    (n, strs, ints)
}

fn words(f: &str) -> Vec<Tok<'_>> {
    // "static unsigned char %s = {"
    let mut v = Vec::new();
    for (i, w) in f.split(' ').enumerate() {
        if i > 0 {
            v.push(Tok::Ws);
        }
        if w == "%s" {
            v.push(Tok::Str);
        } else if w == "%d" {
            v.push(Tok::Int);
        } else {
            v.push(Tok::Lit(w.as_bytes()));
        }
    }
    v
}

/// The part of a name after its last '_'
fn type_of(iname: &[u8]) -> &[u8] {
    match iname.iter().rposition(|&c| c == b'_') {
        Some(p) => &iname[p + 1..],
        None => iname,
    }
}

/// sscanf(h, "%x", &b)
fn scan_hex(h: &[u8]) -> Option<u32> {
    let h = &h[..h.iter().position(|&c| c == 0).unwrap_or(h.len())];
    let mut p = 0;
    while p < h.len() && is_space(h[p]) {
        p += 1;
    }
    if p >= h.len() {
        return None;
    }
    let mut neg = false;
    if h[p] == b'-' || h[p] == b'+' {
        neg = h[p] == b'-';
        p += 1;
    }
    if p + 1 < h.len() && h[p] == b'0' && (h[p + 1] | 0x20) == b'x' {
        if p + 2 < h.len() && h[p + 2].is_ascii_hexdigit() {
            p += 2;
        } else {
            // "0x" without digits: the "0" is converted
            return Some(0);
        }
    }
    let st = p;
    let mut v: u32 = 0;
    while p < h.len() && h[p].is_ascii_hexdigit() {
        v = v
            .wrapping_mul(16)
            .wrapping_add((h[p] as char).to_digit(16).unwrap());
        p += 1;
    }
    if p == st {
        return None;
    }
    Some(if neg { v.wrapping_neg() } else { v })
}

/// gdImageCreateFromXbm()
pub fn create_from_xbm(data: &[u8]) -> Option<Image> {
    let mut pos = 0usize;
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut max_bit = 0u32;
    let mut bytes: i32 = 0;
    let mut iname: Vec<u8> = Vec::new();
    let def = words("#define %s %d");
    let fmts = [
        (words("static unsigned char %s = {"), 128u32),
        (words("static char %s = {"), 128),
        (words("static unsigned short %s = {"), 32768),
        (words("static short %s = {"), 32768),
    ];
    // fgets() loop
    while pos < data.len() {
        let mut end = pos;
        while end < data.len() && end - pos < MAX_XBM_LINE_SIZE - 1 {
            end += 1;
            if data[end - 1] == b'\n' {
                break;
            }
        }
        let raw = &data[pos..end];
        pos = end;
        let line = &raw[..raw.iter().position(|&c| c == 0).unwrap_or(raw.len())];
        if line.len() == MAX_XBM_LINE_SIZE - 1 {
            return None;
        }
        let (n, strs, ints) = sscanf(line, &def);
        if let Some(s) = strs.first() {
            iname = s.clone();
        }
        if n == 2 {
            let ty = type_of(&iname);
            let value = ints[0] as i32;
            if ty == b"width" {
                width = value as u32;
            }
            if ty == b"height" {
                height = value as u32;
            }
        } else {
            for (f, mb) in fmts.iter() {
                let (n, strs, _) = sscanf(line, f);
                if let Some(s) = strs.first() {
                    iname = s.clone();
                }
                if n == 1 {
                    max_bit = *mb;
                    break;
                }
            }
            if max_bit != 0 {
                bytes = ((width.wrapping_add(7)) / 8).wrapping_mul(height) as i32;
                if bytes == 0 {
                    return None;
                }
                if type_of(&iname) == b"bits[]" {
                    break;
                }
            }
        }
    }
    if bytes == 0 || max_bit == 0 {
        return None;
    }
    let mut im = Image::create(width as i32, height as i32)?;
    im.color_allocate(255, 255, 255);
    im.color_allocate(0, 0, 0);
    let mut h = [0u8; 8];
    let (mut x, mut y) = (0, 0);
    let getc = |pos: &mut usize| -> i32 {
        if *pos < data.len() {
            *pos += 1;
            data[*pos - 1] as i32
        } else {
            -1
        }
    };
    let mut i = 0;
    'outer: while i < bytes {
        loop {
            let ch = getc(&mut pos);
            if ch == -1 {
                break 'outer;
            }
            if ch == b'x' as i32 {
                break;
            }
        }
        let ch = getc(&mut pos);
        if ch == -1 {
            break;
        }
        h[0] = ch as u8;
        let ch = getc(&mut pos);
        if ch == -1 {
            break;
        }
        h[1] = ch as u8;
        if max_bit == 32768 {
            let ch = getc(&mut pos);
            if ch == -1 {
                break;
            }
            h[2] = ch as u8;
            let ch = getc(&mut pos);
            if ch == -1 {
                break;
            }
            h[3] = ch as u8;
        }
        let Some(b) = scan_hex(&h) else {
            gd_error("Invalid XBM");
            return None;
        };
        let mut bit = 1u32;
        while bit <= max_bit {
            im.set_pixel(x, y, if b & bit != 0 { 1 } else { 0 });
            x += 1;
            if x == im.sx {
                x = 0;
                y += 1;
                if y == im.sy {
                    return Some(im);
                }
                break;
            }
            bit <<= 1;
        }
        i += 1;
    }
    gd_error("EOF before image was complete");
    None
}

/// gdImageXbmCtx()
pub fn image_xbm(image: &Image, file_name: &[u8], fg: i32) -> Vec<u8> {
    let mut name = file_name;
    if let Some(p) = name.iter().rposition(|&c| c == b'/') {
        name = &name[p + 1..];
    }
    if let Some(p) = name.iter().rposition(|&c| c == b'\\') {
        name = &name[p + 1..];
    }
    let mut name: Vec<u8> =
        name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())].to_vec();
    if let Some(p) = name.iter().rposition(|&c| c == b'.') {
        if name[p..].eq_ignore_ascii_case(b".XBM") {
            name.truncate(p);
        }
    }
    if name.is_empty() {
        name = b"image".to_vec();
    } else {
        for c in name.iter_mut() {
            if !c.is_ascii_alphanumeric() {
                *c = b'_';
            }
        }
    }
    let name = String::from_utf8_lossy(&name).into_owned();
    let mut out = String::new();
    out.push_str(&format!("#define {}_width {}\n", name, image.sx));
    out.push_str(&format!("#define {}_height {}\n", name, image.sy));
    out.push_str(&format!("static unsigned char {}_bits[] = {{\n  ", name));
    let mut b = 1;
    let mut p = 0;
    let mut c = 0;
    let (sx, sy) = (image.sx, image.sy);
    for y in 0..sy {
        for x in 0..sx {
            if image.get_pixel(x, y) == fg {
                c |= b;
            }
            if b == 128 || x == sx - 1 {
                b = 1;
                if p != 0 {
                    out.push_str(", ");
                    if p % 12 == 0 {
                        out.push_str("\n  ");
                        p = 12;
                    }
                }
                p += 1;
                out.push_str(&format!("0x{:02X}", c));
                c = 0;
            } else {
                b <<= 1;
            }
        }
    }
    out.push_str("};\n");
    out.into_bytes()
}
