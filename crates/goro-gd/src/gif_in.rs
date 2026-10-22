//! gd_gif_in.c: the GIF decoder (first frame only).

use crate::io::In;
use crate::*;

const MAXCOLORMAPSIZE: usize = 256;
const MAX_LWZ_BITS: i32 = 12;
const INTERLACE: u8 = 0x40;
const LOCALCOLORMAP: u8 = 0x80;
const STACK_SIZE: usize = (1 << MAX_LWZ_BITS) * 2;
const CSD_BUF_SIZE: usize = 280;

type ColorMap = [[u8; MAXCOLORMAPSIZE]; 3];

/// ReadOK(): gdGetBuf() returned something (a short read counts)
fn read_ok(fd: &mut In, buf: &mut [u8]) -> bool {
    fd.get_buf(buf) > 0
}

fn lm_to_uint(a: u8, b: u8) -> i32 {
    ((b as i32) << 8) | a as i32
}

struct CodeStaticData {
    buf: [u8; CSD_BUF_SIZE],
    curbit: i32,
    lastbit: i32,
    done: bool,
    last_byte: i32,
}

struct LzwStaticData {
    fresh: bool,
    code_size: i32,
    set_code_size: i32,
    max_code: i32,
    max_code_size: i32,
    firstcode: i32,
    oldcode: i32,
    clear_code: i32,
    end_code: i32,
    table: [[i32; 1 << MAX_LWZ_BITS]; 2],
    stack: Vec<i32>,
    sp: usize,
    scd: CodeStaticData,
}

/// gdImageCreateFromGifCtx()
pub fn create_from_gif(data: &[u8]) -> Option<Image> {
    let mut fd = In::new(data);
    let mut transparent = -1;
    let mut buf = [0u8; 16];
    let mut c = [0u8; 1];
    let mut color_map: ColorMap = [[0; MAXCOLORMAPSIZE]; 3];
    let mut local_color_map: ColorMap = [[0; MAXCOLORMAPSIZE]; 3];
    let mut zero_data_block = false;
    let mut im: Option<Image> = None;

    if !read_ok(&mut fd, &mut buf[..6]) {
        return None;
    }
    if &buf[..3] != b"GIF" {
        return None;
    }
    if &buf[3..6] != b"87a" && &buf[3..6] != b"89a" {
        return None;
    }
    if !read_ok(&mut fd, &mut buf[..7]) {
        return None;
    }
    let bit_pixel = 2 << (buf[4] & 0x07);
    let screen_width = lm_to_uint(buf[0], buf[1]);
    let screen_height = lm_to_uint(buf[2], buf[3]);
    let have_global_colormap = buf[4] & LOCALCOLORMAP == LOCALCOLORMAP;
    if have_global_colormap && read_color_map(&mut fd, bit_pixel, &mut color_map) {
        return None;
    }
    loop {
        if !read_ok(&mut fd, &mut c) {
            return None;
        }
        if c[0] == b';' {
            break;
        }
        if c[0] == b'!' {
            if !read_ok(&mut fd, &mut c) {
                return None;
            }
            do_extension(&mut fd, c[0], &mut transparent, &mut zero_data_block);
            continue;
        }
        if c[0] != b',' {
            continue;
        }
        if !read_ok(&mut fd, &mut buf[..9]) {
            return None;
        }
        let use_global_colormap = buf[8] & LOCALCOLORMAP != LOCALCOLORMAP;
        let bit_pixel = 1 << ((buf[8] & 0x07) + 1);
        let left = lm_to_uint(buf[0], buf[1]);
        let top = lm_to_uint(buf[2], buf[3]);
        let width = lm_to_uint(buf[4], buf[5]);
        let height = lm_to_uint(buf[6], buf[7]);
        if left + width > screen_width || top + height > screen_height {
            return None;
        }
        let mut img = Image::create(width, height)?;
        let interlace = buf[8] & INTERLACE == INTERLACE;
        img.interlace = interlace as i32;
        if !use_global_colormap {
            if read_color_map(&mut fd, bit_pixel, &mut local_color_map) {
                return None;
            }
            read_image(
                &mut img,
                &mut fd,
                width,
                height,
                &local_color_map,
                interlace,
                &mut zero_data_block,
            );
        } else {
            if !have_global_colormap {
                color_map[0][1] = 0xff;
                color_map[1][1] = 0xff;
                color_map[2][1] = 0xff;
            }
            read_image(
                &mut img,
                &mut fd,
                width,
                height,
                &color_map,
                interlace,
                &mut zero_data_block,
            );
        }
        if transparent != -1 {
            img.color_transparent(transparent);
        }
        im = Some(img);
        break;
    }
    let mut im = im?;
    let mut i = im.colors_total - 1;
    while i >= 0 {
        if im.open[i as usize] != 0 {
            im.colors_total -= 1;
        } else {
            break;
        }
        i -= 1;
    }
    if im.colors_total == 0 {
        return None;
    }
    Some(im)
}

