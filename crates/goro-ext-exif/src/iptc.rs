//! `iptcparse()` and `iptcembed()` (PHP manual: iptcparse, iptcembed).
//!
//! IPTC-NAA Information Interchange Model datasets: tag marker 0x1C,
//! record number, dataset number, then a 16-bit length, or 0x80 0x04
//! and a 32-bit length (IIM 4.1 section 1.5). iptcembed() stores the
//! block in a JPEG APP13 "Photoshop 3.0" segment as an 8BIM resource
//! 0x0404 (Adobe Photoshop file format specification, image resource
//! blocks); the layout of what it writes was observed on PHP 8.5 (test
//! ext/standard/tests/image/gh20582.phpt).

/// iptcparse(): `("rec#ddd", values)` in first-seen order, or None.
pub fn parse(d: &[u8]) -> Option<Vec<(String, Vec<Vec<u8>>)>> {
    let mut i = d.iter().position(|&c| c == 0x1C)?;
    let mut out: Vec<(String, Vec<Vec<u8>>)> = Vec::new();
    while i < d.len() {
        if d[i] != 0x1C {
            break;
        }
        if i + 4 >= d.len() {
            break;
        }
        let rec = d[i + 1];
        let set = d[i + 2];
        i += 3;
        let len;
        if d[i] & 0x80 != 0 {
            if i + 6 >= d.len() {
                break;
            }
            len = u32::from_be_bytes([d[i + 2], d[i + 3], d[i + 4], d[i + 5]]) as usize;
            i += 6;
        } else {
            len = u16::from_be_bytes([d[i], d[i + 1]]) as usize;
            i += 2;
        }
        if len > d.len() - i {
            return None;
        }
        let key = format!("{}#{:03}", rec, set);
        let val = d[i..i + len].to_vec();
        match out.iter_mut().find(|e| e.0 == key) {
            Some(e) => e.1.push(val),
            None => out.push((key, vec![val])),
        }
        i += len;
    }
    if out.is_empty() { None } else { Some(out) }
}

/// The APP13 segment holding `iptc`.
fn app13(iptc: &[u8]) -> Vec<u8> {
    let mut data = iptc.to_vec();
    if data.len() % 2 == 1 {
        data.push(0);
    }
    let mut seg = Vec::with_capacity(data.len() + 30);
    let body_len = 2 + 14 + 4 + 2 + 2 + 4 + data.len();
    seg.extend_from_slice(&[0xFF, 0xED]);
    seg.extend_from_slice(&(body_len as u16).to_be_bytes());
    seg.extend_from_slice(b"Photoshop 3.0\0");
    seg.extend_from_slice(b"8BIM");
    seg.extend_from_slice(&[0x04, 0x04, 0, 0]);
    seg.extend_from_slice(&(data.len() as u32).to_be_bytes());
    seg.extend_from_slice(&data);
    seg
}

/// iptcembed(): `jpeg` with its APP13 segments replaced by one holding
/// `iptc`, placed after SOI and a leading APP0. None when `jpeg` is not
/// a JPEG file.
pub fn embed(iptc: &[u8], jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut out = Vec::with_capacity(jpeg.len() + iptc.len() + 32);
    out.extend_from_slice(&jpeg[..2]);
    let mut pos = 2;
    let mut done = false;
    loop {
        // The next marker.
        let mut p = pos;
        while p < jpeg.len() && jpeg[p] == 0xFF {
            p += 1;
        }
        if p >= jpeg.len() || p == pos {
            // Not a marker: copy the rest.
            if !done {
                out.extend_from_slice(&app13(iptc));
            }
            out.extend_from_slice(&jpeg[pos..]);
            return Some(out);
        }
        let m = jpeg[p];
        let seg_start = pos;
        let body = p + 1;
        if m == 0xD9 || m == 0xDA || (0xD0..=0xD7).contains(&m) || m == 0x01 {
            if !done {
                out.extend_from_slice(&app13(iptc));
            }
            out.extend_from_slice(&jpeg[seg_start..]);
            return Some(out);
        }
        if body + 2 > jpeg.len() {
            if !done {
                out.extend_from_slice(&app13(iptc));
            }
            out.extend_from_slice(&jpeg[seg_start..]);
            return Some(out);
        }
        let len = u16::from_be_bytes([jpeg[body], jpeg[body + 1]]) as usize;
        let end = (body + len).min(jpeg.len());
        match m {
            0xE0 if !done && seg_start == 2 => {
                out.extend_from_slice(&jpeg[seg_start..end]);
                out.extend_from_slice(&app13(iptc));
                done = true;
            }
            0xED => {}
            _ => {
                if !done {
                    out.extend_from_slice(&app13(iptc));
                    done = true;
                }
                out.extend_from_slice(&jpeg[seg_start..end]);
            }
        }
        pos = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let d = b"\x1C\x02\x78\x00\x04Test\x1C\x02\x78\x00\x01X";
        let r = parse(d).unwrap();
        assert_eq!(r[0].0, "2#120");
        assert_eq!(r[0].1, vec![b"Test".to_vec(), b"X".to_vec()]);
        assert!(parse(b"\x1C\x02_\x80___").is_none());
    }

    #[test]
    fn embed_twice() {
        let j = b"\xFF\xD8\xFF\xE0\x00\x04ab\xFF\xDA\x00\x02xyz\xFF\xD9".to_vec();
        let a = embed(b"\x1C\x02\x78\x00\x01A", &j).unwrap();
        let b = embed(b"\x1C\x02\x78\x00\x01A", &a).unwrap();
        assert_eq!(a, b);
    }
}
