//! gd_gd2.c: the chunked GD2 image format (raw or zlib-compressed).

use crate::gd_gd::{get_colors, put_colors};
use crate::io::{EOF, In, Out};
use crate::*;

const GD2_FMT_TRUECOLOR_RAW: i32 = 3;
const GD2_FMT_TRUECOLOR_COMPRESSED: i32 = 4;

fn gd2_compressed(fmt: i32) -> bool {
    fmt == GD2_FMT_COMPRESSED || fmt == GD2_FMT_TRUECOLOR_COMPRESSED
}

fn gd2_truecolor(fmt: i32) -> bool {
    fmt == GD2_FMT_TRUECOLOR_RAW || fmt == GD2_FMT_TRUECOLOR_COMPRESSED
}

#[derive(Clone, Copy, Default)]
struct ChunkInfo {
    offset: i32,
    size: i32,
}

struct Header {
    sx: i32,
    sy: i32,
    cs: i32,
    vers: i32,
    fmt: i32,
    ncx: i32,
    ncy: i32,
    cidx: Vec<ChunkInfo>,
}

/// zlib's uncompress(): Some(length) on Z_OK
pub(crate) fn uncompress(dest: &mut [u8], src: &[u8]) -> Option<usize> {
    let mut inf = goro_zlib::Inflate::new(goro_zlib::MAX_WBITS).ok()?;
    let mut io = goro_zlib::Io::new(src, dest);
    let r = inf.inflate(&mut io, goro_zlib::Z_FINISH);
    if r == goro_zlib::Z_STREAM_END {
        Some(io.out_pos)
    } else {
        None
    }
}

/// zlib's compress() into a buffer of `max` bytes
pub(crate) fn compress(src: &[u8], max: usize) -> Option<Vec<u8>> {
    let (st, out) = goro_zlib::deflate_once(
        src,
        goro_zlib::Z_DEFAULT_COMPRESSION,
        goro_zlib::MAX_WBITS,
        goro_zlib::DEF_MEM_LEVEL,
        goro_zlib::Z_DEFAULT_STRATEGY,
        max,
    );
    if st == goro_zlib::Z_STREAM_END {
        Some(out)
    } else {
        None
    }
}

/// _gd2GetHeader()
fn get_header(inp: &mut In) -> Option<Header> {
    let mut id = [0u8; 4];
    for b in id.iter_mut() {
        let ch = inp.get_c();
        if ch == EOF {
            return None;
        }
        *b = ch as u8;
    }
    // strcmp(): a NUL ends the id
    let idlen = id.iter().position(|&c| c == 0).unwrap_or(4);
    if &id[..idlen] != b"gd2" {
        return None;
    }
    let vers = inp.get_word()?;
    if vers != 1 && vers != 2 {
        return None;
    }
    let sx = inp.get_word()?;
    let sy = inp.get_word()?;
    let cs = inp.get_word()?;
    if !(GD2_CHUNKSIZE_MIN..=GD2_CHUNKSIZE_MAX).contains(&cs) {
        return None;
    }
    let fmt = inp.get_word()?;
    if fmt != GD2_FMT_RAW
        && fmt != GD2_FMT_COMPRESSED
        && fmt != GD2_FMT_TRUECOLOR_RAW
        && fmt != GD2_FMT_TRUECOLOR_COMPRESSED
    {
        return None;
    }
    let ncx = inp.get_word()?;
    let ncy = inp.get_word()?;
    let mut cidx = Vec::new();
    if gd2_compressed(fmt) {
        if overflow2(ncx, ncy) {
            return None;
        }
        let nc = ncx * ncy;
        if overflow2(8, nc) {
            return None;
        }
        let sidx = 8 * nc;
        if sidx <= 0 {
            return None;
        }
        for _ in 0..nc {
            let offset = inp.get_int()?;
            let size = inp.get_int()?;
            if offset < 0 || size < 0 {
                return None;
            }
            cidx.push(ChunkInfo { offset, size });
        }
    }
    Some(Header {
        sx,
        sy,
        cs,
        vers,
        fmt,
        ncx,
        ncy,
        cidx,
    })
}

