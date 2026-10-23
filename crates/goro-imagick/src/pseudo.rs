//! Pseudo images: `xc:` / `canvas:`, `gradient:`, `radial-gradient:`,
//! `plasma:`, `pattern:`, `hald:`, `null:` and goro's own stand-ins for
//! ImageMagick's built-in pictures (`logo:`, `rose:`, `wizard:`,
//! `granite:`, `netscape:`). The stand-ins are synthetic drawings of the
//! same sizes and kinds as ImageMagick's, generated deterministically;
//! none of ImageMagick's image data is used.

use crate::color::{PixelInfo, clamp_to_quantum};
use crate::error::{MResult, MagickError, severity};
use crate::geometry::Rect;
use crate::image::Image;
use crate::types::*;

/// Whether `magick` names a pseudo format handled here.
pub fn is_pseudo(magick: &str) -> bool {
    matches!(
        magick.to_ascii_uppercase().as_str(),
        "XC" | "CANVAS"
            | "GRADIENT"
            | "RADIAL-GRADIENT"
            | "RADIAL_GRADIENT"
            | "PLASMA"
            | "FRACTAL"
            | "PATTERN"
            | "HALD"
            | "NULL"
            | "MAGICK"
            | "LOGO"
            | "ROSE"
            | "WIZARD"
            | "GRANITE"
            | "NETSCAPE"
    )
}

fn bad_color(c: &str) -> MagickError {
    MagickError::with_desc(
        severity::OPTION_WARNING,
        "unrecognized color",
        c,
        "warning/color.c/GetColorCompliance/1064",
    )
}

fn color(c: &str, default: PixelInfo) -> MResult<PixelInfo> {
    if c.is_empty() {
        return Ok(default);
    }
    PixelInfo::parse(c).ok_or_else(|| bad_color(c))
}

/// Split "color1-color2" at the dash outside parentheses.
fn split_colors(s: &str) -> (String, String) {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '-' if depth == 0 && i > 0 => return (s[..i].to_string(), s[i + 1..].to_string()),
            _ => {}
        }
    }
    (s.to_string(), String::new())
}

/// Read a pseudo image. `size` is the wand's size (newPseudoImage's).
pub fn read(magick: &str, arg: &str, size: Option<(usize, usize)>) -> MResult<Image> {
    let up = magick.to_ascii_uppercase();
    let (w, h) = size.unwrap_or((1, 1));
    let mut img = match up.as_str() {
        "XC" | "CANVAS" => {
            let c = color(arg, PixelInfo::white())?;
            let mut img = Image::new(w, h, &c);
            img.format = up.clone();
            img
        }
        "NULL" => {
            let mut img = Image::new(w, h, &PixelInfo::white());
            img.enable_alpha();
            for p in img.pixels.chunks_exact_mut(4) {
                p[3] = 0;
            }
            img.format = "NULL".to_string();
            img
        }
        "GRADIENT" => {
            let (a, b) = split_colors(arg);
            let c1 = color(&a, PixelInfo::white())?;
            let c2 = color(
                &b,
                if a.is_empty() {
                    PixelInfo::black()
                } else {
                    complement(&c1)
                },
            )?;
            let mut img = linear_gradient(w, h, &c1, &c2);
            img.format = "GRADIENT".to_string();
            img
        }
        "RADIAL-GRADIENT" | "RADIAL_GRADIENT" => {
            let (a, b) = split_colors(arg);
            let c1 = color(&a, PixelInfo::white())?;
            let c2 = color(
                &b,
                if a.is_empty() {
                    PixelInfo::black()
                } else {
                    complement(&c1)
                },
            )?;
            let mut img = radial_gradient(w, h, &c1, &c2);
            img.format = "RADIAL-GRADIENT".to_string();
            img
        }
        "PLASMA" | "FRACTAL" => {
            let mut img = plasma(w, h, arg)?;
            img.format = "GRADIENT".to_string();
            img
        }
        "PATTERN" => {
            let Some(mut tile) = pattern(arg) else {
                return Err(MagickError::with_desc(
                    severity::OPTION_ERROR,
                    "UnrecognizedImageFormat",
                    arg,
                    "error/pattern.c/ReadPATTERNImage",
                ));
            };
            match size {
                Some((w, h)) => {
                    let mut img = tile_to(&tile, w, h);
                    img.format = "PATTERN".to_string();
                    img.filename = format!("/{}", arg.to_ascii_lowercase());
                    img
                }
                None => {
                    tile.format = "GIF".to_string();
                    tile
                }
            }
        }
        "HALD" => {
            let level: usize = arg.trim().parse().unwrap_or(8).clamp(2, 16);
            let mut img = hald(level);
            img.format = "HALD".to_string();
            img
        }
        "MAGICK" | "LOGO" | "ROSE" | "WIZARD" | "GRANITE" | "NETSCAPE" => {
            let which = if up == "MAGICK" {
                arg.to_ascii_uppercase()
            } else {
                up.clone()
            };
            builtin(&which).ok_or_else(|| {
                MagickError::with_desc(
                    severity::OPTION_ERROR,
                    "unrecognized image format",
                    &which.to_ascii_lowercase(),
                    "error/magick.c/ReadMAGICKImage/13072",
                )
            })?
        }
        _ => {
            return Err(MagickError::no_decode_delegate(&format!(
                "{}:{}",
                magick, arg
            )));
        }
    };
    if img.page.width == 0 {
        img.page = Rect::new(img.columns, img.rows, 0, 0);
    }
    img.magick_columns = img.columns;
    img.magick_rows = img.rows;
    Ok(img)
}

