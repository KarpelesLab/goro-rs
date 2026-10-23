//! Alpha compositing: the Porter-Duff operators and the blend modes of
//! the W3C "Compositing and Blending" specification, on straight
//! (non-premultiplied) alpha, as ImageMagick's compose methods name them.

use crate::color::rgb_to_hsl;
use crate::image::Image;
use crate::types::{Colorspace, CompositeOp as C};

/// Extra arguments of some operators ("compose:args").
#[derive(Clone, Copy, Debug, Default)]
pub struct ComposeArgs {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub set: bool,
}

impl ComposeArgs {
    pub fn parse(s: &str) -> ComposeArgs {
        let v: Vec<f64> = s
            .split(|c: char| c == ',' || c == 'x' || c.is_ascii_whitespace())
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.trim_end_matches('%').parse().ok())
            .collect();
        ComposeArgs {
            a: v.first().copied().unwrap_or(0.0),
            b: v.get(1).copied().unwrap_or(f64::NAN),
            c: v.get(2).copied().unwrap_or(0.0),
            d: v.get(3).copied().unwrap_or(0.0),
            set: !v.is_empty(),
        }
    }
}

/// Whether the operator affects the destination outside the source.
pub fn is_unbounded(op: C) -> bool {
    matches!(
        op,
        C::Clear | C::Src | C::In | C::SrcIn | C::Out | C::SrcOut | C::DstIn | C::DstAtop
    )
}

fn blend(op: C, s: f64, d: f64) -> f64 {
    match op {
        C::Multiply => s * d,
        C::Screen => s + d - s * d,
        C::Overlay => {
            if d <= 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }
        C::HardLight => {
            if s <= 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }
        C::SoftLight => {
            if s <= 0.5 {
                d - (1.0 - 2.0 * s) * d * (1.0 - d)
            } else {
                let g = if d <= 0.25 {
                    ((16.0 * d - 12.0) * d + 4.0) * d
                } else {
                    d.sqrt()
                };
                d + (2.0 * s - 1.0) * (g - d)
            }
        }
        C::PegtopLight => 2.0 * s * d + d * d * (1.0 - 2.0 * s),
        C::Darken => s.min(d),
        C::Lighten => s.max(d),
        C::ColorDodge => {
            if d <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (d / (1.0 - s)).min(1.0)
            }
        }
        C::ColorBurn => {
            if d >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - d) / s).min(1.0)
            }
        }
        C::Difference => (s - d).abs(),
        C::Exclusion => s + d - 2.0 * s * d,
        C::LinearDodge => (s + d).min(1.0),
        C::LinearBurn => (s + d - 1.0).max(0.0),
        C::LinearLight => (d + 2.0 * s - 1.0).clamp(0.0, 1.0),
        C::VividLight => {
            if s <= 0.5 {
                if s <= 0.0 {
                    0.0
                } else {
                    (1.0 - (1.0 - d) / (2.0 * s)).max(0.0)
                }
            } else if s >= 1.0 {
                1.0
            } else {
                (d / (2.0 * (1.0 - s))).min(1.0)
            }
        }
        C::PinLight => {
            if s <= 0.5 {
                d.min(2.0 * s)
            } else {
                d.max(2.0 * s - 1.0)
            }
        }
        C::HardMix => {
            if s + d >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        C::MinusDst => (d - s).max(0.0),
        C::MinusSrc => (s - d).max(0.0),
        C::DivideDst => {
            if s <= 0.0 {
                if d <= 0.0 { 0.0 } else { 1.0 }
            } else {
                (d / s).min(1.0)
            }
        }
        C::DivideSrc => {
            if d <= 0.0 {
                if s <= 0.0 { 0.0 } else { 1.0 }
            } else {
                (s / d).min(1.0)
            }
        }
        C::ModulusAdd => {
            let v = s + d;
            if v > 1.0 { v - 1.0 } else { v }
        }
        C::ModulusSubtract => {
            let v = d - s;
            if v < 0.0 { v + 1.0 } else { v }
        }
        C::Freeze => {
            if s <= 0.0 {
                0.0
            } else {
                (1.0 - (1.0 - d) * (1.0 - d) / s).max(0.0)
            }
        }
        C::Reflect => {
            if s >= 1.0 {
                1.0
            } else {
                (d * d / (1.0 - s)).min(1.0)
            }
        }
        C::Interpolate => {
            0.5 - 0.25 * (std::f64::consts::PI * s).cos() - 0.25 * (std::f64::consts::PI * d).cos()
        }
        C::Negate => 1.0 - (1.0 - s - d).abs(),
        C::SoftBurn => {
            if s + d < 1.0 {
                if s >= 1.0 {
                    1.0
                } else {
                    (0.5 * d / (1.0 - s)).min(1.0)
                }
            } else if d <= 0.0 {
                0.0
            } else {
                (1.0 - 0.5 * (1.0 - s) / d).max(0.0)
            }
        }
        C::SoftDodge => {
            if s + d < 1.0 {
                if s >= 1.0 {
                    1.0
                } else {
                    (0.5 * s / (1.0 - d)).min(1.0)
                }
            } else if s <= 0.0 {
                0.0
            } else {
                (1.0 - 0.5 * (1.0 - d) / s).max(0.0)
            }
        }
        C::Stamp => (s + d - 1.0).clamp(0.0, 1.0),
        _ => s,
    }
}

