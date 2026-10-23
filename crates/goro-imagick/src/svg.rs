//! The SVG coder (read only): oxideav-svg parses the document into its
//! vector scene graph, oxideav-raster paints it.
//!
//! As ImageMagick's SVG delegate does, the canvas is the document's
//! `width` × `height` in CSS pixels at 96 dpi, scaled by `density / 96`
//! and rounded to whole pixels (the drawing is stretched onto the
//! rounded canvas), and the rendering is composited over the background
//! colour (`-background none` keeps it transparent). The result is an
//! 8-bit sRGB image with an alpha channel, at 37.79 pixels per
//! centimetre (96 dpi) unless a density was set.

use crate::color::PixelInfo;
use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use crate::types::ResolutionType;
use oxideav_core::vector as v;

/// A `<text>` font resolver for oxideav-svg (installed once): the family
/// list's first installed font, else a sans-serif fallback.
fn install_font_resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = oxideav_svg::text::set_font_resolver(|family, _size| {
            let mut candidates: Vec<String> = family
                .split(',')
                .map(|f| f.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
                .filter(|f| !f.is_empty())
                .collect();
            candidates.extend(
                ["DejaVuSans", "LiberationSans-Regular", "FreeSans", "Arial"]
                    .iter()
                    .map(|s| s.to_string()),
            );
            for name in candidates {
                let generic = match name.to_ascii_lowercase().as_str() {
                    "sans-serif" | "sans" => Some("DejaVuSans"),
                    "serif" => Some("DejaVuSerif"),
                    "monospace" => Some("DejaVuSansMono"),
                    _ => None,
                };
                let lookup = generic.unwrap_or(&name);
                let Some(path) = crate::fonts::resolve(lookup)
                    .or_else(|| crate::fonts::resolve(&name.replace(' ', "")))
                else {
                    continue;
                };
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let face = if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("otf"))
                {
                    oxideav_scribe::Face::from_otf_bytes(bytes)
                } else {
                    oxideav_scribe::Face::from_ttf_bytes(bytes)
                };
                if let Ok(face) = face {
                    return Some(oxideav_scribe::FaceChain::new(face));
                }
            }
            None
        });
    });
}

/// Read an SVG (or gzipped SVGZ) document.
pub fn decode(
    data: &[u8],
    density: Option<(f64, f64)>,
    background: &PixelInfo,
) -> MResult<Vec<Image>> {
    install_font_resolver();
    let doc = oxideav_svg::parse(data).map_err(|e| {
        MagickError::with_desc(
            severity::CORRUPT_IMAGE_ERROR,
            &format!("{}", e),
            "",
            "error/svg.c/ReadSVGImage",
        )
    })?;
    let (dx, dy) = density.unwrap_or((96.0, 96.0));
    let (sx, sy) = (dx / 96.0, dy / 96.0);
    let (dw, dh) = (doc.width as f64, doc.height as f64);
    let cols = (dw * sx).round().max(1.0) as usize;
    let rows = (dh * sy).round().max(1.0) as usize;
    if dw <= 0.0 || dh <= 0.0 || cols > 65535 || rows > 65535 {
        return Err(MagickError::with_desc(
            severity::CORRUPT_IMAGE_ERROR,
            "NegativeOrZeroImageSize",
            "",
            "error/svg.c/ReadSVGImage",
        ));
    }
    let mut frame: v::VectorFrame = doc.into();
    let vb = *frame.view_box.get_or_insert(v::ViewBox {
        min_x: 0.0,
        min_y: 0.0,
        width: dw as f32,
        height: dh as f32,
    });
    if vb.width > 0.0 && vb.height > 0.0 {
        let (kx, ky) = (cols as f32 / vb.width, rows as f32 / vb.height);
        let initial = v::Transform2D {
            a: kx,
            b: 0.0,
            c: 0.0,
            d: ky,
            e: -vb.min_x * kx,
            f: -vb.min_y * ky,
        };
        fix_gradients(data, &mut frame, initial);
    }
    // The document already maps its viewBox under its own
    // preserveAspectRatio; the canvas fit itself is a plain stretch.
    let par = oxideav_raster::PreserveAspectRatio {
        align: oxideav_raster::AspectRatioAlign::None,
        meet_or_slice: oxideav_raster::MeetOrSlice::Meet,
    };
    let mut renderer =
        oxideav_raster::Renderer::new(cols as u32, rows as u32).with_preserve_aspect_ratio(par);
    // the finest vertical coverage the rasteriser offers (cairo's is
    // exact-area)
    renderer.supersampling = 8;
    let out = renderer.render(&frame);
    let plane = &out.planes[0];
    let mut img = Image::blank(cols, rows);
    img.alpha = true;
    img.depth = 8;
    let bg = background.to_rgba16();
    let bga = bg[3] as f64 / 65535.0;
    for y in 0..rows {
        for x in 0..cols {
            let s = &plane.data[y * plane.stride + x * 4..];
            let sa = s[3] as f64 / 255.0;
            let oa = sa + bga * (1.0 - sa);
            let px = if oa <= 0.0 {
                [0, 0, 0, 0]
            } else {
                let ch = |i: usize| {
                    let c = (s[i] as f64 * 257.0 * sa + bg[i] as f64 * bga * (1.0 - sa)) / oa;
                    // an 8-bit rendering
                    ((c / 257.0).round().clamp(0.0, 255.0) as u16) * 257
                };
                [ch(0), ch(1), ch(2), ((oa * 255.0).round() as u16) * 257]
            };
            img.set_px(x, y, px);
        }
    }
    match density {
        Some((x, y)) => {
            img.x_resolution = x;
            img.y_resolution = y;
        }
        None => {
            img.x_resolution = 37.79;
            img.y_resolution = 37.79;
        }
    }
    img.units = ResolutionType::PixelsPerCentimeter;
    Ok(vec![img])
}

