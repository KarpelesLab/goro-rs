//! The LABEL: and CAPTION: pseudo formats: text drawn onto a canvas of
//! the background colour.
//!
//! Black-box (ImageMagick 7.1.2): a label with no size is its text width
//! plus one by its ascender − descender plus one (grid-fitted metrics,
//! see text.rs), drawn from the top-left; with a size and no point size
//! the point size is the largest that fits. A caption wraps its words to
//! the width.

use crate::color::PixelInfo;
use crate::draw::DrawingWand;
use crate::error::{MResult, MagickError, severity};
use crate::image::Image;
use crate::text::FontMetrics;
use crate::types::Gravity;

pub struct LabelOptions<'a> {
    pub font: &'a str,
    pub pointsize: f64,
    pub size: Option<(usize, usize)>,
    pub gravity: Gravity,
    pub background: &'a PixelInfo,
    pub fill: PixelInfo,
}

fn wand(o: &LabelOptions, pointsize: f64) -> DrawingWand {
    let mut dw = DrawingWand::new();
    if !o.font.is_empty() {
        dw.gc_mut().font = Some(o.font.to_string());
    }
    dw.gc_mut().font_size = pointsize;
    dw.set_fill_color(&o.fill);
    dw
}

fn metrics(o: &LabelOptions, pointsize: f64, text: &str) -> MResult<FontMetrics> {
    wand(o, pointsize)
        .font_metrics(text)
        .ok_or_else(|| MagickError::new(severity::TYPE_ERROR, "unable to read font".to_string()))
}

/// The natural size of `text` at `pointsize`.
fn extent(m: &FontMetrics, lines: usize) -> (f64, f64) {
    let h = if lines > 1 {
        m.text_height
    } else {
        m.ascender - m.descender
    };
    (m.text_width + 1.0, h + 1.0)
}

/// Greedy word wrap of `text` to `width` pixels.
fn wrap(o: &LabelOptions, pointsize: f64, text: &str, width: f64) -> MResult<String> {
    let mut out: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            if !line.is_empty() && metrics(o, pointsize, &cand)?.text_width + 1.0 > width {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = cand;
            }
        }
        out.push(line);
    }
    Ok(out.join("\n"))
}

pub fn render(o: &LabelOptions, text: &str, caption: bool) -> MResult<Image> {
    let fits = |ps: f64| -> MResult<(bool, String)> {
        let t = match (caption, o.size) {
            (true, Some((w, _))) if w > 0 => wrap(o, ps, text, w as f64)?,
            _ => text.to_string(),
        };
        let m = metrics(o, ps, &t)?;
        let (tw, th) = extent(&m, t.split('\n').count());
        let ok = match o.size {
            Some((w, h)) => (w == 0 || tw <= w as f64) && (h == 0 || th <= h as f64),
            None => true,
        };
        Ok((ok, t))
    };
    let mut pointsize = o.pointsize;
    if pointsize <= 0.0 {
        pointsize = 12.0;
        if o.size.is_some_and(|(w, h)| w > 0 || h > 0) {
            // the largest whole point size that fits
            let (mut lo, mut hi) = (1.0f64, 2.0f64);
            while fits(hi)?.0 && hi < 4096.0 {
                lo = hi;
                hi *= 2.0;
            }
            while hi - lo > 1.0 {
                let mid = ((lo + hi) / 2.0).floor();
                if fits(mid)?.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            pointsize = lo;
        }
    }
    let (_, t) = fits(pointsize)?;
    let m = metrics(o, pointsize, &t)?;
    let (nw, nh) = extent(&m, t.split('\n').count());
    let (w, h) = match o.size {
        Some((w, h)) => (
            if w > 0 { w } else { nw as usize },
            if h > 0 { h } else { nh as usize },
        ),
        None => (nw as usize, nh as usize),
    };
    let mut img = Image::new(w.max(1), h.max(1), o.background);
    let mut dw = wand(o, pointsize);
    match o.gravity {
        Gravity::Undefined | Gravity::NorthWest => dw.annotation(0.0, m.ascender, &t),
        g => {
            dw.set_gravity(g);
            dw.annotation(0.0, 0.0, &t);
        }
    }
    dw.render_onto(&mut img)?;
    img.format = if caption { "CAPTION" } else { "LABEL" }.to_string();
    img.properties.set("label", text);
    Ok(img)
}
