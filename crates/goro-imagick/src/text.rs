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
pub fn layout(
    data: &[u8],
    pointsize: f64,
    res: (f64, f64),
    text: &str,
    interline: f64,
    kerning: f64,
) -> Option<(Vec<v::PathCommand>, FontMetrics)> {
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
    let ascent = m.ascent as f64;
    let descent = m.descent as f64;
    let line_h = ascent - descent + interline;
    let mut cmds = Vec::new();
    let mut max_w: f64 = 0.0;
    let mut max_adv: f64 = 0.0;
    let lines: Vec<&str> = text.split('\n').collect();
    for (li, line) in lines.iter().enumerate() {
        let mut x = 0f64;
        let y = li as f64 * line_h;
        for ch in line.chars() {
            let gid = cmap.map(ch).unwrap_or_default();
            if let Some(g) = outlines.get(gid) {
                let mut pen = Pen {
                    out: &mut cmds,
                    dx: x as f32,
                    dy: y as f32,
                    sx,
                };
                let _ = g.draw(DrawSettings::unhinted(size, loc), &mut pen);
            }
            let adv = gm.advance_width(gid).unwrap_or(px_y * 0.5) as f64 * sx as f64;
            max_adv = max_adv.max(adv);
            x += adv + kerning;
        }
        max_w = max_w.max(x);
    }
    let n = lines.len() as f64;
    let metrics = FontMetrics {
        character_width: pointsize * res.0 / 72.0,
        character_height: pointsize * res.1 / 72.0,
        ascender: ascent,
        descender: descent,
        text_width: max_w,
        text_height: line_h * n - if n > 1.0 { 0.0 } else { interline },
        max_horizontal_advance: m.max_width.map(|w| w as f64).unwrap_or(max_adv),
        bounds: (0.0, descent, max_w, ascent),
        origin: (max_w, 0.0),
    };
    Some((cmds, metrics))
}
