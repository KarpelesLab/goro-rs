//! FxImage(): a subset of the -fx expression language, as documented
//! ("The Fx Special Effects Image Operator"): numbers, the pixel and image
//! symbols (u, v, s, r, g, b, a, i, j, w, h, p{x,y}, intensity...),
//! arithmetic, comparisons, logic, ?:, user variables with `;`-separated
//! assignments and the usual math functions. Each colour channel is
//! evaluated with `u` bound to that channel.

use crate::error::{MResult, MagickError, severity};
use crate::image::Image;

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Var(String),
    /// image index (u=0, v=1), channel (None: the current one), offset
    Pixel(usize, Option<char>, Option<(Box<Expr>, Box<Expr>)>, bool),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Assign(String, Box<Expr>),
    Seq(Vec<Expr>),
}

fn err(msg: &str) -> MagickError {
    MagickError::with_desc(
        severity::OPTION_ERROR,
        "ParseExpression",
        msg,
        "error/fx.c/FxEvaluateSubexpression",
    )
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(t.as_bytes()) {
            self.i += t.len();
            true
        } else {
            false
        }
    }
    fn seq(&mut self) -> MResult<Expr> {
        let mut v = vec![self.assign()?];
        while self.eat(";") {
            if self.peek().is_none() {
                break;
            }
            v.push(self.assign()?);
        }
        Ok(if v.len() == 1 {
            v.pop().unwrap()
        } else {
            Expr::Seq(v)
        })
    }
    fn assign(&mut self) -> MResult<Expr> {
        let save = self.i;
        self.ws();
        let st = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        if self.i > st {
            let name = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
            self.ws();
            if self.s.get(self.i) == Some(&b'=') && self.s.get(self.i + 1) != Some(&b'=') {
                self.i += 1;
                let e = self.cond()?;
                return Ok(Expr::Assign(name, Box::new(e)));
            }
        }
        self.i = save;
        self.cond()
    }
    fn cond(&mut self) -> MResult<Expr> {
        let c = self.or()?;
        if self.eat("?") {
            let a = self.assign()?;
            if !self.eat(":") {
                return Err(err("expected ':'"));
            }
            let b = self.assign()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }
    fn or(&mut self) -> MResult<Expr> {
        let mut l = self.and()?;
        while self.eat("||") {
            let r = self.and()?;
            l = Expr::Bin('|', Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn and(&mut self) -> MResult<Expr> {
        let mut l = self.cmp()?;
        while self.eat("&&") {
            let r = self.cmp()?;
            l = Expr::Bin('&', Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn cmp(&mut self) -> MResult<Expr> {
        let mut l = self.add()?;
        loop {
            let op = if self.eat("==") {
                'e'
            } else if self.eat("!=") {
                'n'
            } else if self.eat("<=") {
                'l'
            } else if self.eat(">=") {
                'g'
            } else if self.eat("<") {
                '<'
            } else if self.eat(">") {
                '>'
            } else {
                break;
            };
            let r = self.add()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn add(&mut self) -> MResult<Expr> {
        let mut l = self.mul()?;
        loop {
            let op = match self.peek() {
                Some(b'+') => '+',
                Some(b'-') => '-',
                _ => break,
            };
            self.i += 1;
            let r = self.mul()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn mul(&mut self) -> MResult<Expr> {
        let mut l = self.pow()?;
        loop {
            let op = match self.peek() {
                Some(b'*') => '*',
                Some(b'/') => '/',
                Some(b'%') => '%',
                _ => break,
            };
            self.i += 1;
            let r = self.pow()?;
            l = Expr::Bin(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn pow(&mut self) -> MResult<Expr> {
        let l = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let r = self.pow()?;
            return Ok(Expr::Bin('^', Box::new(l), Box::new(r)));
        }
        Ok(l)
    }
    fn unary(&mut self) -> MResult<Expr> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            Some(b'!') => {
                self.i += 1;
                Ok(Expr::Not(Box::new(self.unary()?)))
            }
            _ => self.primary(),
        }
    }
    fn primary(&mut self) -> MResult<Expr> {
        let Some(c) = self.peek() else {
            return Err(err("unexpected end"));
        };
        if c == b'(' {
            self.i += 1;
            let e = self.seq()?;
            if !self.eat(")") {
                return Err(err("expected ')'"));
            }
            return Ok(e);
        }
        if c.is_ascii_digit() || c == b'.' {
            let st = self.i;
            while self.i < self.s.len()
                && (self.s[self.i].is_ascii_digit()
                    || self.s[self.i] == b'.'
                    || ((self.s[self.i] == b'e' || self.s[self.i] == b'E')
                        && self
                            .s
                            .get(self.i + 1)
                            .is_some_and(|d| d.is_ascii_digit() || *d == b'-' || *d == b'+')))
            {
                if self.s[self.i] == b'e' || self.s[self.i] == b'E' {
                    self.i += 1;
                }
                self.i += 1;
            }
            let t = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
            let mut v: f64 = t.parse().map_err(|_| err(&t))?;
            // SI suffixes and %
            if self.i < self.s.len() && self.s[self.i] == b'%' {
                self.i += 1;
                v /= 100.0;
            }
            return Ok(Expr::Num(v));
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let st = self.i;
            while self.i < self.s.len()
                && (self.s[self.i].is_ascii_alphanumeric()
                    || self.s[self.i] == b'_'
                    || self.s[self.i] == b'.')
            {
                self.i += 1;
            }
            let name = String::from_utf8_lossy(&self.s[st..self.i]).to_ascii_lowercase();
            if self.peek() == Some(b'(') {
                self.i += 1;
                let mut args = Vec::new();
                if self.peek() != Some(b')') {
                    loop {
                        args.push(self.assign()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                if !self.eat(")") {
                    return Err(err("expected ')'"));
                }
                return Ok(Expr::Call(name, args));
            }
            // u, v, s, p with an offset: p{x,y} / p[x,y] (relative)
            let (img, chan) = match name.as_str() {
                "u" | "s" => (Some(0), None),
                "v" => (Some(1), None),
                "p" => (Some(0), None),
                "u.r" | "s.r" | "p.r" => (Some(0), Some('r')),
                "u.g" | "s.g" | "p.g" => (Some(0), Some('g')),
                "u.b" | "s.b" | "p.b" => (Some(0), Some('b')),
                "u.a" | "s.a" | "p.a" => (Some(0), Some('a')),
                "v.r" => (Some(1), Some('r')),
                "v.g" => (Some(1), Some('g')),
                "v.b" => (Some(1), Some('b')),
                "v.a" => (Some(1), Some('a')),
                _ => (None, None),
            };
            if let Some(img) = img {
                let abs = self.peek() == Some(b'{');
                if abs || self.peek() == Some(b'[') {
                    self.i += 1;
                    let x = self.assign()?;
                    if !self.eat(",") {
                        return Err(err("expected ','"));
                    }
                    let y = self.assign()?;
                    if !(self.eat("}") || self.eat("]")) {
                        return Err(err("expected '}'"));
                    }
                    let mut chan = chan;
                    if self.eat(".") {
                        if let Some(ch) = self.peek() {
                            self.i += 1;
                            chan = Some(ch as char);
                        }
                    }
                    return Ok(Expr::Pixel(
                        img,
                        chan,
                        Some((Box::new(x), Box::new(y))),
                        abs,
                    ));
                }
                return Ok(Expr::Pixel(img, chan, None, false));
            }
            return Ok(Expr::Var(name));
        }
        Err(err(&format!("unexpected '{}'", c as char)))
    }
}

struct Ctx<'a> {
    images: &'a [Image],
    x: usize,
    y: usize,
    chan: usize,
    vars: Vec<(String, f64)>,
    seed: u64,
}

fn chan_of(c: char) -> usize {
    match c {
        'r' | 'c' => 0,
        'g' | 'm' => 1,
        'b' | 'y' => 2,
        'a' | 'o' => 3,
        _ => 0,
    }
}

impl Ctx<'_> {
    fn sample(&self, img: usize, chan: Option<usize>, x: f64, y: f64) -> f64 {
        let Some(im) = self.images.get(img).or_else(|| self.images.first()) else {
            return 0.0;
        };
        let s = im.sample_bilinear(x + 0.5, y + 0.5);
        let c = chan.unwrap_or(self.chan);
        if c == 4 {
            return Image::intensity(&[s[0] as u16, s[1] as u16, s[2] as u16, 0]) / 65535.0;
        }
        s[c.min(3)] / 65535.0
    }

    fn var(&mut self, name: &str) -> f64 {
        if let Some(v) = self.vars.iter().rev().find(|(n, _)| n == name) {
            return v.1;
        }
        let im = &self.images[0];
        let (x, y) = (self.x as f64, self.y as f64);
        let pix = |c: usize| self.sample(0, Some(c), x, y);
        match name {
            "i" => x,
            "j" => y,
            "w" => im.columns as f64,
            "h" => im.rows as f64,
            "n" => self.images.len() as f64,
            "t" => 0.0,
            "r" | "c" => pix(0),
            "g" | "m" => pix(1),
            "b" | "y" => pix(2),
            "a" | "o" => pix(3),
            "k" => 0.0,
            "intensity" | "luma" | "lightness" | "luminance" => pix(4),
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            "quantumrange" => 65535.0,
            "quantumscale" => 1.0 / 65535.0,
            "opaquealpha" => 1.0,
            "transparentalpha" => 0.0,
            "epsilon" => 1e-12,
            "phi" => std::f64::consts::GOLDEN_RATIO,
            "hue" | "saturation" => {
                let (h, s, _) = crate::color::rgb_to_hsl(pix(0), pix(1), pix(2));
                if name == "hue" { h } else { s }
            }
            _ => {
                if let Some(c) = name.strip_prefix("u.").or(name.strip_prefix("s.")) {
                    return self.sample(0, Some(chan_of(c.chars().next().unwrap_or('r'))), x, y);
                }
                0.0
            }
        }
    }

    fn eval(&mut self, e: &Expr) -> f64 {
        match e {
            Expr::Num(v) => *v,
            Expr::Var(n) => self.var(n),
            Expr::Pixel(img, chan, off, abs) => {
                let (mut x, mut y) = (self.x as f64, self.y as f64);
                if let Some((ox, oy)) = off {
                    let (dx, dy) = (self.eval(ox), self.eval(oy));
                    if *abs {
                        x = dx;
                        y = dy;
                    } else {
                        x += dx;
                        y += dy;
                    }
                }
                self.sample(*img, chan.map(chan_of), x, y)
            }
            Expr::Neg(a) => -self.eval(a),
            Expr::Not(a) => (self.eval(a) == 0.0) as i32 as f64,
            Expr::Bin(op, a, b) => {
                let x = self.eval(a);
                if *op == '&' && x == 0.0 {
                    return 0.0;
                }
                if *op == '|' && x != 0.0 {
                    return 1.0;
                }
                let y = self.eval(b);
                match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    '/' => {
                        if y == 0.0 {
                            0.0
                        } else {
                            x / y
                        }
                    }
                    '%' => {
                        if y == 0.0 {
                            0.0
                        } else {
                            x % y
                        }
                    }
                    '^' => x.powf(y),
                    '<' => (x < y) as i32 as f64,
                    '>' => (x > y) as i32 as f64,
                    'l' => (x <= y) as i32 as f64,
                    'g' => (x >= y) as i32 as f64,
                    'e' => ((x - y).abs() < 1e-12) as i32 as f64,
                    'n' => ((x - y).abs() >= 1e-12) as i32 as f64,
                    '&' | '|' => (y != 0.0) as i32 as f64,
                    _ => 0.0,
                }
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c) != 0.0 {
                    self.eval(a)
                } else {
                    self.eval(b)
                }
            }
            Expr::Assign(n, v) => {
                let v = self.eval(v);
                self.vars.push((n.clone(), v));
                v
            }
            Expr::Seq(v) => {
                let mut r = 0.0;
                for e in v {
                    r = self.eval(e);
                }
                r
            }
            Expr::Call(name, args) => {
                let a: Vec<f64> = args.iter().map(|x| self.eval(x)).collect();
                let a0 = a.first().copied().unwrap_or(0.0);
                let a1 = a.get(1).copied().unwrap_or(0.0);
                match name.as_str() {
                    "abs" => a0.abs(),
                    "acos" => a0.acos(),
                    "acosh" => a0.acosh(),
                    "asin" => a0.asin(),
                    "asinh" => a0.asinh(),
                    "atan" => a0.atan(),
                    "atanh" => a0.atanh(),
                    "atan2" => a0.atan2(a1),
                    "ceil" => a0.ceil(),
                    "cos" => a0.cos(),
                    "cosh" => a0.cosh(),
                    "exp" => a0.exp(),
                    "floor" => a0.floor(),
                    "hypot" => a0.hypot(a1),
                    "int" | "trunc" => a0.trunc(),
                    "isnan" => a0.is_nan() as i32 as f64,
                    "ln" => a0.ln(),
                    "log" => a0.log10(),
                    "logtwo" => a0.log2(),
                    "max" => a.iter().cloned().fold(f64::MIN, f64::max),
                    "min" => a.iter().cloned().fold(f64::MAX, f64::min),
                    "mod" => {
                        if a1 == 0.0 {
                            0.0
                        } else {
                            a0 % a1
                        }
                    }
                    "pow" => a0.powf(a1),
                    "rand" => {
                        self.seed ^= self.seed << 13;
                        self.seed ^= self.seed >> 7;
                        self.seed ^= self.seed << 17;
                        (self.seed >> 11) as f64 / (1u64 << 53) as f64
                    }
                    "round" => a0.round(),
                    "sign" => a0.signum(),
                    "sin" => a0.sin(),
                    "sinh" => a0.sinh(),
                    "sqrt" => a0.sqrt(),
                    "squish" => 1.0 / (1.0 + (-a0).exp()),
                    "tan" => a0.tan(),
                    "tanh" => a0.tanh(),
                    "sinc" => {
                        if a0 == 0.0 {
                            1.0
                        } else {
                            (std::f64::consts::PI * a0).sin() / (std::f64::consts::PI * a0)
                        }
                    }
                    "gauss" => (-a0 * a0 / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt(),
                    "debug" => a0,
                    "clamp" => a0.clamp(0.0, 1.0),
                    _ => 0.0,
                }
            }
        }
    }
}

/// FxImage(): a new image from the expression.
pub fn fx(images: &[Image], current: usize, expr: &str) -> MResult<Image> {
    let mut p = Parser {
        s: expr.as_bytes(),
        i: 0,
    };
    let e = p.seq()?;
    if p.peek().is_some() {
        return Err(err(expr));
    }
    let mut list: Vec<Image> = images.to_vec();
    if current < list.len() {
        list.swap(0, current);
    }
    let base = list[0].clone();
    let mut out = base.clone();
    let mut ctx = Ctx {
        images: &list,
        x: 0,
        y: 0,
        chan: 0,
        vars: Vec::new(),
        seed: 0x9E37_79B9_7F4A_7C15,
    };
    let channels = if base.alpha { 4 } else { 3 };
    for y in 0..base.rows {
        for x in 0..base.columns {
            let mut px = base.px(x, y);
            for c in 0..channels {
                ctx.x = x;
                ctx.y = y;
                ctx.chan = c;
                ctx.vars.clear();
                let v = ctx.eval(&e);
                px[c] = crate::color::to_q16(v * 65535.0);
            }
            out.set_px(x, y, px);
        }
    }
    out.colors = None;
    Ok(out)
}
