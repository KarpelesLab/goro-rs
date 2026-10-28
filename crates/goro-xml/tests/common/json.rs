//! A small JSON reader for fixture files.

/// A JSON value.
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
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Parses JSON text.
pub fn parse(s: &str) -> Json {
    let b = s.as_bytes();
    let mut i = 0;
    value(b, &mut i)
}

fn ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && (b[*i] as char).is_whitespace() {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Json {
    ws(b, i);
    match b[*i] {
        b'n' => {
            *i += 4;
            Json::Null
        }
        b't' => {
            *i += 4;
            Json::Bool(true)
        }
        b'f' => {
            *i += 5;
            Json::Bool(false)
        }
        b'"' => Json::Str(string(b, i)),
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                ws(b, i);
                if b[*i] == b']' {
                    *i += 1;
                    break;
                }
                v.push(value(b, i));
                ws(b, i);
                if b[*i] == b',' {
                    *i += 1;
                }
            }
            Json::Arr(v)
        }
        b'{' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                ws(b, i);
                if b[*i] == b'}' {
                    *i += 1;
                    break;
                }
                let k = string(b, i);
                ws(b, i);
                *i += 1;
                let val = value(b, i);
                v.push((k, val));
                ws(b, i);
                if b[*i] == b',' {
                    *i += 1;
                }
            }
            Json::Obj(v)
        }
        _ => {
            let st = *i;
            while *i < b.len() && matches!(b[*i], b'-' | b'.' | b'e' | b'E' | b'+' | b'0'..=b'9') {
                *i += 1;
            }
            Json::Num(std::str::from_utf8(&b[st..*i]).unwrap().parse().unwrap())
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> String {
    *i += 1;
    let mut units: Vec<u16> = Vec::new();
    let mut s = String::new();
    while b[*i] != b'"' {
        if b[*i] == b'\\' {
            *i += 1;
            let c = b[*i];
            *i += 1;
            match c {
                b'n' => s.push('\n'),
                b'r' => s.push('\r'),
                b't' => s.push('\t'),
                b'b' => s.push('\u{8}'),
                b'f' => s.push('\u{c}'),
                b'u' => {
                    let h = std::str::from_utf8(&b[*i..*i + 4]).unwrap();
                    *i += 4;
                    units.push(u16::from_str_radix(h, 16).unwrap());
                    if !(0xD800..0xDC00).contains(units.last().unwrap()) {
                        s.push_str(&String::from_utf16_lossy(&units));
                        units.clear();
                    }
                }
                c => s.push(c as char),
            }
        } else {
            let st = *i;
            while b[*i] != b'"' && b[*i] != b'\\' {
                *i += 1;
            }
            s.push_str(std::str::from_utf8(&b[st..*i]).unwrap());
        }
    }
    *i += 1;
    s
}
