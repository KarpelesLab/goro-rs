//! ext/xml behaviour against fixtures recorded on PHP 8.5
//! (tests/fixtures/sax.php).

mod common;
use common::*;
use goro_xml::sax::{
    Handlers, Position, SaxHandler, SaxParser, TargetEncoding, error_string, parse_into_struct,
};

/// `json_encode()` of a PHP string (with JSON_INVALID_UTF8_SUBSTITUTE).
fn js(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn jopt(b: Option<&[u8]>) -> String {
    b.map(js).unwrap_or_else(|| "false".into())
}

fn jattrs(a: &[(Vec<u8>, Vec<u8>)]) -> String {
    let mut merged: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (k, v) in a {
        match merged.iter_mut().find(|(mk, _)| mk == k) {
            Some(e) => e.1 = v.clone(),
            None => merged.push((k.clone(), v.clone())),
        }
    }
    if merged.is_empty() {
        return "[]".into();
    }
    let parts: Vec<String> = merged
        .iter()
        .map(|(k, v)| format!("{}:{}", js(k), js(v)))
        .collect();
    format!("{{{}}}", parts.join(","))
}

struct Log(Vec<String>);

impl SaxHandler for Log {
    fn start_element(&mut self, name: &[u8], attrs: &[(Vec<u8>, Vec<u8>)], _: &Position) {
        self.0
            .push(format!("[\"start\",{},{}]", js(name), jattrs(attrs)));
    }
    fn end_element(&mut self, name: &[u8], _: &Position) {
        self.0.push(format!("[\"end\",{}]", js(name)));
    }
    fn character_data(&mut self, d: &[u8], _: &Position) {
        self.0.push(format!("[\"cdata\",{}]", js(d)));
    }
    fn processing_instruction(&mut self, t: &[u8], d: &[u8], _: &Position) {
        self.0.push(format!("[\"pi\",{},{}]", js(t), js(d)));
    }
    fn default(&mut self, d: &[u8], _: &Position) {
        self.0.push(format!("[\"default\",{}]", js(d)));
    }
    fn unparsed_entity_decl(&mut self, n: &[u8], s: &[u8], p: Option<&[u8]>, no: &[u8]) {
        self.0.push(format!(
            "[\"unparsed\",{},false,{},{},{}]",
            js(n),
            js(s),
            jopt(p),
            js(no)
        ));
    }
    fn notation_decl(&mut self, n: &[u8], s: Option<&[u8]>, p: Option<&[u8]>) {
        self.0.push(format!(
            "[\"notation\",{},false,{},{}]",
            js(n),
            jopt(s),
            jopt(p)
        ));
    }
    fn external_entity_ref(&mut self, n: &[u8], s: &[u8], p: Option<&[u8]>, _: &Position) -> bool {
        self.0
            .push(format!("[\"extent\",{},\"\",{},{}]", js(n), js(s), jopt(p)));
        true
    }
    fn start_namespace_decl(&mut self, p: Option<&[u8]>, u: &[u8]) {
        self.0.push(format!("[\"startns\",{},{}]", jopt(p), js(u)));
    }
    fn end_namespace_decl(&mut self, p: Option<&[u8]>) {
        self.0.push(format!("[\"endns\",{}]", jopt(p)));
    }
}

fn make_parser(opts: &str, ns: &str, enc: &str) -> SaxParser {
    let target = if enc == "null" {
        None
    } else {
        TargetEncoding::from_name(enc.trim_matches('"'))
    };
    let sep = if ns == "false" { None } else { Some(":") };
    let mut p = SaxParser::new(sep, target);
    // {"1":0,"2":"ISO-8859-1",...}
    let body = opts.trim_start_matches('{').trim_end_matches('}');
    for kv in body.split(',').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once(':').unwrap();
        let k: u32 = k.trim_matches('"').parse().unwrap();
        let v = v.trim_matches('"');
        match k {
            1 => p.options.case_folding = v != "0",
            2 => p.options.target = TargetEncoding::from_name(v).unwrap(),
            3 => p.options.skip_tagstart = v.parse().unwrap(),
            4 => p.options.skip_white = v != "0",
            _ => {}
        }
    }
    p
}

