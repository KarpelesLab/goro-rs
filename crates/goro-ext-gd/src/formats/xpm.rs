//! XPM 3 (X PixMap C source, "XPM Manual", Arnaud Le Hors 1996): a
//! values line "width height ncolors cpp", ncolors color lines and the
//! pixel rows, each a C string. Images of up to 256 colors are read as
//! palette images, larger ones as truecolor; "None" is transparent.

use super::{DecodeError, DecodeResult, OVERFLOW_MSG};
use crate::image::{ALPHA_TRANSPARENT, Image, tc};
use std::collections::HashMap;

fn strings(d: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_comment = false;
    while i < d.len() {
        if in_comment {
            if d[i] == b'*' && d.get(i + 1) == Some(&b'/') {
                in_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if d[i] == b'/' && d.get(i + 1) == Some(&b'*') {
            in_comment = true;
            i += 2;
            continue;
        }
        if d[i] == b'"' {
            let mut s = Vec::new();
            i += 1;
            while i < d.len() && d[i] != b'"' {
                if d[i] == b'\\' && i + 1 < d.len() {
                    i += 1;
                }
                s.push(d[i]);
                i += 1;
            }
            out.push(s);
        }
        i += 1;
    }
    out
}

fn named(n: &str) -> Option<(i32, i32, i32)> {
    Some(match n.to_ascii_lowercase().as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "gray" | "grey" => (190, 190, 190),
        _ => return None,
    })
}

fn parse_color(spec: &str) -> Option<Option<(i32, i32, i32)>> {
    let s = spec.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    if let Some(h) = s.strip_prefix('#') {
        let v = |a: &str| i32::from_str_radix(a, 16).ok();
        return match h.len() {
            3 => Some(Some((v(&h[0..1])? * 17, v(&h[1..2])? * 17, v(&h[2..3])? * 17))),
            6 => Some(Some((v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?))),
            12 => Some(Some((v(&h[0..2])?, v(&h[4..6])?, v(&h[8..10])?))),
            _ => None,
        };
    }
    named(s).map(Some)
}

pub fn decode(d: &[u8]) -> DecodeResult {
    let s = strings(d);
    let hdr = s.first().ok_or_else(DecodeError::silent)?;
    let hdr = String::from_utf8_lossy(hdr);
    let v: Vec<i64> = hdr.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if v.len() < 4 {
        return Err(DecodeError::silent());
    }
    let (w, h, nc, cpp) = (v[0], v[1], v[2] as usize, v[3] as usize);
    if w <= 0 || h <= 0 || cpp == 0 || nc == 0 || s.len() < 1 + nc + h as usize {
        return Err(DecodeError::silent());
    }
    if w * h > i32::MAX as i64 {
        return Err(DecodeError::with(OVERFLOW_MSG));
    }
    let mut map: HashMap<Vec<u8>, Option<(i32, i32, i32)>> = HashMap::new();
    for line in &s[1..1 + nc] {
        if line.len() < cpp {
            return Err(DecodeError::silent());
        }
        let key = line[..cpp].to_vec();
        let rest = String::from_utf8_lossy(&line[cpp..]).to_string();
        let toks: Vec<&str> = rest.split_whitespace().collect();
        // Prefer the "c" (color) visual, then "g", "g4", "m".
        let mut col = None;
        for vis in ["c", "g", "g4", "m"] {
            if let Some(p) = toks.iter().position(|t| *t == vis) {
                let spec: Vec<&str> = toks[p + 1..]
                    .iter()
                    .take_while(|t| !["c", "g", "g4", "m", "s"].contains(t))
                    .copied()
                    .collect();
                col = parse_color(&spec.join(" "));
                if col.is_some() {
                    break;
                }
            }
        }
        map.insert(key, col.unwrap_or(Some((0, 0, 0))));
    }
    let truecolor = nc > 256;
    let mut im = if truecolor {
        Image::new_truecolor(w as i32, h as i32)
    } else {
        Image::new_palette(w as i32, h as i32)
    }
    .ok_or_else(|| DecodeError::with(OVERFLOW_MSG))?;
    let mut idx: HashMap<Vec<u8>, i32> = HashMap::new();
    if !truecolor {
        for line in &s[1..1 + nc] {
            let key = line[..cpp].to_vec();
            let c = match map[&key] {
                Some((r, g, b)) => im.color_allocate(r, g, b, 0),
                None => {
                    let i = im.color_allocate(0, 0, 0, ALPHA_TRANSPARENT);
                    im.transparent = i;
                    i
                }
            };
            idx.insert(key, c);
        }
    }
    for y in 0..h as usize {
        let row = &s[1 + nc + y];
        for x in 0..w as usize {
            let Some(k) = row.get(x * cpp..(x + 1) * cpp) else {
                break;
            };
            let c = if truecolor {
                match map.get(k).copied().flatten() {
                    Some((r, g, b)) => tc(r, g, b, 0),
                    None => tc(0, 0, 0, ALPHA_TRANSPARENT),
                }
            } else {
                *idx.get(k).unwrap_or(&0)
            };
            im.put_raw(x as i32, y as i32, c);
        }
    }
    Ok(im)
}