/// _gd2ReadChunk()
fn read_chunk(
    offset: i32,
    comp_buf: &mut [u8],
    comp_size: i32,
    chunk_buf: &mut [u8],
    inp: &mut In,
) -> bool {
    if inp.tell() != offset as i64 {
        inp.seek(offset);
    }
    let cs = comp_size as usize;
    if inp.get_buf(&mut comp_buf[..cs]) != comp_size {
        return false;
    }
    uncompress(chunk_buf, &comp_buf[..cs]).is_some()
}

/// gdImageCreateFromGd2Ctx()
pub fn create_from_gd2(data: &[u8]) -> Option<Image> {
    let mut inp = In::new(data);
    let h = get_header(&mut inp)?;
    let mut im = if gd2_truecolor(h.fmt) {
        Image::create_true_color(h.sx, h.sy)?
    } else {
        Image::create(h.sx, h.sy)?
    };
    if !get_colors(&mut inp, &mut im, h.vers == 2) {
        return None;
    }
    let bpp = if im.true_color { 4 } else { 1 };
    let (ncx, ncy, cs) = (h.ncx, h.ncy, h.cs);
    let mut chunk_buf = Vec::new();
    let mut comp_buf = Vec::new();
    if gd2_compressed(h.fmt) {
        let comp_max = h.cidx.iter().map(|c| c.size).max().unwrap_or(0).max(0) + 1;
        let chunk_max = cs * bpp * cs;
        if chunk_max <= 0 {
            return None;
        }
        chunk_buf = vec![0u8; chunk_max as usize];
        comp_buf = vec![0u8; comp_max as usize];
    }
    let mut chunk_num = 0usize;
    for cy in 0..ncy {
        for cx in 0..ncx {
            let ylo = cy * cs;
            let yhi = (ylo + cs).min(im.sy);
            let mut chunk_pos = 0usize;
            if gd2_compressed(h.fmt) {
                let ci = h.cidx[chunk_num];
                if !read_chunk(ci.offset, &mut comp_buf, ci.size, &mut chunk_buf, &mut inp) {
                    return None;
                }
            }
            for y in ylo..yhi {
                let xlo = cx * cs;
                let xhi = (xlo + cs).min(im.sx);
                if !gd2_compressed(h.fmt) {
                    for x in xlo..xhi {
                        if im.true_color {
                            let Some(v) = inp.get_int() else {
                                gd_error("gd2: EOF while reading\n");
                                return None;
                            };
                            im.tpixels[y as usize][x as usize] = v;
                        } else {
                            let Some(ch) = inp.get_byte() else {
                                gd_error("gd2: EOF while reading\n");
                                return None;
                            };
                            im.pixels[y as usize][x as usize] = ch as u8;
                        }
                    }
                } else {
                    for x in xlo..xhi {
                        if im.true_color {
                            let b = &chunk_buf[chunk_pos..chunk_pos + 4];
                            chunk_pos += 4;
                            im.tpixels[y as usize][x as usize] = ((b[0] as i32) << 24)
                                .wrapping_add((b[1] as i32) << 16)
                                .wrapping_add((b[2] as i32) << 8)
                                .wrapping_add(b[3] as i32);
                        } else {
                            im.pixels[y as usize][x as usize] = chunk_buf[chunk_pos];
                            chunk_pos += 1;
                        }
                    }
                }
            }
            chunk_num += 1;
        }
    }
    Some(im)
}

