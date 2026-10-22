//! gdxpm.c on top of a parser for what libXpm's XpmReadFileToXpmImage()
//! accepts (XPM3 C source, and XPM2).
//!
//! Note that gdxpm.c interprets the color strings itself: only "#rgb",
//! "#rrggbb", "#rrrgggbbb" and "#rrrrggggbbbb" (by length) are decoded;
//! any other string of one of those lengths (e.g. "None") goes through
//! strtol() on its characters, and other lengths keep the previous color.

use crate::*;

struct XpmImage {
    width: i32,
    height: i32,
    colors: Vec<Option<Vec<u8>>>,
    data: Vec<usize>,
}

/// The strings of an XPM3 file (C string literals outside comments) or
/// the lines of an XPM2 one.
fn strings(data: &[u8]) -> Option<Vec<&[u8]>> {
    let mut p = 0;
    while p < data.len() && data[p].is_ascii_whitespace() {
        p += 1;
    }
    let rest = &data[p..];
    if rest.starts_with(b"! XPM2") {
        let body = &rest[6..];
        let mut v: Vec<&[u8]> = body.split(|&c| c == b'\n').collect();
        if v.first()
            .is_some_and(|l| l.iter().all(|c| c.is_ascii_whitespace()))
        {
            v.remove(0);
        }
        return Some(
            v.into_iter()
                .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
                .collect(),
        );
    }
    // XPM3: "/* XPM */" header
    if !rest.starts_with(b"/*") {
        return None;
    }
    let end = find(rest, b"*/")?;
    let hdr = &rest[2..end];
    let word: Vec<&[u8]> = hdr
        .split(|c| c.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    if word.first() != Some(&&b"XPM"[..]) {
        return None;
    }
    let mut out = Vec::new();
    let mut i = end + 2;
    while i < rest.len() {
        let c = rest[i];
        if c == b'/' && rest.get(i + 1) == Some(&b'*') {
            match find(&rest[i + 2..], b"*/") {
                Some(e) => i = i + 2 + e + 2,
                None => break,
            }
            continue;
        }
        if c == b'"' {
            let st = i + 1;
            let mut j = st;
            while j < rest.len() && rest[j] != b'"' {
                j += 1;
            }
            out.push(&rest[st..j]);
            i = j + 1;
            continue;
        }
        i += 1;
    }
    Some(out)
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn words(s: &[u8]) -> Vec<&[u8]> {
    s.split(|&c| c == b' ' || c == b'\t')
        .filter(|w| !w.is_empty())
        .collect()
}

fn parse_ui(w: &[u8]) -> Option<i32> {
    if w.is_empty() || !w.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut v: u32 = 0;
    for &c in w {
        v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
    }
    Some(v as i32)
}

/// XpmReadFileToXpmImage()
fn read_xpm(data: &[u8]) -> Option<XpmImage> {
    let strs = strings(data)?;
    let mut it = strs.into_iter();
    let values = it.next()?;
    let vw = words(values);
    if vw.len() < 4 {
        return None;
    }
    let width = parse_ui(vw[0])?;
    let height = parse_ui(vw[1])?;
    let ncolors = parse_ui(vw[2])?;
    let cpp = parse_ui(vw[3])? as usize;
    if ncolors <= 0 || cpp == 0 {
        return None;
    }
    // colors
    const KEYS: [&[u8]; 5] = [b"s", b"m", b"g4", b"g", b"c"];
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut colors = Vec::new();
    for _ in 0..ncolors {
        let line = it.next()?;
        if line.len() < cpp {
            return None;
        }
        names.push(line[..cpp].to_vec());
        let mut curkey = 0usize;
        let mut lastwaskey = false;
        let mut curbuf: Vec<u8> = Vec::new();
        let mut c_color: Option<Vec<u8>> = None;
        for w in words(&line[cpp..]) {
            let key = if !lastwaskey {
                KEYS.iter().position(|k| *k == w)
            } else {
                None
            };
            if let Some(key) = key {
                if curkey == 5 {
                    c_color = Some(std::mem::take(&mut curbuf));
                }
                curkey = key + 1;
                curbuf.clear();
                lastwaskey = true;
            } else {
                if curkey == 0 {
                    return None;
                }
                if !lastwaskey {
                    curbuf.push(b' ');
                }
                curbuf.extend_from_slice(w);
                lastwaskey = false;
            }
        }
        if curkey == 0 {
            return None;
        }
        if curkey == 5 {
            c_color = Some(curbuf);
        }
        colors.push(c_color);
    }
    // pixels
    let mut map = std::collections::HashMap::new();
    for (i, n) in names.iter().enumerate() {
        map.entry(n.clone()).or_insert(i);
    }
    let mut pix = Vec::with_capacity((width.max(0) as usize) * (height.max(0) as usize));
    for _ in 0..height {
        let line = it.next()?;
        for x in 0..width as usize {
            let s = line.get(x * cpp..(x + 1) * cpp)?;
            pix.push(*map.get(s)?);
        }
    }
    Some(XpmImage {
        width,
        height,
        colors,
        data: pix,
    })
}

/// strtol(buf, NULL, 16)
fn strtol16(b: &[u8]) -> i32 {
    let mut p = 0;
    while p < b.len() && (b[p] == b' ' || (9..=13).contains(&b[p])) {
        p += 1;
    }
    let mut neg = false;
    if p < b.len() && (b[p] == b'+' || b[p] == b'-') {
        neg = b[p] == b'-';
        p += 1;
    }
    if p + 1 < b.len()
        && b[p] == b'0'
        && (b[p + 1] | 0x20) == b'x'
        && b.get(p + 2).is_some_and(|c| c.is_ascii_hexdigit())
    {
        p += 2;
    }
    let mut v: i64 = 0;
    while p < b.len() && b[p].is_ascii_hexdigit() {
        v = v * 16 + (b[p] as char).to_digit(16).unwrap() as i64;
        p += 1;
    }
    (if neg { -v } else { v }) as i32
}

/// gdImageCreateFromXpm() (on the file contents)
pub fn create_from_xpm(data: &[u8]) -> Option<Image> {
    let image = read_xpm(data)?;
    if image.colors.iter().any(|c| c.is_none()) {
        return None;
    }
    let mut im = Image::create(image.width, image.height)?;
    let (mut red, mut green, mut blue) = (0, 0, 0);
    let mut colors = Vec::with_capacity(image.colors.len());
    for c in &image.colors {
        let c = c.as_ref().unwrap();
        // strlen(): a NUL cannot be in a parsed word
        let ch = |i: usize| c.get(i).copied().unwrap_or(0);
        let part = |a: usize, n: usize| -> Vec<u8> { (a..a + n).map(ch).collect() };
        match c.len() {
            4 => {
                red = strtol16(&part(1, 1));
                green = strtol16(&part(2, 1));
                blue = strtol16(&part(3, 1));
            }
            7 => {
                red = strtol16(&part(1, 2));
                green = strtol16(&part(3, 2));
                blue = strtol16(&part(5, 2));
            }
            10 => {
                red = strtol16(&part(1, 3)) / 64;
                green = strtol16(&part(4, 3)) / 64;
                blue = strtol16(&part(7, 3)) / 64;
            }
            13 => {
                red = strtol16(&part(1, 4)) / 256;
                green = strtol16(&part(5, 4)) / 256;
                blue = strtol16(&part(9, 4)) / 256;
            }
            _ => {}
        }
        colors.push(im.color_resolve(red, green, blue));
    }
    let mut k = 0;
    for i in 0..image.height {
        for j in 0..image.width {
            im.set_pixel(j, i, colors[image.data[k]]);
            k += 1;
        }
    }
    Some(im)
}