/// The default second gradient colour for one given colour: its
/// complement for a single colour, as "gradient:red" shades to cyan.
fn complement(c: &PixelInfo) -> PixelInfo {
    let mut o = c.clone();
    o.red = 65535.0 - c.red;
    o.green = 65535.0 - c.green;
    o.blue = 65535.0 - c.blue;
    o
}

fn mix(a: &PixelInfo, b: &PixelInfo, t: f64) -> [u16; 4] {
    let (ar, ag, ab) = a.rgb_q();
    let (br, bg, bb) = b.rgb_q();
    let l = |x: f64, y: f64| clamp_to_quantum(x + (y - x) * t) as u16;
    let alpha = a.alpha + (b.alpha - a.alpha) * t;
    if (a.alpha_trait || b.alpha_trait) && alpha > 0.0 {
        // the colours blend premultiplied by their alpha
        let (wa, wb) = (a.alpha * (1.0 - t), b.alpha * t);
        let p = |x: f64, y: f64| clamp_to_quantum((x * wa + y * wb) / alpha) as u16;
        return [p(ar, br), p(ag, bg), p(ab, bb), l(a.alpha, b.alpha)];
    }
    [l(ar, br), l(ag, bg), l(ab, bb), l(a.alpha, b.alpha)]
}

fn linear_gradient(w: usize, h: usize, c1: &PixelInfo, c2: &PixelInfo) -> Image {
    let mut img = Image::blank(w, h);
    img.alpha = c1.alpha_trait || c2.alpha_trait;
    for y in 0..h {
        let t = if h > 1 {
            y as f64 / (h - 1) as f64
        } else {
            0.0
        };
        let p = mix(c1, c2, t);
        for x in 0..w {
            img.set_px(x, y, p);
        }
    }
    img
}

fn radial_gradient(w: usize, h: usize, c1: &PixelInfo, c2: &PixelInfo) -> Image {
    let mut img = Image::blank(w, h);
    img.alpha = c1.alpha_trait || c2.alpha_trait;
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;
    let r = (w.max(h) as f64 / 2.0).max(1e-9);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let t = ((dx * dx + dy * dy).sqrt() / r).min(1.0);
            img.set_px(x, y, mix(c1, c2, t));
        }
    }
    img
}

