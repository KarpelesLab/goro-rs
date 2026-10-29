//! GIF reading and writing (GIF89a specification, CompuServe 1990):
//! the first image of the file as a palette image, the Graphic Control
//! Extension's transparent index, interlacing; LZW coding.

use crate::image::{Image, tc_a, tc_b, tc_g, tc_r};

/// Reads the first image of a GIF file. `None`: not a valid GIF.
pub fn decode(data: &[u8]) -> Option<Image> {
    if data.len() < 13 || !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return None;
    }
    let sw = u16::from_le_bytes([data[6], data[7]]) as i32;
    let sh = u16::from_le_bytes([data[8], data[9]]) as i32;
    let flags = data[10];
    let mut pos = 13;
    let mut global: Vec<[u8; 3]> = Vec::new();
    if flags & 0x80 != 0 {
        let n = 2usize << (flags & 7);
        if pos + 3 * n > data.len() {
            return None;
        }
        global = data[pos..pos + 3 * n]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        pos += 3 * n;
    }
    let mut transparent: i32 = -1;
    loop {
        let b = *data.get(pos)?;
        pos += 1;
        match b {
            b'!' => {
                let label = *data.get(pos)?;
                pos += 1;
                // Sub-blocks.
                let mut first = true;
                loop {
                    let n = *data.get(pos)? as usize;
                    pos += 1;
                    if n == 0 {
                        break;
                    }
                    let blk = data.get(pos..pos + n)?;
                    if label == 0xf9 && first && n >= 4 && blk[0] & 1 != 0 {
                        transparent = blk[3] as i32;
                    }
                    first = false;
                    pos += n;
                }
            }
            b',' => {
                let d = data.get(pos..pos + 9)?;
                pos += 9;
                let left = u16::from_le_bytes([d[0], d[1]]) as i32;
                let top = u16::from_le_bytes([d[2], d[3]]) as i32;
                let w = u16::from_le_bytes([d[4], d[5]]) as i32;
                let h = u16::from_le_bytes([d[6], d[7]]) as i32;
                let iflags = d[8];
                let mut local: Vec<[u8; 3]> = Vec::new();
                if iflags & 0x80 != 0 {
                    let n = 2usize << (iflags & 7);
                    let t = data.get(pos..pos + 3 * n)?;
                    local = t
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|c| [c[0], c[1], c[2]])
                        .collect();
                    pos += 3 * n;
                }
                let pal = if local.is_empty() { &global } else { &local };
                let interlaced = iflags & 0x40 != 0;
                if w == 0 || h == 0 {
                    return None;
                }
                if left + w > sw || top + h > sh {
                    return None;
                }
                let min_code = *data.get(pos)?;
                pos += 1;
                if !(1..=11).contains(&min_code) {
                    return None;
                }
                // Gather the sub-blocks.
                let mut lzw = Vec::new();
                while let Some(&n) = data.get(pos) {
                    pos += 1;
                    if n == 0 {
                        break;
                    }
                    let end = (pos + n as usize).min(data.len());
                    lzw.extend_from_slice(&data[pos..end]);
                    pos = end;
                }
                let npix = w as usize * h as usize;
                let indices = lzw_decode(&lzw, min_code, npix)?;
                let mut im = Image::new_palette(w, h)?;
                im.interlace = interlaced;
                for (i, c) in pal.iter().enumerate() {
                    im.red[i] = c[0] as i32;
                    im.green[i] = c[1] as i32;
                    im.blue[i] = c[2] as i32;
                    im.open[i] = false;
                }
                im.colors_total = pal.len() as i32;
                // Rows in interlaced order.
                let rows: Vec<usize> = if interlaced {
                    let hh = h as usize;
                    (0..hh)
                        .step_by(8)
                        .chain((4..hh).step_by(8))
                        .chain((2..hh).step_by(4))
                        .chain((1..hh).step_by(2))
                        .collect()
                } else {
                    (0..h as usize).collect()
                };
                let mut max_index = 0u8;
                for (ri, &y) in rows.iter().enumerate() {
                    let src = &indices[ri * w as usize..(ri + 1) * w as usize];
                    let dst = &mut im.pixels[y * w as usize..(y + 1) * w as usize];
                    dst.copy_from_slice(src);
                    if let Some(&m) = src.iter().max() {
                        max_index = max_index.max(m);
                    }
                }
                // Colors past the palette read as black entries.
                if (max_index as i32) >= im.colors_total {
                    for i in im.colors_total as usize..=max_index as usize {
                        im.open[i] = false;
                    }
                    im.colors_total = max_index as i32 + 1;
                }
                if (0..256).contains(&transparent) {
                    im.transparent = transparent;
                }
                let _ = top;
                return Some(im);
            }
            b';' => return None,
            _ => return None,
        }
    }
}

