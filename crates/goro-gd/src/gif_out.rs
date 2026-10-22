//! gd_gif_out.c: the GIF encoder (LZW from ppmtogif / compress).

use crate::io::{EOF, Out};
use crate::*;

const GIFBITS: i32 = 12;
const MAXBITS: i32 = GIFBITS;
const MAXMAXCODE: i32 = 1 << GIFBITS;
const HSIZE: i32 = 5003;
const CUR_BITS_FINISHED: i32 = -1000;

const MASKS: [u64; 17] = [
    0x0000, 0x0001, 0x0003, 0x0007, 0x000F, 0x001F, 0x003F, 0x007F, 0x00FF, 0x01FF, 0x03FF, 0x07FF,
    0x0FFF, 0x1FFF, 0x3FFF, 0x7FFF, 0xFFFF,
];

fn maxcode(n_bits: i32) -> i32 {
    (1 << n_bits) - 1
}

struct GifCtx<'a> {
    width: i32,
    height: i32,
    curx: i32,
    cury: i32,
    count_down: i64,
    pass: i32,
    interlace: bool,
    n_bits: i32,
    maxcode: i32,
    htab: Vec<i64>,
    codetab: Vec<u16>,
    free_ent: i32,
    clear_flg: bool,
    in_count: i64,
    out_count: i64,
    g_init_bits: i32,
    clear_code: i32,
    eof_code: i32,
    cur_accum: u64,
    cur_bits: i32,
    a_count: usize,
    accum: [u8; 256],
    out: &'a mut Out,
}

/// gdImageGifPtr() / _gdImageGifCtx(): None when the truecolor image
/// cannot be converted to a palette
pub fn image_gif(im: &Image) -> Option<Vec<u8>> {
    let mut out = Out::new();
    let interlace = im.interlace != 0;
    let pim;
    let tim = if im.true_color {
        pim = crate::topal::create_palette_from_true_color(im, true, 256)?;
        &pim
    } else {
        im
    };
    let bpp = colors_to_bpp(tim.colors_total);
    gif_encode(
        &mut out,
        tim.sx,
        tim.sy,
        interlace,
        0,
        tim.transparent,
        bpp,
        tim,
    );
    Some(out.into_vec())
}

fn colors_to_bpp(colors: i32) -> i32 {
    if colors <= 2 {
        1
    } else if colors <= 4 {
        2
    } else if colors <= 8 {
        3
    } else if colors <= 16 {
        4
    } else if colors <= 32 {
        5
    } else if colors <= 64 {
        6
    } else if colors <= 128 {
        7
    } else if colors <= 256 {
        8
    } else {
        0
    }
}

fn put_word(out: &mut Out, w: i32) {
    out.put_c(w & 0xff);
    out.put_c((w >> 8) & 0xff);
}

