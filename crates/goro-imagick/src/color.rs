//! Colours: the PixelWand / PixelInfo model (channels as quantum values of a
//! Q16 build, so 0..=65535, kept as `f64` the way PixelInfo keeps
//! MagickRealType), colour-string parsing (names, `#hex`, the functional
//! notations) and formatting (`srgb(...)`, `gray(...)`, `cmyk(...)`).

use crate::color_names::COLOR_NAMES;
use crate::types::Colorspace;
use crate::{QUANTUM_RANGE, QUANTUM_SCALE};

/// A colour as ImageMagick's PixelInfo holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelInfo {
    pub colorspace: Colorspace,
    /// Whether the colour carries alpha (an `rgba()`, `#rgba`, `none`...).
    pub alpha_trait: bool,
    pub fuzz: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub black: f64,
    pub alpha: f64,
    pub count: u64,
    pub index: f64,
}

impl Default for PixelInfo {
    fn default() -> Self {
        PixelInfo {
            colorspace: Colorspace::SRGB,
            alpha_trait: false,
            fuzz: 0.0,
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            black: 0.0,
            alpha: QUANTUM_RANGE,
            count: 0,
            index: 0.0,
        }
    }
}

/// ClampToQuantum() of a Q16 non-HDRI build.
pub fn clamp_to_quantum(v: f64) -> f64 {
    if v.is_nan() || v <= 0.0 {
        0.0
    } else if v >= QUANTUM_RANGE {
        QUANTUM_RANGE
    } else {
        (v + 0.5).floor()
    }
}

/// ClampToQuantum() as an integer sample.
pub fn to_q16(v: f64) -> u16 {
    clamp_to_quantum(v) as u16
}

/// `%g` as C's printf formats it (6 significant digits).
pub fn fmt_g(v: f64) -> String {
    fmt_g_prec(v, 6)
}