/// A small deterministic PRNG (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// goro's plasma: midpoint displacement per channel, from a fixed seed,
/// over a base gradient of the given colours.
fn plasma(w: usize, h: usize, arg: &str) -> MResult<Image> {
    let fractal = arg.is_empty() || arg.eq_ignore_ascii_case("fractal");
    let (c1, c2) = if fractal {
        (PixelInfo::white(), PixelInfo::white())
    } else {
        let (a, b) = split_colors(arg);
        let c1 = color(&a, PixelInfo::white())?;
        let c2 = if b.is_empty() {
            c1.clone()
        } else {
            color(&b, PixelInfo::white())?
        };
        (c1, c2)
    };
    let mut img = if fractal {
        Image::new(w, h, &PixelInfo::from_rgba8([128, 128, 128, 255]))
    } else {
        linear_gradient(w, h, &c1, &c2)
    };
    let n = w.max(h).max(2).next_power_of_two() + 1;
    let mut rng = Rng::new(0x676f726f);
    let mut field = vec![vec![0f64; n * n]; 3];
    for ch in field.iter_mut() {
        for &(x, y) in &[(0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1)] {
            ch[y * n + x] = rng.next_f64() - 0.5;
        }
        let mut step = n - 1;
        let mut amp = 0.5;
        while step > 1 {
            let half = step / 2;
            for y in (half..n).step_by(step) {
                for x in (half..n).step_by(step) {
                    let a = ch[(y - half) * n + x - half];
                    let b = ch[(y - half) * n + x + half];
                    let c = ch[(y + half) * n + x - half];
                    let d = ch[(y + half) * n + x + half];
                    ch[y * n + x] = (a + b + c + d) / 4.0 + (rng.next_f64() - 0.5) * amp;
                }
            }
            for y in (0..n).step_by(half) {
                let start = if (y / half).is_multiple_of(2) {
                    half
                } else {
                    0
                };
                for x in (start..n).step_by(step) {
                    let mut s = 0.0;
                    let mut k = 0.0;
                    if y >= half {
                        s += ch[(y - half) * n + x];
                        k += 1.0;
                    }
                    if y + half < n {
                        s += ch[(y + half) * n + x];
                        k += 1.0;
                    }
                    if x >= half {
                        s += ch[y * n + x - half];
                        k += 1.0;
                    }
                    if x + half < n {
                        s += ch[y * n + x + half];
                        k += 1.0;
                    }
                    ch[y * n + x] = s / k + (rng.next_f64() - 0.5) * amp;
                }
            }
            step = half;
            amp *= 0.55;
        }
    }
    let scale = if fractal { 65535.0 } else { 32768.0 };
    for y in 0..h {
        for x in 0..w {
            let mut p = img.px(x, y);
            for (c, ch) in field.iter().enumerate() {
                let v = p[c] as f64 + ch[y * n + x] * scale;
                p[c] = clamp_to_quantum(v) as u16;
            }
            img.set_px(x, y, p);
        }
    }
    Ok(img)
}

fn tile_to(tile: &Image, w: usize, h: usize) -> Image {
    let mut img = Image::blank(w, h);
    img.colorspace = Colorspace::SRGB;
    for y in 0..h {
        for x in 0..w {
            img.set_px(x, y, tile.px(x % tile.columns, y % tile.rows));
        }
    }
    img
}

fn gray_tile(w: usize, h: usize, f: impl Fn(usize, usize) -> u8) -> Image {
    let mut img = Image::blank(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = f(x, y) as u16 * 257;
            img.set_px(x, y, [v, v, v, 65535]);
        }
    }
    img.depth = 8;
    img
}