impl GifCtx<'_> {
    fn bump_pixel(&mut self) {
        self.curx += 1;
        if self.curx == self.width {
            self.curx = 0;
            if !self.interlace {
                self.cury += 1;
            } else {
                match self.pass {
                    0 => {
                        self.cury += 8;
                        if self.cury >= self.height {
                            self.pass += 1;
                            self.cury = 4;
                        }
                    }
                    1 => {
                        self.cury += 8;
                        if self.cury >= self.height {
                            self.pass += 1;
                            self.cury = 2;
                        }
                    }
                    2 => {
                        self.cury += 4;
                        if self.cury >= self.height {
                            self.pass += 1;
                            self.cury = 1;
                        }
                    }
                    3 => self.cury += 2,
                    _ => {}
                }
            }
        }
    }

    fn next_pixel(&mut self, im: &Image) -> i32 {
        if self.count_down == 0 {
            return EOF;
        }
        self.count_down -= 1;
        let r = im.get_pixel(self.curx, self.cury);
        self.bump_pixel();
        r
    }

    fn compress(&mut self, init_bits: i32, im: &Image) {
        self.g_init_bits = init_bits;
        self.out_count = 0;
        self.clear_flg = false;
        self.in_count = 1;
        self.n_bits = self.g_init_bits;
        self.maxcode = maxcode(self.n_bits);
        self.clear_code = 1 << (init_bits - 1);
        self.eof_code = self.clear_code + 1;
        self.free_ent = self.clear_code + 2;
        self.a_count = 0;
        let mut ent = self.next_pixel(im);
        let mut hshift = 0;
        let mut fcode: i64 = HSIZE as i64;
        while fcode < 65536 {
            hshift += 1;
            fcode *= 2;
        }
        hshift = 8 - hshift;
        let hsize_reg = HSIZE;
        self.cl_hash();
        self.output(self.clear_code);
        loop {
            let c = self.next_pixel(im);
            if c == EOF {
                break;
            }
            self.in_count += 1;
            let fcode = ((c as i64) << MAXBITS) + ent as i64;
            let mut i = (c << hshift) ^ ent;
            let mut matched = false;
            'search: {
                if !(0..HSIZE).contains(&i) {
                    // out of range indexes only come from bogus pixels
                    i = i.rem_euclid(HSIZE);
                }
                if self.htab[i as usize] == fcode {
                    ent = self.codetab[i as usize] as i32;
                    matched = true;
                    break 'search;
                } else if self.htab[i as usize] < 0 {
                    break 'search;
                }
                let disp = if i == 0 { 1 } else { hsize_reg - i };
                loop {
                    i -= disp;
                    if i < 0 {
                        i += hsize_reg;
                    }
                    if self.htab[i as usize] == fcode {
                        ent = self.codetab[i as usize] as i32;
                        matched = true;
                        break 'search;
                    }
                    if self.htab[i as usize] <= 0 {
                        break 'search;
                    }
                }
            }
            if matched {
                continue;
            }
            self.output(ent);
            self.out_count += 1;
            ent = c;
            if self.free_ent < MAXMAXCODE {
                self.codetab[i as usize] = self.free_ent as u16;
                self.free_ent += 1;
                self.htab[i as usize] = fcode;
            } else {
                self.cl_block();
            }
        }
        self.output(ent);
        self.out_count += 1;
        self.output(self.eof_code);
    }

    fn output(&mut self, code: i32) {
        if self.cur_bits == CUR_BITS_FINISHED {
            return;
        }
        self.cur_accum &= MASKS[self.cur_bits.clamp(0, 16) as usize];
        if self.cur_bits > 0 {
            self.cur_accum |= ((code as i64) << self.cur_bits) as u64;
        } else {
            self.cur_accum = code as i64 as u64;
        }
        self.cur_bits += self.n_bits;
        while self.cur_bits >= 8 {
            self.char_out((self.cur_accum & 0xff) as u8);
            self.cur_accum >>= 8;
            self.cur_bits -= 8;
        }
        if self.free_ent > self.maxcode || self.clear_flg {
            if self.clear_flg {
                self.n_bits = self.g_init_bits;
                self.maxcode = maxcode(self.n_bits);
                self.clear_flg = false;
            } else {
                self.n_bits += 1;
                if self.n_bits == MAXBITS {
                    self.maxcode = MAXMAXCODE;
                } else {
                    self.maxcode = maxcode(self.n_bits);
                }
            }
        }
        if code == self.eof_code {
            while self.cur_bits > 0 {
                self.char_out((self.cur_accum & 0xff) as u8);
                self.cur_accum >>= 8;
                self.cur_bits -= 8;
            }
            self.cur_bits = CUR_BITS_FINISHED;
            self.flush_char();
        }
    }

    fn cl_block(&mut self) {
        self.cl_hash();
        self.free_ent = self.clear_code + 2;
        self.clear_flg = true;
        self.output(self.clear_code);
    }

    fn cl_hash(&mut self) {
        for h in self.htab.iter_mut() {
            *h = -1;
        }
    }

    fn char_out(&mut self, c: u8) {
        self.accum[self.a_count] = c;
        self.a_count += 1;
        if self.a_count >= 254 {
            self.flush_char();
        }
    }

    fn flush_char(&mut self) {
        if self.a_count > 0 {
            self.out.put_c(self.a_count as i32);
            let n = self.a_count;
            let acc = self.accum;
            self.out.put_buf(&acc[..n]);
            self.a_count = 0;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn gif_encode(
    fp: &mut Out,
    gwidth: i32,
    gheight: i32,
    ginterlace: bool,
    background: i32,
    transparent: i32,
    bits_per_pixel: i32,
    im: &Image,
) {
    let color_map_size = 1 << bits_per_pixel;
    let resolution = bits_per_pixel;
    let init_code_size = if bits_per_pixel <= 1 {
        2
    } else {
        bits_per_pixel
    };
    fp.put_buf(if transparent < 0 {
        b"GIF87a"
    } else {
        b"GIF89a"
    });
    put_word(fp, gwidth);
    put_word(fp, gheight);
    let mut b = 0x80;
    b |= (resolution - 1) << 4;
    b |= bits_per_pixel - 1;
    fp.put_c(b);
    fp.put_c(background);
    fp.put_c(0);
    for i in 0..color_map_size as usize {
        let i = i & 0xff;
        fp.put_c(im.red[i]);
        fp.put_c(im.green[i]);
        fp.put_c(im.blue[i]);
    }
    if transparent >= 0 {
        fp.put_c(b'!' as i32);
        fp.put_c(0xf9);
        fp.put_c(4);
        fp.put_c(1);
        fp.put_c(0);
        fp.put_c(0);
        fp.put_c(transparent & 0xff);
        fp.put_c(0);
    }
    fp.put_c(b',' as i32);
    put_word(fp, 0);
    put_word(fp, 0);
    put_word(fp, gwidth);
    put_word(fp, gheight);
    fp.put_c(if ginterlace { 0x40 } else { 0x00 });
    fp.put_c(init_code_size);
    {
        let mut ctx = GifCtx {
            width: gwidth,
            height: gheight,
            curx: 0,
            cury: 0,
            count_down: gwidth as i64 * gheight as i64,
            pass: 0,
            interlace: ginterlace,
            n_bits: 0,
            maxcode: 0,
            htab: vec![0; HSIZE as usize],
            codetab: vec![0; HSIZE as usize],
            free_ent: 0,
            clear_flg: false,
            in_count: 1,
            out_count: 0,
            g_init_bits: 0,
            clear_code: 0,
            eof_code: 0,
            cur_accum: 0,
            cur_bits: 0,
            a_count: 0,
            accum: [0; 256],
            out: fp,
        };
        ctx.compress(init_code_size + 1, im);
    }
    fp.put_c(0);
    fp.put_c(b';' as i32);
}
