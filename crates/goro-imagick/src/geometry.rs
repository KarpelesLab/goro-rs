//! Geometry strings (`WxH+X+Y` with the `%`, `!`, `<`, `>`, `^` and `@`
//! flags), parsed and applied the way ImageMagick's documentation of
//! image geometry describes them.

/// A parsed geometry string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub x_negative: bool,
    pub y_negative: bool,
    /// `%`: the sizes are percentages.
    pub percent: bool,
    /// `!`: ignore the aspect ratio.
    pub aspect: bool,
    /// `<`: only enlarge.
    pub less: bool,
    /// `>`: only shrink.
    pub greater: bool,
    /// `^`: fill (minimum) instead of fit.
    pub minimum: bool,
    /// `@`: an area in pixels.
    pub area: bool,
}

/// A rectangle (RectangleInfo).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub width: usize,
    pub height: usize,
    pub x: i64,
    pub y: i64,
}

impl Rect {
    pub fn new(width: usize, height: usize, x: i64, y: i64) -> Rect {
        Rect {
            width,
            height,
            x,
            y,
        }
    }
}

fn parse_num(s: &[u8], i: &mut usize) -> Option<f64> {
    let start = *i;
    let mut j = *i;
    if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
        j += 1;
    }
    let digits_start = j;
    while j < s.len() && (s[j].is_ascii_digit() || s[j] == b'.') {
        j += 1;
    }
    if j < s.len() && (s[j] == b'e' || s[j] == b'E') && j > digits_start {
        let mut k = j + 1;
        if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
            k += 1;
        }
        if k < s.len() && s[k].is_ascii_digit() {
            j = k;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
        }
    }
    if j == digits_start {
        return None;
    }
    let v = std::str::from_utf8(&s[start..j]).ok()?.parse().ok()?;
    *i = j;
    Some(v)
}

impl Geometry {
    /// GetGeometry() / ParseGeometry(): None when nothing could be parsed.
    pub fn parse(s: &str) -> Option<Geometry> {
        let mut g = Geometry::default();
        let mut body = Vec::new();
        for &c in s.trim().as_bytes() {
            match c {
                b'%' => g.percent = true,
                b'!' => g.aspect = true,
                b'<' => g.less = true,
                b'>' => g.greater = true,
                b'^' => g.minimum = true,
                b'@' => g.area = true,
                b' ' => {}
                _ => body.push(c),
            }
        }
        let s = &body[..];
        let mut i = 0;
        let mut any = false;
        // width
        if i < s.len() && (s[i].is_ascii_digit() || s[i] == b'.') {
            g.width = Some(parse_num(s, &mut i)?);
            any = true;
        }
        // height
        if i < s.len() && (s[i] == b'x' || s[i] == b'X') {
            i += 1;
            if i < s.len() && (s[i].is_ascii_digit() || s[i] == b'.') {
                g.height = Some(parse_num(s, &mut i)?);
            }
            any = true;
        }
        // offsets
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            let neg = s[i] == b'-';
            let v = parse_num(s, &mut i)?;
            g.x = Some(v);
            g.x_negative = neg;
            any = true;
            if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
                let neg = s[i] == b'-';
                let v = parse_num(s, &mut i)?;
                g.y = Some(v);
                g.y_negative = neg;
            }
        }
        if i != s.len() && !any {
            return None;
        }
        if !any && !(g.percent || g.area) {
            return None;
        }
        Some(g)
    }

    /// ParseAbsoluteGeometry(): width/height/x/y as given (missing = 0).
    pub fn to_rect(&self) -> Rect {
        Rect {
            width: self.width.unwrap_or(0.0).max(0.0).round() as usize,
            height: self.height.or(self.width).unwrap_or(0.0).max(0.0).round() as usize,
            x: self.x.unwrap_or(0.0).round() as i64,
            y: self.y.unwrap_or(0.0).round() as i64,
        }
    }

    /// ParseMetaGeometry(): the size an image of `cols`x`rows` takes for
    /// this geometry (resize semantics).
    pub fn meta_size(&self, cols: usize, rows: usize) -> (usize, usize) {
        let (cw, ch) = (cols as f64, rows as f64);
        let w = self.width;
        let h = self.height;
        if self.percent {
            let sx = w.unwrap_or(100.0);
            let sy = h.unwrap_or(sx);
            let nw = (cw * sx / 100.0 + 0.5).floor();
            let nh = (ch * sy / 100.0 + 0.5).floor();
            return clamp_wh(nw, nh);
        }
        if self.area {
            let area = w.unwrap_or(0.0) * h.unwrap_or(1.0);
            if area <= 0.0 {
                return (cols, rows);
            }
            let scale = (area / (cw * ch)).sqrt();
            if (self.greater && scale >= 1.0) || (self.less && scale <= 1.0) {
                return (cols, rows);
            }
            return clamp_wh((cw * scale + 0.5).floor(), (ch * scale + 0.5).floor());
        }
        if w.is_none() && h.is_none() {
            return (cols, rows);
        }
        let gw = w.unwrap_or_else(|| h.unwrap() * cw / ch);
        let gh = h.unwrap_or_else(|| w.unwrap() * ch / cw);
        if self.greater && cw <= gw && ch <= gh {
            return (cols, rows);
        }
        if self.less && cw >= gw && ch >= gh {
            return (cols, rows);
        }
        let (nw, nh);
        if self.aspect && w.is_some() && h.is_some() {
            nw = gw;
            nh = gh;
        } else {
            let sx = gw / cw;
            let sy = gh / ch;
            let scale = if self.minimum { sx.max(sy) } else { sx.min(sy) };
            if self.width.is_some() && self.height.is_some() || self.minimum {
                nw = (cw * scale + 0.5).floor();
                nh = (ch * scale + 0.5).floor();
            } else if self.width.is_some() {
                nw = gw.floor();
                nh = (ch * sx + 0.5).floor();
            } else {
                nw = (cw * sy + 0.5).floor();
                nh = gh.floor();
            }
        }
        clamp_wh(nw, nh)
    }
}