fn read_color_map(fd: &mut In, number: i32, buffer: &mut ColorMap) -> bool {
    let mut rgb = [0u8; 3];
    #[allow(clippy::needless_range_loop)]
    for i in 0..number as usize {
        if !read_ok(fd, &mut rgb) {
            return true;
        }
        for (ch, v) in rgb.iter().enumerate() {
            buffer[ch][i] = *v;
        }
    }
    false
}

fn do_extension(fd: &mut In, label: u8, transparent: &mut i32, zdb: &mut bool) {
    let mut buf = [0u8; 256];
    if label == 0xf9 {
        let _ = get_data_block(fd, &mut buf, zdb);
        if buf[0] & 0x1 != 0 {
            *transparent = buf[3] as i32;
        }
        while get_data_block(fd, &mut buf, zdb) > 0 {}
        return;
    }
    while get_data_block(fd, &mut buf, zdb) > 0 {}
}

fn get_data_block(fd: &mut In, buf: &mut [u8], zdb: &mut bool) -> i32 {
    let mut count = [0u8; 1];
    if !read_ok(fd, &mut count) {
        return -1;
    }
    let count = count[0] as usize;
    *zdb = count == 0;
    if count != 0 && !read_ok(fd, &mut buf[..count]) {
        return -1;
    }
    count as i32
}

fn get_code(
    fd: &mut In,
    scd: &mut CodeStaticData,
    code_size: i32,
    flag: bool,
    zdb: &mut bool,
) -> i32 {
    if flag {
        scd.curbit = 0;
        scd.lastbit = 0;
        scd.last_byte = 2;
        scd.done = false;
        return 0;
    }
    if scd.curbit + code_size >= scd.lastbit {
        if scd.done {
            return -1;
        }
        scd.buf[0] = scd.buf[(scd.last_byte - 2) as usize];
        scd.buf[1] = scd.buf[(scd.last_byte - 1) as usize];
        let count = get_data_block(fd, &mut scd.buf[2..], zdb);
        if count <= 0 {
            scd.done = true;
        }
        scd.last_byte = 2 + count;
        scd.curbit = (scd.curbit - scd.lastbit) + 16;
        scd.lastbit = (2 + count) * 8;
    }
    let ret = if scd.curbit + code_size > (CSD_BUF_SIZE * 8) as i32 {
        -1
    } else {
        let mut r = 0;
        for j in 0..code_size {
            let i = scd.curbit + j;
            if i >= 0 && (scd.buf[(i / 8) as usize] & (1 << (i % 8))) != 0 {
                r |= 1 << j;
            }
        }
        r
    };
    scd.curbit += code_size;
    ret
}

