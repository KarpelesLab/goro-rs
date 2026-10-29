//! The INI format of parse_ini_string()/parse_ini_file() (PHP manual:
//! parse_ini_file, "INI_SCANNER_*" modes; details and syntax error texts
//! observed on PHP 8.5 and in tests ext/standard/tests/general_functions/
//! parse_ini_*.phpt).
//!
//! Lines hold `[section]`, `key = value` or `key[offset] = value`;
//! `;` starts a comment. Values concatenate unquoted text (constants
//! resolved, booleans and null converted), `"double"` strings (with `\"`,
//! `\\`, `\$` and `${var}` expansion), `'single'` strings and `${var}` /
//! `${var:-fallback}` references; `| & ^ ~ ! ( )` form integer
//! expressions.

/// Scanner mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Raw,
    Typed,
}

impl Mode {
    pub fn from_int(v: i64) -> Option<Mode> {
        Some(match v {
            0 => Mode::Normal,
            1 => Mode::Raw,
            2 => Mode::Typed,
            _ => return None,
        })
    }
}

/// A parsed value.
#[derive(Clone, Debug, PartialEq)]
pub enum IniValue {
    Str(Vec<u8>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

/// What the parser reports, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Section(Vec<u8>),
    /// `offset`: None for `key = v`, Some(None) for `key[] = v`.
    Entry {
        key: Vec<u8>,
        offset: Option<Option<Vec<u8>>>,
        value: IniValue,
    },
}

/// Lookups the parser needs: PHP constants and `${var}` (ini settings,
/// then the environment).
pub trait Env {
    fn constant(&mut self, name: &[u8]) -> Option<Vec<u8>>;
    fn var(&mut self, name: &[u8]) -> Option<Vec<u8>>;
}

/// A syntax error: the message (`syntax error, unexpected ...`) and line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniError {
    pub message: String,
    pub line: usize,
}

struct P<'a, E: Env> {
    s: &'a [u8],
    i: usize,
    line: usize,
    mode: Mode,
    env: &'a mut E,
    /// Inside a `${var:-fallback}`: only the true/false/null constants
    /// convert (observed on PHP 8.5: `${unset:-on}` is "on").
    fallback: usize,
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// One piece of a value.
enum Tok {
    Text(Vec<u8>, bool), // text, from an unquoted run
    Op(u8),
}