fn clamp_wh(w: f64, h: f64) -> (usize, usize) {
    (w.max(1.0) as usize, h.max(1.0) as usize)
}

/// Where an object of `w`x`h` goes in a `cw`x`ch` canvas for a gravity
/// (GravityAdjustGeometry), offsets measured inward from the anchor.
pub fn gravity_adjust(
    cw: usize,
    ch: usize,
    gravity: crate::types::Gravity,
    w: usize,
    h: usize,
    x: i64,
    y: i64,
) -> (i64, i64) {
    use crate::types::Gravity as G;
    let (cw, ch, w, h) = (cw as i64, ch as i64, w as i64, h as i64);
    let gx = match gravity {
        G::North | G::Center | G::South => (cw - w) / 2 + x,
        G::NorthEast | G::East | G::SouthEast => cw - w - x,
        _ => x,
    };
    let gy = match gravity {
        G::West | G::Center | G::East => (ch - h) / 2 + y,
        G::SouthWest | G::South | G::SouthEast => ch - h - y,
        _ => y,
    };
    (gx, gy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(g: &str, w: usize, h: usize) -> (usize, usize) {
        Geometry::parse(g).unwrap().meta_size(w, h)
    }

    #[test]
    fn meta() {
        assert_eq!(m("100x100", 640, 480), (100, 75));
        assert_eq!(m("100", 640, 480), (100, 75));
        assert_eq!(m("x100", 640, 480), (133, 100));
        assert_eq!(m("50%", 640, 480), (320, 240));
        assert_eq!(m("50%x25%", 640, 480), (320, 120));
        assert_eq!(m("33.3%", 640, 480), (213, 160));
        assert_eq!(m("100x100!", 640, 480), (100, 100));
        assert_eq!(m("1000x1000>", 640, 480), (640, 480));
        assert_eq!(m("1000x1000<", 640, 480), (1000, 750));
        assert_eq!(m("100x100>", 640, 480), (100, 75));
        assert_eq!(m("100x100<", 640, 480), (640, 480));
        assert_eq!(m("100x100^", 640, 480), (133, 100));
        assert_eq!(m("10000@", 640, 480), (115, 87));
        assert_eq!(m("101x77", 640, 480), (101, 76));
        assert_eq!(m("7x", 640, 480), (7, 5));
        assert_eq!(m("x7", 640, 480), (9, 7));
        assert_eq!(m("100x100", 333, 500), (67, 100));
        assert_eq!(m("100x100^", 333, 500), (100, 150));
    }

    #[test]
    fn parse() {
        let g = Geometry::parse("100x50+10-20").unwrap();
        assert_eq!(g.to_rect(), Rect::new(100, 50, 10, -20));
        let g = Geometry::parse("+5+6").unwrap();
        assert_eq!(g.x, Some(5.0));
        assert!(Geometry::parse("abc").is_none());
    }
}