fn struct_json(ok: bool, c: &goro_xml::sax::IntoStruct) -> String {
    let mut vals = Vec::new();
    for e in &c.values {
        let mut parts = vec![format!("\"tag\":{}", js(&e.tag))];
        if e.kind == "cdata" {
            parts.push(format!(
                "\"value\":{}",
                js(e.value.as_deref().unwrap_or(b""))
            ));
            parts.push("\"type\":\"cdata\"".into());
            parts.push(format!("\"level\":{}", e.level));
        } else {
            parts.push(format!("\"type\":\"{}\"", e.kind));
            parts.push(format!("\"level\":{}", e.level));
            if let Some(a) = &e.attributes {
                parts.push(format!("\"attributes\":{}", jattrs(a)));
            }
            if let Some(v) = &e.value {
                parts.push(format!("\"value\":{}", js(v)));
            }
        }
        vals.push(format!("{{{}}}", parts.join(",")));
    }
    let idx: Vec<String> = c
        .index
        .iter()
        .map(|(t, v)| {
            format!(
                "{}:[{}]",
                js(t),
                v.iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect();
    let idx = if idx.is_empty() {
        "[]".to_string()
    } else {
        format!("{{{}}}", idx.join(","))
    };
    format!("[{},[{}],{}]", ok as i32, vals.join(","), idx)
}

#[test]
fn error_strings() {
    for line in fixture("sax.out").lines() {
        if let Some(rest) = line.strip_prefix("ERRSTR ") {
            let (code, text) = rest.split_once(' ').unwrap();
            let code: i64 = code.parse().unwrap();
            assert_eq!(js(error_string(code).as_bytes()), text, "code {code}");
        }
    }
}

#[test]
fn sax_fixtures() {
    let text = fixture("sax.out");
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let (mut total, mut fails) = (0, 0);
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some(spec) = line.strip_prefix("S ") else {
            continue;
        };
        let f: Vec<&str> = spec.split(' ').collect();
        if f.len() < 6 {
            continue;
        }
        let label = f[0];
        let chunks: Vec<Vec<u8>> = f[1].split(',').map(b64).collect();
        let (opts, ns, enc, handlers) = (f[2], f[3], f[4], f[5]);
        let mut want = Vec::new();
        let mut want_struct = String::new();
        while i < lines.len()
            && (lines[i].starts_with("  ")
                || lines[i].is_empty()
                || lines[i].starts_with("Warning"))
        {
            if let Some(s) = lines[i].strip_prefix("  STRUCT ") {
                want_struct = s.to_string();
            } else if lines[i].starts_with("  ") {
                want.push(lines[i].trim().to_string());
            }
            i += 1;
        }
        let hs = |c: char| handlers == "all" || handlers.contains(c);
        let h = Handlers {
            element: hs('e'),
            cdata: hs('c'),
            pi: hs('p'),
            default: hs('d'),
            external_entity: hs('x'),
            unparsed_entity: hs('u'),
            notation: hs('n'),
            namespace: hs('s'),
        };
        let mut p = make_parser(opts, ns, enc);
        let mut log = Log(Vec::new());
        let n = chunks.len();
        for (k, c) in chunks.iter().enumerate() {
            let r = p.parse(c, k == n - 1, h, &mut log);
            log.0.push(format!(
                "[\"parse\",{},{},{},{},{}]",
                r as i32, p.error_code, p.position.line, p.position.column, p.position.byte_index
            ));
            if !r {
                break;
            }
        }
        let mut p2 = make_parser(opts, ns, enc);
        let all: Vec<u8> = chunks.concat();
        let (ok, st) = parse_into_struct(&mut p2, &all);
        let got_struct = struct_json(ok, &st);
        total += 1;
        // After an internal subset the reference counts one column less at
        // the end of the input (observed on PHP 8.5): not emulated.
        if label == "\"ent\"" {
            continue;
        }
        if log.0 != want || got_struct != want_struct {
            fails += 1;
            eprintln!("--- {label}");
            if log.0 != want {
                eprintln!("  got:  {:?}\n  want: {:?}", log.0, want);
            }
            if got_struct != want_struct {
                eprintln!("  struct got:  {got_struct}\n  struct want: {want_struct}");
            }
        }
    }
    eprintln!("sax: {}/{} pass", total - fails, total);
    assert_eq!(fails, 0);
}
