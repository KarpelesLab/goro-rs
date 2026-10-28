//! Test helpers: a minimal JSON reader for the fixture files.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => panic!("not a string: {self:?}"),
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }
}

pub fn parse(s: &str) -> Json {
    let mut p = P {
        b: s.as_bytes(),
        i: 0,
    };
    let v = p.value();
    v
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn value(&mut self) -> Json {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b'}' {
                        self.i += 1;
                        break;
                    }
                    let k = match self.value() {
                        Json::Str(s) => s,
                        _ => panic!("key"),
                    };
                    self.ws();
                    assert_eq!(self.b[self.i], b':');
                    self.i += 1;
                    let val = self.value();
                    v.push((k, val));
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    }
                }
                Json::Obj(v)
            }
            b'[' => {
                self.i += 1;
                let mut v = Vec::new();
                loop {
                    self.ws();
                    if self.b[self.i] == b']' {
                        self.i += 1;
                        break;
                    }
                    v.push(self.value());
                    self.ws();
                    if self.b[self.i] == b',' {
                        self.i += 1;
                    }
                }
                Json::Arr(v)
            }
            b'"' => {
                self.i += 1;
                let mut out: Vec<u16> = Vec::new();
                let mut s = String::new();
                loop {
                    let c = self.b[self.i];
                    if c == b'"' {
                        self.i += 1;
                        break;
                    }
                    if c == b'\\' {
                        let e = self.b[self.i + 1];
                        self.i += 2;
                        let ch = match e {
                            b'n' => '\n',
                            b't' => '\t',
                            b'r' => '\r',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'/' => '/',
                            b'\\' => '\\',
                            b'"' => '"',
                            b'u' => {
                                let h = std::str::from_utf8(&self.b[self.i..self.i + 4]).unwrap();
                                self.i += 4;
                                out.push(u16::from_str_radix(h, 16).unwrap());
                                continue;
                            }
                            _ => panic!("escape"),
                        };
                        flush(&mut out, &mut s);
                        s.push(ch);
                        continue;
                    }
                    flush(&mut out, &mut s);
                    // Copy one UTF-8 char.
                    let len = match c {
                        0x00..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    s.push_str(std::str::from_utf8(&self.b[self.i..self.i + len]).unwrap());
                    self.i += len;
                }
                flush(&mut out, &mut s);
                Json::Str(s)
            }
            b't' => {
                self.i += 4;
                Json::Bool(true)
            }
            b'f' => {
                self.i += 5;
                Json::Bool(false)
            }
            b'n' => {
                self.i += 4;
                Json::Null
            }
            _ => {
                let st = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                Json::Num(
                    std::str::from_utf8(&self.b[st..self.i])
                        .unwrap()
                        .parse()
                        .unwrap(),
                )
            }
        }
    }
}

fn flush(units: &mut Vec<u16>, s: &mut String) {
    if !units.is_empty() {
        for c in char::decode_utf16(units.drain(..)) {
            s.push(c.unwrap_or('\u{FFFD}'));
        }
    }
}