fn is_separable_blend(op: C) -> bool {
    matches!(
        op,
        C::Multiply
            | C::Screen
            | C::Overlay
            | C::HardLight
            | C::SoftLight
            | C::PegtopLight
            | C::Darken
            | C::Lighten
            | C::ColorDodge
            | C::ColorBurn
            | C::Difference
            | C::Exclusion
            | C::LinearDodge
            | C::LinearBurn
            | C::LinearLight
            | C::VividLight
            | C::PinLight
            | C::HardMix
            | C::MinusDst
            | C::MinusSrc
            | C::DivideDst
            | C::DivideSrc
            | C::ModulusAdd
            | C::ModulusSubtract
            | C::Freeze
            | C::Reflect
            | C::Interpolate
            | C::Negate
            | C::SoftBurn
            | C::SoftDodge
            | C::Stamp
    )
}

/// One pixel: `s` composed onto `d` (channels 0..1, straight alpha).
pub fn compose_pixel(op: C, s: [f64; 4], d: [f64; 4], args: &ComposeArgs) -> [f64; 4] {
    let (sa, da) = (s[3], d[3]);
    let over = |s: [f64; 4], d: [f64; 4]| -> [f64; 4] {
        let ao = s[3] + d[3] * (1.0 - s[3]);
        if ao <= 0.0 {
            return [0.0; 4];
        }
        let mut o = [0.0; 4];
        for c in 0..3 {
            o[c] = (s[c] * s[3] + d[c] * d[3] * (1.0 - s[3])) / ao;
        }
        o[3] = ao;
        o
    };
    match op {
        C::No => d,
        C::Over | C::SrcOver | C::Undefined => over(s, d),
        C::DstOver => over(d, s),
        C::Src | C::Copy | C::Replace => s,
        C::Dst => d,
        C::Clear => [0.0, 0.0, 0.0, 0.0],
        C::In | C::SrcIn => [s[0], s[1], s[2], sa * da],
        C::DstIn => [d[0], d[1], d[2], sa * da],
        C::Out | C::SrcOut => [s[0], s[1], s[2], sa * (1.0 - da)],
        C::DstOut => [d[0], d[1], d[2], da * (1.0 - sa)],
        C::Atop | C::SrcAtop => {
            if da <= 0.0 {
                return [0.0; 4];
            }
            let mut o = [0.0; 4];
            for c in 0..3 {
                o[c] = s[c] * sa + d[c] * (1.0 - sa);
            }
            o[3] = da;
            o
        }
        C::DstAtop => {
            if sa <= 0.0 {
                return [0.0; 4];
            }
            let mut o = [0.0; 4];
            for c in 0..3 {
                o[c] = d[c] * da + s[c] * (1.0 - da);
            }
            o[3] = sa;
            o
        }
        C::Xor => {
            let ao = sa + da - 2.0 * sa * da;
            if ao <= 0.0 {
                return [0.0; 4];
            }
            let mut o = [0.0; 4];
            for c in 0..3 {
                o[c] = (s[c] * sa * (1.0 - da) + d[c] * da * (1.0 - sa)) / ao;
            }
            o[3] = ao;
            o
        }
        C::Plus => {
            let ao = (sa + da).min(1.0);
            if ao <= 0.0 {
                return [0.0; 4];
            }
            let mut o = [0.0; 4];
            for c in 0..3 {
                o[c] = ((s[c] * sa + d[c] * da) / ao).min(1.0);
            }
            o[3] = ao;
            o
        }
        C::Dissolve => {
            let f = if args.set { args.a / 100.0 } else { 1.0 };
            let g = if args.set && !args.b.is_nan() {
                args.b / 100.0
            } else {
                1.0
            };
            let s2 = [s[0], s[1], s[2], sa * f];
            let d2 = [d[0], d[1], d[2], da * g];
            over(s2, d2)
        }
        C::Blend => {
            let f = if args.set { args.a / 100.0 } else { 1.0 };
            let g = if args.set && !args.b.is_nan() {
                args.b / 100.0
            } else {
                1.0 - f
            };
            let mut o = [0.0; 4];
            let ao = (sa * f + da * g).clamp(0.0, 1.0);
            for c in 0..3 {
                o[c] = if ao > 0.0 {
                    ((s[c] * sa * f + d[c] * da * g) / ao).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            o[3] = ao;
            o
        }
        C::CopyRed | C::CopyCyan => [s[0], d[1], d[2], da],
        C::CopyGreen | C::CopyMagenta => [d[0], s[1], d[2], da],
        C::CopyBlue | C::CopyYellow => [d[0], d[1], s[2], da],
        C::CopyAlpha => {
            // the source's alpha, or its intensity when it has none
            let a = if sa < 1.0 {
                sa
            } else {
                0.212656 * s[0] + 0.715158 * s[1] + 0.072186 * s[2]
            };
            [d[0], d[1], d[2], a]
        }
        C::CopyBlack => d,
        C::Bumpmap => {
            let i = 0.212656 * s[0] + 0.715158 * s[1] + 0.072186 * s[2];
            let mut o = d;
            for v in o.iter_mut().take(3) {
                *v *= i;
            }
            over([o[0], o[1], o[2], sa], d)
        }
        C::Hue | C::Saturate | C::Luminize | C::Colorize | C::Modulate => {
            let (sh, ss, sl) = rgb_to_hsl(s[0], s[1], s[2]);
            let (dh, ds, dl) = rgb_to_hsl(d[0], d[1], d[2]);
            let (h, sat, l) = match op {
                C::Hue => (sh, ds, dl),
                C::Saturate => (dh, ss, dl),
                C::Luminize => (dh, ds, sl),
                C::Colorize => (sh, ss, dl),
                _ => {
                    let percent_b = if args.set { args.a } else { 100.0 };
                    let percent_s = if args.set && !args.b.is_nan() {
                        args.b
                    } else {
                        100.0
                    };
                    let i = 0.212656 * s[0] + 0.715158 * s[1] + 0.072186 * s[2];
                    let off = i - 0.5;
                    (
                        dh,
                        (ds * (1.0 + off * percent_s / 100.0 * 2.0)).clamp(0.0, 1.0),
                        (dl + off * percent_b / 100.0).clamp(0.0, 1.0),
                    )
                }
            };
            let (r, g, b) = crate::color::hsl_to_rgb(h, sat, l);
            let blended = [r, g, b, sa];
            if op == C::Modulate {
                return [r, g, b, da];
            }
            composite_blended(blended, s, d)
        }
        C::Threshold => {
            let t = if args.set && !args.b.is_nan() {
                args.b / 100.0
            } else {
                0.0
            };
            let amt = if args.set { args.a } else { 1.0 };
            let mut o = d;
            for c in 0..3 {
                let delta = s[c] - d[c];
                if delta.abs() >= t {
                    o[c] = (d[c] + amt * delta).clamp(0.0, 1.0);
                }
            }
            o
        }
        C::Mathematics => {
            let (a, b, cc, dd) = if args.set {
                (
                    args.a,
                    if args.b.is_nan() { 0.0 } else { args.b },
                    args.c,
                    args.d,
                )
            } else {
                (0.0, 1.0, 1.0, 0.0)
            };
            let mut o = [0.0; 4];
            for c in 0..3 {
                o[c] = (a * s[c] * d[c] + b * s[c] + cc * d[c] + dd).clamp(0.0, 1.0);
            }
            o[3] = (sa + da - sa * da).clamp(0.0, 1.0);
            o
        }
        _ if is_separable_blend(op) => {
            let mut b = [0.0; 4];
            for c in 0..3 {
                b[c] = blend(op, s[c], d[c]);
            }
            composite_blended(b, s, d)
        }
        _ => over(s, d),
    }
}

/// co = Sa·Da·B + Sa·(1−Da)·Sc + (1−Sa)·Da·Dc ; ao = Sa + Da − Sa·Da
fn composite_blended(b: [f64; 4], s: [f64; 4], d: [f64; 4]) -> [f64; 4] {
    let (sa, da) = (s[3], d[3]);
    let ao = sa + da - sa * da;
    if ao <= 0.0 {
        return [0.0; 4];
    }
    let mut o = [0.0; 4];
    for c in 0..3 {
        o[c] = ((sa * da * b[c] + sa * (1.0 - da) * s[c] + (1.0 - sa) * da * d[c]) / ao)
            .clamp(0.0, 1.0);
    }
    o[3] = ao;
    o
}

fn to_f(p: [u16; 4], alpha: bool) -> [f64; 4] {
    [
        p[0] as f64 / 65535.0,
        p[1] as f64 / 65535.0,
        p[2] as f64 / 65535.0,
        if alpha { p[3] as f64 / 65535.0 } else { 1.0 },
    ]
}

fn from_f(p: [f64; 4]) -> [u16; 4] {
    let q = |v: f64| crate::color::to_q16(v * 65535.0);
    [q(p[0]), q(p[1]), q(p[2]), q(p[3])]
}

/// CompositeImage(): compose `src` onto `dst` at (x, y).
pub fn composite_image(dst: &mut Image, src: &Image, op: C, x: i64, y: i64, args: &ComposeArgs) {
    let mut src_owned;
    let mut src = src;
    if src.colorspace == Colorspace::CMYK && dst.colorspace != Colorspace::CMYK {
        src_owned = src.clone();
        src_owned.cmyk_to_rgb();
        src = &src_owned;
    }
    if dst.colorspace == Colorspace::CMYK && src.colorspace != Colorspace::CMYK {
        dst.cmyk_to_rgb();
    }
    // operators whose result can be transparent on an opaque destination
    let alpha_ops = matches!(
        op,
        C::Clear
            | C::In
            | C::SrcIn
            | C::Out
            | C::SrcOut
            | C::DstIn
            | C::DstOut
            | C::Xor
            | C::CopyAlpha
    ) || (src.alpha
        && matches!(
            op,
            C::Src | C::Copy | C::Replace | C::DstAtop | C::Dissolve | C::Blend
        ));
    if !dst.alpha && alpha_ops {
        dst.enable_alpha();
    }
    if op == C::Dst || op == C::No {
        return;
    }
    let dst_alpha = dst.alpha;
    let unbounded = is_unbounded(op);
    let (dw, dh) = (dst.columns as i64, dst.rows as i64);
    let transparent = [0.0, 0.0, 0.0, 0.0];
    for dy in 0..dh {
        let sy = dy - y;
        let row_in = sy >= 0 && sy < src.rows as i64;
        if !row_in && !unbounded {
            continue;
        }
        for dx in 0..dw {
            let sx = dx - x;
            let inside = row_in && sx >= 0 && sx < src.columns as i64;
            if !inside && !unbounded {
                continue;
            }
            let s = if inside {
                to_f(src.px(sx as usize, sy as usize), src.alpha)
            } else {
                transparent
            };
            let d = to_f(dst.px(dx as usize, dy as usize), dst_alpha);
            let mut o = compose_pixel(op, s, d, args);
            if !dst.alpha {
                o[3] = 1.0;
            }
            dst.set_px(dx as usize, dy as usize, from_f(o));
        }
    }
}

/// Composite the image over its background colour and drop alpha
/// (what an alpha-less format gets).
pub fn flatten_alpha(img: &mut Image) {
    if !img.alpha {
        return;
    }
    let bg = img.background_color.to_rgba16();
    let bgf = to_f(bg, true);
    for p in img.pixels.chunks_exact_mut(4) {
        let s = to_f([p[0], p[1], p[2], p[3]], true);
        let o = compose_pixel(
            C::Over,
            s,
            [bgf[0], bgf[1], bgf[2], 1.0],
            &ComposeArgs::default(),
        );
        let q = from_f(o);
        p[0] = q[0];
        p[1] = q[1];
        p[2] = q[2];
        p[3] = 65535;
    }
    img.alpha = false;
}
