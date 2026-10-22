//! gd_bmp.c: Windows / OS/2 bitmap reading (incl. RLE4/RLE8) and writing.

use crate::io::{In, Out};
use crate::*;

const BMP_PALETTE_3: i32 = 1;
const BMP_PALETTE_4: i32 = 2;
const BMP_WINDOWS_V3: i32 = 40;
const BMP_OS2_V1: i32 = 12;
const BMP_OS2_V2: i32 = 64;
const BMP_WINDOWS_V4: i32 = 108;
const BMP_WINDOWS_V5: i32 = 124;
const BMP_BI_RGB: i32 = 0;
const BMP_BI_RLE8: i32 = 1;
const BMP_BI_RLE4: i32 = 2;
const BMP_BI_BITFIELDS: i32 = 3;
const BMP_RLE_COMMAND: i32 = 0;
const BMP_RLE_ENDOFLINE: i32 = 0;
const BMP_RLE_ENDOFBITMAP: i32 = 1;
const BMP_RLE_DELTA: i32 = 2;
const BMP_RLE_TYPE_RAW: i32 = 0;
const BMP_RLE_TYPE_RLE: i32 = 1;

#[derive(Default)]
struct Hdr {
    magic: i16,
    off: i32,
}

#[derive(Default)]
struct Info {
    ty: i32,
    width: i32,
    height: i32,
    topdown: bool,
    numplanes: i16,
    depth: i16,
    enctype: i32,
    numcolors: i32,
    mincolors: i32,
}

fn put_word(out: &mut Out, w: i32) {
    out.put_c(w & 0xff);
    out.put_c((w >> 8) & 0xff);
}

fn put_int(out: &mut Out, w: i32) {
    out.put_c(w & 0xff);
    out.put_c((w >> 8) & 0xff);
    out.put_c((w >> 16) & 0xff);
    out.put_c((w >> 24) & 0xff);
}

/// gdImageBmpPtr() / _gdImageBmpCtx()
pub fn image_bmp(im: &Image, compression: i32) -> Option<Vec<u8>> {
    let mut out = Out::new();
    let mut compression = compression;
    if im.true_color {
        compression = 0;
    }
    let mut bitmap_size = ((im.sx * if im.true_color { 24 } else { 8 }) / 8) * im.sy;
    let mut info_size = BMP_WINDOWS_V3;
    if !im.true_color {
        info_size += im.colors_total * 4;
        if compression != 0 {
            bitmap_size = 0;
        }
    }
    let total_size = 14 + info_size + bitmap_size;
    out.put_buf(b"BM");
    put_int(&mut out, total_size);
    put_word(&mut out, 0);
    put_word(&mut out, 0);
    put_int(&mut out, 14 + info_size);
    put_int(&mut out, BMP_WINDOWS_V3);
    put_int(&mut out, im.sx);
    put_int(&mut out, im.sy);
    put_word(&mut out, 1);
    put_word(&mut out, if im.true_color { 24 } else { 8 });
    put_int(
        &mut out,
        if compression != 0 {
            BMP_BI_RLE8
        } else {
            BMP_BI_RGB
        },
    );
    put_int(&mut out, bitmap_size);
    put_int(&mut out, 0);
    put_int(&mut out, 0);
    put_int(&mut out, im.colors_total);
    put_int(&mut out, 0);
    let mut padding = ((if im.true_color { 3 } else { 1 }) * im.sx) % 4;
    if padding != 0 {
        padding = 4 - padding;
    }
    if !im.true_color {
        for i in 0..im.colors_total.max(0) {
            out.put_c(im.blue_of(i));
            out.put_c(im.green_of(i));
            out.put_c(im.red_of(i));
            out.put_c(0);
        }
        let mut row_buf: Vec<u8> = Vec::new();
        if compression != 0 {
            row_buf = vec![0u8; (im.sx as usize) * 2];
        }
        let mut row = im.sy - 1;
        while row >= 0 {
            if compression != 0 {
                for b in row_buf.iter_mut().take(im.sx as usize) {
                    *b = 0;
                }
            }
            for xpos in 0..im.sx {
                if compression != 0 {
                    row_buf[xpos as usize] = im.get_pixel(xpos, row) as u8;
                } else {
                    out.put_c(im.get_pixel(xpos, row));
                }
            }
            if compression == 0 {
                for _ in 0..padding {
                    out.put_c(0);
                }
            } else {
                let compressed_size = compress_row(&mut row_buf, im.sx as usize);
                bitmap_size += compressed_size as i32;
                out.put_buf(&row_buf[..compressed_size]);
                out.put_c(BMP_RLE_COMMAND);
                out.put_c(BMP_RLE_ENDOFLINE);
                bitmap_size += 2;
            }
            row -= 1;
        }
        if compression != 0 {
            out.put_c(BMP_RLE_COMMAND);
            out.put_c(BMP_RLE_ENDOFBITMAP);
            bitmap_size += 2;
            out.seek(2);
            put_int(&mut out, total_size + bitmap_size);
            out.seek(34);
            put_int(&mut out, bitmap_size);
        }
    } else {
        let mut row = im.sy - 1;
        while row >= 0 {
            for xpos in 0..im.sx {
                let pixel = im.get_pixel(xpos, row);
                out.put_c(true_color_get_blue(pixel));
                out.put_c(true_color_get_green(pixel));
                out.put_c(true_color_get_red(pixel));
            }
            for _ in 0..padding {
                out.put_c(0);
            }
            row -= 1;
        }
    }
    Some(out.into_vec())
}

