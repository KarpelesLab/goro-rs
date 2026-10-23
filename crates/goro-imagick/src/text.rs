//! Text: font metrics and glyph outlines (TrueType / OpenType through
//! skrifa), laid out on a baseline, as vector paths for the rasterizer.

use std::path::PathBuf;

use oxideav_core::vector as v;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

thread_local! {
    static FONT_DATA: std::cell::RefCell<Vec<(PathBuf, std::rc::Rc<Vec<u8>>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The bytes of a font file (cached).
pub fn font_data(path: &std::path::Path) -> Option<std::rc::Rc<Vec<u8>>> {
    if let Some(d) = FONT_DATA.with(|c| {
        c.borrow()
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, d)| d.clone())
    }) {
        return Some(d);
    }
    let d = std::rc::Rc::new(std::fs::read(path).ok()?);
    FontRef::from_index(&d, 0).ok()?;
    FONT_DATA.with(|c| c.borrow_mut().push((path.to_path_buf(), d.clone())));
    Some(d)
}

/// The font a drawing uses: its font (a name or a file), else a default
/// one of the system.
pub fn find_font(font: Option<&str>, family: Option<&str>) -> Option<PathBuf> {
    if let Some(f) = font {
        if let Some(p) = crate::fonts::resolve(f) {
            return Some(p);
        }
    }
    if let Some(f) = family {
        let want = f.replace(' ', "").to_ascii_lowercase();
        if let Some((_, p)) = crate::fonts::list()
            .iter()
            .find(|(n, _)| n.replace(['-', '_'], "").to_ascii_lowercase() == want)
        {
            return Some(p.clone());
        }
    }
    for name in [
        "DejaVuSans",
        "LiberationSans-Regular",
        "FreeSans",
        "Arial",
        "Helvetica",
    ] {
        if let Some(p) = crate::fonts::resolve(name) {
            return Some(p);
        }
    }
    crate::fonts::list()
        .iter()
        .find(|(_, p)| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
        })
        .map(|(_, p)| p.clone())
}

/// What MagickQueryFontMetrics() reports.
#[derive(Clone, Debug, Default)]
pub struct FontMetrics {
    pub character_width: f64,
    pub character_height: f64,
    pub ascender: f64,
    pub descender: f64,
    pub text_width: f64,
    pub text_height: f64,
    pub max_horizontal_advance: f64,
    pub bounds: (f64, f64, f64, f64),
    pub origin: (f64, f64),
    /// The post table's underline position and thickness at the rounded
    /// ppem (pixels; the position is negative below the baseline).
    pub underline_position: f64,
    pub underline_thickness: f64,
}

struct Pen<'a> {
    out: &'a mut Vec<v::PathCommand>,
    dx: f32,
    dy: f32,
    sx: f32,
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.out.push(v::PathCommand::MoveTo(v::Point {
            x: self.dx + x * self.sx,
            y: self.dy - y,
        }));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.out.push(v::PathCommand::LineTo(v::Point {
            x: self.dx + x * self.sx,
            y: self.dy - y,
        }));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.out.push(v::PathCommand::QuadCurveTo {
            control: v::Point {
                x: self.dx + cx0 * self.sx,
                y: self.dy - cy0,
            },
            end: v::Point {
                x: self.dx + x * self.sx,
                y: self.dy - y,
            },
        });
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.out.push(v::PathCommand::CubicCurveTo {
            c1: v::Point {
                x: self.dx + cx0 * self.sx,
                y: self.dy - cy0,
            },
            c2: v::Point {
                x: self.dx + cx1 * self.sx,
                y: self.dy - cy1,
            },
            end: v::Point {
                x: self.dx + x * self.sx,
                y: self.dy - y,
            },
        });
    }
    fn close(&mut self) {
        self.out.push(v::PathCommand::Close);
    }
}