// --- objectBoundingBox gradients -------------------------------------------
//
// oxideav-svg 0.1.9 flattens every gradient into user-space coordinates
// without applying `gradientUnits="objectBoundingBox"` (the default), so a
// `x2="1"` gradient spans one user unit. The gradient elements are scanned
// here, and the paints that came from bounding-box gradients are mapped
// onto the bounding box of the shape they paint.

/// A bounding-box gradient's flattened geometry (as oxideav-svg emits it).
#[derive(Debug, Clone)]
enum BBoxGrad {
    Linear([f32; 4]),
    Radial { c: (f32, f32), r: f32 },
}

type Tag = (String, Vec<(String, String)>);

/// The attributes of the start tags named `names`.
fn scan_tags(src: &str, names: &[&str]) -> Vec<Tag> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = src[i..].find('<') {
        i += off + 1;
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b':' || b[i] == b'_') {
            i += 1;
        }
        let name = &src[start..i];
        let local = name.rsplit(':').next().unwrap_or(name);
        if !names.contains(&local) {
            continue;
        }
        let mut attrs = Vec::new();
        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() || b[i] == b'>' || b[i] == b'/' {
                break;
            }
            let ks = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' {
                i += 1;
            }
            let key = src[ks..i].to_string();
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && b[i] == b'=' {
                i += 1;
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                    let q = b[i];
                    i += 1;
                    let vs = i;
                    while i < b.len() && b[i] != q {
                        i += 1;
                    }
                    attrs.push((key, src[vs..i.min(b.len())].to_string()));
                    i += 1;
                } else {
                    break;
                }
            } else if key.is_empty() {
                break;
            }
        }
        out.push((local.to_string(), attrs));
    }
    out
}