fn lwz_read_byte(
    fd: &mut In,
    sd: &mut LzwStaticData,
    flag: bool,
    input_code_size: i32,
    zdb: &mut bool,
) -> i32 {
    if flag {
        sd.set_code_size = input_code_size;
        sd.code_size = sd.set_code_size + 1;
        sd.clear_code = 1 << sd.set_code_size;
        sd.end_code = sd.clear_code + 1;
        sd.max_code_size = 2 * sd.clear_code;
        sd.max_code = sd.clear_code + 2;
        get_code(fd, &mut sd.scd, 0, true, zdb);
        sd.fresh = true;
        let mut i = 0usize;
        while (i as i32) < sd.clear_code {
            sd.table[0][i] = 0;
            sd.table[1][i] = i as i32;
            i += 1;
        }
        while i < (1 << MAX_LWZ_BITS) {
            sd.table[0][i] = 0;
            sd.table[1][0] = 0;
            i += 1;
        }
        sd.sp = 0;
        return 0;
    } else if sd.fresh {
        sd.fresh = false;
        loop {
            sd.firstcode = get_code(fd, &mut sd.scd, sd.code_size, false, zdb);
            sd.oldcode = sd.firstcode;
            if sd.firstcode != sd.clear_code {
                break;
            }
        }
        return sd.firstcode;
    }
    if sd.sp > 0 {
        sd.sp -= 1;
        return sd.stack[sd.sp];
    }
    let mut code;
    loop {
        code = get_code(fd, &mut sd.scd, sd.code_size, false, zdb);
        if code < 0 {
            break;
        }
        if code == sd.clear_code {
            let mut i = 0usize;
            while (i as i32) < sd.clear_code {
                sd.table[0][i] = 0;
                sd.table[1][i] = i as i32;
                i += 1;
            }
            while i < (1 << MAX_LWZ_BITS) {
                sd.table[0][i] = 0;
                sd.table[1][i] = 0;
                i += 1;
            }
            sd.code_size = sd.set_code_size + 1;
            sd.max_code_size = 2 * sd.clear_code;
            sd.max_code = sd.clear_code + 2;
            sd.sp = 0;
            sd.firstcode = get_code(fd, &mut sd.scd, sd.code_size, false, zdb);
            sd.oldcode = sd.firstcode;
            return sd.firstcode;
        } else if code == sd.end_code {
            let mut buf = [0u8; 260];
            if *zdb {
                return -2;
            }
            let mut count;
            loop {
                count = get_data_block(fd, &mut buf, zdb);
                if count <= 0 {
                    break;
                }
            }
            if count != 0 {
                return -2;
            }
        }
        let incode = code;
        if sd.sp == STACK_SIZE {
            return -1;
        }
        if code >= sd.max_code {
            sd.stack[sd.sp] = sd.firstcode;
            sd.sp += 1;
            code = sd.oldcode;
        }
        while code >= sd.clear_code {
            if sd.sp == STACK_SIZE {
                return -1;
            }
            let cu = (code as usize) & ((1 << MAX_LWZ_BITS) - 1);
            sd.stack[sd.sp] = sd.table[1][cu];
            sd.sp += 1;
            code = sd.table[0][cu];
        }
        let cu = (code as usize) & ((1 << MAX_LWZ_BITS) - 1);
        sd.firstcode = sd.table[1][cu];
        sd.stack[sd.sp] = sd.firstcode;
        sd.sp += 1;
        code = sd.max_code;
        if code < (1 << MAX_LWZ_BITS) {
            sd.table[0][code as usize] = sd.oldcode;
            sd.table[1][code as usize] = sd.firstcode;
            sd.max_code += 1;
            if sd.max_code >= sd.max_code_size && sd.max_code_size < (1 << MAX_LWZ_BITS) {
                sd.max_code_size *= 2;
                sd.code_size += 1;
            }
        }
        sd.oldcode = incode;
        if sd.sp > 0 {
            sd.sp -= 1;
            return sd.stack[sd.sp];
        }
    }
    code
}

fn read_image(
    im: &mut Image,
    fd: &mut In,
    len: i32,
    height: i32,
    cmap: &ColorMap,
    interlace: bool,
    zdb: &mut bool,
) {
    let mut c = [0u8; 1];
    let mut xpos = 0;
    let mut ypos = 0;
    let mut pass = 0;
    if !read_ok(fd, &mut c) {
        return;
    }
    let c = c[0] as i32;
    if c > MAX_LWZ_BITS {
        return;
    }
    #[allow(clippy::needless_range_loop)]
    for i in 0..MAX_COLORS {
        im.red[i] = cmap[0][i] as i32;
        im.green[i] = cmap[1][i] as i32;
        im.blue[i] = cmap[2][i] as i32;
        im.open[i] = 1;
    }
    im.colors_total = MAX_COLORS as i32;
    let mut sd = Box::new(LzwStaticData {
        fresh: false,
        code_size: 0,
        set_code_size: 0,
        max_code: 0,
        max_code_size: 0,
        firstcode: 0,
        oldcode: 0,
        clear_code: 0,
        end_code: 0,
        table: [[0; 1 << MAX_LWZ_BITS]; 2],
        stack: vec![0; STACK_SIZE],
        sp: 0,
        scd: CodeStaticData {
            buf: [0; CSD_BUF_SIZE],
            curbit: 0,
            lastbit: 0,
            done: false,
            last_byte: 0,
        },
    });
    if lwz_read_byte(fd, &mut sd, true, c, zdb) < 0 {
        return;
    }
    loop {
        let mut v = lwz_read_byte(fd, &mut sd, false, c, zdb);
        if v < 0 {
            break;
        }
        if v >= MAX_COLORS as i32 {
            v = 0;
        }
        im.open[v as usize] = 0;
        im.set_pixel(xpos, ypos, v);
        xpos += 1;
        if xpos == len {
            xpos = 0;
            if interlace {
                match pass {
                    0 | 1 => ypos += 8,
                    2 => ypos += 4,
                    3 => ypos += 2,
                    _ => {}
                }
                if ypos >= height {
                    pass += 1;
                    match pass {
                        1 => ypos = 4,
                        2 => ypos = 2,
                        3 => ypos = 1,
                        _ => break,
                    }
                }
            } else {
                ypos += 1;
            }
        }
        if ypos >= height {
            break;
        }
    }
    let _ = lwz_read_byte(fd, &mut sd, false, c, zdb);
}