/// Text laid out with a font: the glyph outlines (baseline of the first
/// line at y = 0, the text starting at x = 0) and the metrics.
/// `res` is the (x, y) resolution: sizes are points at that density.
///
/// Like ImageMagick on FreeType, glyphs are hinted for the light target
/// (the auto-hinter, vertical only) and advance by their rounded hinted
/// advances; the ascender is rounded
/// up and the descender down, the character size is the rounded ppem,
/// and the bounding box is the union of the glyphs' own boxes, each
/// relative to its origin.
pub fn layout(
    data: &[u8],
    pointsize: f64,
    res: (f64, f64),
    text: &str,
    interline: f64,
    kerning: f64,
) -> Option<(Vec<v::PathCommand>, FontMetrics)> {
    use skrifa::outline::{HintingInstance, HintingOptions};
    let font = FontRef::from_index(data, 0).ok()?;
    let px_y = (pointsize * res.1 / 72.0) as f32;
    let sx = if res.1 > 0.0 {
        (res.0 / res.1) as f32
    } else {
        1.0
    };
    let size = Size::new(px_y.max(0.01));
    let loc = LocationRef::default();
    let m = font.metrics(size, loc);
    let gm = font.glyph_metrics(size, loc);
    let cmap = font.charmap();
    let outlines = font.outline_glyphs();
    // FreeType's light target: the auto-hinter, vertical only
    let opts = HintingOptions {
        engine: skrifa::outline::Engine::Auto(None),
        target: skrifa::outline::SmoothMode::Light.into(),
    };
    let hinter = HintingInstance::new(&outlines, size, loc, opts).ok();
    let ascent = (m.ascent as f64).ceil();
    let descent = (m.descent as f64).floor();
    let line_h = ascent - descent + interline;
    let mut cmds = Vec::new();
    let mut max_w: f64 = 0.0;
    let mut max_adv: f64 = 0.0;
    let (mut bx0, mut by0, mut bx1, mut by1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let lines: Vec<&str> = text.split('\n').collect();
    // FreeType's FT_Get_Kerning(): the 'kern' table, rounded to pixels
    let kern_table = {
        use skrifa::raw::TableProvider;
        font.kern().ok()
    };
    let upem_k = {
        use skrifa::raw::TableProvider;
        font.head()
            .map(|h| h.units_per_em() as f64)
            .unwrap_or(1000.0)
    };
    let kern = |l: skrifa::GlyphId, r: skrifa::GlyphId| -> f64 {
        let Some(k) = &kern_table else {
            return 0.0;
        };
        let mut v = 0i32;
        for st in k.subtables().flatten() {
            if !st.is_horizontal() || st.is_cross_stream() || st.is_variable() {
                continue;
            }
            if let Ok(skrifa::raw::tables::kern::SubtableKind::Format0(t)) = st.kind() {
                v += t.kerning(l, r).unwrap_or(0);
            }
        }
        // scaled, damped below 25 ppem, rounded to whole pixels
        let ppem = (px_y as f64 + 0.5).floor();
        let mut k = v as f64 * px_y as f64 / upem_k;
        if ppem < 25.0 {
            k = k * ppem / 25.0;
        }
        ((k * 64.0).round() / 64.0 + 0.5).floor()
    };
    for (li, line) in lines.iter().enumerate() {
        let mut x = 0f64;
        let y = li as f64 * line_h;
        let mut prev: Option<skrifa::GlyphId> = None;
        let mut ink_right = f64::MIN;
        for ch in line.chars() {
            let gid = cmap.map(ch).unwrap_or_default();
            if let Some(pg) = prev {
                x += kern(pg, gid) * sx as f64;
            }
            prev = Some(gid);
            let mut adv = None;
            let mut glyph_x2 = f64::MIN;
            if let Some(g) = outlines.get(gid) {
                let start = cmds.len();
                let mut pen = Pen {
                    out: &mut cmds,
                    dx: x as f32,
                    dy: y as f32,
                    sx,
                };
                let r = match &hinter {
                    Some(h) => g.draw(DrawSettings::hinted(h, false), &mut pen),
                    None => g.draw(DrawSettings::unhinted(size, loc), &mut pen),
                };
                if let Ok(am) = r {
                    adv = am.advance_width.map(|a| a as f64);
                }
                for c in &cmds[start..] {
                    let pts: &[v::Point] = match c {
                        v::PathCommand::MoveTo(p) | v::PathCommand::LineTo(p) => {
                            std::slice::from_ref(p)
                        }
                        v::PathCommand::QuadCurveTo { end, .. } => std::slice::from_ref(end),
                        v::PathCommand::CubicCurveTo { end, .. } => std::slice::from_ref(end),
                        _ => &[],
                    };
                    for p in pts {
                        let (gx, gy) = (p.x as f64 - x, -(p.y as f64 - y));
                        glyph_x2 = glyph_x2.max(gx / sx as f64);
                        bx0 = bx0.min(gx);
                        bx1 = bx1.max(gx);
                        by0 = by0.min(gy);
                        by1 = by1.max(gy);
                    }
                }
            }
            let linear = gm.advance_width(gid).unwrap_or(px_y * 0.5) as f64;
            ink_right = ink_right.max((x + glyph_x2 * sx as f64).ceil());
            let adv = adv.unwrap_or(linear.round()) * sx as f64;
            max_adv = max_adv.max(adv);
            x += adv + kerning;
        }
        // the text ends at the last pen position, or past it where a
        // glyph's ink reaches further (rounded up)
        max_w = max_w.max(x).max(ink_right);
    }
    if bx0 > bx1 {
        (bx0, by0, bx1, by1) = (0.0, 0.0, 0.0, 0.0);
    }
    // the box always holds the origin's column
    bx0 = bx0.min(0.0);
    let n = lines.len() as f64;
    let ppem_x = (pointsize * res.0 / 72.0 + 0.5).floor();
    let ppem_y = (pointsize * res.1 / 72.0 + 0.5).floor();
    let (upos, uthick) = {
        use skrifa::raw::TableProvider;
        let upem = font
            .head()
            .map(|h| h.units_per_em() as f64)
            .unwrap_or(1000.0);
        font.post()
            .map(|p| {
                // FreeType moves the position to the bar's top
                let (pos, thick) = (
                    p.underline_position().to_i16() as i32,
                    p.underline_thickness().to_i16() as i32,
                );
                (
                    (pos - thick / 2) as f64 * ppem_y / upem,
                    thick as f64 * ppem_y / upem,
                )
            })
            .unwrap_or((0.0, 0.0))
    };
    let metrics = FontMetrics {
        character_width: ppem_x,
        character_height: ppem_y,
        ascender: ascent,
        descender: descent,
        text_width: max_w,
        text_height: if n > 1.0 { line_h * n } else { ppem_y },
        max_horizontal_advance: m.max_width.map(|w| (w as f64).round()).unwrap_or(max_adv),
        bounds: (bx0, by0, bx1, by1),
        origin: (max_w, 0.0),
        underline_position: upos,
        underline_thickness: uthick,
    };
    Some((cmds, metrics))
}