/// A `gradientTransform` list (matrix / translate / scale / rotate /
/// skewX / skewY) as (a, b, c, d, e, f).
fn parse_transform(s: &str) -> [f64; 6] {
    let mul = |m: [f64; 6], n: [f64; 6]| -> [f64; 6] {
        [
            m[0] * n[0] + m[2] * n[1],
            m[1] * n[0] + m[3] * n[1],
            m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3],
            m[0] * n[4] + m[2] * n[5] + m[4],
            m[1] * n[4] + m[3] * n[5] + m[5],
        ]
    };
    let mut m = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut rest = s;
    while let Some(p) = rest.find('(') {
        let name = rest[..p].trim().trim_start_matches(',').trim();
        let Some(e) = rest[p..].find(')') else {
            break;
        };
        let args: Vec<f64> = rest[p + 1..p + e]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter_map(|t| t.parse().ok())
            .collect();
        rest = &rest[p + e + 1..];
        let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
        let n = match name {
            "matrix" if args.len() == 6 => [a(0), a(1), a(2), a(3), a(4), a(5)],
            "translate" => [1.0, 0.0, 0.0, 1.0, a(0), a(1)],
            "scale" => {
                let sy = if args.len() > 1 { a(1) } else { a(0) };
                [a(0), 0.0, 0.0, sy, 0.0, 0.0]
            }
            "rotate" => {
                let (s, c) = a(0).to_radians().sin_cos();
                let r = [c, s, -s, c, 0.0, 0.0];
                if args.len() == 3 {
                    let t = mul([1.0, 0.0, 0.0, 1.0, a(1), a(2)], r);
                    mul(t, [1.0, 0.0, 0.0, 1.0, -a(1), -a(2)])
                } else {
                    r
                }
            }
            "skewX" => [1.0, 0.0, a(0).to_radians().tan(), 1.0, 0.0, 0.0],
            "skewY" => [1.0, a(0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => continue,
        };
        m = mul(m, n);
    }
    m
}

/// The bounding-box gradients of a document.
fn bbox_gradients(src: &str) -> Vec<BBoxGrad> {
    let tags = scan_tags(src, &["linearGradient", "radialGradient"]);
    let find = |id: &str| {
        tags.iter()
            .find(|t| t.1.iter().any(|(k, v)| k == "id" && v == id))
    };
    // an attribute, following the href template chain
    let get = |t: &Tag, key: &str| -> Option<String> {
        let mut cur = t;
        for _ in 0..16 {
            if let Some((_, v)) = cur.1.iter().find(|(k, _)| k == key) {
                return Some(v.clone());
            }
            let href = cur
                .1
                .iter()
                .find(|(k, _)| k == "href" || k == "xlink:href")
                .map(|(_, v)| v.trim().trim_start_matches('#').to_string())?;
            cur = find(&href)?;
        }
        None
    };
    let num = |v: Option<String>, d: f64| -> f64 {
        match v {
            Some(s) => {
                let s = s.trim();
                if let Some(p) = s.strip_suffix('%') {
                    p.trim().parse::<f64>().map(|x| x / 100.0).unwrap_or(d)
                } else {
                    s.parse().unwrap_or(d)
                }
            }
            None => d,
        }
    };
    let mut out = Vec::new();
    for t in &tags {
        if get(t, "gradientUnits").is_some_and(|u| u.trim() == "userSpaceOnUse") {
            continue;
        }
        let m = parse_transform(&get(t, "gradientTransform").unwrap_or_default());
        let ap = |x: f64, y: f64| -> (f32, f32) {
            (
                (m[0] * x + m[2] * y + m[4]) as f32,
                (m[1] * x + m[3] * y + m[5]) as f32,
            )
        };
        if t.0 == "linearGradient" {
            let p1 = ap(num(get(t, "x1"), 0.0), num(get(t, "y1"), 0.0));
            let p2 = ap(num(get(t, "x2"), 1.0), num(get(t, "y2"), 0.0));
            out.push(BBoxGrad::Linear([p1.0, p1.1, p2.0, p2.1]));
        } else {
            let (cx, cy) = (num(get(t, "cx"), 0.5), num(get(t, "cy"), 0.5));
            let r = num(get(t, "r"), 0.5);
            let sx = (m[0] * m[0] + m[1] * m[1]).sqrt();
            let sy = (m[2] * m[2] + m[3] * m[3]).sqrt();
            out.push(BBoxGrad::Radial {
                c: ap(cx, cy),
                r: (r * (sx * sy).sqrt()) as f32,
            });
        }
    }
    out
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4 * (1.0 + a.abs())
}

/// Map a paint that came from a bounding-box gradient onto `bbox`.
fn map_paint(p: &mut v::Paint, grads: &[BBoxGrad], bbox: (f32, f32, f32, f32)) {
    let (bx, by, bw, bh) = bbox;
    let map = |pt: &mut v::Point| {
        pt.x = bx + pt.x * bw;
        pt.y = by + pt.y * bh;
    };
    match p {
        v::Paint::LinearGradient(g) => {
            let hit = grads.iter().any(|b| {
                matches!(b, BBoxGrad::Linear(c) if close(c[0], g.start.x) && close(c[1], g.start.y)
                    && close(c[2], g.end.x) && close(c[3], g.end.y))
            });
            if hit {
                map(&mut g.start);
                map(&mut g.end);
            }
        }
        v::Paint::RadialGradient(g) => {
            let hit = grads.iter().any(|b| {
                matches!(b, BBoxGrad::Radial { c, r } if close(c.0, g.center.x)
                    && close(c.1, g.center.y) && close(*r, g.radius))
            });
            if hit {
                map(&mut g.center);
                if let Some(f) = g.focal.as_mut() {
                    map(f);
                }
                g.radius *= (bw * bh).abs().sqrt();
            }
        }
        _ => {}
    }
}

fn path_bbox(path: &v::Path) -> Option<(f32, f32, f32, f32)> {
    let contours = oxideav_raster::flatten_path(&path.commands, &v::Transform2D::identity());
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for c in &contours {
        for &(x, y) in &c.points {
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
    }
    (b.0 <= b.2).then_some((b.0, b.1, b.2 - b.0, b.3 - b.1))
}

/// Map a gradient paint from user space to device space (oxideav-raster
/// 0.1.3 samples gradients in device pixels, ignoring the CTM).
fn paint_to_device(p: &mut v::Paint, t: &v::Transform2D) {
    match p {
        v::Paint::LinearGradient(g) => {
            g.start = t.apply(g.start);
            g.end = t.apply(g.end);
        }
        v::Paint::RadialGradient(g) => {
            g.center = t.apply(g.center);
            if let Some(f) = g.focal.as_mut() {
                *f = t.apply(*f);
            }
            g.radius *= (t.a * t.d - t.b * t.c).abs().sqrt();
        }
        _ => {}
    }
}

fn fix_node(n: &mut v::Node, grads: &[BBoxGrad], ctm: &v::Transform2D) {
    match n {
        v::Node::Path(p) => {
            let is_grad = |p: &v::Paint| !matches!(p, v::Paint::Solid(_));
            if p.fill.as_ref().is_some_and(is_grad)
                || p.stroke.as_ref().is_some_and(|s| is_grad(&s.paint))
            {
                let bbox = path_bbox(&p.path);
                let paints = p
                    .fill
                    .as_mut()
                    .into_iter()
                    .chain(p.stroke.as_mut().map(|s| &mut s.paint));
                for paint in paints {
                    if let Some(bbox) = bbox {
                        map_paint(paint, grads, bbox);
                    }
                    paint_to_device(paint, ctm);
                }
            }
        }
        v::Node::Group(g) => {
            let ctm = ctm.compose(&g.transform);
            for c in g.children.iter_mut() {
                fix_node(c, grads, &ctm);
            }
        }
        v::Node::SoftMask { mask, content, .. } => {
            fix_node(mask, grads, ctm);
            fix_node(content, grads, ctm);
        }
        _ => {}
    }
}

/// Fix the gradient paints of a parsed document: `objectBoundingBox`
/// units, and user space → device space.
fn fix_gradients(src: &[u8], frame: &mut v::VectorFrame, initial: v::Transform2D) {
    let grads = std::str::from_utf8(src)
        .map(bbox_gradients)
        .unwrap_or_default();
    let mut root = v::Node::Group(std::mem::take(&mut frame.root));
    fix_node(&mut root, &grads, &initial);
    if let v::Node::Group(g) = root {
        frame.root = g;
    }
}