/// Decodes LZW data to exactly `npix` indices (missing pixels: 0).
/// `None` on a corrupt code stream.
fn lzw_decode(data: &[u8], min_code: u8, npix: usize) -> Option<Vec<u8>> {
    let clear = 1u16 << min_code;
    let eoi = clear + 1;
    let mut prefix = vec![0u16; 4096];
    let mut suffix = vec![0u8; 4096];
    let mut first_of = vec![0u8; 4096];
    let mut lens = vec![0u16; 4096];
    for i in 0..clear {
        suffix[i as usize] = i as u8;
        first_of[i as usize] = i as u8;
        lens[i as usize] = 1;
    }
    let mut out = Vec::with_capacity(npix);
    let mut code_size = min_code as u32 + 1;
    let mut next = eoi + 1;
    let mut prev: Option<u16> = None;
    let mut bitbuf: u32 = 0;
    let mut nbits = 0u32;
    let mut i = 0;
    let mut stack = Vec::with_capacity(4096);
    'outer: while out.len() < npix {
        while nbits < code_size {
            if i >= data.len() {
                break 'outer;
            }
            bitbuf |= (data[i] as u32) << nbits;
            nbits += 8;
            i += 1;
        }
        let code = (bitbuf & ((1 << code_size) - 1)) as u16;
        bitbuf >>= code_size;
        nbits -= code_size;
        if code == clear {
            code_size = min_code as u32 + 1;
            next = eoi + 1;
            prev = None;
            continue;
        }
        if code == eoi {
            break;
        }
        let Some(p) = prev else {
            if code >= clear {
                return None;
            }
            out.push(code as u8);
            prev = Some(code);
            continue;
        };
        let cur = if code < next {
            code
        } else if code == next {
            // KwKwK case.
            if next < 4096 {
                prefix[next as usize] = p;
                suffix[next as usize] = first_of[p as usize];
                first_of[next as usize] = first_of[p as usize];
                lens[next as usize] = lens[p as usize] + 1;
            }
            code
        } else {
            return None;
        };
        stack.clear();
        let mut c = cur;
        loop {
            stack.push(suffix[c as usize]);
            if c < clear {
                break;
            }
            c = prefix[c as usize];
        }
        for &b in stack.iter().rev() {
            if out.len() < npix {
                out.push(b);
            }
        }
        if code < next && next < 4096 {
            prefix[next as usize] = p;
            suffix[next as usize] = first_of[cur as usize];
            first_of[next as usize] = first_of[p as usize];
            lens[next as usize] = lens[p as usize] + 1;
        }
        if next < 4096 {
            next += 1;
            if next == (1 << code_size) && code_size < 12 {
                code_size += 1;
            }
        }
        prev = Some(code);
    }
    out.resize(npix, 0);
    Some(out)
}

