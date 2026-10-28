//! Checks the string functions against `fixtures/strings.txt`, the output of
//! `fixtures/strings.php` on PHP 8.5.

use goro_ext_standard::error::{Diag, StdError};
use goro_ext_standard::natcmp::strnatcmp;
use goro_ext_standard::phonetic;
use goro_ext_standard::strings::{self as s, PosKind, Replacement, TrimMode, WordCount};

#[derive(Debug, Clone, PartialEq)]
enum V {
    I(i64),
    F(f64),
    S(Vec<u8>),
    B(bool),
    N,
    A(Vec<(V, V)>),
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn parse(a: &[u8], i: &mut usize) -> V {
    let t = a[*i];
    if t == b'A' {
        *i += 2;
        let mut items = Vec::new();
        while a[*i] != b'}' {
            let k = parse(a, i);
            assert_eq!(a[*i], b'=');
            *i += 1;
            let v = parse(a, i);
            assert_eq!(a[*i], b';');
            *i += 1;
            items.push((k, v));
        }
        *i += 1;
        return V::A(items);
    }
    let start = *i + 2;
    let mut j = start;
    while j < a.len() && !matches!(a[j], b',' | b'=' | b';' | b'}' | b'\t') {
        j += 1;
    }
    let v = std::str::from_utf8(&a[start..j]).unwrap();
    *i = j;
    match t {
        b'i' => V::I(v.parse().unwrap()),
        b'f' => V::F(f64::from_bits(u64::from_str_radix(v, 16).unwrap())),
        b's' => V::S(unhex(v)),
        b'b' => V::B(v == "1"),
        b'n' => V::N,
        _ => panic!("bad tag {}", t as char),
    }
}

fn parse_args(s: &str) -> Vec<V> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        out.push(parse(b, &mut i));
        if i < b.len() && b[i] == b',' {
            i += 1;
        }
    }
    out
}

fn enc(v: &V) -> String {
    match v {
        V::I(i) => format!("i:{i}"),
        V::F(f) => format!("f:{:016x}", f.to_bits()),
        V::S(s) => format!("s:{}", hex(s)),
        V::B(b) => format!("b:{}", *b as i32),
        V::N => "n:".into(),
        V::A(items) => {
            let mut o = String::from("A{");
            for (k, v) in items {
                o.push_str(&enc(k));
                o.push('=');
                o.push_str(&enc(v));
                o.push(';');
            }
            o.push('}');
            o
        }
    }
}

fn st(v: &V) -> Vec<u8> {
    match v {
        V::S(s) => s.clone(),
        V::I(i) => i.to_string().into_bytes(),
        V::N => Vec::new(),
        V::B(true) => b"1".to_vec(),
        V::B(false) => Vec::new(),
        V::F(f) => goro_ext_standard::printf::float_to_string(*f),
        V::A(_) => panic!("array to string"),
    }
}

fn int(v: &V) -> i64 {
    match v {
        V::I(i) => *i,
        V::B(b) => *b as i64,
        _ => panic!("{v:?}"),
    }
}

fn opt_int(a: &[V], i: usize) -> Option<i64> {
    match a.get(i) {
        None | Some(V::N) => None,
        Some(v) => Some(int(v)),
    }
}

fn list(v: &[Vec<u8>]) -> V {
    V::A(
        v.iter()
            .enumerate()
            .map(|(i, x)| (V::I(i as i64), V::S(x.clone())))
            .collect(),
    )
}

fn opt_str(o: Option<Vec<u8>>) -> V {
    o.map_or(V::B(false), V::S)
}

fn err(e: StdError) -> String {
    match e {
        StdError::ValueError(m) => format!("E:ValueError:{m}"),
        StdError::TypeError(m) => format!("E:TypeError:{m}"),
        e => format!("{e:?}"),
    }
}

type R = Result<V, StdError>;