/// goro's patterns (a small set of simple tiles).
fn pattern(name: &str) -> Option<Image> {
    let n = name.to_ascii_lowercase();
    let img = match n.as_str() {
        "checkerboard" => gray_tile(30, 30, |x, y| {
            if ((x / 15) + (y / 15)) % 2 == 0 {
                153
            } else {
                102
            }
        }),
        "horizontal" => gray_tile(8, 8, |_, y| if y % 4 == 0 { 0 } else { 255 }),
        "vertical" => gray_tile(8, 8, |x, _| if x % 4 == 0 { 0 } else { 255 }),
        "horizontal2" => gray_tile(8, 8, |_, y| if y % 2 == 0 { 0 } else { 255 }),
        "vertical2" => gray_tile(8, 8, |x, _| if x % 2 == 0 { 0 } else { 255 }),
        "crosshatch" => gray_tile(8, 8, |x, y| if x % 4 == 0 || y % 4 == 0 { 0 } else { 255 }),
        "left30" | "left45" => gray_tile(8, 8, |x, y| if (x + y) % 8 == 0 { 0 } else { 255 }),
        "right30" | "right45" => {
            gray_tile(8, 8, |x, y| if (x + 8 - y % 8) % 8 == 0 { 0 } else { 255 })
        }
        "bricks" => gray_tile(16, 16, |x, y| {
            let row = y / 8;
            let off = if row % 2 == 0 { 0 } else { 8 };
            if y % 8 == 0 || (x + off) % 16 == 0 {
                0
            } else {
                255
            }
        }),
        "hexagons" => gray_tile(30, 18, |x, y| {
            let yy = y % 18;
            let xx = x % 30;
            if yy == 0 && (xx < 8 || xx >= 23) || yy == 9 && (8..23).contains(&xx) {
                0
            } else {
                255
            }
        }),
        "circles" => gray_tile(16, 16, |x, y| {
            let dx = x as f64 - 7.5;
            let dy = y as f64 - 7.5;
            let r = (dx * dx + dy * dy).sqrt();
            if (r - 6.0).abs() < 0.8 { 0 } else { 255 }
        }),
        _ => {
            // gray0 .. gray100: ordered dither levels
            let lvl: u32 = n.strip_prefix("gray")?.parse().ok()?;
            if lvl > 100 {
                return None;
            }
            const BAYER: [[u32; 4]; 4] =
                [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
            gray_tile(32, 32, |x, y| {
                let t = BAYER[y % 4][x % 4] * 100 / 16;
                if lvl > t { 255 } else { 0 }
            })
        }
    };
    Some(img)
}

fn hald(level: usize) -> Image {
    let cube = level * level;
    let size = cube * level;
    let mut img = Image::blank(size, size);
    let mut i = 0usize;
    for y in 0..size {
        for x in 0..size {
            let r = i % cube;
            let g = (i / cube) % cube;
            let b = i / (cube * cube);
            let s = |v: usize| clamp_to_quantum(v as f64 * 65535.0 / (cube - 1) as f64) as u16;
            img.set_px(x, y, [s(r), s(g), s(b), 65535]);
            i += 1;
        }
    }
    img
}

fn put(img: &mut Image, x: usize, y: usize, c: [u8; 3]) {
    if x < img.columns && y < img.rows {
        img.set_px(
            x,
            y,
            [
                c[0] as u16 * 257,
                c[1] as u16 * 257,
                c[2] as u16 * 257,
                65535,
            ],
        );
    }
}

/// Quantize a channel to `n` levels (to keep the stand-ins palette images).
fn qz(v: f64, n: f64) -> u8 {
    ((v.clamp(0.0, 1.0) * (n - 1.0)).round() / (n - 1.0) * 255.0).round() as u8
}

/// goro's stand-in for the logo: a starry wizard hat over a sunset.
fn draw_logo(w: usize, h: usize) -> Image {
    let mut img = Image::new(w, h, &PixelInfo::white());
    let (wf, hf) = (w as f64, h as f64);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f64 / wf, y as f64 / hf);
            // sun disc
            let (dx, dy) = (u - 0.72, (v - 0.32) * hf / wf);
            let r = (dx * dx + dy * dy).sqrt();
            if r < 0.16 {
                let t = r / 0.16;
                put(
                    &mut img,
                    x,
                    y,
                    [255, qz(0.85 - 0.4 * t, 8.0), qz(0.2 * (1.0 - t), 4.0)],
                );
            }
            // hills
            let hill = 0.78 + 0.06 * (u * 9.0).sin() + 0.03 * (u * 23.0).cos();
            if v > hill {
                let t = (v - hill) / (1.0 - hill + 1e-9);
                put(
                    &mut img,
                    x,
                    y,
                    [
                        qz(0.1 + 0.2 * t, 8.0),
                        qz(0.45 + 0.3 * u, 8.0),
                        qz(0.2, 4.0),
                    ],
                );
            }
            // hat: a triangle with a brim
            let apex = (0.36, 0.08);
            let base_y = 0.72;
            if v > apex.1 && v < base_y {
                let t = (v - apex.1) / (base_y - apex.1);
                let half = 0.17 * t;
                let cx = apex.0 + 0.05 * (1.0 - t) * (1.0 - t);
                if (u - cx).abs() < half {
                    let shade = 0.35 + 0.4 * ((u - cx) / (half + 1e-9) + 1.0) / 2.0;
                    put(
                        &mut img,
                        x,
                        y,
                        [
                            qz(0.3 * shade, 8.0),
                            qz(0.1 * shade, 4.0),
                            qz(shade + 0.2, 8.0),
                        ],
                    );
                }
            }
            let (bx, by) = ((u - 0.36) / 0.27, (v - 0.73) / 0.035);
            if bx * bx + by * by < 1.0 {
                put(&mut img, x, y, [64, 32, 144]);
            }
            // stars on the hat
            for &(sx, sy) in &[(0.33, 0.45), (0.39, 0.58), (0.36, 0.3), (0.31, 0.62)] {
                let (ddx, ddy) = ((u - sx) * wf, (v - sy) * hf);
                if ddx.abs() + ddy.abs() < 7.0 {
                    put(&mut img, x, y, [255, 230, 0]);
                }
            }
        }
    }
    img
}