/// gdImageCreateFromGd2PartCtx()
pub fn create_from_gd2_part(data: &[u8], srcx: i32, srcy: i32, w: i32, h: i32) -> Option<Image> {
    if w < 1 || h < 1 {
        return None;
    }
    let mut inp = In::new(data);
    let hd = get_header(&mut inp)?;
    let (fsx, fsy, cs, ncx, ncy, fmt) = (hd.sx, hd.sy, hd.cs, hd.ncx, hd.ncy, hd.fmt);
    let mut im = if gd2_truecolor(fmt) {
        Image::create_true_color(w, h)?
    } else {
        Image::create(w, h)?
    };
    if !get_colors(&mut inp, &mut im, hd.vers == 2) {
        return None;
    }
    let mut chunk_buf = Vec::new();
    let mut comp_buf = Vec::new();
    if gd2_compressed(fmt) {
        let comp_max = hd.cidx.iter().map(|c| c.size).max().unwrap_or(0).max(0) + 1;
        let chunk_max = if im.true_color { cs * cs * 4 } else { cs * cs };
        if chunk_max <= 0 {
            return None;
        }
        chunk_buf = vec![0u8; chunk_max as usize];
        comp_buf = vec![0u8; comp_max as usize];
    }
    let scx = (srcx / cs).max(0);
    let scy = (srcy / cs).max(0);
    let mut ecx = srcx.wrapping_add(w) / cs;
    let mut ecy = srcy.wrapping_add(h) / cs;
    if ecx >= ncx {
        ecx = ncx - 1;
    }
    if ecy >= ncy {
        ecy = ncy - 1;
    }
    let dstart = inp.tell() as i32;
    for cy in scy..=ecy {
        let ylo = cy * cs;
        let yhi = (ylo + cs).min(fsy);
        for cx in scx..=ecx {
            let xlo = cx * cs;
            let xhi = (xlo + cs).min(fsx);
            let mut chunk_pos = 0usize;
            if !gd2_compressed(fmt) {
                let dpos = if im.true_color {
                    (cy.wrapping_mul(cs.wrapping_mul(fsx)).wrapping_mul(4))
                        .wrapping_add(cx.wrapping_mul(cs).wrapping_mul(yhi - ylo).wrapping_mul(4))
                        .wrapping_add(dstart)
                } else {
                    cy.wrapping_mul(cs.wrapping_mul(fsx))
                        .wrapping_add(cx.wrapping_mul(cs).wrapping_mul(yhi - ylo))
                        .wrapping_add(dstart)
                };
                if !inp.seek(dpos) {
                    gd_error_ex(GD_WARNING, "Error from seek: 0");
                    return None;
                }
            } else {
                let chunk_num = (cx + cy * ncx) as usize;
                let Some(ci) = hd.cidx.get(chunk_num).copied() else {
                    gd_error("Error reading comproessed chunk");
                    return None;
                };
                if !read_chunk(ci.offset, &mut comp_buf, ci.size, &mut chunk_buf, &mut inp) {
                    gd_error("Error reading comproessed chunk");
                    return None;
                }
            }
            for y in ylo..yhi {
                for x in xlo..xhi {
                    let ch: u32;
                    if !gd2_compressed(fmt) {
                        if im.true_color {
                            ch = inp.get_int().map_or(0, |v| v as u32);
                        } else {
                            let c = inp.get_c();
                            ch = if c == EOF { 0 } else { c as u32 };
                        }
                    } else if im.true_color {
                        let b = &chunk_buf[chunk_pos..chunk_pos + 4];
                        chunk_pos += 4;
                        ch = ((b[0] as u32) << 24)
                            | ((b[1] as u32) << 16)
                            | ((b[2] as u32) << 8)
                            | b[3] as u32;
                    } else {
                        ch = chunk_buf[chunk_pos] as u32;
                        chunk_pos += 1;
                    }
                    if x >= srcx
                        && x < srcx.wrapping_add(w)
                        && x < fsx
                        && x >= 0
                        && y >= srcy
                        && y < srcy.wrapping_add(h)
                        && y < fsy
                        && y >= 0
                    {
                        let (dy, dx) = ((y - srcy) as usize, (x - srcx) as usize);
                        if im.true_color {
                            im.tpixels[dy][dx] = ch as i32;
                        } else {
                            im.pixels[dy][dx] = ch as u8;
                        }
                    }
                }
            }
        }
    }
    Some(im)
}

