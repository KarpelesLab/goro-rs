//! Runs the literal conversion cases of php-src's mbstring encoding tests.
//!
//! The ext/mbstring/tests/*_encoding.phpt tests share helpers from
//! encoding_tests.inc (`testValidString`, `convertInvalidString`, ...). This
//! test reads the .phpt files at test time (from `GORO_PHP_TESTS`, default
//! /tmp/claude-1000/php-tests; skipped when absent), picks out the helper
//! calls whose arguments are all string literals, tracks the literal
//! `mb_substitute_character()` calls between them, and checks each case the
//! way the helper does: `mb_check_encoding` for the input, the converted
//! bytes, and whether `illegal_chars` grew.

use goro_charset::{Encoding, Subst, SubstMode, check_encoding, convert};
use std::path::PathBuf;

fn tests_dir() -> Option<PathBuf> {
    let base = std::env::var("GORO_PHP_TESTS").unwrap_or_else(|_| "/tmp/claude-1000/php-tests".into());
    let p = PathBuf::from(base).join("ext/mbstring/tests");
    p.is_dir().then_some(p)
}

#[derive(Debug, Clone)]
enum Arg {
    Str(Vec<u8>),
    Int(i64),
    Bool(bool),
}

/// Parses a PHP argument list of literals: strings (with `.` concatenation),
/// integers and booleans. Returns `None` for anything else.
fn parse_args(s: &str) -> Option<Vec<Arg>> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut args = Vec::new();
    let ws = |i: &mut usize| {
        while *i < b.len() && b[*i].is_ascii_whitespace() {
            *i += 1;
        }
    };
    loop {
        ws(&mut i);
        if i >= b.len() {
            break;
        }
        // One argument: a string concatenation, a number or a boolean.
        if b[i] == b'"' || b[i] == b'\'' {
            let mut v = Vec::new();
            loop {
                ws(&mut i);
                let q = *b.get(i)?;
                if q != b'"' && q != b'\'' {
                    return None;
                }
                i += 1;
                loop {
                    let c = *b.get(i)?;
                    i += 1;
                    if c == q {
                        break;
                    }
                    if c != b'\\' {
                        if q == b'"' && c == b'$' {
                            return None; // interpolation
                        }
                        v.push(c);
                        continue;
                    }
                    let e = *b.get(i)?;
                    i += 1;
                    if q == b'\'' {
                        if e == b'\'' || e == b'\\' {
                            v.push(e);
                        } else {
                            v.extend_from_slice(&[b'\\', e]);
                        }
                        continue;
                    }
                    match e {
                        b'n' => v.push(b'\n'),
                        b'r' => v.push(b'\r'),
                        b't' => v.push(b'\t'),
                        b'v' => v.push(0x0B),
                        b'f' => v.push(0x0C),
                        b'e' => v.push(0x1B),
                        b'\\' | b'"' | b'$' => v.push(e),
                        b'x' if b.get(i).is_some_and(u8::is_ascii_hexdigit) => {
                            let mut n = 0u32;
                            let mut k = 0;
                            while k < 2 && b.get(i).is_some_and(u8::is_ascii_hexdigit) {
                                n = n * 16 + (b[i] as char).to_digit(16).unwrap();
                                i += 1;
                                k += 1;
                            }
                            v.push(n as u8);
                        }
                        b'0'..=b'7' => {
                            let mut n = (e - b'0') as u32;
                            let mut k = 1;
                            while k < 3 && b.get(i).is_some_and(|c| (b'0'..=b'7').contains(c)) {
                                n = n * 8 + (b[i] - b'0') as u32;
                                i += 1;
                                k += 1;
                            }
                            v.push(n as u8);
                        }
                        b'u' if b.get(i) == Some(&b'{') => {
                            let end = i + b[i..].iter().position(|&c| c == b'}')?;
                            let cp = u32::from_str_radix(std::str::from_utf8(&b[i + 1..end]).ok()?, 16).ok()?;
                            let mut buf = [0u8; 4];
                            v.extend_from_slice(char::from_u32(cp)?.encode_utf8(&mut buf).as_bytes());
                            i = end + 1;
                        }
                        _ => v.extend_from_slice(&[b'\\', e]),
                    }
                }
                ws(&mut i);
                if b.get(i) == Some(&b'.') {
                    i += 1;
                    continue;
                }
                break;
            }
            args.push(Arg::Str(v));
        } else {
            let start = i;
            while i < b.len() && b[i] != b',' {
                i += 1;
            }
            let tok = s[start..i].trim();
            let a = match tok {
                "true" => Arg::Bool(true),
                "false" => Arg::Bool(false),
                _ if tok.starts_with("0x") => Arg::Int(i64::from_str_radix(&tok[2..], 16).ok()?),
                _ => Arg::Int(tok.parse().ok()?),
            };
            args.push(a);
        }
        ws(&mut i);
        match b.get(i) {
            None => break,
            Some(b',') => i += 1,
            _ => return None,
        }
    }
    Some(args)
}