fn eval(name: &str, a: &[V], d: &mut Vec<Diag>) -> Option<R> {
    let s0 = || st(&a[0]);
    Some(match name {
        "str_pad" => s::str_pad(
            &s0(),
            int(&a[1]),
            &a.get(2).map_or(b" ".to_vec(), st),
            a.get(3).map_or(1, int),
        )
        .map(V::S),
        "wordwrap" => s::wordwrap(
            &s0(),
            a.get(1).map_or(75, int),
            &a.get(2).map_or(b"\n".to_vec(), st),
            a.get(3).is_some_and(|v| *v == V::B(true)),
        )
        .map(V::S),
        "chunk_split" => s::chunk_split(
            &s0(),
            a.get(1).map_or(76, int),
            &a.get(2).map_or(b"\r\n".to_vec(), st),
        )
        .map(V::S),
        "nl2br" => Ok(V::S(s::nl2br(&s0(), a.get(1) != Some(&V::B(false))))),
        "strrev" => Ok(V::S(s::strrev(&s0()))),
        "ucfirst" => Ok(V::S(s::ucfirst(&s0()))),
        "lcfirst" => Ok(V::S(s::lcfirst(&s0()))),
        "strtolower" => Ok(V::S(s::strtolower(&s0()))),
        "strtoupper" => Ok(V::S(s::strtoupper(&s0()))),
        "strlen" => Ok(V::I(s0().len() as i64)),
        "ucwords" => Ok(V::S(s::ucwords(
            &s0(),
            &a.get(1).map_or(s::UCWORDS_DELIMITERS.to_vec(), st),
        ))),
        "str_word_count" => {
            let cl = a.get(2).map(st);
            s::str_word_count(&s0(), a.get(1).map_or(0, int), cl.as_deref(), d).map(|w| match w {
                WordCount::Count(n) => V::I(n),
                WordCount::Words(w) => list(&w),
                WordCount::Positions(p) => {
                    V::A(p.into_iter().map(|(k, v)| (V::I(k), V::S(v))).collect())
                }
            })
        }
        "trim" | "ltrim" | "rtrim" => {
            let mode = match name {
                "trim" => TrimMode::Both,
                "ltrim" => TrimMode::Left,
                _ => TrimMode::Right,
            };
            let ch = a.get(1).map(st);
            Ok(V::S(s::trim(&s0(), ch.as_deref(), mode, d)))
        }
        "addslashes" => Ok(V::S(s::addslashes(&s0()))),
        "stripslashes" => Ok(V::S(s::stripslashes(&s0()))),
        "addcslashes" => Ok(V::S(s::addcslashes(&s0(), &st(&a[1]), d))),
        "stripcslashes" => Ok(V::S(s::stripcslashes(&s0()))),
        "quotemeta" => Ok(V::S(s::quotemeta(&s0()))),
        "strtr" => match &a[1] {
            V::A(items) => {
                let pairs: Vec<(Vec<u8>, Vec<u8>)> =
                    items.iter().map(|(k, v)| (st(k), st(v))).collect();
                Ok(V::S(s::strtr_pairs(&s0(), &pairs, d)))
            }
            from => Ok(V::S(s::strtr_chars(&s0(), &st(from), &st(&a[2])))),
        },
        "str_replace" | "str_ireplace" => {
            let ci = name == "str_ireplace";
            let searches: Vec<Vec<u8>> = match &a[0] {
                V::A(items) => items.iter().map(|(_, v)| st(v)).collect(),
                v => vec![st(v)],
            };
            let reps: Vec<Vec<u8>>;
            let rs: Vec<u8>;
            let replacement = match (&a[0], &a[1]) {
                (V::A(_), V::A(items)) => {
                    reps = items.iter().map(|(_, v)| st(v)).collect();
                    Replacement::List(&reps)
                }
                (V::A(_), v) => {
                    rs = st(v);
                    Replacement::Str(&rs)
                }
                (_, V::A(_)) => {
                    return Some(Err(StdError::TypeError(format!(
                        "{name}(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string"
                    ))));
                }
                (_, v) => {
                    rs = st(v);
                    Replacement::Str(&rs)
                }
            };
            let mut total = 0;
            let result = match &a[2] {
                V::A(items) => V::A(
                    items
                        .iter()
                        .map(|(k, v)| {
                            let (r, n) = s::str_replace_one(&st(v), &searches, replacement, ci);
                            total += n;
                            (k.clone(), V::S(r))
                        })
                        .collect(),
                ),
                v => {
                    let (r, n) = s::str_replace_one(&st(v), &searches, replacement, ci);
                    total = n;
                    V::S(r)
                }
            };
            Ok(V::A(vec![(V::I(0), result), (V::I(1), V::I(total as i64))]))
        }
        "substr_replace" => match (&a[0], &a[1], &a[2]) {
            (V::S(x), V::S(r), V::I(o)) => Ok(V::S(s::substr_replace(x, r, *o, opt_int(a, 3)))),
            _ => return None,
        },
        "substr_count" => {
            s::substr_count(&s0(), &st(&a[1]), a.get(2).map_or(0, int), opt_int(a, 3)).map(V::I)
        }
        "similar_text" => {
            let (n, p) = phonetic::similar_text(&s0(), &st(&a[1]));
            Ok(V::A(vec![(V::I(0), V::I(n)), (V::I(1), V::F(p))]))
        }
        "levenshtein" => Ok(V::I(phonetic::levenshtein(
            &s0(),
            &st(&a[1]),
            a.get(2).map_or(1, int),
            a.get(3).map_or(1, int),
            a.get(4).map_or(1, int),
        ))),
        "soundex" => Ok(V::S(phonetic::soundex(&s0()))),
        "metaphone" => phonetic::metaphone(&s0(), a.get(1).map_or(0, int)).map(V::S),
        "strnatcmp" => Ok(V::I(strnatcmp(&s0(), &st(&a[1]), false))),
        "strnatcasecmp" => Ok(V::I(strnatcmp(&s0(), &st(&a[1]), true))),
        "str_contains" => Ok(V::B(s::str_contains(&s0(), &st(&a[1])))),
        "str_starts_with" => Ok(V::B(s::str_starts_with(&s0(), &st(&a[1])))),
        "str_ends_with" => Ok(V::B(s::str_ends_with(&s0(), &st(&a[1])))),
        "strpbrk" => s::strpbrk(&s0(), &st(&a[1])).map(opt_str),
        "strspn" => Ok(V::I(s::strspn(
            &s0(),
            &st(&a[1]),
            a.get(2).map_or(0, int),
            opt_int(a, 3),
        ))),
        "strcspn" => Ok(V::I(s::strcspn(
            &s0(),
            &st(&a[1]),
            a.get(2).map_or(0, int),
            opt_int(a, 3),
        ))),
        "explode" => {
            s::explode(&s0(), &st(&a[1]), a.get(2).map_or(i64::MAX, int)).map(|v| list(&v))
        }
        "implode" => match (&a[0], a.get(1)) {
            (sep, Some(V::A(items))) if !matches!(sep, V::A(_)) => {
                let parts: Vec<Vec<u8>> = items
                    .iter()
                    .filter_map(|(_, v)| match v {
                        V::F(_) => None,
                        v => Some(st(v)),
                    })
                    .collect();
                if parts.len() != items.len() {
                    return None;
                }
                Ok(V::S(s::implode(&st(sep), &parts)))
            }
            _ => return None,
        },
        "str_split" => s::str_split(&s0(), a.get(1).map_or(1, int)).map(|v| list(&v)),
        "str_repeat" => s::str_repeat(&s0(), int(&a[1])).map(V::S),
        "substr" => Ok(V::S(s::substr(&s0(), int(&a[1]), opt_int(a, 2)))),
        "strpos" | "stripos" | "strrpos" | "strripos" => {
            let k = match name {
                "strpos" => PosKind::Pos,
                "stripos" => PosKind::IPos,
                "strrpos" => PosKind::RPos,
                _ => PosKind::RIPos,
            };
            s::strpos(&s0(), &st(&a[1]), a.get(2).map_or(0, int), k)
                .map(|o| o.map_or(V::B(false), V::I))
        }
        "strstr" | "stristr" => Ok(opt_str(s::strstr(
            &s0(),
            &st(&a[1]),
            a.get(2) == Some(&V::B(true)),
            name == "stristr",
        ))),
        "strrchr" => Ok(opt_str(s::strrchr(
            &s0(),
            &st(&a[1]),
            a.get(2) == Some(&V::B(true)),
        ))),
        "strcmp" => Ok(V::I(s::strcmp(&s0(), &st(&a[1])))),
        "strcasecmp" => Ok(V::I(s::strcasecmp(&s0(), &st(&a[1])))),
        "strncmp" => s::strncmp(&s0(), &st(&a[1]), int(&a[2]), false).map(V::I),
        "strncasecmp" => s::strncmp(&s0(), &st(&a[1]), int(&a[2]), true).map(V::I),
        "substr_compare" => s::substr_compare(
            &s0(),
            &st(&a[1]),
            int(&a[2]),
            opt_int(a, 3),
            a.get(4) == Some(&V::B(true)),
        )
        .map(V::I),
        "count_chars" => match a.get(1).map_or(0, int) {
            m @ 3..=5 => s::count_chars_string(&s0(), m).map(V::S),
            1 => {
                let c = s::count_chars(&s0());
                Ok(V::A(
                    (0..256)
                        .filter(|&i| c[i] > 0)
                        .map(|i| (V::I(i as i64), V::I(c[i] as i64)))
                        .collect(),
                ))
            }
            _ => return None,
        },
        _ => return None,
    })
}

