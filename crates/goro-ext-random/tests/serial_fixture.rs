//! Checks engine seeding, jumps and the serialization formats against
//! `fixtures/serial.txt`, the output of `fixtures/serial.php` on PHP 8.5.

use goro_ext_random::state::{StateItem, StateValue};
use goro_ext_random::{
    Engine, Mt19937, MtMode, PcgOneseq128XslRr64, RandomError, Xoshiro256StarStar,
};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn items(v: &[StateItem]) -> String {
    v.iter()
        .map(|i| match i {
            StateItem::Hex(h) => format!("s:{h}"),
            StateItem::Int(n) => format!("i:{n}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn next_hex(e: &mut impl Engine) -> String {
    e.generate()
        .unwrap()
        .bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn serial_fixture() {
    let data = include_str!("fixtures/serial.txt");
    let mut n = 0;
    for line in data.lines() {
        let (head, want) = line.split_once(" | ").unwrap();
        let h: Vec<&str> = head.split(' ').collect();
        let int = |s: &str| s.parse::<i64>().unwrap();
        let got = match (h[0], h.get(1).copied()) {
            ("ser", Some("mt")) => {
                items(&Mt19937::new(int(h[2]), MtMode::from_int(int(h[3]))).state())
            }
            ("ser", Some("mtgen")) => {
                let mut e = Mt19937::new(int(h[2]), MtMode::Mt19937);
                for _ in 0..int(h[3]) {
                    e.next_u32();
                }
                items(&e.state())
            }
            ("ser", Some("pcg")) => items(&PcgOneseq128XslRr64::from_int(int(h[2])).state()),
            ("ser", Some("pcgs")) => items(
                &PcgOneseq128XslRr64::from_string(&unhex(h[2]))
                    .unwrap()
                    .state(),
            ),
            ("ser", Some("xo")) => items(&Xoshiro256StarStar::from_int(int(h[2])).state()),
            ("ser", Some("xos")) => items(
                &Xoshiro256StarStar::from_string(&unhex(h[2]))
                    .unwrap()
                    .state(),
            ),
            ("jump", Some("pcg")) => {
                let mut e = PcgOneseq128XslRr64::from_int(int(h[2]));
                e.jump(int(h[3])).unwrap();
                items(&e.state())
            }
            ("jump", Some("xo")) => {
                let mut e = Xoshiro256StarStar::from_int(int(h[2]));
                if h[3] == "jump" {
                    e.jump()
                } else {
                    e.jump_long()
                }
                items(&e.state())
            }
            ("unser", Some(class)) => {
                // Values for keys 0..count of the state array.
                let toks: Vec<&str> = h[2..].to_vec();
                let strs: Vec<Vec<u8>> = toks
                    .iter()
                    .map(|t| {
                        t.strip_prefix("s:")
                            .unwrap_or("")
                            .replace('~', " ")
                            .into_bytes()
                    })
                    .collect();
                let mut by_key: Vec<StateValue<'_>> = toks
                    .iter()
                    .zip(&strs)
                    .map(|(t, s)| match *t {
                        "-" => StateValue::Missing,
                        "n" | "f" | "a" => StateValue::Other,
                        t if t.starts_with("i:") => StateValue::Int(int(&t[2..])),
                        _ => StateValue::Str(s),
                    })
                    .collect();
                let count = toks.iter().filter(|t| **t != "-").count();
                by_key.resize(count.max(by_key.len()), StateValue::Missing);
                let vals = &by_key[..count];
                let r: Result<String, RandomError> = match class {
                    "mt" => Mt19937::from_state(vals).map(|mut e| next_hex(&mut e)),
                    "pcg" => PcgOneseq128XslRr64::from_state(vals).map(|mut e| next_hex(&mut e)),
                    _ => Xoshiro256StarStar::from_state(vals).map(|mut e| next_hex(&mut e)),
                };
                match r {
                    Ok(h) => format!("ok {h}"),
                    Err(RandomError::Exception(m)) => format!("E Exception: {m}"),
                    Err(e) => format!("{e:?}"),
                }
            }
            // The outer data shape and Randomizer cases are checked by the
            // wiring layer (documented in `goro_ext_random::state`).
            _ => continue,
        };
        assert_eq!(got, want, "{head}");
        n += 1;
    }
    assert!(n > 90, "{n}");
}
