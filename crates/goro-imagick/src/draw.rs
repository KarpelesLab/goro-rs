//! The DrawingWand: a graphic-context stack, the vector graphics (MVG) it
//! records, and the rendering of its primitives onto an image through
//! oxideav-raster.

use oxideav_core::vector as v;

use crate::color::{PixelInfo, fmt_g_prec};
use crate::composite::{ComposeArgs, composite_image};
use crate::image::Image;
use crate::types::*;

/// An affine matrix (sx, rx, ry, sy, tx, ty) as AffineMatrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub sx: f64,
    pub rx: f64,
    pub ry: f64,
    pub sy: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Affine {
            sx: 1.0,
            rx: 0.0,
            ry: 0.0,
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

impl Affine {
    /// `self` then `o` in local coordinates (CTM × o).
    pub fn then(&self, o: &Affine) -> Affine {
        // points: x' = sx*x + ry*y + tx ; y' = rx*x + sy*y + ty
        Affine {
            sx: self.sx * o.sx + self.ry * o.rx,
            rx: self.rx * o.sx + self.sy * o.rx,
            ry: self.sx * o.ry + self.ry * o.sy,
            sy: self.rx * o.ry + self.sy * o.sy,
            tx: self.sx * o.tx + self.ry * o.ty + self.tx,
            ty: self.rx * o.tx + self.sy * o.ty + self.ty,
        }
    }

    fn to_v(self) -> v::Transform2D {
        v::Transform2D {
            a: self.sx as f32,
            b: self.rx as f32,
            c: self.ry as f32,
            d: self.sy as f32,
            e: self.tx as f32,
            f: self.ty as f32,
        }
    }
}

/// The DrawInfo of a graphic context.
#[derive(Clone, Debug)]
pub struct GraphicContext {
    pub fill: PixelInfo,
    pub fill_opacity: f64,
    pub fill_rule: i32,
    pub fill_pattern: Option<String>,
    pub stroke: PixelInfo,
    pub stroke_width: f64,
    pub stroke_antialias: bool,
    pub stroke_pattern: Option<String>,
    pub line_cap: i32,
    pub line_join: i32,
    pub miter_limit: f64,
    pub dash: Vec<f64>,
    pub dash_offset: f64,
    pub clip_path: Option<String>,
    pub clip_rule: i32,
    pub clip_units: i32,
    pub font: Option<String>,
    pub family: Option<String>,
    pub font_size: f64,
    pub font_style: i32,
    pub font_weight: i64,
    pub font_stretch: i32,
    pub text_align: i32,
    pub text_antialias: bool,
    pub decoration: i32,
    pub under_color: PixelInfo,
    pub encoding: Option<String>,
    pub kerning: f64,
    pub interline: f64,
    pub interword: f64,
    pub direction: i32,
    pub gravity: Gravity,
    pub opacity: f64,
    pub border_color: PixelInfo,
    pub affine: Affine,
    pub viewbox: Option<(i64, i64, i64, i64)>,
}

impl Default for GraphicContext {
    fn default() -> Self {
        let mut fill = PixelInfo::black();
        fill.alpha_trait = true;
        let mut stroke = PixelInfo::white();
        stroke.alpha = 0.0;
        stroke.alpha_trait = true;
        // transparent black: no box under the text
        let under = PixelInfo {
            colorspace: Colorspace::Undefined,
            alpha: 0.0,
            ..Default::default()
        };
        GraphicContext {
            fill,
            fill_opacity: 1.0,
            fill_rule: 1,
            fill_pattern: None,
            stroke,
            stroke_width: 1.0,
            stroke_antialias: true,
            stroke_pattern: None,
            line_cap: 1,
            line_join: 1,
            miter_limit: 10.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            clip_path: None,
            clip_rule: 1,
            clip_units: 0,
            font: None,
            family: None,
            font_size: 12.0,
            font_style: 0,
            font_weight: 0,
            font_stretch: 0,
            text_align: 0,
            text_antialias: true,
            decoration: 1,
            under_color: under,
            encoding: None,
            kerning: 0.0,
            interline: 0.0,
            interword: 0.0,
            direction: 0,
            gravity: Gravity::Undefined,
            opacity: 1.0,
            border_color: PixelInfo::from_rgba8([223, 223, 223, 255]),
            affine: Affine::default(),
            viewbox: None,
        }
    }
}

/// A recorded primitive, in user space.
#[derive(Clone, Debug)]
pub enum Shape {
    Path {
        path: Vec<v::PathCommand>,
        /// Open paths are stroked only (lines, polylines).
        open: bool,
    },
    Point(f64, f64),
    Text {
        x: f64,
        y: f64,
        text: String,
    },
    Composite {
        op: CompositeOp,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        image: Box<Image>,
    },
}

#[derive(Clone, Debug)]
pub struct Primitive {
    pub shape: Shape,
    pub gc: GraphicContext,
}

/// Where primitives go: the drawing, a clip-path or a pattern definition.
#[derive(Clone, Debug)]
enum Target {
    Clip(String),
    Pattern(String),
    Defs,
}

#[derive(Clone, Debug, Default)]
pub struct DrawingWand {
    pub stack: Vec<GraphicContext>,
    pub primitives: Vec<Primitive>,
    pub clip_paths: Vec<(String, Vec<Primitive>)>,
    pub patterns: Vec<(String, (f64, f64, f64, f64), Vec<Primitive>)>,
    targets: Vec<Target>,
    /// The vector graphics (MVG) text.
    pub mvg: String,
    indent: usize,
    path: Vec<v::PathCommand>,
    path_mvg: String,
    path_start: Option<(f64, f64)>,
    cur: (f64, f64),
    last_ctrl: Option<(f64, f64)>,
    last_qctrl: Option<(f64, f64)>,
    pub density: Option<String>,
    pub font_resolution: (f64, f64),
    pub exception: Option<crate::error::MagickError>,
}

/// A number as MVG prints it.
pub fn num(x: f64) -> String {
    if x == x.trunc() && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let s = fmt_g_prec(x, 20);
        // the shortest representation reads better and matches %g for
        // the values callers pass
        let short = format!("{}", x);
        if short.len() < s.len() { short } else { s }
    }
}

/// A colour as "#RRGGBB" ("#RRGGBBAA" when it carries alpha).
fn hex_color(c: &PixelInfo) -> String {
    let q = c.to_rgba16();
    let b = |v: u16| crate::image::q_to_u8(v);
    if c.alpha_trait {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            b(q[0]),
            b(q[1]),
            b(q[2]),
            b(q[3])
        )
    } else {
        format!("#{:02X}{:02X}{:02X}", b(q[0]), b(q[1]), b(q[2]))
    }
}

fn mvg_color(c: &PixelInfo) -> String {
    hex_color(c)
}

fn pt(x: f64, y: f64) -> v::Point {
    v::Point {
        x: x as f32,
        y: y as f32,
    }
}