impl<E: Env> P<'_, E> {
    fn err<T>(&self, m: &str) -> Result<T, IniError> {
        Err(IniError {
            message: format!("syntax error, {}", m),
            line: self.line,
        })
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn skip_blank(&mut self) {
        while self.peek().is_some_and(is_blank) {
            self.i += 1;
        }
    }

    /// Skips a comment and the line end.
    fn end_line(&mut self) -> Result<(), IniError> {
        self.skip_blank();
        if self.peek() == Some(b';') {
            while !matches!(self.peek(), None | Some(b'\n')) {
                self.i += 1;
            }
        }
        match self.peek() {
            None => Ok(()),
            Some(b'\r') => {
                self.i += 1;
                if self.peek() == Some(b'\n') {
                    self.i += 1;
                }
                self.line += 1;
                Ok(())
            }
            Some(b'\n') => {
                self.i += 1;
                self.line += 1;
                Ok(())
            }
            Some(b'=') => self.err("unexpected '='"),
            Some(c) => self.err(&format!("unexpected '{}'", c as char)),
        }
    }

    /// A double-quoted string (the opening quote consumed).
    fn dq(&mut self) -> Result<Vec<u8>, IniError> {
        let mut out = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return self.err(
                    "unexpected end of file, expecting TC_DOLLAR_CURLY or TC_QUOTED_STRING or '\"'",
                );
            };
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' if self.mode != Mode::Raw => match self.peek() {
                    Some(b'"') => {
                        // "special case\" at the end of a line keeps the
                        // backslash and closes the string (test
                        // parse_ini_basic.phpt).
                        let after = self.s.get(self.i + 1).copied();
                        let ends = {
                            let mut k = self.i + 1;
                            while self.s.get(k).copied().is_some_and(is_blank) {
                                k += 1;
                            }
                            matches!(self.s.get(k), None | Some(b'\n') | Some(b'\r'))
                        };
                        let _ = after;
                        if ends {
                            out.push(b'\\');
                            self.i += 1;
                            return Ok(out);
                        }
                        out.push(b'"');
                        self.i += 1;
                    }
                    Some(b'\\') => {
                        out.push(b'\\');
                        self.i += 1;
                    }
                    Some(b'$') => {
                        out.push(b'$');
                        self.i += 1;
                    }
                    _ => out.push(b'\\'),
                },
                b'$' if self.mode != Mode::Raw && self.peek() == Some(b'{') => {
                    self.i += 1;
                    let v = self.var_ref()?;
                    out.extend_from_slice(&v);
                }
                b'\n' => {
                    self.line += 1;
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
    }

    /// `${name}` / `${name:-fallback}` (after `${`).
    fn var_ref(&mut self) -> Result<Vec<u8>, IniError> {
        let start = self.i;
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return self.err("unexpected end of file, expecting TC_FALLBACK or '}'");
                }
                Some(b'}') => break,
                Some(b':') if self.s.get(self.i + 1) == Some(&b'-') => break,
                Some(_) => self.i += 1,
            }
        }
        let name = self.s[start..self.i].to_vec();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(self.lookup(&name).unwrap_or_default());
        }
        if name.is_empty() {
            return self.err("unexpected TC_FALLBACK, expecting TC_VARNAME");
        }
        self.i += 2; // ":-"
        self.fallback += 1;
        let fallback = self.value_until(b'}');
        self.fallback -= 1;
        let fallback = fallback?;
        if self.peek() != Some(b'}') {
            // Observed on PHP 8.5 for "a=${b:-a".
            return self.err("unexpected TC_FALLBACK, expecting TC_VARNAME");
        }
        self.i += 1;
        match self.lookup(&name) {
            Some(v) if !v.is_empty() => Ok(v),
            _ => Ok(match fallback {
                IniValue::Str(s) => s,
                IniValue::Int(i) => i.to_string().into_bytes(),
                IniValue::Float(f) => f.to_string().into_bytes(),
                IniValue::Bool(true) => b"1".to_vec(),
                _ => Vec::new(),
            }),
        }
    }

    fn lookup(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        self.env.var(name)
    }

    /// The pieces of a value up to the line end (or `stop`).
    fn tokens(&mut self, stop: u8) -> Result<Vec<Tok>, IniError> {
        let mut toks = Vec::new();
        loop {
            self.skip_blank();
            let Some(c) = self.peek() else { break };
            if c == stop || c == b'\n' || c == b'\r' || c == b';' {
                break;
            }
            match c {
                b'"' => {
                    self.i += 1;
                    let s = self.dq()?;
                    toks.push(Tok::Text(s, false));
                }
                b'\'' => {
                    self.i += 1;
                    let start = self.i;
                    while !matches!(self.peek(), None | Some(b'\'')) {
                        if self.peek() == Some(b'\n') {
                            self.line += 1;
                        }
                        self.i += 1;
                    }
                    let s = self.s[start..self.i].to_vec();
                    if self.peek() == Some(b'\'') {
                        self.i += 1;
                    }
                    toks.push(Tok::Text(s, false));
                }
                b'$' if self.s.get(self.i + 1) == Some(&b'{') => {
                    self.i += 2;
                    let v = self.var_ref()?;
                    toks.push(Tok::Text(v, false));
                }
                b'|' | b'&' | b'^' | b'~' | b'!' | b'(' | b')' => {
                    self.i += 1;
                    toks.push(Tok::Op(c));
                }
                b'=' => return self.err("unexpected '='"),
                _ => {
                    let start = self.i;
                    while let Some(c) = self.peek() {
                        if c == stop
                            || matches!(
                                c,
                                b'"' | b'\''
                                    | b';'
                                    | b'\n'
                                    | b'\r'
                                    | b'|'
                                    | b'&'
                                    | b'^'
                                    | b'~'
                                    | b'!'
                                    | b'('
                                    | b')'
                                    | b'='
                            )
                            || (c == b'$' && self.s.get(self.i + 1) == Some(&b'{'))
                        {
                            break;
                        }
                        self.i += 1;
                    }
                    let mut t = self.s[start..self.i].to_vec();
                    // Trailing blanks go, unless a single-quoted string
                    // follows (observed on PHP 8.5: `y 'z'` is "y z") or
                    // the input ends there ("a=b " keeps "b ": bug51899).
                    if self.peek() != Some(b'\'') && self.peek().is_some() {
                        while t.last().copied().is_some_and(is_blank) {
                            t.pop();
                        }
                    }
                    if t == b"$" {
                        // Observed on PHP 8.5: a lone "$" is empty.
                        t.clear();
                    }
                    toks.push(Tok::Text(t, true));
                }
            }
        }
        Ok(toks)
    }

    fn convert_word(&mut self, w: &[u8], only: bool) -> IniValue {
        if self.mode == Mode::Normal || self.mode == Mode::Typed {
            if only && self.fallback > 0 {
                let l = w.to_ascii_lowercase();
                match l.as_slice() {
                    b"true" => return IniValue::Str(b"1".to_vec()),
                    b"false" | b"null" => return IniValue::Str(Vec::new()),
                    _ => {}
                }
            } else if only {
                let l = w.to_ascii_lowercase();
                match l.as_slice() {
                    b"true" | b"on" | b"yes" => {
                        return if self.mode == Mode::Typed {
                            IniValue::Bool(true)
                        } else {
                            IniValue::Str(b"1".to_vec())
                        };
                    }
                    b"false" | b"off" | b"no" | b"none" => {
                        return if self.mode == Mode::Typed {
                            IniValue::Bool(false)
                        } else {
                            IniValue::Str(Vec::new())
                        };
                    }
                    b"null" => {
                        return if self.mode == Mode::Typed {
                            IniValue::Null
                        } else {
                            IniValue::Str(Vec::new())
                        };
                    }
                    _ => {}
                }
            }
            // Each blank-separated word naming a constant is replaced
            // (test parse_ini_booleans.phpt: "E_ALL E_NOTICE").
            let mut out = Vec::with_capacity(w.len());
            let mut k = 0;
            while k < w.len() {
                if is_blank(w[k]) {
                    out.push(w[k]);
                    k += 1;
                    continue;
                }
                let st = k;
                while k < w.len() && !is_blank(w[k]) {
                    k += 1;
                }
                let word = &w[st..k];
                let is_name = word.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    && !word[0].is_ascii_digit();
                match is_name.then(|| self.env.constant(word)).flatten() {
                    Some(v) => out.extend_from_slice(&v),
                    None => out.extend_from_slice(word),
                }
            }
            return IniValue::Str(out);
        }
        IniValue::Str(w.to_vec())
    }

    fn typed(&self, s: Vec<u8>, from_raw: bool) -> IniValue {
        if self.mode == Mode::Typed && from_raw {
            if let Some(i) = crate::ini::int_of(&s) {
                return IniValue::Int(i);
            }
            if let Some(f) = float_of(&s) {
                return IniValue::Float(f);
            }
        }
        IniValue::Str(s)
    }

    /// A value up to the line end or `stop`.
    fn value_until(&mut self, stop: u8) -> Result<IniValue, IniError> {
        if self.mode == Mode::Raw && stop != b'}' {
            return self.raw_value();
        }
        let toks = self.tokens(stop)?;
        if toks.iter().any(|t| matches!(t, Tok::Op(_))) {
            let mut pos = 0;
            let v = self.expr(&toks, &mut pos)?;
            if pos < toks.len() {
                return self.err("unexpected ')'");
            }
            return Ok(if self.mode == Mode::Typed {
                IniValue::Int(v)
            } else {
                IniValue::Str(v.to_string().into_bytes())
            });
        }
        if toks.len() == 1
            && let Tok::Text(t, true) = &toks[0]
        {
            let t = t.clone();
            return Ok(match self.convert_word(&t, true) {
                IniValue::Str(s) => self.typed(s, true),
                other => other,
            });
        }
        let mut out = Vec::new();
        let mut all_raw = true;
        for t in toks {
            if let Tok::Text(s, raw) = t {
                if raw {
                    if let IniValue::Str(v) = self.convert_word(&s, false) {
                        out.extend_from_slice(&v);
                    }
                } else {
                    all_raw = false;
                    out.extend_from_slice(&s);
                }
            }
        }
        Ok(if all_raw {
            self.typed(out, true)
        } else {
            IniValue::Str(out)
        })
    }

    fn raw_value(&mut self) -> Result<IniValue, IniError> {
        self.skip_blank();
        let start = self.i;
        let mut in_dq = false;
        while let Some(c) = self.peek() {
            if c == b'"' {
                in_dq = !in_dq;
            }
            if !in_dq && matches!(c, b'\n' | b'\r' | b';') {
                break;
            }
            if in_dq && c == b'\\' && self.s.get(self.i + 1) == Some(&b'"') {
                self.i += 2;
                continue;
            }
            self.i += 1;
        }
        // an unterminated quote doesn't hide a comment (test
        // bug51094.phpt: `ini="ini;raw` gives `"ini`)
        if in_dq && let Some(p) = self.s[start..self.i].iter().position(|&c| c == b';') {
            self.i = start + p;
        }
        let mut v = self.s[start..self.i].to_vec();
        while v.last().copied().is_some_and(is_blank) {
            v.pop();
        }
        if v.len() >= 2 && v[0] == b'"' && v[v.len() - 1] == b'"' {
            let inner = &v[1..v.len() - 1];
            // Only when the quotes enclose the whole value.
            let mut ok = true;
            let mut k = 0;
            while k < inner.len() {
                if inner[k] == b'\\' && inner.get(k + 1) == Some(&b'"') {
                    k += 2;
                    continue;
                }
                if inner[k] == b'"' {
                    ok = false;
                    break;
                }
                k += 1;
            }
            if ok {
                v = inner.to_vec();
            }
        }
        Ok(IniValue::Str(v))
    }

    fn operand(&mut self, toks: &[Tok], pos: &mut usize) -> Result<i64, IniError> {
        match toks.get(*pos) {
            Some(Tok::Op(b'~')) => {
                *pos += 1;
                Ok(!self.operand(toks, pos)?)
            }
            Some(Tok::Op(b'!')) => {
                *pos += 1;
                Ok((self.operand(toks, pos)? == 0) as i64)
            }
            Some(Tok::Op(b'(')) => {
                *pos += 1;
                let v = self.expr(toks, pos)?;
                match toks.get(*pos) {
                    Some(Tok::Op(b')')) => {
                        *pos += 1;
                        Ok(v)
                    }
                    _ => self.err("unexpected END_OF_LINE, expecting '^' or '|' or '&' or ')'"),
                }
            }
            Some(Tok::Text(..)) => {
                let mut s = Vec::new();
                while let Some(Tok::Text(t, raw)) = toks.get(*pos) {
                    if *raw {
                        if let IniValue::Str(v) = self.convert_word(t, false) {
                            s.extend_from_slice(&v);
                        }
                    } else {
                        s.extend_from_slice(t);
                    }
                    *pos += 1;
                }
                Ok(leading_int(&s))
            }
            _ => self.err("unexpected END_OF_LINE"),
        }
    }

    fn expr(&mut self, toks: &[Tok], pos: &mut usize) -> Result<i64, IniError> {
        let mut v = self.operand(toks, pos)?;
        while let Some(Tok::Op(op @ (b'|' | b'&' | b'^'))) = toks.get(*pos) {
            let op = *op;
            *pos += 1;
            let r = self.operand(toks, pos)?;
            v = match op {
                b'|' => v | r,
                b'&' => v & r,
                _ => v ^ r,
            };
        }
        Ok(v)
    }

    /// Bracketed text: a section name or an offset (after `[`).
    fn bracket(&mut self) -> Result<Vec<u8>, IniError> {
        // Pieces: (bytes, quoted); blanks around the whole are dropped
        // from unquoted pieces only (test parse_ini_basic.phpt: [" "]).
        let mut pieces: Vec<(Vec<u8>, bool)> = Vec::new();
        let mut raw = Vec::new();
        let flush = |raw: &mut Vec<u8>, pieces: &mut Vec<(Vec<u8>, bool)>| {
            if !raw.is_empty() {
                pieces.push((std::mem::take(raw), false));
            }
        };
        loop {
            match self.peek() {
                None | Some(b'\n') | Some(b'\r') => {
                    return self.err("unexpected end of file, expecting ']'");
                }
                Some(b']') => {
                    self.i += 1;
                    break;
                }
                Some(b'"') => {
                    flush(&mut raw, &mut pieces);
                    self.i += 1;
                    let s = self.dq()?;
                    pieces.push((s, true));
                }
                Some(b'\'') => {
                    flush(&mut raw, &mut pieces);
                    self.i += 1;
                    let mut q = Vec::new();
                    while !matches!(self.peek(), None | Some(b'\'')) {
                        q.push(self.s[self.i]);
                        self.i += 1;
                    }
                    self.i += 1;
                    pieces.push((q, true));
                }
                Some(b'$') if self.s.get(self.i + 1) == Some(&b'{') && self.mode != Mode::Raw => {
                    flush(&mut raw, &mut pieces);
                    self.i += 2;
                    let v = self.var_ref()?;
                    pieces.push((v, true));
                }
                Some(c) => {
                    raw.push(c);
                    self.i += 1;
                }
            }
        }
        flush(&mut raw, &mut pieces);
        let n = pieces.len();
        let mut out = Vec::new();
        for (k, (mut p, quoted)) in pieces.into_iter().enumerate() {
            if !quoted {
                if k == 0 {
                    let a = p.iter().position(|&c| !is_blank(c)).unwrap_or(p.len());
                    p.drain(..a);
                }
                if k + 1 == n {
                    while p.last().copied().is_some_and(is_blank) {
                        p.pop();
                    }
                }
            }
            out.extend_from_slice(&p);
        }
        Ok(out)
    }

    fn run(&mut self, events: &mut Vec<Event>) -> Result<(), IniError> {
        while self.i < self.s.len() {
            self.skip_blank();
            match self.peek() {
                None => break,
                Some(b'\n' | b'\r' | b';') => {
                    self.end_line()?;
                    continue;
                }
                Some(b'[') => {
                    self.i += 1;
                    let name = self.bracket()?;
                    events.push(Event::Section(name));
                    // Anything after the closing bracket is ignored.
                    while !matches!(self.peek(), None | Some(b'\n') | Some(b'\r')) {
                        self.i += 1;
                    }
                    self.end_line()?;
                    continue;
                }
                Some(b'=') => return self.err("unexpected '='"),
                _ => {}
            }
            let start = self.i;
            while let Some(c) = self.peek() {
                if matches!(c, b'=' | b'[' | b'\n' | b'\r' | b';') {
                    break;
                }
                self.i += 1;
            }
            let mut key = self.s[start..self.i].to_vec();
            while key.last().copied().is_some_and(is_blank) {
                key.pop();
            }
            let mut offset = None;
            if self.peek() == Some(b'[') {
                self.i += 1;
                let o = self.bracket()?;
                offset = Some(if o.is_empty() { None } else { Some(o) });
                self.skip_blank();
            }
            if self.peek() != Some(b'=') {
                // A bare key: no entry (observed on PHP 8.5).
                self.end_line()?;
                continue;
            }
            self.i += 1;
            let value = self.value_until(b'\n')?;
            events.push(Event::Entry { key, offset, value });
            self.end_line()?;
        }
        Ok(())
    }
}