/// `%.*g`.
pub fn fmt_g_prec(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    if !v.is_finite() {
        return if v.is_nan() {
            "nan".to_string()
        } else if v > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let prec = prec.max(1);
    let e = format!("{:.*e}", prec - 1, v);
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if exp < -4 || exp >= prec as i32 {
        let mant = trim_zeros(mant);
        format!(
            "{}e{}{:02}",
            mant,
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    } else {
        let decimals = (prec as i32 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", decimals, v)).to_string()
    }
}

fn trim_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

impl PixelInfo {
    pub fn from_rgba_q(r: f64, g: f64, b: f64, a: f64) -> Self {
        PixelInfo {
            red: r,
            green: g,
            blue: b,
            alpha: a,
            alpha_trait: a < QUANTUM_RANGE,
            ..Default::default()
        }
    }

    pub fn from_rgba8(c: [u8; 4]) -> Self {
        let s = |v: u8| v as f64 * 257.0;
        PixelInfo {
            red: s(c[0]),
            green: s(c[1]),
            blue: s(c[2]),
            alpha: s(c[3]),
            alpha_trait: c[3] != 255,
            ..Default::default()
        }
    }

    pub fn black() -> Self {
        Self::default()
    }

    pub fn white() -> Self {
        PixelInfo {
            red: QUANTUM_RANGE,
            green: QUANTUM_RANGE,
            blue: QUANTUM_RANGE,
            ..Default::default()
        }
    }

    pub fn transparent() -> Self {
        PixelInfo {
            alpha: 0.0,
            alpha_trait: true,
            ..Default::default()
        }
    }

    /// The colour as an RGBA sample quadruple (CMYK converted to RGB).
    pub fn to_rgba16(&self) -> [u16; 4] {
        let (r, g, b) = self.rgb_q();
        [to_q16(r), to_q16(g), to_q16(b), to_q16(self.alpha)]
    }

    /// The four sample slots as they are (no CMYK conversion).
    pub fn to_rgba16_raw(&self) -> [u16; 4] {
        [
            to_q16(self.red),
            to_q16(self.green),
            to_q16(self.blue),
            to_q16(self.alpha),
        ]
    }

    /// sRGB channels in quantum scale (CMYK converted).
    pub fn rgb_q(&self) -> (f64, f64, f64) {
        if self.colorspace == Colorspace::CMYK {
            let k = self.black * QUANTUM_SCALE;
            let f = |c: f64| QUANTUM_RANGE * (1.0 - (c * QUANTUM_SCALE * (1.0 - k) + k));
            (f(self.red), f(self.green), f(self.blue))
        } else {
            (self.red, self.green, self.blue)
        }
    }

    /// Parse a colour string (PixelSetColor / QueryColorCompliance).
    pub fn parse(s: &str) -> Option<PixelInfo> {
        parse_color(s)
    }

    /// PixelGetColorAsString() / GetColorTuple(): "srgb(255,0,0)",
    /// "srgba(255,0,0,0.5)", "gray(50%)", "cmyk(...)"...
    pub fn to_color_string(&self) -> String {
        let alpha = if self.alpha_trait {
            Some(fmt_g(self.alpha * QUANTUM_SCALE))
        } else {
            None
        };
        let suffix = |base: &str| -> String {
            if alpha.is_some() {
                format!("{}a", base)
            } else {
                base.to_string()
            }
        };
        let push_alpha = |mut parts: Vec<String>| -> Vec<String> {
            if let Some(a) = &alpha {
                parts.push(a.clone());
            }
            parts
        };
        match self.colorspace {
            Colorspace::CMYK => {
                let f = |v: f64| fmt_g(v * 255.0 / QUANTUM_RANGE);
                let parts = push_alpha(vec![
                    f(self.red),
                    f(self.green),
                    f(self.blue),
                    f(self.black),
                ]);
                format!("{}({})", suffix("cmyk"), parts.join(","))
            }
            Colorspace::Gray | Colorspace::LinearGray => {
                let base = if self.colorspace == Colorspace::Gray {
                    "gray"
                } else {
                    "lineargray"
                };
                let parts = push_alpha(vec![fmt_channels(&[self.red])[0].clone()]);
                format!("{}({})", suffix(base), parts.join(","))
            }
            cs => {
                let base = match cs {
                    Colorspace::RGB => "rgb",
                    Colorspace::HSL => "hsl",
                    Colorspace::HSB => "hsb",
                    Colorspace::HWB => "hwb",
                    Colorspace::Lab => "cielab",
                    Colorspace::Undefined => "undefined",
                    _ => "srgb",
                };
                if cs == Colorspace::Lab {
                    let l = fmt_g(self.red * 100.0 / QUANTUM_RANGE);
                    let a = fmt_g((self.green - QUANTUM_RANGE / 2.0) * 255.0 / QUANTUM_RANGE);
                    let b = fmt_g((self.blue - QUANTUM_RANGE / 2.0) * 255.0 / QUANTUM_RANGE);
                    let parts = push_alpha(vec![l, a, b]);
                    return format!("{}({})", suffix(base), parts.join(","));
                }
                let parts = push_alpha(fmt_channels(&[self.red, self.green, self.blue]));
                format!("{}({})", suffix(base), parts.join(","))
            }
        }
    }

    /// IsFuzzyEquivalencePixelInfo() as IsPixelWandSimilar() uses it: the
    /// colour distance (alpha-weighted RGB, plus alpha) against
    /// max(fuzz, the colours' own fuzz, sqrt(0.5)) squared... `fuzz` is in
    /// quantum units.
    pub fn is_similar(&self, other: &PixelInfo, fuzz: f64) -> bool {
        let fuzz = fuzz
            .max(self.fuzz)
            .max(other.fuzz)
            .max(std::f64::consts::FRAC_1_SQRT_2);
        let fuzz2 = fuzz * fuzz;
        let mut scale = 1.0;
        let mut distance = 0.0;
        let a1 = if self.alpha_trait {
            self.alpha
        } else {
            QUANTUM_RANGE
        };
        let a2 = if other.alpha_trait {
            other.alpha
        } else {
            QUANTUM_RANGE
        };
        if self.alpha_trait || other.alpha_trait {
            let p = a1 - a2;
            distance = p * p;
            if distance > fuzz2 {
                return false;
            }
            scale = (a1 * QUANTUM_SCALE) * (a2 * QUANTUM_SCALE);
        }
        let (r1, g1, b1) = self.rgb_q();
        let (r2, g2, b2) = other.rgb_q();
        for (x, y) in [(r1, r2), (g1, g2), (b1, b2)] {
            let p = x - y;
            distance += scale * p * p;
            if distance > fuzz2 {
                return false;
            }
        }
        true
    }

    /// RGB -> HSL (hue, saturation, lightness all in 0..1).
    pub fn to_hsl(&self) -> (f64, f64, f64) {
        let (r, g, b) = self.rgb_q();
        rgb_to_hsl(
            clamp_to_quantum(r) * QUANTUM_SCALE,
            clamp_to_quantum(g) * QUANTUM_SCALE,
            clamp_to_quantum(b) * QUANTUM_SCALE,
        )
    }

    pub fn set_hsl(&mut self, h: f64, s: f64, l: f64) {
        let (r, g, b) = hsl_to_rgb(h, s, l);
        self.red = clamp_to_quantum(r * QUANTUM_RANGE);
        self.green = clamp_to_quantum(g * QUANTUM_RANGE);
        self.blue = clamp_to_quantum(b * QUANTUM_RANGE);
    }
}

/// The colour channels as GetColorTuple prints them: 8-bit integers when
/// every channel is exact at 8 bits, else percentages.
fn fmt_channels(vals: &[f64]) -> Vec<String> {
    let eight = vals.iter().all(|&v| {
        let q = v / 257.0;
        (q - q.round()).abs() < 1e-9 && (0.0..=255.0).contains(&q.round())
    });
    vals.iter()
        .map(|&v| {
            if eight {
                format!("{}", (v / 257.0).round() as i64)
            } else {
                format!("{}%", fmt_g(v * 100.0 / QUANTUM_RANGE))
            }
        })
        .collect()
}

pub fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let c = max - min;
    let l = (max + min) / 2.0;
    if c <= 0.0 {
        return (0.0, 0.0, l);
    }
    let mut h = if max == r {
        ((g - b) / c) % 6.0
    } else if max == g {
        (b - r) / c + 2.0
    } else {
        (r - g) / c + 4.0
    };
    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    }
    let s = if l <= 0.5 {
        c / (2.0 * l)
    } else {
        c / (2.0 - 2.0 * l)
    };
    (h / 360.0, s, l)
}

pub fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let h = (h - h.floor()) * 6.0;
    let c = if l <= 0.5 {
        2.0 * l * s
    } else {
        (2.0 - 2.0 * l) * s
    };
    let m = l - 0.5 * c;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (r + m, g + m, b + m)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let c = v * s;
    let h6 = (h - h.floor()) * 6.0;
    let x = c * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h6 as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

pub fn lookup_name(name: &[u8]) -> Option<[u8; 4]> {
    COLOR_NAMES
        .binary_search_by(|(n, _, _)| (*n).cmp(name))
        .ok()
        .map(|i| COLOR_NAMES[i].1)
}

fn hexval(c: u8) -> Option<u32> {
    (c as char).to_digit(16)
}

fn parse_hex(h: &[u8]) -> Option<PixelInfo> {
    let (n, alpha) = match h.len() {
        3 => (1, false),
        4 => (1, true),
        6 => (2, false),
        8 => (2, true),
        9 => (3, false),
        12 => (4, false),
        16 => (4, true),
        _ => return None,
    };
    let mut vals = [0f64; 4];
    let max = ((1u64 << (4 * n)) - 1) as f64;
    let channels = if alpha { 4 } else { 3 };
    for (i, v) in vals.iter_mut().enumerate().take(channels) {
        let mut x = 0u32;
        for &c in &h[i * n..(i + 1) * n] {
            x = x * 16 + hexval(c)?;
        }
        *v = clamp_to_quantum(x as f64 * QUANTUM_RANGE / max);
    }
    let mut p = PixelInfo {
        red: vals[0],
        green: vals[1],
        blue: vals[2],
        ..Default::default()
    };
    if alpha {
        p.alpha = vals[3];
        p.alpha_trait = true;
    }
    Some(p)
}

/// One argument of a functional notation: (value, is_percent).
fn parse_args(inner: &str) -> Option<Vec<(f64, bool)>> {
    let mut out = Vec::new();
    for tok in inner
        .split(|c: char| c == ',' || c.is_ascii_whitespace() || c == '/')
        .filter(|t| !t.is_empty())
    {
        let (num, pct) = match tok.strip_suffix('%') {
            Some(n) => (n, true),
            None => (tok, false),
        };
        let num = num.trim_end_matches("deg");
        let v: f64 = num.parse().ok()?;
        out.push((v, pct));
    }
    Some(out)
}

fn parse_color(s: &str) -> Option<PixelInfo> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if let Some(h) = lower.strip_prefix('#') {
        return parse_hex(h.as_bytes());
    }
    if let Some(open) = lower.find('(') {
        let close = lower.rfind(')')?;
        if close < open {
            return None;
        }
        let name = lower[..open].trim();
        let args = parse_args(&lower[open + 1..close])?;
        return parse_functional(name, &args);
    }
    let rgba = lookup_name(lower.as_bytes())?;
    let mut p = PixelInfo::from_rgba8(rgba);
    p.alpha_trait = rgba[3] != 255;
    let b = lower.as_bytes();
    let gray_name = (b.starts_with(b"gray") || b.starts_with(b"grey"))
        && b[4..].iter().all(|c| c.is_ascii_digit());
    if gray_name {
        p.colorspace = Colorspace::Gray;
    }
    Some(p)
}

