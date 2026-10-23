//! Colourspace conversions (TransformImageColorspace()), sample by
//! sample, between sRGB and every other colourspace, with the encodings
//! ImageMagick 7 stores the channels in (all in [0, 1] here; a Q16 image
//! clamps them to the quantum range).
//!
//! The constants were established against `magick -colorspace X` (black
//! box): RGB → XYZ is the inverse of the IEC 61966-2-1 four-decimal
//! matrix, XYZ → RGB the full-precision one; Lab, Luv and their LCH forms
//! use the D65 white (0.95047, 1, 1.08883) and unrounded a/b; Lab / LCH store
//! L/100, a/255+½, b/255+½, C/255+½, H/360; Luv stores (u+134)/354 and
//! (v+140)/262; the luma-based spaces use (0.298839, 0.586811, 0.114350)
//! with chroma offset by ½; Oklch stores its hue in radians; Jzazbz uses
//! a white luminance of 1 cd/m²; Display P3, Adobe98 and ProPhoto encode
//! with the sRGB transfer; PhotoYCC crushes samples below 0.018 and
//! inverts with Kodak's matrix scaled by 1/1.3555.

use crate::image::Image;
use crate::types::Colorspace;

type V3 = [f64; 3];

/// RGB → XYZ is the inverse of the four-decimal matrix ...
const XYZ_TO_RGB: [V3; 3] = [
    [3.2406, -1.5372, -0.4986],
    [-0.9689, 1.8758, 0.0415],
    [0.0557, -0.2040, 1.0570],
];

/// ... while XYZ → RGB uses the full-precision IEC 61966-2-1 matrix.
const XYZ_TO_RGB_FULL: [V3; 3] = [
    [3.2409699419, -1.5373831776, -0.4986107603],
    [-0.9692436363, 1.8759675015, 0.0415550574],
    [0.0556300797, -0.2039769589, 1.0569715142],
];

fn mul(m: &[V3; 3], v: V3) -> V3 {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn inv(m: &[V3; 3]) -> [V3; 3] {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [
            (e * i - f * h) / det,
            (c * h - b * i) / det,
            (b * f - c * e) / det,
        ],
        [
            (f * g - d * i) / det,
            (a * i - c * g) / det,
            (c * d - a * f) / det,
        ],
        [
            (d * h - e * g) / det,
            (b * g - a * h) / det,
            (a * e - b * d) / det,
        ],
    ]
}

fn rgb_to_xyz_m() -> [V3; 3] {
    inv(&XYZ_TO_RGB)
}

/// The D65 reference white Lab, Luv and their LCH forms use.
fn white() -> V3 {
    [0.95047, 1.0, 1.08883]
}