const LINECAP: &[&str] = &["Undefined", "Butt", "Round", "Square"];
const LINEJOIN: &[&str] = &["Undefined", "Miter", "Round", "Bevel"];
const FILLRULE: &[&str] = &["Undefined", "Evenodd", "NonZero"];

fn name_of(t: &[&'static str], i: i32) -> &'static str {
    t.get(i as usize).copied().unwrap_or("Undefined")
}

impl DrawingWand {
    pub fn new() -> DrawingWand {
        DrawingWand {
            stack: vec![GraphicContext::default()],
            font_resolution: (72.0, 72.0),
            ..Default::default()
        }
    }

    pub fn gc(&self) -> &GraphicContext {
        self.stack.last().unwrap()
    }

    pub fn gc_mut(&mut self) -> &mut GraphicContext {
        self.stack.last_mut().unwrap()
    }

    /// Append a line of MVG.
    pub fn print(&mut self, line: &str) {
        for _ in 0..self.indent {
            self.mvg.push(' ');
        }
        self.mvg.push_str(line);
        self.mvg.push('\n');
    }

    fn add(&mut self, shape: Shape) {
        let p = Primitive {
            shape,
            gc: self.gc().clone(),
        };
        match self.targets.last_mut() {
            Some(Target::Clip(id)) => {
                let id = id.clone();
                if let Some(e) = self.clip_paths.iter_mut().find(|e| e.0 == id) {
                    e.1.push(p);
                }
            }
            Some(Target::Pattern(id)) => {
                let id = id.clone();
                if let Some(e) = self.patterns.iter_mut().find(|e| e.0 == id) {
                    e.2.push(p);
                }
            }
            Some(Target::Defs) | None => self.primitives.push(p),
        }
    }

    // --- graphic context ------------------------------------------------

    pub fn push(&mut self) {
        let gc = self.gc().clone();
        self.stack.push(gc);
        self.print("push graphic-context");
        self.indent += 1;
    }

    pub fn pop(&mut self) -> bool {
        if self.stack.len() <= 1 {
            return false;
        }
        self.stack.pop();
        self.indent = self.indent.saturating_sub(1);
        self.print("pop graphic-context");
        true
    }

    pub fn set_fill_color(&mut self, c: &PixelInfo) {
        let c = c.clone();
        self.print(&format!("fill '{}'", mvg_color(&c)));
        self.gc_mut().fill = c;
        self.gc_mut().fill_pattern = None;
    }

    pub fn set_stroke_color(&mut self, c: &PixelInfo) {
        let c = c.clone();
        self.print(&format!("stroke '{}'", mvg_color(&c)));
        self.gc_mut().stroke = c;
        self.gc_mut().stroke_pattern = None;
    }

    pub fn set_fill_opacity(&mut self, o: f64) {
        self.print(&format!("fill-opacity {}", num(o)));
        self.gc_mut().fill_opacity = crate::color::clamp_to_quantum(o * 65535.0) / 65535.0;
    }

    pub fn set_stroke_opacity(&mut self, o: f64) {
        self.print(&format!("stroke-opacity {}", num(o)));
        self.gc_mut().stroke.alpha = crate::color::clamp_to_quantum(o * 65535.0);
    }

    pub fn set_opacity(&mut self, o: f64) {
        self.print(&format!("opacity {}", num(o)));
        self.gc_mut().opacity = o;
    }

    pub fn set_stroke_width(&mut self, w: f64) {
        self.print(&format!("stroke-width {}", num(w)));
        self.gc_mut().stroke_width = w;
    }

    pub fn set_stroke_line_cap(&mut self, c: i32) {
        self.print(&format!("stroke-linecap '{}'", name_of(LINECAP, c)));
        self.gc_mut().line_cap = c;
    }

    pub fn set_stroke_line_join(&mut self, j: i32) {
        self.print(&format!("stroke-linejoin '{}'", name_of(LINEJOIN, j)));
        self.gc_mut().line_join = j;
    }

    pub fn set_stroke_miter_limit(&mut self, m: f64) {
        self.print(&format!("stroke-miterlimit {}", num(m)));
        self.gc_mut().miter_limit = m;
    }

    pub fn set_stroke_dash_array(&mut self, d: Vec<f64>) {
        if d.is_empty() {
            self.print("stroke-dasharray none");
        } else {
            let s: Vec<String> = d.iter().map(|x| num(*x)).collect();
            self.print(&format!("stroke-dasharray {}", s.join(",")));
        }
        self.gc_mut().dash = d;
    }

    pub fn set_stroke_dash_offset(&mut self, o: f64) {
        self.print(&format!("stroke-dashoffset {}", num(o)));
        self.gc_mut().dash_offset = o;
    }

    pub fn set_fill_rule(&mut self, r: i32) {
        self.print(&format!(
            "fill-rule '{}'",
            name_of(FILLRULE, r).to_ascii_lowercase()
        ));
        self.gc_mut().fill_rule = r;
    }

    pub fn set_gravity(&mut self, g: Gravity) {
        self.print(&format!("gravity '{}'", g.name()));
        self.gc_mut().gravity = g;
    }

    // --- transforms -----------------------------------------------------

    fn transform(&mut self, a: Affine) {
        let cur = self.gc().affine;
        self.gc_mut().affine = cur.then(&a);
    }

    pub fn affine(&mut self, a: Affine) {
        self.print(&format!(
            "affine {} {} {} {} {} {}",
            num(a.sx),
            num(a.rx),
            num(a.ry),
            num(a.sy),
            num(a.tx),
            num(a.ty)
        ));
        self.transform(a);
    }

    pub fn translate(&mut self, x: f64, y: f64) {
        self.print(&format!("translate {} {}", num(x), num(y)));
        self.transform(Affine {
            tx: x,
            ty: y,
            ..Default::default()
        });
    }

    pub fn scale(&mut self, x: f64, y: f64) {
        self.print(&format!("scale {} {}", num(x), num(y)));
        self.transform(Affine {
            sx: x,
            sy: y,
            ..Default::default()
        });
    }

    pub fn rotate(&mut self, deg: f64) {
        self.print(&format!("rotate {}", num(deg)));
        let (s, c) = deg.to_radians().sin_cos();
        self.transform(Affine {
            sx: c,
            rx: s,
            ry: -s,
            sy: c,
            ..Default::default()
        });
    }

    pub fn skew_x(&mut self, deg: f64) {
        self.print(&format!("skewX {}", num(deg)));
        self.transform(Affine {
            ry: deg.to_radians().tan(),
            ..Default::default()
        });
    }

    pub fn skew_y(&mut self, deg: f64) {
        self.print(&format!("skewY {}", num(deg)));
        self.transform(Affine {
            rx: deg.to_radians().tan(),
            ..Default::default()
        });
    }

    // --- primitives -----------------------------------------------------

    pub fn rectangle(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.print(&format!(
            "rectangle {} {} {} {}",
            num(x1),
            num(y1),
            num(x2),
            num(y2)
        ));
        // the corners are pixel centres: the fill covers both of them
        let (x1, x2) = (x1.min(x2) - 0.5, x1.max(x2) + 0.5);
        let (y1, y2) = (y1.min(y2) - 0.5, y1.max(y2) + 0.5);
        let path = vec![
            v::PathCommand::MoveTo(pt(x1, y1)),
            v::PathCommand::LineTo(pt(x2, y1)),
            v::PathCommand::LineTo(pt(x2, y2)),
            v::PathCommand::LineTo(pt(x1, y2)),
            v::PathCommand::Close,
        ];
        self.add(Shape::Path { path, open: false });
    }

    pub fn round_rectangle(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, rx: f64, ry: f64) {
        self.print(&format!(
            "roundrectangle {} {} {} {} {} {}",
            num(x1),
            num(y1),
            num(x2),
            num(y2),
            num(rx),
            num(ry)
        ));
        let (x1, x2) = (x1.min(x2) - 0.5, x1.max(x2) + 0.5);
        let (y1, y2) = (y1.min(y2) - 0.5, y1.max(y2) + 0.5);
        let rx = rx.min((x2 - x1) / 2.0).max(0.0);
        let ry = ry.min((y2 - y1) / 2.0).max(0.0);
        let arc = |end: v::Point| v::PathCommand::ArcTo {
            rx: rx as f32,
            ry: ry as f32,
            x_axis_rot: 0.0,
            large_arc: false,
            sweep: true,
            end,
        };
        let path = vec![
            v::PathCommand::MoveTo(pt(x1 + rx, y1)),
            v::PathCommand::LineTo(pt(x2 - rx, y1)),
            arc(pt(x2, y1 + ry)),
            v::PathCommand::LineTo(pt(x2, y2 - ry)),
            arc(pt(x2 - rx, y2)),
            v::PathCommand::LineTo(pt(x1 + rx, y2)),
            arc(pt(x1, y2 - ry)),
            v::PathCommand::LineTo(pt(x1, y1 + ry)),
            arc(pt(x1 + rx, y1)),
            v::PathCommand::Close,
        ];
        self.add(Shape::Path { path, open: false });
    }

    fn ellipse_path(
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        start: f64,
        end: f64,
        close_center: bool,
    ) -> Vec<v::PathCommand> {
        // a polyline approximation of the arc (angles in degrees, y down)
        let mut end = end;
        while end < start {
            end += 360.0;
        }
        let span = (end - start).min(360.0);
        let steps = ((span / 360.0) * (rx.max(ry) * 4.0).clamp(16.0, 720.0))
            .ceil()
            .max(2.0) as usize;
        let mut path = Vec::with_capacity(steps + 3);
        if close_center {
            path.push(v::PathCommand::MoveTo(pt(cx, cy)));
        }
        for i in 0..=steps {
            let a = (start + span * i as f64 / steps as f64).to_radians();
            let p = pt(cx + rx * a.cos(), cy + ry * a.sin());
            if i == 0 && !close_center {
                path.push(v::PathCommand::MoveTo(p));
            } else {
                path.push(v::PathCommand::LineTo(p));
            }
        }
        if close_center || span >= 360.0 {
            path.push(v::PathCommand::Close);
        }
        path
    }

    pub fn circle(&mut self, ox: f64, oy: f64, px: f64, py: f64) {
        self.print(&format!(
            "circle {} {} {} {}",
            num(ox),
            num(oy),
            num(px),
            num(py)
        ));
        let r = ((px - ox).powi(2) + (py - oy).powi(2)).sqrt();
        let path = Self::ellipse_path(ox, oy, r, r, 0.0, 360.0, false);
        self.add(Shape::Path { path, open: false });
    }

    pub fn ellipse(&mut self, ox: f64, oy: f64, rx: f64, ry: f64, start: f64, end: f64) {
        self.print(&format!(
            "ellipse {} {} {} {} {} {}",
            num(ox),
            num(oy),
            num(rx),
            num(ry),
            num(start),
            num(end)
        ));
        let path = Self::ellipse_path(ox, oy, rx, ry, start, end, false);
        let open = (end - start).abs() < 360.0;
        self.add(Shape::Path { path, open });
    }

    pub fn arc(&mut self, sx: f64, sy: f64, ex: f64, ey: f64, sd: f64, ed: f64) {
        self.print(&format!(
            "arc {} {} {} {} {} {}",
            num(sx),
            num(sy),
            num(ex),
            num(ey),
            num(sd),
            num(ed)
        ));
        let (cx, cy) = ((sx + ex) / 2.0, (sy + ey) / 2.0);
        let (rx, ry) = ((ex - sx).abs() / 2.0, (ey - sy).abs() / 2.0);
        let path = Self::ellipse_path(cx, cy, rx, ry, sd, ed, false);
        self.add(Shape::Path { path, open: false });
    }

    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.print(&format!(
            "line {} {} {} {}",
            num(x1),
            num(y1),
            num(x2),
            num(y2)
        ));
        // both end points are drawn: extend the line by half a pixel
        let (dx, dy) = (x2 - x1, y2 - y1);
        let len = (dx * dx + dy * dy).sqrt();
        let (ex, ey) = if len > 0.0 {
            (dx / len * 0.5, dy / len * 0.5)
        } else {
            (0.5, 0.0)
        };
        let path = vec![
            v::PathCommand::MoveTo(pt(x1 - ex, y1 - ey)),
            v::PathCommand::LineTo(pt(x2 + ex, y2 + ey)),
        ];
        self.add(Shape::Path { path, open: true });
    }

    pub fn point(&mut self, x: f64, y: f64) {
        self.print(&format!("point {} {}", num(x), num(y)));
        self.add(Shape::Point(x, y));
    }

    fn poly(&mut self, kind: &str, pts: &[(f64, f64)], close: bool) {
        let s: Vec<String> = pts
            .iter()
            .map(|(x, y)| format!("{} {}", num(*x), num(*y)))
            .collect();
        self.print(&format!("{} {}", kind, s.join(" ")));
        if pts.is_empty() {
            return;
        }
        let mut path = Vec::with_capacity(pts.len() + 1);
        for (i, &(x, y)) in pts.iter().enumerate() {
            path.push(if i == 0 {
                v::PathCommand::MoveTo(pt(x, y))
            } else {
                v::PathCommand::LineTo(pt(x, y))
            });
        }
        if close {
            path.push(v::PathCommand::Close);
        }
        self.add(Shape::Path { path, open: !close });
    }

    pub fn polygon(&mut self, pts: &[(f64, f64)]) {
        self.poly("polygon", pts, true);
    }

    pub fn polyline(&mut self, pts: &[(f64, f64)]) {
        self.poly("polyline", pts, false);
    }

    pub fn bezier(&mut self, pts: &[(f64, f64)]) {
        let s: Vec<String> = pts
            .iter()
            .map(|(x, y)| format!("{} {}", num(*x), num(*y)))
            .collect();
        self.print(&format!("bezier {}", s.join(" ")));
        if pts.len() < 2 {
            return;
        }
        // a Bézier curve of any degree: evaluate it with de Casteljau
        let n = 64;
        let mut path = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let mut p: Vec<(f64, f64)> = pts.to_vec();
            while p.len() > 1 {
                p = p
                    .windows(2)
                    .map(|w| {
                        (
                            w[0].0 + (w[1].0 - w[0].0) * t,
                            w[0].1 + (w[1].1 - w[0].1) * t,
                        )
                    })
                    .collect();
            }
            let q = pt(p[0].0, p[0].1);
            path.push(if i == 0 {
                v::PathCommand::MoveTo(q)
            } else {
                v::PathCommand::LineTo(q)
            });
        }
        self.add(Shape::Path { path, open: false });
    }

    pub fn annotation(&mut self, x: f64, y: f64, text: &str) {
        let esc = text.replace('\\', "\\\\").replace('\'', "\\'");
        self.print(&format!("text {} {} '{}'", num(x), num(y), esc));
        self.add(Shape::Text {
            x,
            y,
            text: text.to_string(),
        });
    }

    pub fn composite(&mut self, op: CompositeOp, x: f64, y: f64, w: f64, h: f64, image: &Image) {
        self.print(&format!(
            "image {} {} {} {} {} 'data:'",
            op_name(op),
            num(x),
            num(y),
            num(w),
            num(h)
        ));
        self.add(Shape::Composite {
            op,
            x,
            y,
            w,
            h,
            image: Box::new(image.clone()),
        });
    }

    // --- paths ----------------------------------------------------------

    pub fn path_start(&mut self) {
        self.path.clear();
        self.path_mvg.clear();
        self.path_start = None;
        self.cur = (0.0, 0.0);
        self.last_ctrl = None;
        self.last_qctrl = None;
    }

    pub fn path_finish(&mut self) {
        let mvg = std::mem::take(&mut self.path_mvg);
        self.print(&format!("path '{}'", mvg));
        let path = std::mem::take(&mut self.path);
        if !path.is_empty() {
            self.add(Shape::Path { path, open: false });
        }
    }

    fn path_text(&mut self, s: &str) {
        self.path_mvg.push_str(s);
    }

    pub fn path_move_to(&mut self, rel: bool, x: f64, y: f64) {
        let (x, y) = if rel {
            (self.cur.0 + x, self.cur.1 + y)
        } else {
            (x, y)
        };
        self.path_text(&format!(
            "{}{} {}",
            if rel { 'm' } else { 'M' },
            num(x),
            num(y)
        ));
        self.path.push(v::PathCommand::MoveTo(pt(x, y)));
        self.path_start = Some((x, y));
        self.cur = (x, y);
        self.last_ctrl = None;
        self.last_qctrl = None;
    }

    pub fn path_line_to(&mut self, rel: bool, x: f64, y: f64) {
        let (ox, oy) = (x, y);
        let (x, y) = if rel {
            (self.cur.0 + x, self.cur.1 + y)
        } else {
            (x, y)
        };
        self.path_text(&format!(
            "{}{} {}",
            if rel { 'l' } else { 'L' },
            num(ox),
            num(oy)
        ));
        self.ensure_start();
        self.path.push(v::PathCommand::LineTo(pt(x, y)));
        self.cur = (x, y);
        self.last_ctrl = None;
        self.last_qctrl = None;
    }

    pub fn path_line_to_h(&mut self, rel: bool, x: f64) {
        let nx = if rel { self.cur.0 + x } else { x };
        self.path_text(&format!("{}{}", if rel { 'h' } else { 'H' }, num(x)));
        self.ensure_start();
        self.path.push(v::PathCommand::LineTo(pt(nx, self.cur.1)));
        self.cur.0 = nx;
    }

    pub fn path_line_to_v(&mut self, rel: bool, y: f64) {
        let ny = if rel { self.cur.1 + y } else { y };
        self.path_text(&format!("{}{}", if rel { 'v' } else { 'V' }, num(y)));
        self.ensure_start();
        self.path.push(v::PathCommand::LineTo(pt(self.cur.0, ny)));
        self.cur.1 = ny;
    }

    fn ensure_start(&mut self) {
        if self.path.is_empty() {
            self.path
                .push(v::PathCommand::MoveTo(pt(self.cur.0, self.cur.1)));
            self.path_start = Some(self.cur);
        }
    }

    pub fn path_curve_to(&mut self, rel: bool, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) {
        self.path_text(&format!(
            "{}{} {} {} {} {} {}",
            if rel { 'c' } else { 'C' },
            num(x1),
            num(y1),
            num(x2),
            num(y2),
            num(x),
            num(y)
        ));
        let o = if rel { self.cur } else { (0.0, 0.0) };
        let (c1, c2, e) = (
            (o.0 + x1, o.1 + y1),
            (o.0 + x2, o.1 + y2),
            (o.0 + x, o.1 + y),
        );
        self.ensure_start();
        self.path.push(v::PathCommand::CubicCurveTo {
            c1: pt(c1.0, c1.1),
            c2: pt(c2.0, c2.1),
            end: pt(e.0, e.1),
        });
        self.last_ctrl = Some(c2);
        self.last_qctrl = None;
        self.cur = e;
    }

    pub fn path_curve_to_smooth(&mut self, rel: bool, x2: f64, y2: f64, x: f64, y: f64) {
        self.path_text(&format!(
            "{}{} {} {} {}",
            if rel { 's' } else { 'S' },
            num(x2),
            num(y2),
            num(x),
            num(y)
        ));
        let o = if rel { self.cur } else { (0.0, 0.0) };
        let c1 = match self.last_ctrl {
            Some(c) => (2.0 * self.cur.0 - c.0, 2.0 * self.cur.1 - c.1),
            None => self.cur,
        };
        let (c2, e) = ((o.0 + x2, o.1 + y2), (o.0 + x, o.1 + y));
        self.ensure_start();
        self.path.push(v::PathCommand::CubicCurveTo {
            c1: pt(c1.0, c1.1),
            c2: pt(c2.0, c2.1),
            end: pt(e.0, e.1),
        });
        self.last_ctrl = Some(c2);
        self.last_qctrl = None;
        self.cur = e;
    }

    pub fn path_quad_to(&mut self, rel: bool, x1: f64, y1: f64, x: f64, y: f64) {
        self.path_text(&format!(
            "{}{} {} {} {}",
            if rel { 'q' } else { 'Q' },
            num(x1),
            num(y1),
            num(x),
            num(y)
        ));
        let o = if rel { self.cur } else { (0.0, 0.0) };
        let (c, e) = ((o.0 + x1, o.1 + y1), (o.0 + x, o.1 + y));
        self.ensure_start();
        self.path.push(v::PathCommand::QuadCurveTo {
            control: pt(c.0, c.1),
            end: pt(e.0, e.1),
        });
        self.last_qctrl = Some(c);
        self.last_ctrl = None;
        self.cur = e;
    }

    pub fn path_quad_to_smooth(&mut self, rel: bool, x: f64, y: f64) {
        self.path_text(&format!(
            "{}{} {}",
            if rel { 't' } else { 'T' },
            num(x),
            num(y)
        ));
        let o = if rel { self.cur } else { (0.0, 0.0) };
        let c = match self.last_qctrl {
            Some(c) => (2.0 * self.cur.0 - c.0, 2.0 * self.cur.1 - c.1),
            None => self.cur,
        };
        let e = (o.0 + x, o.1 + y);
        self.ensure_start();
        self.path.push(v::PathCommand::QuadCurveTo {
            control: pt(c.0, c.1),
            end: pt(e.0, e.1),
        });
        self.last_qctrl = Some(c);
        self.last_ctrl = None;
        self.cur = e;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn path_arc(
        &mut self,
        rel: bool,
        rx: f64,
        ry: f64,
        rot: f64,
        large: bool,
        sweep: bool,
        x: f64,
        y: f64,
    ) {
        self.path_text(&format!(
            "{}{} {} {} {} {} {} {}",
            if rel { 'a' } else { 'A' },
            num(rx),
            num(ry),
            num(rot),
            large as u8,
            sweep as u8,
            num(x),
            num(y)
        ));
        let o = if rel { self.cur } else { (0.0, 0.0) };
        let e = (o.0 + x, o.1 + y);
        self.ensure_start();
        self.path.push(v::PathCommand::ArcTo {
            rx: rx as f32,
            ry: ry as f32,
            x_axis_rot: rot.to_radians() as f32,
            large_arc: large,
            sweep,
            end: pt(e.0, e.1),
        });
        self.cur = e;
        self.last_ctrl = None;
        self.last_qctrl = None;
    }

    pub fn path_close(&mut self) {
        self.path_text("Z");
        self.path.push(v::PathCommand::Close);
        if let Some(s) = self.path_start {
            self.cur = s;
        }
    }

    // --- clip paths, patterns, defs ---------------------------------------

    pub fn push_clip_path(&mut self, id: &str) {
        self.print(&format!("push clip-path &quot;{}&quot;", id));
        self.indent += 1;
        self.clip_paths.retain(|e| e.0 != id);
        self.clip_paths.push((id.to_string(), Vec::new()));
        self.targets.push(Target::Clip(id.to_string()));
    }

    pub fn pop_clip_path(&mut self) {
        self.indent = self.indent.saturating_sub(1);
        self.print("pop clip-path");
        if matches!(self.targets.last(), Some(Target::Clip(_))) {
            self.targets.pop();
        }
    }

    pub fn set_clip_path(&mut self, id: &str) {
        self.print(&format!("clip-path url(#{})", id));
        self.gc_mut().clip_path = Some(id.to_string());
    }

    pub fn push_pattern(&mut self, id: &str, x: f64, y: f64, w: f64, h: f64) {
        self.print(&format!(
            "push pattern {} {} {} {} {}",
            id,
            num(x),
            num(y),
            num(w),
            num(h)
        ));
        self.indent += 1;
        self.patterns.retain(|e| e.0 != id);
        self.patterns
            .push((id.to_string(), (x, y, w, h), Vec::new()));
        self.targets.push(Target::Pattern(id.to_string()));
    }

    pub fn pop_pattern(&mut self) -> bool {
        if !matches!(self.targets.last(), Some(Target::Pattern(_))) {
            return false;
        }
        self.indent = self.indent.saturating_sub(1);
        self.print("pop pattern");
        self.targets.pop();
        true
    }

    pub fn push_defs(&mut self) {
        self.print("push defs");
        self.indent += 1;
        self.targets.push(Target::Defs);
    }

    pub fn pop_defs(&mut self) {
        self.indent = self.indent.saturating_sub(1);
        self.print("pop defs");
        if matches!(self.targets.last(), Some(Target::Defs)) {
            self.targets.pop();
        }
    }

    pub fn set_fill_pattern(&mut self, url: &str) -> bool {
        let id = url
            .trim_start_matches("url(")
            .trim_end_matches(')')
            .trim_start_matches('#');
        if !self.patterns.iter().any(|e| e.0 == id) {
            return false;
        }
        self.print(&format!("fill url(#{})", id));
        self.gc_mut().fill_pattern = Some(id.to_string());
        true
    }

    pub fn set_stroke_pattern(&mut self, url: &str) -> bool {
        let id = url
            .trim_start_matches("url(")
            .trim_end_matches(')')
            .trim_start_matches('#');
        if !self.patterns.iter().any(|e| e.0 == id) {
            return false;
        }
        self.print(&format!("stroke url(#{})", id));
        self.gc_mut().stroke_pattern = Some(id.to_string());
        true
    }

    /// DrawGetVectorGraphics(): the state as XML and the MVG.
    pub fn vector_graphics(&self) -> String {
        let g = self.gc();
        let opt = |tag: &str, v: &Option<String>| match v {
            Some(v) => format!("<{t}>{v}</{t}>", t = tag, v = v),
            None => format!("<{}/>", tag),
        };
        let mut s = String::from("<drawing-wand>");
        s.push_str(&opt("clip-path", &g.clip_path));
        s.push_str(&format!(
            "<clip-units>{}</clip-units>",
            [
                "Undefined",
                "UserSpace",
                "UserSpaceOnUse",
                "ObjectBoundingBox"
            ]
            .get(g.clip_units as usize)
            .unwrap_or(&"Undefined")
        ));
        s.push_str(&format!(
            "<decorate>{}</decorate>",
            ["Undefined", "None", "Underline", "Overline", "LineThrough"]
                .get(g.decoration as usize)
                .unwrap_or(&"Undefined")
        ));
        s.push_str(&opt("encoding", &g.encoding));
        s.push_str(&format!("<fill>{}</fill>", hex_color(&g.fill)));
        s.push_str(&format!(
            "<fill-opacity>{}</fill-opacity>",
            fmt_g_prec(g.fill_opacity, 20)
        ));
        s.push_str(&format!(
            "<fill-rule>{}</fill-rule>",
            name_of(FILLRULE, g.fill_rule)
        ));
        s.push_str(&opt("font", &g.font));
        s.push_str(&opt("font-family", &g.family));
        s.push_str(&format!(
            "<font-size>{}</font-size>",
            fmt_g_prec(g.font_size, 20)
        ));
        s.push_str(&format!(
            "<font-stretch>{}</font-stretch>",
            [
                "Undefined",
                "Normal",
                "UltraCondensed",
                "ExtraCondensed",
                "Condensed",
                "SemiCondensed",
                "SemiExpanded",
                "Expanded",
                "ExtraExpanded",
                "UltraExpanded",
                "Any"
            ]
            .get(g.font_stretch as usize)
            .unwrap_or(&"Undefined")
        ));
        s.push_str(&format!(
            "<font-style>{}</font-style>",
            ["Undefined", "Normal", "Italic", "Oblique", "Any", "Bold"]
                .get(g.font_style as usize)
                .unwrap_or(&"Undefined")
        ));
        s.push_str(&format!("<font-weight>{}</font-weight>", g.font_weight));
        s.push_str(&format!("<gravity>{}</gravity>", g.gravity.name()));
        s.push_str(&format!("<stroke>{}</stroke>", hex_color(&g.stroke)));
        s.push_str(&format!(
            "<stroke-antialias>{}</stroke-antialias>",
            g.stroke_antialias as u8
        ));
        if g.dash.is_empty() {
            s.push_str("<stroke-dasharray/>");
        } else {
            let d: Vec<String> = g.dash.iter().map(|x| num(*x)).collect();
            s.push_str(&format!(
                "<stroke-dasharray>{}</stroke-dasharray>",
                d.join(",")
            ));
        }
        s.push_str(&format!(
            "<stroke-dashoffset>{}</stroke-dashoffset>",
            fmt_g_prec(g.dash_offset, 20)
        ));
        s.push_str(&format!(
            "<stroke-linecap>{}</stroke-linecap>",
            name_of(LINECAP, g.line_cap)
        ));
        s.push_str(&format!(
            "<stroke-linejoin>{}</stroke-linejoin>",
            name_of(LINEJOIN, g.line_join)
        ));
        s.push_str(&format!(
            "<stroke-miterlimit>{}</stroke-miterlimit>",
            fmt_g_prec(g.miter_limit, 20)
        ));
        s.push_str(&format!(
            "<stroke-opacity>{}</stroke-opacity>",
            fmt_g_prec(g.stroke.alpha / 65535.0, 20)
        ));
        s.push_str(&format!(
            "<stroke-width>{}</stroke-width>",
            fmt_g_prec(g.stroke_width, 20)
        ));
        s.push_str(&format!(
            "<text-align>{}</text-align>",
            ["Undefined", "Left", "Center", "Right"]
                .get(g.text_align as usize)
                .unwrap_or(&"Undefined")
        ));
        s.push_str(&format!(
            "<text-antialias>{}</text-antialias>",
            g.text_antialias as u8
        ));
        s.push_str(&format!(
            "<text-undercolor>{}</text-undercolor>",
            hex_color(&g.under_color)
        ));
        if self.mvg.is_empty() {
            s.push_str("<vector-graphics/>");
        } else {
            s.push_str(&format!("<vector-graphics>{}</vector-graphics>", self.mvg));
        }
        s.push_str(" </drawing-wand>");
        s
    }

    // --- rendering --------------------------------------------------------

    /// DrawImage(): render the primitives onto the image.
    /// Render vector nodes onto the image (over).
    fn flush_nodes(img: &mut Image, nodes: &mut Vec<v::Node>) {
        let (w, h) = (img.columns, img.rows);
        if !nodes.is_empty() {
            let root = v::Group::new()
                .with_transform(v::Transform2D::translate(0.5, 0.5))
                .with_children(std::mem::take(nodes));
            let frame = v::VectorFrame::new(w as f32, h as f32).with_root(root);
            let out = oxideav_raster::Renderer::new(w as u32, h as u32).render(&frame);
            let plane = &out.planes[0];
            let mut layer = Image::blank(w, h);
            layer.alpha = true;
            for y in 0..h {
                for x in 0..w {
                    let s = &plane.data[y * plane.stride + x * 4..];
                    let a = s[3];
                    if a == 0 {
                        layer.set_px(x, y, [0, 0, 0, 0]);
                        continue;
                    }
                    layer.set_px(
                        x,
                        y,
                        [
                            s[0] as u16 * 257,
                            s[1] as u16 * 257,
                            s[2] as u16 * 257,
                            a as u16 * 257,
                        ],
                    );
                }
            }
            composite_image(
                img,
                &layer,
                CompositeOp::Over,
                0,
                0,
                &ComposeArgs::default(),
            );
        }
    }

    /// Fill a path with a pattern: the pattern's tile, drawn on its own,
    /// repeated from its origin, through the path's coverage.
    fn fill_with_pattern(
        &self,
        img: &mut Image,
        p: &Primitive,
        cmds: &[v::PathCommand],
        pattern: &(String, (f64, f64, f64, f64), Vec<Primitive>),
    ) -> crate::error::MResult<()> {
        let (w, h) = (img.columns, img.rows);
        let (px, py, pw, ph) = pattern.1;
        let (tw, th) = (pw.round().max(1.0) as usize, ph.round().max(1.0) as usize);
        let mut tile = Image::blank(tw, th);
        tile.alpha = true;
        for q in tile.pixels.chunks_exact_mut(4) {
            q.copy_from_slice(&[0, 0, 0, 0]);
        }
        let mut sub = DrawingWand::default();
        sub.clip_paths = self.clip_paths.clone();
        sub.patterns = self.patterns.clone();
        sub.font_resolution = self.font_resolution;
        sub.primitives = pattern.2.clone();
        sub.render_onto(&mut tile)?;
        // the coverage of the shape
        let mut mp = p.clone();
        mp.gc.fill_pattern = None;
        mp.gc.fill = PixelInfo::white();
        mp.gc.fill_opacity = 1.0;
        mp.gc.opacity = 1.0;
        mp.gc.stroke.alpha = 0.0;
        let mut mask = Image::blank(w, h);
        mask.alpha = true;
        for q in mask.pixels.chunks_exact_mut(4) {
            q.copy_from_slice(&[0, 0, 0, 0]);
        }
        if let Some(n) = self.path_node(&mp, cmds, false) {
            Self::flush_nodes(&mut mask, &mut vec![n]);
        }
        let k = p.gc.fill_opacity * p.gc.opacity;
        let mut layer = Image::blank(w, h);
        layer.alpha = true;
        // the tiles start a pixel before the shape's bounds (the
        // pattern's own origin is not used), as ImageMagick lays them
        let _ = (px, py);
        let a = p.gc.affine;
        let (mut minx, mut miny) = (f64::MAX, f64::MAX);
        for c in cmds {
            let pts: &[v::Point] = match c {
                v::PathCommand::MoveTo(q) | v::PathCommand::LineTo(q) => std::slice::from_ref(q),
                v::PathCommand::QuadCurveTo { end, .. }
                | v::PathCommand::CubicCurveTo { end, .. }
                | v::PathCommand::ArcTo { end, .. } => std::slice::from_ref(end),
                _ => &[],
            };
            for q in pts {
                let (x, y) = (q.x as f64, q.y as f64);
                minx = minx.min(a.sx * x + a.ry * y + a.tx);
                miny = miny.min(a.rx * x + a.sy * y + a.ty);
            }
        }
        if minx == f64::MAX {
            (minx, miny) = (0.0, 0.0);
        }
        let (ox, oy) = (minx.floor() as i64 - 1, miny.floor() as i64 - 1);
        for y in 0..h {
            for x in 0..w {
                let cov = mask.px(x, y)[3] as f64 / 65535.0 * k;
                let tx = (x as i64 - ox).rem_euclid(tw as i64) as usize;
                let ty = (y as i64 - oy).rem_euclid(th as i64) as usize;
                let mut c = tile.px(tx, ty);
                c[3] = (c[3] as f64 * cov).round() as u16;
                layer.set_px(x, y, c);
            }
        }
        composite_image(
            img,
            &layer,
            CompositeOp::Over,
            0,
            0,
            &ComposeArgs::default(),
        );
        Ok(())
    }

    pub fn render_onto(&self, img: &mut Image) -> crate::error::MResult<()> {
        let (w, h) = (img.columns, img.rows);
        if w == 0 || h == 0 {
            return Ok(());
        }
        let mut nodes = Vec::new();
        let mut points: Vec<(i64, i64, [u16; 4])> = Vec::new();
        let mut composites = Vec::new();
        for p in &self.primitives {
            match &p.shape {
                Shape::Point(x, y) => {
                    let a = p.gc.affine;
                    let (px, py) = (a.sx * x + a.ry * y + a.tx, a.rx * x + a.sy * y + a.ty);
                    let mut c = p.gc.fill.clone();
                    c.alpha *= p.gc.fill_opacity * p.gc.opacity;
                    points.push((px.round() as i64, py.round() as i64, c.to_rgba16()));
                }
                Shape::Text { x, y, text } => {
                    if let Some(n) = self.text_node(p, *x, *y, text, w, h) {
                        nodes.extend(n);
                    }
                }
                Shape::Composite { .. } => composites.push(p),
                Shape::Path { path, open } => {
                    let pattern =
                        p.gc.fill_pattern
                            .as_ref()
                            .filter(|_| !*open)
                            .and_then(|id| self.patterns.iter().find(|e| &e.0 == id));
                    if let Some(pat) = pattern {
                        // keep the drawing order
                        Self::flush_nodes(img, &mut nodes);
                        self.fill_with_pattern(img, p, path, pat)?;
                        let mut sp = p.clone();
                        sp.gc.fill.alpha = 0.0;
                        if let Some(n) = self.path_node(&sp, path, *open) {
                            nodes.push(n);
                        }
                        continue;
                    }
                    if let Some(n) = self.path_node(p, path, *open) {
                        nodes.push(n);
                    }
                }
            }
        }
        Self::flush_nodes(img, &mut nodes);
        for (x, y, c) in points {
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                if c[3] < 65535 {
                    let mut one = Image::blank(1, 1);
                    one.alpha = true;
                    one.set_px(0, 0, c);
                    composite_image(img, &one, CompositeOp::Over, x, y, &ComposeArgs::default());
                } else {
                    img.set_px(x as usize, y as usize, c);
                }
            }
        }
        for p in composites {
            if let Shape::Composite {
                op,
                x,
                y,
                w: cw,
                h: ch,
                image,
            } = &p.shape
            {
                let mut src = (**image).clone();
                let (tw, th) = (
                    if *cw > 0.0 {
                        cw.round() as usize
                    } else {
                        src.columns
                    },
                    if *ch > 0.0 {
                        ch.round() as usize
                    } else {
                        src.rows
                    },
                );
                if (tw, th) != (src.columns, src.rows) && tw > 0 && th > 0 {
                    src = crate::transform::resize(&src, tw, th, FilterType::Undefined)?;
                }
                let a = p.gc.affine;
                let (px, py) = (a.sx * x + a.ry * y + a.tx, a.rx * x + a.sy * y + a.ty);
                composite_image(
                    img,
                    &src,
                    *op,
                    px.round() as i64,
                    py.round() as i64,
                    &ComposeArgs::default(),
                );
            }
        }
        img.colors = None;
        Ok(())
    }

    /// The resolution text is laid out at.
    pub fn text_resolution(&self) -> (f64, f64) {
        let (x, y) = self.font_resolution;
        (
            if x > 0.0 { x } else { 72.0 },
            if y > 0.0 { y } else { 72.0 },
        )
    }

    /// The font metrics of a text with this drawing's settings.
    pub fn font_metrics(&self, text: &str) -> Option<crate::text::FontMetrics> {
        let gc = self.gc();
        let path = crate::text::find_font(gc.font.as_deref(), gc.family.as_deref())?;
        let data = crate::text::font_data(&path)?;
        crate::text::layout(
            &data,
            gc.font_size,
            self.text_resolution(),
            text,
            gc.interline,
            gc.kerning,
        )
        .map(|(_, m)| m)
    }

    /// A text primitive: the glyphs (and the under colour box), placed by
    /// gravity and alignment.
    fn text_node(
        &self,
        p: &Primitive,
        x: f64,
        y: f64,
        text: &str,
        w: usize,
        h: usize,
    ) -> Option<Vec<v::Node>> {
        let gc = &p.gc;
        let path = crate::text::find_font(gc.font.as_deref(), gc.family.as_deref())?;
        let data = crate::text::font_data(&path)?;
        let (cmds, m) = crate::text::layout(
            &data,
            gc.font_size,
            self.text_resolution(),
            text,
            gc.interline,
            gc.kerning,
        )?;
        let (tw, th) = (m.text_width, m.text_height);
        let (cw, ch) = (w as f64, h as f64);
        // the baseline origin of the first line
        let (mut ox, mut oy) = match gc.gravity {
            Gravity::NorthWest => (x, y + m.ascender),
            Gravity::North => ((cw - tw) / 2.0 + x, y + m.ascender),
            Gravity::NorthEast => (cw - tw - x, y + m.ascender),
            Gravity::West => (x, (ch - th) / 2.0 + m.ascender + y),
            Gravity::Center => ((cw - tw) / 2.0 + x, (ch - th) / 2.0 + m.ascender + y),
            Gravity::East => (cw - tw - x, (ch - th) / 2.0 + m.ascender + y),
            Gravity::SouthWest => (x, ch - th + m.ascender - y),
            Gravity::South => ((cw - tw) / 2.0 + x, ch - th + m.ascender - y),
            Gravity::SouthEast => (cw - tw - x, ch - th + m.ascender - y),
            _ => (x, y),
        };
        if gc.gravity == Gravity::Undefined {
            match gc.text_align {
                2 => ox -= tw / 2.0,
                3 => ox -= tw,
                _ => {}
            }
        }
        oy -= 0.0;
        let mut out = Vec::new();
        if gc.under_color.alpha > 0.0 {
            let box_cmds = vec![
                v::PathCommand::MoveTo(pt(ox, oy - m.ascender)),
                v::PathCommand::LineTo(pt(ox + tw, oy - m.ascender)),
                v::PathCommand::LineTo(pt(ox + tw, oy - m.ascender + th)),
                v::PathCommand::LineTo(pt(ox, oy - m.ascender + th)),
                v::PathCommand::Close,
            ];
            let mut under = p.clone();
            under.gc.fill = gc.under_color.clone();
            under.gc.fill_opacity = 1.0;
            under.gc.stroke.alpha = 0.0;
            if let Some(n) = self.path_node(&under, &box_cmds, false) {
                out.push(n);
            }
        }
        let placed: Vec<v::PathCommand> = cmds
            .iter()
            .map(|c| {
                map_cmd(
                    c,
                    &Affine {
                        tx: ox - 0.5,
                        ty: oy - 0.5,
                        ..Default::default()
                    },
                    &Affine::default(),
                )
            })
            .collect();
        let mut tp = p.clone();
        tp.gc.fill_rule = 2;
        if let Some(n) = self.path_node(&tp, &placed, false) {
            out.push(n);
        }
        // text decorations: a bar across the text, its thickness and
        // place in proportion to the point size (as ImageMagick draws them,
        // whatever the font)
        let size = gc.font_size * self.text_resolution().1 / 72.0;
        let centre = match gc.decoration {
            2 => Some(0.09 * size),
            3 => Some(-0.77 * size),
            4 => Some(-0.295 * size),
            _ => None,
        };
        if let Some(c) = centre {
            let t = (size / 20.0).max(1.0);
            let (y0, y1) = (oy + c - t / 2.0 - 0.5, oy + c + t / 2.0 - 0.5);
            let (x0, x1) = (ox - 0.5, ox + tw - 0.5);
            let bar = vec![
                v::PathCommand::MoveTo(pt(x0, y0)),
                v::PathCommand::LineTo(pt(x1, y0)),
                v::PathCommand::LineTo(pt(x1, y1)),
                v::PathCommand::LineTo(pt(x0, y1)),
                v::PathCommand::Close,
            ];
            let mut bp = p.clone();
            bp.gc.stroke.alpha = 0.0;
            if let Some(n) = self.path_node(&bp, &bar, false) {
                out.push(n);
            }
        }
        Some(out)
    }

    fn paint_of(c: &PixelInfo, mult: f64) -> Option<v::Paint> {
        let q = c.to_rgba16();
        let a = (q[3] as f64 / 65535.0 * mult).clamp(0.0, 1.0);
        if a <= 0.0 {
            return None;
        }
        let b = |v: u16| crate::image::q_to_u8(v);
        Some(v::Paint::Solid(v::Rgba {
            r: b(q[0]),
            g: b(q[1]),
            b: b(q[2]),
            a: (a * 255.0).round() as u8,
        }))
    }

    fn path_node(&self, p: &Primitive, cmds: &[v::PathCommand], open: bool) -> Option<v::Node> {
        let gc = &p.gc;
        let mut path = v::Path::new();
        path.commands = cmds.to_vec();
        let mut node = v::PathNode::new(path);
        node.fill_rule = if gc.fill_rule == 2 {
            v::FillRule::NonZero
        } else {
            v::FillRule::EvenOdd
        };
        let fill = if open {
            None
        } else {
            Self::paint_of(&gc.fill, gc.fill_opacity * gc.opacity)
        };
        let mut has = false;
        if let Some(f) = fill {
            node.fill = Some(f);
            has = true;
        }
        let stroke_paint = Self::paint_of(&gc.stroke, gc.opacity);
        // a line with no stroke is drawn with the fill colour
        let stroke_paint = match (stroke_paint, open) {
            (None, true) => Self::paint_of(&gc.fill, gc.fill_opacity * gc.opacity),
            (s, _) => s,
        };
        if let Some(sp) = stroke_paint {
            if gc.stroke_width > 0.0 {
                let mut st = v::Stroke::new(gc.stroke_width as f32, sp);
                st.cap = match gc.line_cap {
                    2 => v::LineCap::Round,
                    3 => v::LineCap::Square,
                    _ => v::LineCap::Butt,
                };
                st.join = match gc.line_join {
                    2 => v::LineJoin::Round,
                    3 => v::LineJoin::Bevel,
                    _ => v::LineJoin::Miter,
                };
                st.miter_limit = gc.miter_limit as f32;
                if !gc.dash.is_empty() && gc.dash.iter().any(|d| *d > 0.0) {
                    st.dash = Some(
                        v::DashPattern::new(gc.dash.iter().map(|d| *d as f32).collect())
                            .with_offset(gc.dash_offset as f32),
                    );
                }
                node.stroke = Some(st);
                has = true;
            }
        }
        if !has {
            return None;
        }
        let mut g = v::Group::new()
            .with_transform(gc.affine.to_v())
            .with_child(v::Node::Path(node));
        if let Some(id) = &gc.clip_path {
            if let Some((_, prims)) = self.clip_paths.iter().find(|e| &e.0 == id) {
                let mut clip = v::Path::new();
                for cp in prims {
                    if let Shape::Path { path, .. } = &cp.shape {
                        // clip geometry in the drawing's coordinates
                        let rel = cp.gc.affine;
                        for c in path {
                            clip.commands.push(map_cmd(c, &rel, &gc.affine));
                        }
                    }
                }
                if !clip.commands.is_empty() {
                    g = g.with_clip(clip);
                }
            }
        }
        Some(v::Node::Group(g))
    }
}