/// compress_row(): RLE8-encodes `row[..length]` in place, returns the size
fn compress_row(row: &mut [u8], length: usize) -> usize {
    let src: Vec<u8> = row[..length].to_vec();
    let mut out_pos = 0usize;
    let mut rle_type = BMP_RLE_TYPE_RAW;
    let mut compressed_length = 0usize;
    let mut compressed_run = 0usize;
    let mut start = 0usize; // uncompressed_row
    let mut p = 0usize; // uncompressed_rowp
    let mut pixel: isize = 0;
    while pixel < length as isize {
        if compressed_run == 0 {
            start = p;
            compressed_run += 1;
            p += 1;
            rle_type = BMP_RLE_TYPE_RAW;
            pixel += 1;
            continue;
        }
        let cur = src.get(p).copied().unwrap_or(0);
        let prev = src[p - 1];
        if compressed_run == 1 && cur == prev {
            rle_type = BMP_RLE_TYPE_RLE;
        }
        if rle_type == BMP_RLE_TYPE_RLE {
            if compressed_run >= 128 || cur != prev {
                let n =
                    build_rle_packet(&mut row[out_pos..], rle_type, compressed_run, &src[start..]);
                out_pos += n;
                compressed_length += n;
                compressed_run = 0;
                pixel -= 1;
            } else {
                compressed_run += 1;
                p += 1;
            }
        } else if compressed_run >= 128 || cur == prev {
            let n = build_rle_packet(&mut row[out_pos..], rle_type, compressed_run, &src[start..]);
            out_pos += n;
            compressed_length += n;
            compressed_run = 0;
            pixel -= 1;
        } else {
            compressed_run += 1;
            p += 1;
        }
        pixel += 1;
    }
    if compressed_run != 0 {
        compressed_length +=
            build_rle_packet(&mut row[out_pos..], rle_type, compressed_run, &src[start..]);
    }
    compressed_length
}

fn build_rle_packet(row: &mut [u8], packet_type: i32, length: usize, data: &[u8]) -> usize {
    if !(1..=128).contains(&length) {
        return 0;
    }
    let mut size = 0;
    if packet_type == BMP_RLE_TYPE_RAW && length < 3 {
        for &d in data.iter().take(length) {
            row[size] = 1;
            row[size + 1] = d;
            size += 2;
        }
    } else if packet_type == BMP_RLE_TYPE_RLE {
        row[0] = length as u8;
        row[1] = data[0];
        size = 2;
    } else {
        row[0] = BMP_RLE_COMMAND as u8;
        row[1] = length as u8;
        row[2..2 + length].copy_from_slice(&data[..length]);
        size = 2 + length;
        if !length.is_multiple_of(2) {
            row[size] = 0;
            size += 1;
        }
    }
    size
}