fn leading_int(s: &[u8]) -> i64 {
    let t: &[u8] = {
        let mut k = 0;
        while k < s.len() && matches!(s[k], b' ' | b'\t' | b'\n' | b'\r') {
            k += 1;
        }
        &s[k..]
    };
    let mut i = 0;
    let neg = t.first() == Some(&b'-');
    if neg || t.first() == Some(&b'+') {
        i = 1;
    }
    let mut v: i64 = 0;
    while i < t.len() && t[i].is_ascii_digit() {
        v = v.wrapping_mul(10).wrapping_add((t[i] - b'0') as i64);
        i += 1;
    }
    if neg { -v } else { v }
}

/// A decimal integer in INI_SCANNER_TYPED (`12`, `-3`).
pub fn int_of(s: &[u8]) -> Option<i64> {
    let t = s.strip_prefix(b"-").unwrap_or(s);
    if t.is_empty() || !t.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

fn float_of(s: &[u8]) -> Option<f64> {
    let t = s.strip_prefix(b"-").unwrap_or(s);
    let dot = t.iter().position(|&c| c == b'.')?;
    let (a, b) = (&t[..dot], &t[dot + 1..]);
    if a.is_empty() && b.is_empty() {
        return None;
    }
    if !a.iter().all(u8::is_ascii_digit) || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

/// Parses an INI document.
pub fn parse<E: Env>(src: &[u8], mode: Mode, env: &mut E) -> Result<Vec<Event>, IniError> {
    let mut p = P {
        s: src,
        i: 0,
        line: 1,
        mode,
        env,
        fallback: 0,
    };
    let mut events = Vec::new();
    p.run(&mut events)?;
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct E;
    impl Env for E {
        fn constant(&mut self, name: &[u8]) -> Option<Vec<u8>> {
            match name {
                b"E_ALL" => Some(b"30719".to_vec()),
                b"E_NOTICE" => Some(b"8".to_vec()),
                _ => None,
            }
        }
        fn var(&mut self, _name: &[u8]) -> Option<Vec<u8>> {
            None
        }
    }

    fn one(s: &str, mode: Mode) -> IniValue {
        match parse(s.as_bytes(), mode, &mut E).unwrap().pop().unwrap() {
            Event::Entry { value, .. } => value,
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn values() {
        // Observed on PHP 8.5.
        assert_eq!(one("a = on", Mode::Normal), IniValue::Str(b"1".to_vec()));
        assert_eq!(
            one("f = E_ALL & ~E_NOTICE", Mode::Normal),
            IniValue::Str(b"30711".to_vec())
        );
        assert_eq!(
            one("g = \"x\" y 'z'", Mode::Normal),
            IniValue::Str(b"xy z".to_vec())
        );
        assert_eq!(one("c = 12", Mode::Typed), IniValue::Int(12));
        assert_eq!(
            one("m = ${U:-on}", Mode::Normal),
            IniValue::Str(b"on".to_vec())
        );
        assert_eq!(
            one("m = ${U:-true}", Mode::Normal),
            IniValue::Str(b"1".to_vec())
        );
    }
}
