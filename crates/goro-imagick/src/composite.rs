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
    /// whether the arguments were given as percentages
    pub percent: bool,
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
            percent: s.contains('%'),
        }
    }
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
            if s >= 1.0 {
                1.0
            } else {
                sdiv(d, 1.0 - s, 1.0).min(1.0)
            }
        }
        C::ColorBurn => {
            if s <= 0.0 {
                0.0
            } else {
                1.0 - sdiv(1.0 - d, s, 1.0).min(1.0)
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

/// `a / b`, or `z` when `b` is zero.
fn sdiv(a: f64, b: f64, z: f64) -> f64 {
    if b.abs() > 1e-12 { a / b } else { z }
}

/// `v` modulo 1, robust to the rounding of sums that land on a whole
/// number (ImageMagick works in quantum units).
fn wrap(v: f64) -> f64 {
    let r = v - v.floor();
    if (1e-9..=1.0 - 1e-9).contains(&r) {
        r
    } else {
        0.0
    }
}

/// The Rec. 709 luma of straight samples.
fn intensity(c: [f64; 4]) -> f64 {
    0.212656 * c[0] + 0.715158 * c[1] + 0.072186 * c[2]
}

/// Vivid light on [0, 1] samples, as HardMix thresholds it.
fn vivid(s: f64, d: f64) -> f64 {
    if s < 0.5 {
        if s <= 0.0 {
            0.0
        } else {
            1.0 - (1.0 - d) / (2.0 * s)
        }
    } else if s >= 1.0 {
        1.0
    } else {
        d / (2.0 * (1.0 - s))
    }
}

fn soft_light(s: f64, d: f64) -> f64 {
    if 2.0 * s < 1.0 {
        d - (1.0 - 2.0 * s) * d * (1.0 - d)
    } else {
        let g = if 4.0 * d <= 1.0 {
            ((16.0 * d - 12.0) * d + 4.0) * d
        } else {
            d.max(0.0).sqrt()
        };
        d + (2.0 * s - 1.0) * (g - d)
    }
}

/// The hue / chroma / luma swaps of Hue, Saturate, Luminize and Colorize,
/// in ImageMagick's HCL.
fn hcl_swap(op: C, s: [f64; 4], d: [f64; 4]) -> [f64; 3] {
    let to =
        |c: [f64; 4]| crate::colorspace::from_srgb(Colorspace::HCL, [c[0], c[1], c[2]]).unwrap();
    let (hs, hd) = (to(s), to(d));
    let h = match op {
        C::Hue => [hs[0], hd[1], hd[2]],
        C::Saturate => [hd[0], hs[1], hd[2]],
        C::Luminize => [hd[0], hd[1], hs[2]],
        _ => [hs[0], hs[1], hd[2]],
    };
    crate::colorspace::to_srgb(Colorspace::HCL, h).unwrap()
}

/// One pixel: `s` composed onto `d` (straight samples in 0..1; the
/// destination alpha is 1 when it has none), as ImageMagick 7.1.2 does
/// it. Established black-box against pecl imagick: many methods leave
/// the colour premultiplied (not divided by the result alpha), several
/// work on premultiplied inputs, and a few treat the alpha channel with
/// the blend itself.
pub fn compose_pixel(op: C, s: [f64; 4], d: [f64; 4], args: &ComposeArgs) -> [f64; 4] {
    let (sa, da) = (s[3], d[3]);
    // a fully transparent pixel's colour reads as black
    let sz = if sa > 0.0 { s } else { [0.0, 0.0, 0.0, 0.0] };
    let dz = if da > 0.0 { d } else { [0.0, 0.0, 0.0, 0.0] };
    let ao = sa + da - sa * da;
    let sca = |c: usize| sz[c] * sa;
    let dca = |c: usize| dz[c] * da;
    let each = |f: &dyn Fn(usize) -> f64, a: f64| -> [f64; 4] { [f(0), f(1), f(2), a] };
    // Sa·Da·B + Sa·(1−Da)·Sc + (1−Sa)·Da·Dc, the W3C mix (premultiplied)
    let w = |c: usize, b: f64| sa * da * b + sa * (1.0 - da) * sz[c] + (1.0 - sa) * da * dz[c];
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
    // Sa = 0 keeps the destination, Da = 0 takes the source, else `b`;
    // the alpha is the larger one
    let pick = |b: [f64; 3]| -> [f64; 4] {
        let c = if sa <= 0.0 {
            [d[0], d[1], d[2]]
        } else if da <= 0.0 {
            [sz[0], sz[1], sz[2]]
        } else {
            b
        };
        [c[0], c[1], c[2], sa.max(da)]
    };
    match op {
        C::Undefined => [s[0], s[1], s[2], 1.0],
        C::Over | C::SrcOver => over(s, d),
        C::DstOver => over(d, s),
        C::Src | C::Copy => s,
        C::Replace => each(&sca, sa),
        C::Dst => d,
        C::No => each(&dca, da),
        C::Clear => [0.0; 4],
        C::In | C::SrcIn => each(&|c| sca(c) * da, sa * da),
        C::Out | C::SrcOut => each(&|c| sca(c) * (1.0 - da), sa * (1.0 - da)),
        C::DstIn => each(&|c| sdiv(dca(c) * sa, sa * da, 0.0), sa * da),
        C::DstOut => each(
            &|c| sdiv(dca(c) * (1.0 - sa), da * (1.0 - sa), 0.0),
            da * (1.0 - sa),
        ),
        C::Atop | C::SrcAtop => each(&|c| sca(c) * da + dca(c) * (1.0 - sa), da),
        C::DstAtop => each(&|c| dca(c) * sa + sca(c) * (1.0 - da), sa),
        C::Xor => each(
            &|c| sca(c) * (1.0 - da) + dca(c) * (1.0 - sa),
            sa + da - 2.0 * sa * da,
        ),
        C::Plus => each(
            &|c| (sz[c] + dz[c]).clamp(0.0, 1.0) * ao,
            (sa + da).min(1.0),
        ),
        C::Multiply => each(&|c| w(c, sz[c] * dz[c]), ao),
        C::Lighten => each(&|c| w(c, sz[c].max(dz[c])), ao),
        C::DivideDst => each(&|c| w(c, sdiv(dz[c], sz[c], 1.0).clamp(0.0, 1.0)), ao),
        C::DivideSrc => each(&|c| w(c, sdiv(sz[c], dz[c], 1.0).clamp(0.0, 1.0)), ao),
        C::LinearBurn => each(&|c| w(c, sz[c] + dz[c] - 1.0), ao),
        C::Darken => each(
            &|c| sca(c).min(dca(c)) + sca(c) * (1.0 - da) + dca(c) * (1.0 - sa),
            ao,
        ),
        C::Screen => each(
            &|c| (sz[c] + dz[c] - sz[c] * dz[c]).clamp(0.0, 1.0) * ao,
            ao,
        ),
        C::Exclusion => each(
            &|c| (sca(c) + dz[c] - 2.0 * sca(c) * dz[c]).clamp(0.0, 1.0) * ao,
            ao,
        ),
        C::Difference => each(&|c| (sz[c] - dz[c]).abs(), (sa - da).abs()),
        C::MinusDst => each(&|c| sdiv(w(c, sz[c] - dz[c]), ao, 0.0), ao),
        C::MinusSrc => each(&|c| sdiv(w(c, dz[c] - sz[c]), ao, 0.0), ao),
        C::LinearDodge => each(&|c| sdiv(sca(c) + dca(c), ao, 0.0), ao),
        C::LinearLight => each(
            &|c| (dz[c] + 2.0 * sz[c] - 1.0).clamp(0.0, 1.0) * sdiv(da, ao, 0.0),
            ao,
        ),
        C::PinLight => each(
            &|c| {
                if 2.0 * sz[c] < 1.0 {
                    dz[c].min(2.0 * sz[c])
                } else {
                    dz[c].max(2.0 * sz[c] - 1.0)
                }
            },
            ao,
        ),
        C::HardMix => each(
            &|c| {
                if vivid(sca(c), dca(c)) < 0.5 {
                    0.0
                } else {
                    1.0
                }
            },
            ao,
        ),
        C::ModulusAdd => {
            let a = sa + da;
            each(
                &|c| wrap(sca(c) + dca(c)),
                if a > 1.0 { a - 1.0 } else { a },
            )
        }
        C::ModulusSubtract => {
            let a = sa - da;
            each(
                &|c| {
                    let v = sca(c) - dca(c);
                    wrap(v)
                },
                if a < 0.0 { a + 1.0 } else { a },
            )
        }
        C::Interpolate => {
            let f = |s: f64, d: f64| {
                0.5 - 0.25 * (std::f64::consts::PI * s).cos()
                    - 0.25 * (std::f64::consts::PI * d).cos()
            };
            each(&|c| f(sca(c), dca(c)), f(sa, da))
        }
        C::Negate => each(&|c| (1.0 - (1.0 - sz[c] - dz[c]).abs()) * da, 1.0 - sa - da),
        C::Stamp => each(&|c| sca(c) + dca(c) - 1.0, sa + da * da - 1.0),
        C::Freeze => {
            let f = |x: f64, y: f64| {
                if x > 0.0 {
                    1.0 - (1.0 - y) * (1.0 - y) / x
                } else if y >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            };
            // the colour (over the result alpha) is 0 where Dca is 0
            let fc = |x: f64, y: f64| if x > 0.0 { f(x, y) } else { 0.0 };
            each(&|c| sdiv(fc(dca(c), sca(c)), ao, 0.0), f(da, sa))
        }
        C::Reflect => {
            let f = |x: f64, y: f64| {
                if y < 1.0 {
                    x * x / (1.0 - y)
                } else if x > 0.0 {
                    1.0
                } else {
                    0.0
                }
            };
            // a full destination channel reflects to 1; the colour is
            // scaled by the result alpha
            let fc = |x: f64, y: f64| if y < 1.0 { x * x / (1.0 - y) } else { 1.0 };
            let k = ao;
            each(&|c| fc(sz[c], dz[c]) * k, f(sa, da))
        }
        // ImageMagick's SoftLight: the W3C soft light of the premultiplied
        // samples weighted by Da, plus Sca(1-Da) + Dca, over the result alpha
        C::SoftLight => each(
            &|c| {
                sdiv(
                    soft_light(sca(c), dca(c)) * da + sca(c) * (1.0 - da) + dca(c),
                    ao,
                    0.0,
                )
            },
            ao,
        ),
        // its PegtopLight is the W3C soft light of the premultiplied
        // samples, mixed the W3C way
        C::PegtopLight => each(&|c| sdiv(w(c, soft_light(sca(c), dca(c))), ao, 0.0), ao),
        // colour burn / dodge of the premultiplied samples, over the
        // result alpha; a full source gives 1 unless the destination is 0
        C::SoftBurn | C::SoftDodge => each(
            &|c| {
                let (s, d) = (sca(c), dca(c));
                let b = if s >= 1.0 {
                    if d > 0.0 { 1.0 } else { 0.0 }
                } else if op == C::SoftBurn {
                    if s <= 0.0 { 0.0 } else { 1.0 - (1.0 - d) / s }
                } else {
                    d / (1.0 - s)
                };
                sdiv(b.clamp(0.0, 1.0), ao, 0.0)
            },
            ao,
        ),
        C::Bumpmap => {
            let i = intensity(s);
            let c = |k: usize| if sa > 0.0 { i * d[k] } else { d[k] };
            [c(0), c(1), c(2), da * i]
        }
        C::Hue | C::Saturate | C::Luminize | C::Colorize => pick(hcl_swap(op, s, d)),
        C::RMSE => {
            let (r, g, b) = (s[0] - d[0], s[1] - d[1], s[2] - d[2]);
            let v = (r * r + g * g + b * b / 3.0).sqrt();
            pick([v, v, v])
        }
        C::LightenIntensity => {
            let p = if intensity(s) > intensity(d) { s } else { d };
            [p[0], p[1], p[2], 0.0]
        }
        C::DarkenIntensity => {
            let p = if intensity(s) > 0.0 { [0.0; 4] } else { d };
            [p[0], p[1], p[2], 0.0]
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
            // source and destination weights: both 1 without arguments,
            // 1 - src when only the source percentage is given
            let (f, g) = if !args.set {
                (1.0, 1.0)
            } else if args.b.is_nan() {
                (args.a / 100.0, 1.0 - args.a / 100.0)
            } else {
                (args.a / 100.0, args.b / 100.0)
            };
            let a = (f * sa + g * da).clamp(0.0, 1.0);
            each(&|c| sdiv(f * sca(c) + g * dca(c), a, 0.0), a)
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
                (0.0, 0.0, 0.0, 0.0)
            };
            each(
                &|c| {
                    let f = a * sz[c] * dz[c] + b * sz[c] + cc * dz[c] + dd;
                    sdiv(w(c, f), ao, 0.0)
                },
                ao,
            )
        }
        C::CopyRed | C::CopyCyan => [s[0], d[1], d[2], da],
        C::CopyGreen | C::CopyMagenta => [d[0], s[1], d[2], da],
        C::CopyBlue | C::CopyYellow => [d[0], d[1], s[2], da],
        // the source's intensity, whatever its alpha
        C::CopyAlpha => [d[0], d[1], d[2], intensity(s)],
        C::CopyBlack => d,
        C::Stereo => [s[0], d[1], d[2], (sa + da) / 2.0],
        C::Threshold if !args.set => each(&|c| dca(c) + sca(c) * (1.0 - da), 1.0),
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
        C::Modulate => {
            let (_, ss, sl) = rgb_to_hsl(s[0], s[1], s[2]);
            let _ = (ss, sl);
            let (dh, ds, dl) = rgb_to_hsl(d[0], d[1], d[2]);
            let percent_b = if args.set { args.a } else { 100.0 };
            let percent_s = if args.set && !args.b.is_nan() {
                args.b
            } else {
                100.0
            };
            let i = intensity(s);
            let off = i - 0.5;
            let (r, g, b) = crate::color::hsl_to_rgb(
                dh,
                (ds * (1.0 + off * percent_s / 100.0 * 2.0)).clamp(0.0, 1.0),
                (dl + off * percent_b / 100.0).clamp(0.0, 1.0),
            );
            [r, g, b, da]
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
    composite_image_channel(dst, src, op, x, y, args, crate::types::channel::DEFAULT);
}

/// CompositeImage() with a channel mask: channels outside the mask keep
/// the destination, except that Over fills unselected colour channels
/// from the source and always composes the alpha (as ImageMagick 7.1.2
/// does).
pub fn composite_image_channel(
    dst: &mut Image,
    src: &Image,
    op: C,
    x: i64,
    y: i64,
    args: &ComposeArgs,
    ch: i64,
) {
    let all = ch == crate::types::channel::DEFAULT || ch == 0 || ch == 0x7fffffff;
    let sel = [
        all || ch & crate::types::channel::RED != 0,
        all || ch & crate::types::channel::GREEN != 0,
        all || ch & crate::types::channel::BLUE != 0,
        all || ch & crate::types::channel::ALPHA != 0,
    ];
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
    // ImageMagick never gives the destination an alpha channel, except
    // for the methods that set it
    if !dst.alpha && matches!(op, C::CopyAlpha | C::ChangeMask) {
        dst.enable_alpha();
    }
    if op == C::Dst {
        return;
    }
    // Copy takes a straight fast path only when the source fits inside the
    // destination; otherwise it composes like Replace (premultiplied)
    let op = if op == C::Copy
        && (x < 0
            || y < 0
            || x + src.columns as i64 > dst.columns as i64
            || y + src.rows as i64 > dst.rows as i64)
    {
        C::Replace
    } else {
        op
    };
    if matches!(op, C::Displace | C::Distort) {
        displace(dst, src, x, y, args, op == C::Distort);
        return;
    }
    let dst_alpha = dst.alpha;
    // only the area under the source changes, whatever the method
    let (dw, dh) = (dst.columns as i64, dst.rows as i64);
    let (x0, x1) = (x.max(0), (x + src.columns as i64).min(dw));
    let (y0, y1) = (y.max(0), (y + src.rows as i64).min(dh));
    for dy in y0..y1 {
        let sy = dy - y;
        for dx in x0..x1 {
            let sx = dx - x;
            let s = to_f(src.px(sx as usize, sy as usize), src.alpha);
            let d = to_f(dst.px(dx as usize, dy as usize), dst_alpha);
            let mut o = if op == C::ChangeMask {
                // matching pixels turn transparent, the rest keep the
                // destination
                let dist: f64 = (0..4).map(|c| (s[c] - d[c]) * (s[c] - d[c])).sum();
                let fuzz = dst.fuzz / 65535.0;
                if dist <= fuzz * fuzz {
                    [d[0], d[1], d[2], 0.0]
                } else {
                    d
                }
            } else {
                compose_pixel(op, s, d, args)
            };
            if !all && op != C::Copy {
                let over = matches!(op, C::Over | C::SrcOver);
                for c in 0..4 {
                    if !sel[c] {
                        o[c] = if over {
                            if c == 3 { o[3] } else { s[c] }
                        } else {
                            d[c]
                        };
                    }
                }
            }
            if !dst.alpha {
                o[3] = 1.0;
            }
            dst.set_px(dx as usize, dy as usize, from_f(o));
        }
    }
}

/// The Displace and Distort methods: the source is a displacement map
/// whose red and green move the sampling point by (v - 1/2)*2*scale; the
/// scale defaults to half the map's size less one (a percentage of it
/// when the arguments carry %). Displace offsets each pixel's own
/// position, Distort the map's centre. The destination is resampled
/// bilinearly at the displaced point.
fn displace(dst: &mut Image, map: &Image, x0: i64, y0: i64, args: &ComposeArgs, distort: bool) {
    let (mw, mh) = (map.columns as f64, map.rows as f64);
    let (hx, hy) = ((mw - 1.0) / 2.0, (mh - 1.0) / 2.0);
    let (mut sx, mut sy) = (hx, hy);
    if args.set {
        let a = args.a;
        let b = if args.b.is_nan() { a } else { args.b };
        if args.percent {
            sx = a / 100.0 * hx;
            sy = b / 100.0 * hy;
        } else {
            sx = a;
            sy = b;
        }
    }
    let mut orig = dst.clone();
    if !orig.alpha {
        for p in orig.pixels.chunks_exact_mut(4) {
            p[3] = 65535;
        }
    }
    let (dw, dh) = (dst.columns as i64, dst.rows as i64);
    for my in 0..map.rows as i64 {
        let ty = my + y0;
        if ty < 0 || ty >= dh {
            continue;
        }
        for mx in 0..map.columns as i64 {
            let tx = mx + x0;
            if tx < 0 || tx >= dw {
                continue;
            }
            let m = map.px(mx as usize, my as usize);
            let dx = (m[0] as f64 / 65535.0 - 0.5) * 2.0 * sx;
            let dy = (m[1] as f64 / 65535.0 - 0.5) * 2.0 * sy;
            let (px, py) = if distort {
                (x0 as f64 + hx + dx, y0 as f64 + hy + dy)
            } else {
                (tx as f64 + dx, ty as f64 + dy)
            };
            let mut v = orig.sample_bilinear(px + 0.5, py + 0.5);
            // the map's alpha scales the result's
            if map.alpha {
                v[3] *= m[3] as f64 / 65535.0;
            }
            let q = crate::color::to_q16;
            dst.set_px(
                tx as usize,
                ty as usize,
                [q(v[0]), q(v[1]), q(v[2]), q(v[3])],
            );
        }
    }
    dst.colors = None;
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