/// gdImageCreateFromBmpCtx()
pub fn create_from_bmp(data: &[u8]) -> Option<Image> {
    let mut inp = In::new(data);
    let hdr = read_header(&mut inp)?;
    if hdr.magic != 0x4d42 {
        return None;
    }
    let mut info = read_info(&mut inp)?;
    let mut im = if info.depth >= 16 {
        Image::create_true_color(info.width, info.height)?
    } else {
        Image::create(info.width, info.height)?
    };
    let ok = match info.depth {
        1 => read_1bit(&mut im, &mut inp, &mut info, &hdr),
        4 => read_4bit(&mut im, &mut inp, &mut info, &hdr),
        8 => read_8bit(&mut im, &mut inp, &mut info, &hdr),
        16 | 24 | 32 => read_direct(&mut im, &mut inp, &info, &hdr),
        _ => false,
    };
    if ok { Some(im) } else { None }
}

fn read_header(inp: &mut In) -> Option<Hdr> {
    let magic = inp.get_word_lsb()?;
    let _size = inp.get_int_lsb()?;
    let _r1 = inp.get_word_lsb()?;
    let _r2 = inp.get_word_lsb()?;
    let off = inp.get_int_lsb()?;
    Some(Hdr { magic, off })
}

fn read_info(inp: &mut In) -> Option<Info> {
    let len = inp.get_int_lsb()?;
    let mut info = Info::default();
    match len {
        BMP_WINDOWS_V3 | BMP_WINDOWS_V4 | BMP_WINDOWS_V5 => read_windows_v3_info(inp, &mut info)?,
        BMP_OS2_V1 => read_os2_v1_info(inp, &mut info)?,
        BMP_OS2_V2 => read_os2_v2_info(inp, &mut info)?,
        _ => return None,
    }
    Some(info)
}

fn read_v3_fields(inp: &mut In, info: &mut Info) -> Option<()> {
    info.width = inp.get_int_lsb()?;
    info.height = inp.get_int_lsb()?;
    info.numplanes = inp.get_word_lsb()?;
    info.depth = inp.get_word_lsb()?;
    info.enctype = inp.get_int_lsb()?;
    let _size = inp.get_int_lsb()?;
    let _hres = inp.get_int_lsb()?;
    let _vres = inp.get_int_lsb()?;
    info.numcolors = inp.get_int_lsb()?;
    info.mincolors = inp.get_int_lsb()?;
    Some(())
}

fn finish_v3(info: &mut Info) -> Option<()> {
    if info.height < 0 {
        info.topdown = true;
        info.height = info.height.wrapping_neg();
    } else {
        info.topdown = false;
    }
    info.ty = BMP_PALETTE_4;
    if info.width <= 0
        || info.height <= 0
        || info.numplanes <= 0
        || info.depth <= 0
        || info.numcolors < 0
        || info.mincolors < 0
    {
        return None;
    }
    Some(())
}

fn read_windows_v3_info(inp: &mut In, info: &mut Info) -> Option<()> {
    read_v3_fields(inp, info)?;
    finish_v3(info)
}

fn read_os2_v1_info(inp: &mut In, info: &mut Info) -> Option<()> {
    // the shorts are read into the low half of zeroed ints
    info.width = inp.get_word_lsb()? as u16 as i32;
    info.height = inp.get_word_lsb()? as u16 as i32;
    info.numplanes = inp.get_word_lsb()?;
    info.depth = inp.get_word_lsb()?;
    info.topdown = false;
    info.numcolors = 1i32.wrapping_shl(info.depth as u32);
    info.ty = BMP_PALETTE_3;
    if info.width <= 0
        || info.height <= 0
        || info.numplanes <= 0
        || info.depth <= 0
        || info.numcolors < 0
    {
        return None;
    }
    Some(())
}

fn read_os2_v2_info(inp: &mut In, info: &mut Info) -> Option<()> {
    read_v3_fields(inp, info)?;
    let mut useless = [0u8; 24];
    if inp.get_buf(&mut useless) == 0 {
        return None;
    }
    finish_v3(info)
}

fn seek_to_data(inp: &mut In, hdr: &Hdr) -> bool {
    if inp.tell() != hdr.off as i64 && !inp.seek(hdr.off) {
        return false;
    }
    true
}