fn parse_functional(name: &str, args: &[(f64, bool)]) -> Option<PixelInfo> {
    // channel value on a 0..255 scale (or a percentage)
    let ch255 = |a: (f64, bool)| -> f64 {
        if a.1 {
            clamp_to_quantum(a.0 * QUANTUM_RANGE / 100.0)
        } else {
            clamp_to_quantum(a.0 * QUANTUM_RANGE / 255.0)
        }
    };
    // alpha on a 0..1 scale (or a percentage)
    let alpha = |a: (f64, bool)| -> f64 {
        if a.1 {
            clamp_to_quantum(a.0 * QUANTUM_RANGE / 100.0)
        } else {
            clamp_to_quantum(a.0 * QUANTUM_RANGE)
        }
    };
    // a 0..1 fraction (or percentage) for hsl's s/l
    let frac = |a: (f64, bool)| -> f64 { if a.1 { a.0 / 100.0 } else { a.0 } };
    let mut p = PixelInfo::default();
    const BASES: &[&str] = &[
        "rgb",
        "srgb",
        "hsl",
        "hsb",
        "hsv",
        "hwb",
        "gray",
        "cmyk",
        "lab",
        "cielab",
        "device-gray",
        "device-rgb",
        "device-cmyk",
        "icc-color",
        "lineargray",
    ];
    let (base, has_alpha) = if BASES.contains(&name) {
        (name, false)
    } else {
        match name.strip_suffix('a') {
            Some(b) if BASES.contains(&b) => (b, true),
            _ => return None,
        }
    };
    let need = match base {
        "gray" | "device-gray" | "lineargray" => 1,
        "cmyk" | "device-cmyk" => 4,
        "icc-color" => return Some(PixelInfo::white()),
        _ => 3,
    };
    if args.len() < need {
        return None;
    }
    let extra = args.get(need).copied();
    match base {
        "rgb" | "srgb" => {
            p.red = ch255(args[0]);
            p.green = ch255(args[1]);
            p.blue = ch255(args[2]);
        }
        "device-rgb" => {
            p.red = clamp_to_quantum(frac(args[0]) * QUANTUM_RANGE);
            p.green = clamp_to_quantum(frac(args[1]) * QUANTUM_RANGE);
            p.blue = clamp_to_quantum(frac(args[2]) * QUANTUM_RANGE);
        }
        "gray" | "lineargray" => {
            let v = ch255(args[0]);
            p.red = v;
            p.green = v;
            p.blue = v;
            p.colorspace = if base == "gray" {
                Colorspace::Gray
            } else {
                Colorspace::LinearGray
            };
        }
        "device-gray" => {
            let v = clamp_to_quantum(frac(args[0]) * QUANTUM_RANGE);
            p.red = v;
            p.green = v;
            p.blue = v;
            p.colorspace = Colorspace::Gray;
        }
        "cmyk" | "device-cmyk" => {
            let f = |a: (f64, bool)| {
                if base == "device-cmyk" {
                    clamp_to_quantum(frac(a) * QUANTUM_RANGE)
                } else {
                    ch255(a)
                }
            };
            p.red = f(args[0]);
            p.green = f(args[1]);
            p.blue = f(args[2]);
            p.black = f(args[3]);
            p.colorspace = Colorspace::CMYK;
        }
        "hsl" | "hsb" | "hsv" | "hwb" => {
            let h = args[0].0 / 360.0;
            let a = frac(args[1]);
            let b = frac(args[2]);
            let (r, g, bl) = match base {
                "hsl" => hsl_to_rgb(h, a, b),
                "hwb" => {
                    // whiteness / blackness
                    let (w, bk) = if a + b >= 1.0 {
                        (a / (a + b), b / (a + b))
                    } else {
                        (a, b)
                    };
                    let v = 1.0 - bk;
                    let s = if v > 0.0 { 1.0 - w / v } else { 0.0 };
                    hsv_to_rgb(h, s, v)
                }
                _ => hsv_to_rgb(h, a, b),
            };
            // the HSL family is not rounded to quantum levels
            p.red = (r * QUANTUM_RANGE).clamp(0.0, QUANTUM_RANGE);
            p.green = (g * QUANTUM_RANGE).clamp(0.0, QUANTUM_RANGE);
            p.blue = (bl * QUANTUM_RANGE).clamp(0.0, QUANTUM_RANGE);
        }
        "lab" | "cielab" => {
            p.colorspace = Colorspace::Lab;
            p.red = clamp_to_quantum(args[0].0 * QUANTUM_RANGE / 100.0);
            p.green = clamp_to_quantum((args[1].0 / 255.0 + 0.5) * QUANTUM_RANGE);
            p.blue = clamp_to_quantum((args[2].0 / 255.0 + 0.5) * QUANTUM_RANGE);
        }
        _ => return None,
    }
    if has_alpha {
        if let Some(a) = extra {
            p.alpha = alpha(a);
        }
        p.alpha_trait = true;
    } else if let Some(a) = extra {
        // rgb(r,g,b,a) is accepted as rgba
        p.alpha = alpha(a);
        p.alpha_trait = true;
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(c: &str) -> String {
        PixelInfo::parse(c).unwrap().to_color_string()
    }

    #[test]
    fn strings() {
        assert_eq!(s("red"), "srgb(255,0,0)");
        assert_eq!(s("#ff000080"), "srgba(255,0,0,0.501961)");
        assert_eq!(s("rgba(255,0,0,0.5)"), "srgba(255,0,0,0.500008)");
        assert_eq!(s("rgb(10%,20%,30%)"), "srgb(10.0008%,20%,30.0008%)");
        assert_eq!(s("hsl(120,100%,50%)"), "srgb(0,255,0)");
        assert_eq!(s("gray50"), "gray(127)");
        assert_eq!(s("gray"), "gray(126)");
        assert_eq!(s("gray(50%)"), "gray(50.0008%)");
        assert_eq!(s("cmyk(10,20,30,40)"), "cmyk(10,20,30,40)");
        assert_eq!(s("#123456789abc"), "srgb(7.1107%,33.7774%,60.444%)");
        assert_eq!(s("none"), "srgba(0,0,0,0)");
        assert_eq!(s("rgb(1.5,2,3)"), "srgb(0.588998%,0.784314%,1.17647%)");
        assert_eq!(s("hsla(120,100%,25%,0.3)"), "srgba(0%,50%,0%,0.300008)");
        assert_eq!(s("graya(50%,0.5)"), "graya(50.0008%,0.500008)");
        assert_eq!(s("cmyka(0,0,0,0,0.5)"), "cmyka(0,0,0,0,0.500008)");
        assert_eq!(s("#FFF8"), "srgba(255,255,255,0.533333)");
        assert_eq!(s("rgb(300,0,0)"), "srgb(255,0,0)");
        assert_eq!(s("cmyk(10%,20%,30%,40%)"), "cmyk(25.5019,51,76.5019,102)");
        assert_eq!(s(" RED "), "srgb(255,0,0)");
        assert_eq!(s("rgb(1 2 3)"), "srgb(1,2,3)");
        assert!(PixelInfo::parse("#12").is_none());
        assert!(PixelInfo::parse("nosuchcolor").is_none());
    }

    #[test]
    fn hsl() {
        let p = PixelInfo::parse("rgb(10,200,30)").unwrap();
        let (h, s, l) = p.to_hsl();
        assert!((h - 0.3508771929824561).abs() < 1e-9);
        assert!((s - 0.9047619047619049).abs() < 1e-9);
        assert!((l - 0.4117647058823529).abs() < 1e-9);
    }

    #[test]
    fn g() {
        assert_eq!(fmt_g(0.501960784), "0.501961");
        assert_eq!(fmt_g(10.000763), "10.0008");
        assert_eq!(fmt_g(1e-7), "1e-07");
        assert_eq!(fmt_g(1234567.0), "1.23457e+06");
        assert_eq!(fmt_g(100.0), "100");
    }
}