/// gdImageGd2Ptr() / _gdImageGd2()
pub fn image_gd2(im: &Image, cs: i32, fmt: i32) -> Option<Vec<u8>> {
    let mut out = Out::new();
    let bpp = if im.true_color { 4 } else { 1 };
    let mut fmt = fmt;
    let mut cs = cs;
    if fmt != GD2_FMT_RAW && fmt != GD2_FMT_COMPRESSED {
        fmt = GD2_FMT_COMPRESSED;
    }
    if im.true_color {
        fmt += 2;
    }
    if cs == 0 {
        cs = GD2_CHUNKSIZE;
    } else {
        cs = cs.clamp(GD2_CHUNKSIZE_MIN, GD2_CHUNKSIZE_MAX);
    }
    let ncx = (im.sx + cs - 1) / cs;
    let ncy = (im.sy + cs - 1) / cs;
    // _gd2PutHeader()
    out.put_buf(b"gd2\0");
    out.put_word(GD2_VERS);
    out.put_word(im.sx);
    out.put_word(im.sy);
    out.put_word(cs);
    out.put_word(fmt);
    out.put_word(ncx);
    out.put_word(ncy);
    let mut comp_max = 0usize;
    let mut idx_pos = 0i32;
    let mut chunk_idx: Vec<ChunkInfo> = Vec::new();
    let mut chunk_data: Vec<u8> = Vec::new();
    if gd2_compressed(fmt) {
        let cm = ((cs * bpp * cs) as f32 * 1.02f32) as i32 + 12;
        chunk_data = vec![0u8; (cs * bpp * cs) as usize];
        if cm <= 0 {
            return Some(out.into_vec());
        }
        comp_max = cm as usize;
        idx_pos = out.tell() as i32;
        let idx_size = ncx * ncy * 8;
        out.seek(idx_pos + idx_size);
    }
    put_colors(im, &mut out);
    for cy in 0..ncy {
        for cx in 0..ncx {
            let ylo = cy * cs;
            let yhi = (ylo + cs).min(im.sy);
            let mut chunk_len = 0usize;
            for y in ylo..yhi {
                let xlo = cx * cs;
                let xhi = (xlo + cs).min(im.sx);
                if gd2_compressed(fmt) {
                    for x in xlo..xhi {
                        if im.true_color {
                            let p = im.tpixels[y as usize][x as usize];
                            chunk_data[chunk_len] = true_color_get_alpha(p) as u8;
                            chunk_data[chunk_len + 1] = true_color_get_red(p) as u8;
                            chunk_data[chunk_len + 2] = true_color_get_green(p) as u8;
                            chunk_data[chunk_len + 3] = true_color_get_blue(p) as u8;
                            chunk_len += 4;
                        } else {
                            chunk_data[chunk_len] = im.pixels[y as usize][x as usize];
                            chunk_len += 1;
                        }
                    }
                } else {
                    for x in xlo..xhi {
                        if im.true_color {
                            out.put_int(im.tpixels[y as usize][x as usize]);
                        } else {
                            out.put_c(im.pixels[y as usize][x as usize] as i32);
                        }
                    }
                }
            }
            if gd2_compressed(fmt) {
                match compress(&chunk_data[..chunk_len], comp_max) {
                    None => gd_error("Error from compressing"),
                    Some(c) => {
                        chunk_idx.push(ChunkInfo {
                            offset: out.tell() as i32,
                            size: c.len() as i32,
                        });
                        out.put_buf(&c);
                    }
                }
            }
        }
    }
    if gd2_compressed(fmt) {
        let pos_save = out.tell() as i32;
        out.seek(idx_pos);
        for c in &chunk_idx {
            out.put_int(c.offset);
            out.put_int(c.size);
        }
        out.seek(pos_save);
    }
    Some(out.into_vec())
}