fn read_direct(im: &mut Image, inp: &mut In, info: &Info, hdr: &Hdr) -> bool {
    match info.enctype {
        BMP_BI_RGB => {}
        BMP_BI_BITFIELDS => return false,
        BMP_BI_RLE8 => {
            if info.depth != 8 {
                return false;
            }
        }
        BMP_BI_RLE4 => {
            if info.depth != 4 {
                return false;
            }
        }
        _ => return false,
    }
    if !seek_to_data(inp, hdr) {
        return false;
    }
    let mut padding = ((info.depth as i32 / 8).wrapping_mul(info.width)) % 4;
    if padding != 0 {
        padding = 4 - padding;
    }
    for ypos in 0..info.height {
        let row = if info.topdown {
            ypos
        } else {
            info.height - ypos - 1
        };
        for xpos in 0..info.width {
            let (red, green, blue);
            if info.depth == 16 {
                let Some(d) = inp.get_word_lsb() else {
                    return false;
                };
                let d = d as i32;
                red = ((d & 0x7C00) >> 10) << 3;
                green = ((d & 0x3E0) >> 5) << 3;
                blue = (d & 0x1F) << 3;
            } else {
                let Some(b) = inp.get_byte() else {
                    return false;
                };
                let Some(g) = inp.get_byte() else {
                    return false;
                };
                let Some(r) = inp.get_byte() else {
                    return false;
                };
                if info.depth != 24 && inp.get_byte().is_none() {
                    return false;
                }
                red = r;
                green = g;
                blue = b;
            }
            im.set_pixel(xpos, row, true_color(red, green, blue));
        }
        for _ in 0..padding {
            if inp.get_byte().is_none() {
                return false;
            }
        }
    }
    true
}

fn read_palette(im: &mut Image, inp: &mut In, count: i32, read_four: bool) -> bool {
    for i in 0..count.max(0) as usize {
        let Some(b) = inp.get_byte() else {
            return false;
        };
        let Some(g) = inp.get_byte() else {
            return false;
        };
        let Some(r) = inp.get_byte() else {
            return false;
        };
        if read_four && inp.get_byte().is_none() {
            return false;
        }
        im.red[i] = r;
        im.green[i] = g;
        im.blue[i] = b;
        im.open[i] = 1;
    }
    true
}

fn read_1bit(im: &mut Image, inp: &mut In, info: &mut Info, hdr: &Hdr) -> bool {
    if info.enctype != BMP_BI_RGB {
        return false;
    }
    if info.numcolors == 0 {
        info.numcolors = 2;
    } else if info.numcolors < 0 || info.numcolors > 2 {
        return false;
    }
    if !read_palette(im, inp, info.numcolors, info.ty == BMP_PALETTE_4) {
        return false;
    }
    im.colors_total = info.numcolors;
    if !seek_to_data(inp, hdr) {
        return false;
    }
    let mut padding = ((info.width.wrapping_add(7)) / 8) % 4;
    if padding != 0 {
        padding = 4 - padding;
    }
    for ypos in 0..info.height {
        let row = if info.topdown {
            ypos
        } else {
            info.height - ypos - 1
        };
        let mut xpos = 0;
        while xpos < info.width {
            let Some(cb) = inp.get_byte() else {
                return false;
            };
            for bit in 0..8 {
                let index = if cb & (0x80 >> bit) != 0 { 1 } else { 0 };
                im.open[index as usize] = 0;
                im.set_pixel(xpos + bit, row, index);
                if xpos + bit >= info.width {
                    break;
                }
            }
            xpos += 8;
        }
        for _ in 0..padding {
            if inp.get_byte().is_none() {
                return false;
            }
        }
    }
    true
}

fn read_4bit(im: &mut Image, inp: &mut In, info: &mut Info, hdr: &Hdr) -> bool {
    if info.enctype != BMP_BI_RGB && info.enctype != BMP_BI_RLE4 {
        return false;
    }
    if info.numcolors == 0 {
        info.numcolors = 16;
    } else if info.numcolors < 0 || info.numcolors > 16 {
        return false;
    }
    if !read_palette(im, inp, info.numcolors, info.ty == BMP_PALETTE_4) {
        return false;
    }
    im.colors_total = info.numcolors;
    if !seek_to_data(inp, hdr) {
        return false;
    }
    let mut padding = ((0.5 * info.width as f64).ceil() as i32) % 4;
    if padding != 0 {
        padding = 4 - padding;
    }
    match info.enctype {
        BMP_BI_RGB => {
            for ypos in 0..info.height {
                let row = if info.topdown {
                    ypos
                } else {
                    info.height - ypos - 1
                };
                let mut xpos = 0;
                while xpos < info.width {
                    let Some(cb) = inp.get_byte() else {
                        return false;
                    };
                    let index = (cb >> 4) & 0x0f;
                    im.open[index as usize] = 0;
                    im.set_pixel(xpos, row, index);
                    let index = cb & 0x0f;
                    im.open[index as usize] = 0;
                    im.set_pixel(xpos + 1, row, index);
                    xpos += 2;
                }
                for _ in 0..padding {
                    if inp.get_byte().is_none() {
                        return false;
                    }
                }
            }
            true
        }
        _ => read_rle(im, inp, info),
    }
}