thread_local! {
    static BUILTINS: std::cell::RefCell<Vec<(String, Image)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A built-in image (generated once per thread).
fn builtin(which: &str) -> Option<Image> {
    if let Some(img) = BUILTINS.with(|b| {
        b.borrow()
            .iter()
            .find(|(n, _)| n == which)
            .map(|(_, i)| i.clone())
    }) {
        return Some(img);
    }
    let mut img = make_builtin(which)?;
    // as read from its encoded form: the blob's size
    img.file_size = crate::codec::encode(&[&img], &img.format, &Default::default())
        .map(|b| b.len() as u64)
        .unwrap_or(0);
    BUILTINS.with(|b| b.borrow_mut().push((which.to_string(), img.clone())));
    Some(img)
}

fn make_builtin(which: &str) -> Option<Image> {
    let mut img = match which {
        "LOGO" => {
            let mut img = draw_logo(640, 480);
            img.format = "GIF".to_string();
            img.colors = Some(256);
            img
        }
        "WIZARD" => {
            let mut img = draw_logo(480, 640);
            img.format = "GIF".to_string();
            img.colors = Some(256);
            img
        }
        "ROSE" => {
            let (w, h) = (70usize, 46usize);
            let mut img = Image::blank(w, h);
            for y in 0..h {
                for x in 0..w {
                    let dx = (x as f64 - 38.0) / 20.0;
                    let dy = (y as f64 - 20.0) / 16.0;
                    let r = (dx * dx + dy * dy).sqrt();
                    let a = dy.atan2(dx);
                    let petal = 0.75 + 0.25 * (5.0 * a + r * 6.0).sin();
                    let p = if r < petal {
                        let s = 1.0 - r / petal * 0.6;
                        [
                            (200.0 * s + 40.0 * (a * 3.0).cos().abs()) as u8,
                            (25.0 * s) as u8,
                            (35.0 * s + 10.0 * r) as u8,
                        ]
                    } else {
                        let g = 0.25 + 0.2 * ((x as f64 * 0.3).sin() * (y as f64 * 0.2).cos());
                        [
                            (48.0 * g * 2.0) as u8,
                            (120.0 * g + 20.0) as u8,
                            (45.0 * g * 2.0) as u8,
                        ]
                    };
                    put(&mut img, x, y, p);
                }
            }
            img.format = "PNM".to_string();
            img
        }
        "GRANITE" => {
            let mut rng = Rng::new(42);
            const LEVELS: [[u8; 3]; 12] = [
                [178, 169, 178],
                [160, 152, 160],
                [140, 134, 140],
                [120, 116, 122],
                [196, 188, 196],
                [212, 206, 212],
                [100, 96, 104],
                [150, 146, 154],
                [168, 160, 166],
                [132, 128, 136],
                [186, 180, 188],
                [110, 106, 114],
            ];
            let mut img = Image::blank(128, 128);
            for y in 0..128 {
                for x in 0..128 {
                    let k = if x == 0 && y == 0 {
                        0
                    } else {
                        (rng.next_u64() % 12) as usize
                    };
                    put(&mut img, x, y, LEVELS[k]);
                }
            }
            img.format = "GIF".to_string();
            img.colors = Some(12);
            img
        }
        "NETSCAPE" => {
            let mut img = Image::blank(216, 144);
            let lv = [255u8, 204, 153, 102, 51, 0];
            for y in 0..144 {
                for x in 0..216 {
                    let i = (x / 36) * 36 + (y / 24) * 6 + (x % 36) / 6;
                    let i = i % 216;
                    let c = [lv[(i / 36 + 1) % 6], lv[(i / 6) % 6], lv[i % 6]];
                    put(&mut img, x, y, c);
                }
            }
            img.format = "GIF".to_string();
            img.colors = Some(216);
            img
        }
        _ => return None,
    };
    img.depth = 8;
    img.colorspace = Colorspace::SRGB;
    Some(img)
}