/// Splits `name(args);` statements out of a file, with the argument text.
fn calls(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        let Some(p) = l.find('(') else { continue };
        let name = &l[..p];
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        // The statement must end on this line with `);` (a comment may follow).
        let rest = &l[p + 1..];
        let Some(end) = find_close(rest) else { continue };
        out.push((name.to_string(), rest[..end].to_string()));
    }
    out
}

/// The index of the `)` closing an argument list, skipping string literals.
fn find_close(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            q @ (b'"' | b'\'') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' => depth += 1,
            b')' if depth == 0 => {
                return s[i + 1..].trim_start().starts_with(';').then_some(i);
            }
            b')' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

fn hex(s: &[u8]) -> String {
    s.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn encoding_test_cases() {
    let Some(dir) = tests_dir() else {
        eprintln!("php-src tests not found; skipping");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "phpt"))
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut checked = 0;
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap_or_default();
        if !src.contains("encoding_tests.inc") {
            continue;
        }
        let fname = f.file_name().unwrap().to_string_lossy().to_string();
        let mut sub = Subst::default();
        for (name, argtext) in calls(&src) {
            if name == "testEncodingFromUTF16ConversionTable" {
                // The helper sets the substitute character to '%'.
                sub = Subst::char(0x25);
                continue;
            }
            let helper = matches!(
                name.as_str(),
                "convertValidString"
                    | "convertInvalidString"
                    | "testValidString"
                    | "testInvalidString"
                    | "identifyValidString"
                    | "identifyInvalidString"
                    | "mb_substitute_character"
            );
            if !helper {
                continue;
            }
            let Some(args) = parse_args(&argtext) else { continue };
            if name == "mb_substitute_character" {
                match args.first() {
                    Some(Arg::Int(n)) => sub = Subst::char(*n as u32),
                    Some(Arg::Str(s)) if s == b"long" => sub.mode = SubstMode::Long,
                    Some(Arg::Str(s)) if s == b"none" => sub.mode = SubstMode::None,
                    Some(Arg::Str(s)) if s == b"entity" => sub.mode = SubstMode::Entity,
                    _ => {}
                }
                continue;
            }
            let strs: Vec<&[u8]> = args
                .iter()
                .filter_map(|a| if let Arg::Str(s) = a { Some(&s[..]) } else { None })
                .collect();
            let both = !matches!(args.last(), Some(Arg::Bool(false)));
            let enc = |n: &[u8]| Encoding::from_name(n);
            let mut fail = |msg: String| failures.push(format!("{fname}: {name}({argtext}): {msg}"));
            let identify = |s: &[u8], e: Encoding, want: bool, fail: &mut dyn FnMut(String)| {
                if check_encoding(s, e) != want {
                    fail(format!("mb_check_encoding({}, {}) != {want}", hex(s), e.name()));
                }
            };
            let conv = |from: &[u8], to: &[u8], fe: Encoding, te: Encoding, valid: bool, fail: &mut dyn FnMut(String)| {
                let c = convert(from, te, fe, &sub);
                if c.bytes != to {
                    fail(format!(
                        "{} -> {}: {} gave {}, want {}",
                        fe.name(),
                        te.name(),
                        hex(from),
                        hex(&c.bytes),
                        hex(to)
                    ));
                } else if valid && c.illegal != 0 {
                    fail(format!("illegal_chars grew on valid {}", hex(from)));
                } else if !valid && c.illegal == 0 {
                    fail(format!("illegal_chars did not grow on {}", hex(from)));
                }
            };
            match (name.as_str(), &strs[..]) {
                ("identifyValidString" | "identifyInvalidString", [s, e]) => {
                    let Some(e) = enc(e) else { continue };
                    identify(s, e, name == "identifyValidString", &mut fail);
                }
                (_, [from, to, fe, te]) => {
                    let (Some(fe), Some(te)) = (enc(fe), enc(te)) else { continue };
                    match name.as_str() {
                        "testValidString" | "convertValidString" => {
                            if name == "testValidString" {
                                identify(from, fe, true, &mut fail);
                            }
                            conv(from, to, fe, te, true, &mut fail);
                            if both {
                                conv(to, from, te, fe, true, &mut fail);
                            }
                        }
                        _ => {
                            if name == "testInvalidString" {
                                identify(from, fe, false, &mut fail);
                            }
                            conv(from, to, fe, te, false, &mut fail);
                        }
                    }
                }
                _ => continue,
            }
            checked += 1;
        }
    }
    eprintln!("{checked} cases checked");
    if !failures.is_empty() {
        panic!("{} failures:\n{}", failures.len(), failures.join("\n"));
    }
}