fn read_8bit(im: &mut Image, inp: &mut In, info: &mut Info, hdr: &Hdr) -> bool {
    if info.enctype != BMP_BI_RGB && info.enctype != BMP_BI_RLE8 {
        return false;
    }
    if info.numcolors == 0 {
        info.numcolors = 256;
    } else if info.numcolors < 0 || info.numcolors > 256 {
        return false;
    }
    if !read_palette(im, inp, info.numcolors, info.ty == BMP_PALETTE_4) {
        return false;
    }
    im.colors_total = info.numcolors;
    if !seek_to_data(inp, hdr) {
        return false;
    }
    let mut padding = info.width % 4;
    if padding != 0 {
        padding = 4 - padding;
    }
    match info.enctype {
        BMP_BI_RGB => {
            for ypos in 0..info.height {
                let row = if info.topdown {
                    ypos
                } else {
                    info.height - ypos - 1
                };
                for xpos in 0..info.width {
                    let Some(index) = inp.get_byte() else {
                        return false;
                    };
                    im.open[index as usize] = 0;
                    im.set_pixel(xpos, row, index);
                }
                for _ in 0..padding {
                    if inp.get_byte().is_none() {
                        return false;
                    }
                }
            }
            true
        }
        _ => read_rle(im, inp, info),
    }
}

fn read_rle(im: &mut Image, inp: &mut In, info: &Info) -> bool {
    let depth = info.depth as i32;
    let ppb = 8 / depth;
    let mut ypos = 0i32;
    let mut xpos = 0i32;
    while ypos < info.height && xpos <= info.width {
        let Some(rle_length) = inp.get_byte() else {
            return false;
        };
        let Some(rle_data) = inp.get_byte() else {
            return false;
        };
        let row = info.height.wrapping_sub(ypos).wrapping_sub(1);
        if rle_length != BMP_RLE_COMMAND {
            im.open[rle_data as usize] = 0;
            let mut i = 0;
            while i < rle_length && xpos < info.width {
                let mut j = 1;
                while j <= ppb && xpos < info.width && i < rle_length {
                    let sh = 8 - j * depth;
                    let index = (rle_data & (((1 << depth) - 1) << sh)) >> sh;
                    im.open[(index & 0xff) as usize] = 0;
                    im.set_pixel(xpos, row, index);
                    j += 1;
                    xpos += 1;
                    i += 1;
                }
            }
        } else if rle_data > 2 {
            let mut padding = 0;
            let mut i = 0;
            while i < rle_data && xpos < info.width {
                let mut max_pixels = ppb;
                let Some(index) = inp.get_byte() else {
                    return false;
                };
                padding += 1;
                if rle_data - i < max_pixels {
                    max_pixels = rle_data - i;
                }
                let mut j = 1;
                while j <= max_pixels && xpos < info.width {
                    let temp = (index >> (8 - j * depth)) & ((1 << depth) - 1);
                    im.open[temp as usize] = 0;
                    im.set_pixel(xpos, row, temp);
                    j += 1;
                    xpos += 1;
                }
                i += ppb;
            }
            if padding % 2 != 0 && inp.get_byte().is_none() {
                return false;
            }
        } else if rle_data == BMP_RLE_ENDOFLINE {
            xpos = 0;
            ypos += 1;
        } else if rle_data == BMP_RLE_DELTA {
            let Some(l) = inp.get_byte() else {
                return false;
            };
            let Some(d) = inp.get_byte() else {
                return false;
            };
            xpos += l;
            ypos += d;
        } else if rle_data == BMP_RLE_ENDOFBITMAP {
            break;
        }
    }
    true
}