#[test]
fn strings_fixture() {
    let data = include_str!("fixtures/strings.txt");
    let mut failures = Vec::new();
    let mut by_func: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    for line in data.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        let args = parse_args(parts[1]);
        let mut d = Vec::new();
        let Some(r) = eval(parts[0], &args, &mut d) else {
            continue;
        };
        let mut got = match r {
            Ok(v) => format!("R:{}", enc(&v)),
            Err(e) => err(e),
        };
        for x in d {
            let (Diag::Warning(m) | Diag::Deprecated(m) | Diag::Notice(m)) = x;
            got.push_str(&format!("\tW:{}(): {m}", parts[0]));
        }
        let e = by_func.entry(parts[0]).or_default();
        e.0 += 1;
        if got != parts[2] {
            e.1 += 1;
            failures.push(format!(
                "{} {:?}\n  want {}\n  got  {}",
                parts[0], args, parts[2], got
            ));
        }
    }
    let summary: Vec<String> = by_func
        .iter()
        .filter(|(_, (_, f))| *f > 0)
        .map(|(k, (n, f))| format!("{k}: {f}/{n}"))
        .collect();
    if !failures.is_empty() {
        panic!(
            "{} cases differ ({})\n{}",
            failures.len(),
            summary.join(", "),
            failures.join("\n")
        );
    }
}
