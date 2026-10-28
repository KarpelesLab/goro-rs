//! Replays tools/charset/fixtures/mb_ops.php's recorded calls (one php85
//! run, gzipped in fixtures/mb_ops.jsonl.gz) against goro-charset.
//!
//! `GORO_CHARSET_OPS=func1,func2` limits the functions checked;
//! `GORO_CHARSET_OPS_SHOW=N` prints up to N mismatches per function.

mod common;

use common::{Json, gunzip};
use goro_charset::ops::{self, MbError};
use goro_charset::{Encoding, Subst, SubstMode};
use std::collections::BTreeMap;

fn bytes(v: &Json) -> Vec<u8> {
    match v {
        Json::Obj(_) => {
            if let Some(Json::Str(s)) = v.get("s") {
                return s.as_bytes().to_vec();
            }
            if let Some(Json::Str(h)) = v.get("h") {
                return (0..h.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
                    .collect();
            }
            panic!("not a string: {v:?}")
        }
        _ => panic!("not a string: {v:?}"),
    }
}

fn int(v: &Json) -> i64 {
    match v {
        Json::Num(n) => *n as i64,
        Json::Bool(b) => *b as i64,
        _ => panic!("not an int: {v:?}"),
    }
}

fn enc(v: &Json) -> Encoding {
    Encoding::from_name(&bytes(v)).unwrap_or_else(|| panic!("encoding {v:?}"))
}

fn s(b: Vec<u8>) -> Json {
    // As the driver's /^[\x20-\x7e]*$/ test, whose `$` also matches before
    // a final newline.
    let body = b.strip_suffix(b"\n").unwrap_or(&b);
    if body.iter().all(|&c| (0x20..0x7F).contains(&c)) {
        Json::Obj(vec![("s".into(), Json::Str(String::from_utf8(b).unwrap()))])
    } else {
        let h: String = b.iter().map(|c| format!("{c:02x}")).collect();
        Json::Obj(vec![("h".into(), Json::Str(h))])
    }
}

fn verr(func: &str, e: MbError) -> Json {
    Json::Obj(vec![
        ("e".into(), Json::Str("ValueError".into())),
        ("m".into(), Json::Str(e.message(func))),
    ])
}

fn num(n: usize) -> Json {
    Json::Num(n as f64)
}

fn opt_pos(r: Result<Option<usize>, MbError>, func: &str) -> Json {
    match r {
        Ok(Some(p)) => num(p),
        Ok(None) => Json::Bool(false),
        Err(e) => verr(func, e),
    }
}

thread_local! {
    /// mb_internal_encoding(), set by "internal" fixture lines.
    static INTERNAL: std::cell::Cell<Encoding> = const { std::cell::Cell::new(Encoding::Utf8) };
}

fn run(func: &str, a: &[Json], sub: &Subst) -> Option<Json> {
    let internal = INTERNAL.with(|c| c.get());
    Some(match func {
        "mb_strlen" => num(ops::strlen(&bytes(&a[0]), enc(&a[1]))),
        "mb_strwidth" => num(ops::strwidth(&bytes(&a[0]), enc(&a[1]))),
        "mb_substr" => {
            let len = match &a[2] {
                Json::Null => None,
                v => Some(int(v)),
            };
            s(ops::substr(&bytes(&a[0]), int(&a[1]), len, enc(&a[3]), sub))
        }
        "mb_strcut" => {
            let len = match &a[2] {
                Json::Null => None,
                v => Some(int(v)),
            };
            s(ops::strcut(&bytes(&a[0]), int(&a[1]), len, enc(&a[3])))
        }
        "mb_str_split" => Json::Arr(
            ops::str_split(&bytes(&a[0]), int(&a[1]) as usize, enc(&a[2]), sub)
                .into_iter()
                .map(s)
                .collect(),
        ),
        "mb_convert_case" => s(ops::convert_case(
            &bytes(&a[0]),
            ops::CaseMode::from_php(int(&a[1])).unwrap(),
            enc(&a[2]),
            sub,
        )),
        "mb_strtoupper" => s(ops::strtoupper(&bytes(&a[0]), enc(&a[1]), sub)),
        "mb_strtolower" => s(ops::strtolower(&bytes(&a[0]), enc(&a[1]), sub)),
        "mb_ucfirst" => s(ops::ucfirst(&bytes(&a[0]), enc(&a[1]), sub)),
        "mb_lcfirst" => s(ops::lcfirst(&bytes(&a[0]), enc(&a[1]), sub)),
        "mb_strimwidth" => match ops::strimwidth(
            &bytes(&a[0]),
            int(&a[1]),
            int(&a[2]),
            &bytes(&a[3]),
            enc(&a[4]),
            sub,
        ) {
            Ok(v) => s(v),
            Err(e) => verr(func, e),
        },
        "mb_str_pad" => {
            let r = ops::str_pad(
                &bytes(&a[0]),
                int(&a[1]),
                &bytes(&a[2]),
                int(&a[3]),
                enc(&a[4]),
                sub,
            );
            match r {
                Ok(v) => s(v),
                Err(e) => verr(func, e),
            }
        }
        "mb_trim" | "mb_ltrim" | "mb_rtrim" => {
            let mode = match func {
                "mb_trim" => ops::TrimMode::Both,
                "mb_ltrim" => ops::TrimMode::Left,
                _ => ops::TrimMode::Right,
            };
            let chars = match &a[1] {
                Json::Null => None,
                v => Some(bytes(v)),
            };
            s(ops::trim(
                &bytes(&a[0]),
                chars.as_deref(),
                mode,
                enc(&a[2]),
                sub,
            ))
        }
        "mb_ord" => {
            let b = bytes(&a[0]);
            if b.is_empty() {
                verr(
                    func,
                    MbError::ValueError("Argument #1 ($string) must not be empty".into()),
                )
            } else {
                match ops::ord(&b, enc(&a[1])) {
                    Ok(Some(c)) => Json::Num(c as f64),
                    Ok(None) => Json::Bool(false),
                    Err(e) => verr(func, e),
                }
            }
        }
        "mb_chr" => match ops::chr(int(&a[0]) as u32, enc(&a[1])) {
            Ok(Some(v)) => s(v),
            Ok(None) => Json::Bool(false),
            Err(e) => verr(func, e),
        },
        "mb_check_encoding" => Json::Bool(goro_charset::check_encoding(&bytes(&a[0]), enc(&a[1]))),
        "mb_scrub" => s(goro_charset::scrub(&bytes(&a[0]), enc(&a[1]), sub)),
        "mb_convert_encoding" => {
            s(goro_charset::convert(&bytes(&a[0]), enc(&a[1]), enc(&a[2]), sub).bytes)
        }
        "mb_strpos" => opt_pos(
            ops::strpos(&bytes(&a[0]), &bytes(&a[1]), int(&a[2]), enc(&a[3])),
            func,
        ),
        "mb_stripos" => opt_pos(
            ops::stripos(&bytes(&a[0]), &bytes(&a[1]), int(&a[2]), enc(&a[3])),
            func,
        ),
        "mb_strrpos" => opt_pos(
            ops::strrpos(&bytes(&a[0]), &bytes(&a[1]), int(&a[2]), enc(&a[3])),
            func,
        ),
        "mb_strripos" => opt_pos(
            ops::strripos(&bytes(&a[0]), &bytes(&a[1]), int(&a[2]), enc(&a[3])),
            func,
        ),
        "mb_strstr" | "mb_stristr" | "mb_strrchr" | "mb_strrichr" => {
            let f = match func {
                "mb_strstr" => ops::strstr,
                "mb_stristr" => ops::stristr,
                "mb_strrchr" => ops::strrchr,
                _ => ops::strrichr,
            };
            match f(
                &bytes(&a[0]),
                &bytes(&a[1]),
                a[2] == Json::Bool(true),
                enc(&a[3]),
                sub,
            ) {
                Some(v) => s(v),
                None => Json::Bool(false),
            }
        }
        "mb_substr_count" => match ops::substr_count(&bytes(&a[0]), &bytes(&a[1]), enc(&a[2])) {
            Ok(n) => num(n),
            Err(e) => verr(func, e),
        },
        "mb_encode_numericentity" | "mb_decode_numericentity" => {
            let Json::Arr(m) = &a[1] else { return None };
            let map: Vec<ops::entity::MapEntry> = m
                .chunks(4)
                .map(|q| {
                    ops::entity::MapEntry::from_php(int(&q[0]), int(&q[1]), int(&q[2]), int(&q[3]))
                })
                .collect();
            if func == "mb_encode_numericentity" {
                s(ops::entity::encode_numericentity(
                    &bytes(&a[0]),
                    &map,
                    enc(&a[2]),
                    a[3] == Json::Bool(true),
                    sub,
                ))
            } else {
                s(ops::entity::decode_numericentity(
                    &bytes(&a[0]),
                    &map,
                    enc(&a[2]),
                    sub,
                ))
            }
        }
        "mb_convert_kana" => match ops::kana::KanaMode::parse(&bytes(&a[1])) {
            Ok(m) => s(ops::kana::convert_kana(&bytes(&a[0]), m, enc(&a[2]), sub)),
            Err(e) => verr(func, e),
        },
        "mb_encode_mimeheader" => {
            let lf = a.get(3).map(bytes).unwrap_or_else(|| b"\r\n".to_vec());
            let indent = a.get(4).map(int).unwrap_or(0) as usize;
            let cs = match a.get(1) {
                Some(v) => Encoding::from_name(&bytes(v))?,
                None => internal,
            };
            let transfer = a.get(2).map(bytes).unwrap_or_else(|| b"B".to_vec());
            match ops::mime::encode_mimeheader(
                &bytes(&a[0]),
                cs,
                ops::mime::Transfer::from_php(&transfer),
                &lf,
                indent,
                internal,
            ) {
                Ok(v) => s(v),
                Err(e) => verr(func, e),
            }
        }
        "mb_decode_mimeheader" => s(ops::mime::decode_mimeheader(
            &bytes(a.first()?),
            internal,
            sub,
        )),
        "mb_detect_encoding" => {
            let Json::Arr(l) = &a[1] else { return None };
            let list: Vec<Encoding> = l.iter().map(enc).collect();
            match ops::detect::detect_encoding(&bytes(&a[0]), &list, a[2] == Json::Bool(true)) {
                Some(e) => s(e.name().as_bytes().to_vec()),
                None => Json::Bool(false),
            }
        }
        _ => return None,
    })
}

#[test]
fn ops_match_php() {
    replay(include_bytes!("fixtures/mb_ops.jsonl.gz"));
}

/// tools/charset/fixtures/mb_detect.php: detection on sample texts, random
/// bytes and around the thresholds of the scoring model.
#[test]
fn detect_match_php() {
    replay(include_bytes!("fixtures/mb_detect.jsonl.gz"));
}

/// tools/charset/fixtures/mb_mime.php: the MIME header calls of the php-src
/// tests (ext/mbstring/tests/mb_*mimeheader*.phpt).
#[test]
fn mime_match_php() {
    replay(include_bytes!("fixtures/mb_mime.jsonl.gz"));
}

fn replay(gz: &[u8]) {
    let raw = gunzip(gz);
    let text = String::from_utf8(raw).unwrap();
    let only: Option<Vec<String>> = std::env::var("GORO_CHARSET_OPS")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let show: usize = std::env::var("GORO_CHARSET_OPS_SHOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut sub = Subst::default();
    INTERNAL.with(|c| c.set(Encoding::Utf8));
    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut shown: BTreeMap<String, usize> = BTreeMap::new();
    for line in text.lines() {
        let Json::Arr(v) = Json::parse(line) else {
            panic!()
        };
        let Json::Str(func) = &v[0] else { panic!() };
        let Json::Arr(args) = &v[1] else { panic!() };
        if func == "internal" {
            let e = enc(&args[0]);
            INTERNAL.with(|c| c.set(e));
            continue;
        }
        if func == "subst" {
            sub = match &args[0] {
                Json::Num(n) => Subst::char(*n as u32),
                Json::Str(m) => Subst {
                    mode: match m.as_str() {
                        "none" => SubstMode::None,
                        "long" => SubstMode::Long,
                        _ => SubstMode::Entity,
                    },
                    ch: sub.ch,
                },
                _ => sub,
            };
            continue;
        }
        if let Some(o) = &only
            && !o.contains(func)
        {
            continue;
        }
        let Some(got) = run(func, args, &sub) else {
            continue;
        };
        let e = stats.entry(func.clone()).or_default();
        e.0 += 1;
        if got != v[2] {
            e.1 += 1;
            let n = shown.entry(func.clone()).or_default();
            if *n < show {
                *n += 1;
                if std::env::var_os("GORO_CHARSET_OPS_RAW").is_some() {
                    eprintln!("RAW {line}\nOURS {got:?}");
                } else {
                    eprintln!("{func} {args:?}\n   php: {:?}\n  ours: {got:?}", v[2]);
                }
            }
        }
    }
    let bad: Vec<String> = stats
        .iter()
        .filter(|(_, (_, b))| *b > 0)
        .map(|(f, (n, b))| format!("{f}: {b}/{n}"))
        .collect();
    let total: usize = stats.values().map(|s| s.0).sum();
    eprintln!("{total} calls checked");
    assert!(bad.is_empty(), "mismatches:\n{}", bad.join("\n"));
}