/// A clip path command, from its own transform into the one of the shape
/// it clips.
fn map_cmd(c: &v::PathCommand, from: &Affine, to: &Affine) -> v::PathCommand {
    // device = to * local  =>  local = to^-1 * from * p
    let det = to.sx * to.sy - to.ry * to.rx;
    let inv = if det.abs() < 1e-12 {
        Affine::default()
    } else {
        Affine {
            sx: to.sy / det,
            rx: -to.rx / det,
            ry: -to.ry / det,
            sy: to.sx / det,
            tx: (to.ry * to.ty - to.sy * to.tx) / det,
            ty: (to.rx * to.tx - to.sx * to.ty) / det,
        }
    };
    let m = inv.then(from);
    let f = |p: &v::Point| {
        let (x, y) = (p.x as f64, p.y as f64);
        pt(m.sx * x + m.ry * y + m.tx, m.rx * x + m.sy * y + m.ty)
    };
    match c {
        v::PathCommand::MoveTo(p) => v::PathCommand::MoveTo(f(p)),
        v::PathCommand::LineTo(p) => v::PathCommand::LineTo(f(p)),
        v::PathCommand::QuadCurveTo { control, end } => v::PathCommand::QuadCurveTo {
            control: f(control),
            end: f(end),
        },
        v::PathCommand::CubicCurveTo { c1, c2, end } => v::PathCommand::CubicCurveTo {
            c1: f(c1),
            c2: f(c2),
            end: f(end),
        },
        v::PathCommand::ArcTo {
            rx,
            ry,
            x_axis_rot,
            large_arc,
            sweep,
            end,
        } => v::PathCommand::ArcTo {
            rx: *rx,
            ry: *ry,
            x_axis_rot: *x_axis_rot,
            large_arc: *large_arc,
            sweep: *sweep,
            end: f(end),
        },
        v::PathCommand::Close => v::PathCommand::Close,
        other => *other,
    }
}

fn op_name(op: CompositeOp) -> &'static str {
    match op {
        CompositeOp::Over => "Over",
        CompositeOp::Copy => "Copy",
        CompositeOp::Src => "Src",
        CompositeOp::Multiply => "Multiply",
        CompositeOp::Screen => "Screen",
        CompositeOp::Dissolve => "Dissolve",
        _ => "Over",
    }
}