/// sRGB transfer: encoded → linear.
pub fn decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB transfer: linear → encoded.
pub fn encode(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn lin(c: V3) -> V3 {
    [decode(c[0]), decode(c[1]), decode(c[2])]
}

fn unlin(c: V3) -> V3 {
    [encode(c[0]), encode(c[1]), encode(c[2])]
}

fn to_xyz(c: V3) -> V3 {
    mul(&rgb_to_xyz_m(), lin(c))
}

fn from_xyz(x: V3) -> V3 {
    unlin(mul(&XYZ_TO_RGB_FULL, x))
}

const LAB_E: f64 = 216.0 / 24389.0;
const LAB_K: f64 = 24389.0 / 27.0;

fn lab_f(t: f64) -> f64 {
    if t > LAB_E {
        t.cbrt()
    } else {
        (LAB_K * t + 16.0) / 116.0
    }
}

fn lab_finv(f: f64) -> f64 {
    let t = f * f * f;
    if t > LAB_E {
        t
    } else {
        (116.0 * f - 16.0) / LAB_K
    }
}

fn xyz_to_lab(x: V3) -> V3 {
    let w = white();
    let (fx, fy, fz) = (lab_f(x[0] / w[0]), lab_f(x[1] / w[1]), lab_f(x[2] / w[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn lab_to_xyz(l: V3) -> V3 {
    let w = white();
    let fy = (l[0] + 16.0) / 116.0;
    let fx = fy + l[1] / 500.0;
    let fz = fy - l[2] / 200.0;
    let y = if l[0] > LAB_K * LAB_E {
        fy * fy * fy
    } else {
        l[0] / LAB_K
    };
    [lab_finv(fx) * w[0], y * w[1], lab_finv(fz) * w[2]]
}

fn xyz_to_luv(x: V3) -> V3 {
    let w = white();
    let den = x[0] + 15.0 * x[1] + 3.0 * x[2];
    let (up, vp) = if den != 0.0 {
        (4.0 * x[0] / den, 9.0 * x[1] / den)
    } else {
        (0.0, 0.0)
    };
    let dn = w[0] + 15.0 * w[1] + 3.0 * w[2];
    let (ur, vr) = (4.0 * w[0] / dn, 9.0 * w[1] / dn);
    let yr = x[1] / w[1];
    let l = if yr > LAB_E {
        116.0 * yr.cbrt() - 16.0
    } else {
        LAB_K * yr
    };
    [l, 13.0 * l * (up - ur), 13.0 * l * (vp - vr)]
}

fn luv_to_xyz(l: V3) -> V3 {
    let w = white();
    if l[0] <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let dn = w[0] + 15.0 * w[1] + 3.0 * w[2];
    let (ur, vr) = (4.0 * w[0] / dn, 9.0 * w[1] / dn);
    let y = if l[0] > LAB_K * LAB_E {
        ((l[0] + 16.0) / 116.0).powi(3)
    } else {
        l[0] / LAB_K
    } * w[1];
    let up = l[1] / (13.0 * l[0]) + ur;
    let vp = l[2] / (13.0 * l[0]) + vr;
    if vp == 0.0 {
        return [0.0, y, 0.0];
    }
    let x = y * 9.0 * up / (4.0 * vp);
    let z = y * (12.0 - 3.0 * up - 20.0 * vp) / (4.0 * vp);
    [x, y, z]
}

/// The hue (in turns) of the hexcone models.
fn hue(c: V3) -> f64 {
    let [r, g, b] = c;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    if d == 0.0 {
        return 0.0;
    }
    let h = if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h / 6.0
}

/// RGB of a hue (turns), chroma and the minimum component.
fn from_hue(h: f64, c: f64, m: f64) -> V3 {
    let hp = (h.rem_euclid(1.0)) * 6.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m]
}

const LUMA: V3 = [0.298839, 0.586811, 0.114350];

fn luma(c: V3) -> f64 {
    LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2]
}

/// The linear luma/chroma spaces: rows, and the offsets added.
fn linear_space(cs: Colorspace) -> Option<([V3; 3], V3)> {
    let y = LUMA;
    Some(match cs {
        Colorspace::YIQ => (
            [
                y,
                [0.595716, -0.274453, -0.321263],
                [0.211456, -0.522591, 0.311135],
            ],
            [0.0, 0.5, 0.5],
        ),
        Colorspace::YUV => (
            [y, [-0.147, -0.289, 0.436], [0.615, -0.515, -0.100]],
            [0.0, 0.5, 0.5],
        ),
        Colorspace::YCbCr | Colorspace::YPbPr | Colorspace::Rec601YCbCr => (
            [y, [-0.1687367, -0.331264, 0.5], [0.5, -0.418688, -0.081312]],
            [0.0, 0.5, 0.5],
        ),
        Colorspace::Rec709YCbCr => (
            [
                [0.212656, 0.715158, 0.072186],
                [-0.114572, -0.385428, 0.5],
                [0.5, -0.454153, -0.045847],
            ],
            [0.0, 0.5, 0.5],
        ),
        Colorspace::YDbDr => (
            [y, [-0.450, -0.883, 1.333], [-1.333, 1.116, 0.217]],
            [0.0, 0.5, 0.5],
        ),
        Colorspace::OHTA => (
            [
                [1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
                [0.5, 0.0, -0.5],
                [-0.25, 0.5, -0.25],
            ],
            [0.0, 0.5, 0.5],
        ),
        _ => return None,
    })
}

const LMS_M: [V3; 3] = [
    [0.7328, 0.4296, -0.1624],
    [-0.7036, 1.6975, 0.0061],
    [0.0030, 0.0136, 0.9834],
];

const P3_M: [V3; 3] = [
    [2.4934969, -0.9313836, -0.4027108],
    [-0.8294890, 1.7626641, 0.0236247],
    [0.0358458, -0.0761724, 0.9568845],
];

const ADOBE_M: [V3; 3] = [
    [2.0413690, -0.5649464, -0.3446944],
    [-0.9692660, 1.8760108, 0.0415560],
    [0.0134474, -0.1183897, 1.0154096],
];

const PROPHOTO_M: [V3; 3] = [
    [1.3459433, -0.2556075, -0.0511118],
    [-0.5445989, 1.5081673, 0.0205351],
    [0.0, 0.0, 1.2118128],
];

fn oklab(c: V3) -> V3 {
    let [r, g, b] = lin(c);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

fn oklab_inv(o: V3) -> V3 {
    let l = o[0] + 0.3963377774 * o[1] + 0.2158037573 * o[2];
    let m = o[0] - 0.1055613458 * o[1] - 0.0638541728 * o[2];
    let s = o[0] - 0.0894841775 * o[1] - 1.2914855480 * o[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    unlin([
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ])
}

// Jzazbz (Safdar et al. 2017), absolute XYZ at 1 cd/m².
const JZ_B: f64 = 1.15;
const JZ_G: f64 = 0.66;
const JZ_D: f64 = -0.56;
const JZ_D0: f64 = 1.629_549_953_282_156_6e-11;
const JZ_LMS: [V3; 3] = [
    [0.41478972, 0.579999, 0.0146480],
    [-0.2015100, 1.120649, 0.0531008],
    [-0.0166008, 0.264800, 0.6684799],
];
const JZ_IAB: [V3; 3] = [
    [0.5, 0.5, 0.0],
    [3.524000, -4.066708, 0.542708],
    [0.199076, 1.096799, -1.295875],
];

fn pq(x: f64) -> f64 {
    let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
    let (n, p) = (2610.0 / 16384.0, 1.7 * 2523.0 / 32.0);
    let xn = (x.max(0.0) / 10000.0).powf(n);
    ((c1 + c2 * xn) / (1.0 + c3 * xn)).powf(p)
}

fn pq_inv(v: f64) -> f64 {
    let (c1, c2, c3) = (3424.0 / 4096.0, 2413.0 / 128.0, 2392.0 / 128.0);
    let (n, p) = (2610.0 / 16384.0, 1.7 * 2523.0 / 32.0);
    let vp = v.max(0.0).powf(1.0 / p);
    10000.0 * ((c1 - vp) / (c3 * vp - c2)).max(0.0).powf(1.0 / n)
}

fn jzazbz(c: V3) -> V3 {
    let x = to_xyz(c);
    let xp = JZ_B * x[0] - (JZ_B - 1.0) * x[2];
    let yp = JZ_G * x[1] - (JZ_G - 1.0) * x[0];
    let lms = mul(&JZ_LMS, [xp, yp, x[2]]);
    let p = [pq(lms[0]), pq(lms[1]), pq(lms[2])];
    let iab = mul(&JZ_IAB, p);
    let jz = (1.0 + JZ_D) * iab[0] / (1.0 + JZ_D * iab[0]) - JZ_D0;
    [jz, iab[1], iab[2]]
}

fn jzazbz_inv(j: V3) -> V3 {
    let jz = j[0] + JZ_D0;
    let iz = jz / (1.0 + JZ_D - JZ_D * jz);
    let p = mul(&inv(&JZ_IAB), [iz, j[1], j[2]]);
    let lms = [pq_inv(p[0]), pq_inv(p[1]), pq_inv(p[2])];
    let [xp, yp, z] = mul(&inv(&JZ_LMS), lms);
    let x = (xp + (JZ_B - 1.0) * z) / JZ_B;
    let y = (yp + (JZ_G - 1.0) * x) / JZ_G;
    from_xyz([x, y, z])
}

// Log (Cineon): reference black 95, white 685, film gamma 0.6, 0.002
// density per code value, on 10-bit codes over 1024.
fn log_black() -> f64 {
    10f64.powf((95.0 - 685.0) * 0.002 / 0.6)
}

fn to_log(v: f64) -> f64 {
    let b = log_black();
    (685.0 + 300.0 * (b + (1.0 - b) * decode(v)).max(1e-30).log10()) / 1024.0
}

fn from_log(v: f64) -> f64 {
    let b = log_black();
    encode(((10f64.powf((v * 1024.0 - 685.0) / 300.0) - b) / (1.0 - b)).max(0.0))
}

const YCC_K: f64 = 1.099;
const YCC_C1: f64 = 156.0 / 255.0;
const YCC_C2: f64 = 137.0 / 255.0;

/// sRGB (encoded, [0, 1]) → the channels of `cs`. None for the spaces
/// handled elsewhere (sRGB, gray, CMYK).
pub fn from_srgb(cs: Colorspace, c: V3) -> Option<V3> {
    let deg = |a: f64, b: f64| b.atan2(a).to_degrees().rem_euclid(360.0);
    Some(match cs {
        Colorspace::RGB | Colorspace::scRGB => lin(c),
        Colorspace::CMY => [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]],
        Colorspace::XYZ => to_xyz(c),
        Colorspace::XyY => {
            let x = to_xyz(c);
            let s = x[0] + x[1] + x[2];
            if s == 0.0 {
                [0.0, 0.0, x[1]]
            } else {
                [x[0] / s, x[1] / s, x[1]]
            }
        }
        Colorspace::Lab => {
            let l = xyz_to_lab(to_xyz(c));
            [l[0] / 100.0, l[1] / 255.0 + 0.5, l[2] / 255.0 + 0.5]
        }
        Colorspace::LCH | Colorspace::LCHab => {
            let l = xyz_to_lab(to_xyz(c));
            [
                l[0] / 100.0,
                l[1].hypot(l[2]) / 255.0 + 0.5,
                deg(l[1], l[2]) / 360.0,
            ]
        }
        Colorspace::Luv => {
            let l = xyz_to_luv(to_xyz(c));
            [l[0] / 100.0, (l[1] + 134.0) / 354.0, (l[2] + 140.0) / 262.0]
        }
        Colorspace::LCHuv => {
            let l = xyz_to_luv(to_xyz(c));
            let h = if l[1] == 0.0 && l[2] == 0.0 {
                0.0
            } else {
                deg(l[1], l[2]) / 360.0
            };
            [l[0] / 100.0, l[1].hypot(l[2]) / 255.0 + 0.5, h]
        }
        Colorspace::LMS => mul(&LMS_M, to_xyz(c)),
        Colorspace::HSL => {
            let mx = c[0].max(c[1]).max(c[2]);
            let mn = c[0].min(c[1]).min(c[2]);
            let l = (mx + mn) / 2.0;
            let d = mx - mn;
            let s = if d == 0.0 {
                0.0
            } else if l <= 0.5 {
                d / (2.0 * l)
            } else {
                d / (2.0 - 2.0 * l)
            };
            [hue(c), s, l]
        }
        Colorspace::HSB | Colorspace::HSV => {
            let mx = c[0].max(c[1]).max(c[2]);
            let mn = c[0].min(c[1]).min(c[2]);
            [hue(c), if mx == 0.0 { 0.0 } else { (mx - mn) / mx }, mx]
        }
        Colorspace::HSI => {
            let i = (c[0] + c[1] + c[2]) / 3.0;
            if i <= 0.0 {
                [0.0, 0.0, 0.0]
            } else {
                let s = 1.0 - c[0].min(c[1]).min(c[2]) / i;
                let a = 0.5 * (2.0 * c[0] - c[1] - c[2]);
                let b = 0.866_025_403_784_438_6 * (c[1] - c[2]);
                [
                    b.atan2(a).rem_euclid(2.0 * std::f64::consts::PI)
                        / (2.0 * std::f64::consts::PI),
                    s,
                    i,
                ]
            }
        }
        Colorspace::HWB => {
            let w = c[0].min(c[1]).min(c[2]);
            let v = c[0].max(c[1]).max(c[2]);
            if v == w {
                [0.0, w, 1.0 - v]
            } else {
                let [r, g, b] = c;
                let f = if r == w {
                    g - b
                } else if g == w {
                    b - r
                } else {
                    r - g
                };
                let i = if r == w {
                    3.0
                } else if g == w {
                    5.0
                } else {
                    1.0
                };
                [(i - f / (v - w)) / 6.0, w, 1.0 - v]
            }
        }
        Colorspace::HCL | Colorspace::HCLp => {
            let mx = c[0].max(c[1]).max(c[2]);
            let mn = c[0].min(c[1]).min(c[2]);
            [hue(c), mx - mn, luma(c)]
        }
        Colorspace::Oklab => {
            let o = oklab(c);
            [o[0], o[1] + 0.5, o[2] + 0.5]
        }
        Colorspace::Oklch => {
            let o = oklab(c);
            // the hue is stored in radians (as ImageMagick 7.1.2 does)
            [
                o[0],
                o[1].hypot(o[2]),
                o[2].atan2(o[1]).rem_euclid(2.0 * std::f64::consts::PI),
            ]
        }
        Colorspace::Jzazbz => {
            // ImageMagick reads the samples as R, B, G
            let j = jzazbz([c[0], c[2], c[1]]);
            [j[0], j[1] + 0.5, j[2] + 0.5]
        }
        Colorspace::DisplayP3 => {
            let v = mul(&P3_M, to_xyz(c));
            [
                encode(v[0].max(0.0)),
                encode(v[1].max(0.0)),
                encode(v[2].max(0.0)),
            ]
        }
        Colorspace::Adobe98 => {
            let v = mul(&ADOBE_M, to_xyz(c));
            [
                encode(v[0].max(0.0)),
                encode(v[1].max(0.0)),
                encode(v[2].max(0.0)),
            ]
        }
        Colorspace::ProPhoto => {
            let v = mul(&PROPHOTO_M, to_xyz(c));
            [
                encode(v[0].max(0.0)),
                encode(v[1].max(0.0)),
                encode(v[2].max(0.0)),
            ]
        }
        Colorspace::Log => [to_log(c[0]), to_log(c[1]), to_log(c[2])],
        Colorspace::YCC => {
            // samples below 0.018 are crushed to 0.018·v/1.099 first; the
            // chroma rows use 0.886 and 0.701 rather than 1 - luma
            let f = |v: f64| if v < 0.018 { 0.018 * v / YCC_K } else { v };
            let (r, g, b) = (f(c[0]), f(c[1]), f(c[2]));
            [
                YCC_K * (LUMA[0] * r + LUMA[1] * g + LUMA[2] * b),
                YCC_C1 + YCC_K * (-LUMA[0] * r - LUMA[1] * g + 0.886 * b),
                YCC_C2 + YCC_K * (0.701 * r - LUMA[1] * g - LUMA[2] * b),
            ]
        }
        _ => {
            let (m, off) = linear_space(cs)?;
            let v = mul(&m, c);
            [v[0] + off[0], v[1] + off[1], v[2] + off[2]]
        }
    })
}

/// The channels of `cs` → sRGB (encoded, [0, 1]).
pub fn to_srgb(cs: Colorspace, c: V3) -> Option<V3> {
    Some(match cs {
        Colorspace::RGB | Colorspace::scRGB => unlin(c),
        Colorspace::CMY => [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]],
        Colorspace::XYZ => from_xyz(c),
        Colorspace::XyY => {
            if c[1] == 0.0 {
                [0.0, 0.0, 0.0]
            } else {
                let y = c[2];
                from_xyz([c[0] * y / c[1], y, (1.0 - c[0] - c[1]) * y / c[1]])
            }
        }
        Colorspace::Lab => from_xyz(lab_to_xyz([
            c[0] * 100.0,
            (c[1] - 0.5) * 255.0,
            (c[2] - 0.5) * 255.0,
        ])),
        Colorspace::LCH | Colorspace::LCHab => {
            let ch = (c[1] - 0.5) * 255.0;
            let h = (c[2] * 360.0).to_radians();
            from_xyz(lab_to_xyz([c[0] * 100.0, ch * h.cos(), ch * h.sin()]))
        }
        Colorspace::Luv => from_xyz(luv_to_xyz([
            c[0] * 100.0,
            c[1] * 354.0 - 134.0,
            c[2] * 262.0 - 140.0,
        ])),
        Colorspace::LCHuv => {
            let ch = (c[1] - 0.5) * 255.0;
            let h = (c[2] * 360.0).to_radians();
            from_xyz(luv_to_xyz([c[0] * 100.0, ch * h.cos(), ch * h.sin()]))
        }
        Colorspace::LMS => from_xyz(mul(&inv(&LMS_M), c)),
        Colorspace::HSL => {
            let (h, s, l) = (c[0], c[1], c[2]);
            let ch = (1.0 - (2.0 * l - 1.0).abs()) * s;
            from_hue(h, ch, l - ch / 2.0)
        }
        Colorspace::HSB | Colorspace::HSV => {
            let (h, s, v) = (c[0], c[1], c[2]);
            let ch = v * s;
            from_hue(h, ch, v - ch)
        }
        Colorspace::HSI => {
            let (h, s, i) = (c[0] * 360.0, c[1], c[2]);
            let h = h.rem_euclid(360.0);
            let f = |hh: f64| -> (f64, f64, f64) {
                let a = i * (1.0 - s);
                let b = i * (1.0 + s * hh.to_radians().cos() / (60.0 - hh).to_radians().cos());
                (a, b, 3.0 * i - a - b)
            };
            if h < 120.0 {
                let (b, r, g) = f(h);
                [r, g, b]
            } else if h < 240.0 {
                let (r, g, b) = f(h - 120.0);
                [r, g, b]
            } else {
                let (g, b, r) = f(h - 240.0);
                [r, g, b]
            }
        }
        Colorspace::HWB => {
            let (h, w, bl) = (c[0], c[1], c[2]);
            let v = 1.0 - bl;
            if w + bl >= 1.0 || v <= w {
                [v, v, v]
            } else {
                let six = h * 6.0;
                let i = six.floor() as i64;
                let mut f = six - i as f64;
                if i & 1 != 0 {
                    f = 1.0 - f;
                }
                let n = w + f * (v - w);
                match i.rem_euclid(6) {
                    0 => [v, n, w],
                    1 => [n, v, w],
                    2 => [w, v, n],
                    3 => [w, n, v],
                    4 => [n, w, v],
                    _ => [v, w, n],
                }
            }
        }
        Colorspace::HCL | Colorspace::HCLp => {
            let (h, ch, l) = (c[0], c[1], c[2]);
            let rgb = from_hue(h, ch, 0.0);
            let m = l - luma(rgb);
            [rgb[0] + m, rgb[1] + m, rgb[2] + m]
        }
        Colorspace::Oklab => oklab_inv([c[0], c[1] - 0.5, c[2] - 0.5]),
        Colorspace::Oklch => oklab_inv([c[0], c[1] * c[2].cos(), c[1] * c[2].sin()]),
        Colorspace::Jzazbz => {
            let v = jzazbz_inv([c[0], c[1] - 0.5, c[2] - 0.5]);
            [v[0], v[2], v[1]]
        }
        Colorspace::DisplayP3 => from_xyz(mul(&inv(&P3_M), lin(c))),
        Colorspace::Adobe98 => from_xyz(mul(&inv(&ADOBE_M), lin(c))),
        Colorspace::ProPhoto => from_xyz(mul(&inv(&PROPHOTO_M), lin(c))),
        Colorspace::Log => [from_log(c[0]), from_log(c[1]), from_log(c[2])],
        Colorspace::YCC => {
            // Kodak's PhotoYCC inverse, scaled by 1/1.3555
            let l = 1.3584 * c[0];
            let c1 = 2.2179 * (c[1] - YCC_C1);
            let c2 = 1.8215 * (c[2] - YCC_C2);
            let k = 1.0 / 1.3555;
            [
                k * (l + c2),
                k * (l - 0.194 * c1 - 0.509 * c2),
                k * (l + c1),
            ]
        }
        _ => {
            let (m, off) = linear_space(cs)?;
            mul(&inv(&m), [c[0] - off[0], c[1] - off[1], c[2] - off[2]])
        }
    })
}

/// TransformImageColorspace(): convert the pixels (through sRGB).
pub fn transform(i: &mut Image, cs: Colorspace) {
    let cs = if cs == Colorspace::Undefined || cs == Colorspace::Transparent {
        Colorspace::SRGB
    } else {
        cs
    };
    if cs == i.colorspace {
        return;
    }
    if i.colorspace == Colorspace::CMYK {
        i.cmyk_to_rgb();
    }
    let q = |v: f64| (v * 65535.0).round().clamp(0.0, 65535.0) as u16;
    let s = |v: u16| v as f64 / 65535.0;
    // to sRGB
    let src = i.colorspace;
    if !matches!(
        src,
        Colorspace::SRGB | Colorspace::Gray | Colorspace::Undefined | Colorspace::CMYK
    ) {
        if src == Colorspace::LinearGray {
            for p in i.pixels.chunks_exact_mut(4) {
                let v = q(encode(s(p[0])));
                p[0] = v;
                p[1] = v;
                p[2] = v;
            }
        } else {
            for p in i.pixels.chunks_exact_mut(4) {
                if let Some(c) = to_srgb(src, [s(p[0]), s(p[1]), s(p[2])]) {
                    p[0] = q(c[0]);
                    p[1] = q(c[1]);
                    p[2] = q(c[2]);
                }
            }
        }
    }
    i.colorspace = Colorspace::SRGB;
    match cs {
        Colorspace::Gray => {
            for p in i.pixels.chunks_exact_mut(4) {
                let v = crate::color::to_q16(Image::intensity(p));
                p[0] = v;
                p[1] = v;
                p[2] = v;
            }
        }
        Colorspace::LinearGray => {
            for p in i.pixels.chunks_exact_mut(4) {
                let v = q(0.212656 * decode(s(p[0]))
                    + 0.715158 * decode(s(p[1]))
                    + 0.072186 * decode(s(p[2])));
                p[0] = v;
                p[1] = v;
                p[2] = v;
            }
        }
        Colorspace::CMYK => i.rgb_to_cmyk(),
        Colorspace::SRGB => {}
        _ => {
            for p in i.pixels.chunks_exact_mut(4) {
                if let Some(c) = from_srgb(cs, [s(p[0]), s(p[1]), s(p[2])]) {
                    p[0] = q(c[0]);
                    p[1] = q(c[1]);
                    p[2] = q(c[2]);
                }
            }
        }
    }
    i.colorspace = cs;
    i.colors = None;
}

/// A copy of an image in a colourspace the encoders write (ImageMagick
/// converts other colourspaces to sRGB on output; linear RGB is written
/// as it is).
pub fn for_output(img: &Image) -> std::borrow::Cow<'_, Image> {
    if matches!(
        img.colorspace,
        Colorspace::SRGB
            | Colorspace::RGB
            | Colorspace::scRGB
            | Colorspace::Gray
            | Colorspace::LinearGray
            | Colorspace::CMYK
            | Colorspace::Undefined
            | Colorspace::Transparent
    ) {
        return std::borrow::Cow::Borrowed(img);
    }
    let mut c = img.clone();
    transform(&mut c, Colorspace::SRGB);
    std::borrow::Cow::Owned(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let samples = [
            [0.2, 0.4, 0.6],
            [0.9, 0.1, 0.3],
            [0.5, 0.5, 0.5],
            [0.05, 0.8, 0.2],
        ];
        for cs in 1..=39 {
            let cs = Colorspace(cs);
            if matches!(
                cs,
                Colorspace::CMYK
                    | Colorspace::Gray
                    | Colorspace::LinearGray
                    | Colorspace::SRGB
                    | Colorspace::Transparent
                    | Colorspace::Undefined
            ) {
                continue;
            }
            for s in samples {
                let Some(f) = from_srgb(cs, s) else {
                    continue;
                };
                let b = to_srgb(cs, f).unwrap();
                // the XYZ pair differs in precision; ImageMagick's PhotoYCC
                // round trip is lossy (about 7% RMSE on a photo)
                if cs == Colorspace::YCC {
                    continue;
                }
                let tol = 2e-3;
                for k in 0..3 {
                    assert!(
                        (b[k] - s[k]).abs() < tol,
                        "{:?} {:?} {:?} {:?}",
                        cs,
                        s,
                        f,
                        b
                    );
                }
            }
        }
    }
}
