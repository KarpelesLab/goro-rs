//! Shared helpers for the fixture tests: the same line-safe rendering the
//! PHP drivers in `tests/fixtures/` use (`common.php`).
#![allow(dead_code)]

pub mod json;

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// `q()` of common.php.
pub fn q(s: Option<&[u8]>) -> String {
    let Some(s) = s else {
        return "N".into();
    };
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for &b in s {
        if !(0x20..0x7f).contains(&b) || b == b'"' || b == b'\\' || b == b'|' {
            o.push_str(&format!("\\x{b:02X}"));
        } else {
            o.push(b as char);
        }
    }
    o.push('"');
    o
}

pub fn qi(v: Option<i64>) -> String {
    v.map_or("N".into(), |v| v.to_string())
}

/// Reads a fixture file (lines of tab-separated fields).
pub fn fixture(name: &str) -> Vec<Vec<String>> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    // The fixtures are ASCII (q() escapes every other byte).
    let text = String::from_utf8(data).expect("fixture is ASCII");
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

/// Collects mismatches and reports them at the end.
pub struct Report {
    pub name: &'static str,
    pub total: usize,
    pub bad: Vec<String>,
}

impl Report {
    pub fn new(name: &'static str) -> Report {
        Report {
            name,
            total: 0,
            bad: Vec::new(),
        }
    }
    pub fn check(&mut self, what: impl FnOnce() -> String, got: &str, want: &str) {
        self.total += 1;
        if got != want {
            self.bad
                .push(format!("{}\n   got: {got}\n  want: {want}", what()));
        }
    }
    pub fn finish(self) {
        let n = self.bad.len();
        for b in self.bad.iter().take(60) {
            eprintln!("{b}");
        }
        eprintln!("{}: {} of {} cases differ", self.name, n, self.total);
        assert_eq!(n, 0, "{}: {n} mismatches", self.name);
    }
}

/// A printable form of an input for failure messages.
pub fn show(b: &[u8]) -> String {
    q(Some(b))
}