/// Writes a palette image as GIF (GIF89a when it has a transparent
/// color, GIF87a otherwise).
pub fn encode(im: &Image) -> Vec<u8> {
    let w = im.sx as usize;
    let h = im.sy as usize;
    let ncolors = im.colors_total.max(1) as usize;
    let mut bits = 1;
    while (1usize << bits) < ncolors {
        bits += 1;
    }
    let mut out = Vec::new();
    let has_t = im.transparent >= 0;
    out.extend_from_slice(if has_t { b"GIF89a" } else { b"GIF87a" });
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    out.push(0x80 | ((bits as u8 - 1) << 4) | (bits as u8 - 1));
    out.push(0);
    out.push(0);
    for i in 0..(1usize << bits) {
        if i < ncolors {
            out.extend_from_slice(&[im.red[i] as u8, im.green[i] as u8, im.blue[i] as u8]);
        } else {
            out.extend_from_slice(&[0, 0, 0]);
        }
    }
    if has_t {
        out.extend_from_slice(&[0x21, 0xf9, 4, 1, 0, 0, im.transparent as u8, 0]);
    }
    out.push(b',');
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(w as u16).to_le_bytes());
    out.extend_from_slice(&(h as u16).to_le_bytes());
    out.push(if im.interlace { 0x40 } else { 0 });
    let min_code = bits.max(2) as u8;
    out.push(min_code);
    let rows: Vec<usize> = if im.interlace {
        (0..h)
            .step_by(8)
            .chain((4..h).step_by(8))
            .chain((2..h).step_by(4))
            .chain((1..h).step_by(2))
            .collect()
    } else {
        (0..h).collect()
    };
    let mut pix = Vec::with_capacity(w * h);
    for y in rows {
        pix.extend_from_slice(&im.pixels[y * w..(y + 1) * w]);
    }
    let lzw = lzw_encode(&pix, min_code);
    for c in lzw.chunks(255) {
        out.push(c.len() as u8);
        out.extend_from_slice(c);
    }
    out.push(0);
    out.push(b';');
    out
}

fn lzw_encode(pix: &[u8], min_code: u8) -> Vec<u8> {
    let clear = 1u32 << min_code;
    let eoi = clear + 1;
    let mut out = Vec::new();
    let mut bitbuf: u64 = 0;
    let mut nbits = 0u32;
    let mut code_size = min_code as u32 + 1;
    let mut emit = |code: u32, size: u32, out: &mut Vec<u8>| {
        bitbuf |= (code as u64) << nbits;
        nbits += size;
        while nbits >= 8 {
            out.push(bitbuf as u8);
            bitbuf >>= 8;
            nbits -= 8;
        }
    };
    let mut dict: std::collections::HashMap<(u32, u8), u32> = std::collections::HashMap::new();
    let mut next = eoi + 1;
    emit(clear, code_size, &mut out);
    let mut cur: Option<u32> = None;
    for &p in pix {
        match cur {
            None => cur = Some(p as u32),
            Some(c) => {
                if let Some(&n) = dict.get(&(c, p)) {
                    cur = Some(n);
                } else {
                    emit(c, code_size, &mut out);
                    if next < 4096 {
                        dict.insert((c, p), next);
                        next += 1;
                        if next > (1 << code_size) && code_size < 12 {
                            code_size += 1;
                        }
                    } else {
                        emit(clear, code_size, &mut out);
                        dict.clear();
                        next = eoi + 1;
                        code_size = min_code as u32 + 1;
                    }
                    cur = Some(p as u32);
                }
            }
        }
    }
    if let Some(c) = cur {
        emit(c, code_size, &mut out);
    }
    emit(eoi, code_size, &mut out);
    if nbits > 0 {
        out.push(bitbuf as u8);
    }
    out
}

/// A palette copy of a truecolor image for GIF output.
pub fn to_palette(im: &Image) -> Image {
    let mut p = im.clone();
    if p.truecolor {
        let t = im.transparent;
        crate::quant::truecolor_to_palette(&mut p, true, 256);
        if t >= 0 {
            let i = p.color_closest(tc_r(t), tc_g(t), tc_b(t), tc_a(t));
            p.transparent = i;
        }
    }
    p
}
