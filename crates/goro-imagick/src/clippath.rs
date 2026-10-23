//! Clipping paths from a Photoshop (8BIM) profile: ClipImage() and
//! ClipImagePath() make a write mask from the path resources (IDs 2000 to
//! 2997, Bézier knots in 8.24 fixed point fractions of the image size).

use crate::error::{MResult, MagickError, severity};
use crate::image::Image;

/// A path resource: its ID, name and data.
struct PathRes {
    id: u16,
    name: String,
    data: Vec<u8>,
}

fn resources(p: &[u8]) -> Vec<PathRes> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 12 <= p.len() {
        if &p[i..i + 4] != b"8BIM" {
            i += 1;
            continue;
        }
        let id = u16::from_be_bytes([p[i + 4], p[i + 5]]);
        let mut j = i + 6;
        let nlen = p[j] as usize;
        let name = String::from_utf8_lossy(p.get(j + 1..j + 1 + nlen).unwrap_or(&[])).into_owned();
        j += 1 + nlen;
        if (1 + nlen) % 2 != 0 {
            j += 1;
        }
        if j + 4 > p.len() {
            break;
        }
        let size = u32::from_be_bytes([p[j], p[j + 1], p[j + 2], p[j + 3]]) as usize;
        j += 4;
        let end = (j + size).min(p.len());
        if (2000..=2997).contains(&id) {
            out.push(PathRes {
                id,
                name,
                data: p[j..end].to_vec(),
            });
        }
        i = end + (size & 1);
    }
    out
}

/// The subpaths of a path resource as polygons in pixel coordinates.
fn polygons(data: &[u8], w: f64, h: f64) -> Vec<Vec<(f64, f64)>> {
    let fx = |b: &[u8]| i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64 / (1u32 << 24) as f64;
    let mut subpaths: Vec<Vec<[(f64, f64); 3]>> = Vec::new();
    let mut remaining = 0usize;
    for rec in data.chunks_exact(26) {
        let sel = u16::from_be_bytes([rec[0], rec[1]]);
        match sel {
            0 | 3 => {
                remaining = u16::from_be_bytes([rec[2], rec[3]]) as usize;
                subpaths.push(Vec::new());
            }
            1 | 2 | 4 | 5 if remaining > 0 => {
                let pt = |o: usize| (fx(&rec[o + 4..]) * w, fx(&rec[o..]) * h);
                if let Some(s) = subpaths.last_mut() {
                    s.push([pt(2), pt(10), pt(18)]);
                }
                remaining -= 1;
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for knots in subpaths {
        if knots.is_empty() {
            continue;
        }
        let mut poly = Vec::new();
        let n = knots.len();
        for k in 0..n {
            let a = knots[k];
            let b = knots[(k + 1) % n];
            let (p0, c1, c2, p3) = (a[1], a[2], b[0], b[1]);
            let straight = p0 == c1 && c2 == p3;
            let steps = if straight { 1 } else { 32 };
            for s in 0..steps {
                let t = s as f64 / steps as f64;
                let u = 1.0 - t;
                let f = |a: f64, b: f64, c: f64, d: f64| {
                    u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                };
                poly.push((f(p0.0, c1.0, c2.0, p3.0), f(p0.1, c1.1, c2.1, p3.1)));
            }
        }
        out.push(poly);
    }
    out
}

fn no_clip_path(img: &Image, reason: &str) -> MagickError {
    MagickError::new(
        severity::OPTION_ERROR,
        format!(
            "no clip path defined '{}': {} @ error/image.c/ClipImagePath/725",
            img.filename, reason
        ),
    )
}

/// ClipImagePath(): `pathname` is "#n" (the n-th path) or a path's
/// name; `inside` keeps later operations inside the path. `clip_image`
/// selects the error ClipImage() reports.
pub fn clip_path(img: &mut Image, pathname: &str, inside: bool, clip_image: bool) -> MResult<()> {
    let reason = if clip_image {
        "Invalid argument"
    } else {
        "No such file or directory"
    };
    let res = img.profile("8bim").map(resources).unwrap_or_default();
    let found = if let Some(n) = pathname.strip_prefix('#') {
        n.trim()
            .parse::<u16>()
            .ok()
            .and_then(|n| res.iter().find(|r| r.id == 1999 + n))
    } else {
        res.iter().find(|r| r.name == pathname)
    };
    let Some(path) = found else {
        return Err(no_clip_path(img, reason));
    };
    let (w, h) = (img.columns, img.rows);
    let polys = polygons(&path.data, w as f64, h as f64);
    let mut mask = Image::blank(w, h);
    mask.colorspace = crate::types::Colorspace::Gray;
    for y in 0..h {
        let cy = y as f64 + 0.5;
        // even-odd crossings of the row through the pixel centres
        let mut xs: Vec<f64> = Vec::new();
        for poly in &polys {
            let n = poly.len();
            for k in 0..n {
                let (a, b) = (poly[k], poly[(k + 1) % n]);
                if (a.1 <= cy) != (b.1 <= cy) {
                    xs.push(a.0 + (cy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for x in 0..w {
            let cx = x as f64 + 0.5;
            let inn = xs.iter().filter(|&&v| v <= cx).count() % 2 == 1;
            let v = if inn == inside { 65535 } else { 0 };
            mask.set_px(x, y, [v, v, v, 65535]);
        }
    }
    img.write_mask = Some(Box::new(mask));
    Ok(())
}
