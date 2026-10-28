//! Hand-written cases from the ext/random tests' expected output and from
//! PHP 8.5 observations with crafted user engines, plus the
//! `fixtures/float_edges.txt` fixture.

use goro_ext_random::legacy::LegacyRandom;
use goro_ext_random::randomizer::{self as rz, PickCaller, PickSource};
use goro_ext_random::{
    Engine, Generated, IntervalBoundary as B, MSG_EMPTY_STRING, MSG_RETRY_LIMIT, Mt19937, MtMode,
    RandomError, Secure, Xoshiro256StarStar,
};

/// A user engine returning scripted strings, then `fallback` forever.
struct Script {
    vals: std::collections::VecDeque<Vec<u8>>,
    fallback: Vec<u8>,
    calls: usize,
}

fn script(vals: &[&str]) -> Script {
    Script {
        vals: vals.iter().map(|h| unhex(h)).collect(),
        fallback: vec![0xff; 8],
        calls: 0,
    }
}

impl Engine for Script {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        self.calls += 1;
        let v = self
            .vals
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone());
        Generated::from_bytes(&v)
    }
}

/// A user engine returning the same string every time.
struct Constant(Vec<u8>);

impl Engine for Constant {
    fn generate(&mut self) -> Result<Generated, RandomError> {
        Generated::from_bytes(&self.0)
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

fn broken() -> RandomError {
    RandomError::BrokenEngine(MSG_RETRY_LIMIT)
}

fn rep(h: &str, n: usize) -> Vec<String> {
    vec![h.to_string(); n]
}

fn script_owned(v: Vec<String>) -> Script {
    let refs: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
    script(&refs)
}

// Observed on PHP 8.5: 50 rejections are retried, the 51st fails.
#[test]
fn retry_limit() {
    for (k, ok) in [(49, true), (50, true), (51, false)] {
        let mut v = rep("ffffffff", k);
        v.push("01000000".into());
        let mut e = script_owned(v);
        let r = rz::get_int(&mut e, 0, 2);
        assert_eq!(r.is_ok(), ok, "{k}");
        assert_eq!(e.calls, if ok { k + 1 } else { 51 });

        let mut v = rep("ffffffffffffffff", k);
        v.push("0100000000000000".into());
        let mut e = script_owned(v);
        assert_eq!(rz::get_int(&mut e, 0, (1 << 40) + 2).is_ok(), ok);
    }
}

// Observed on PHP 8.5: pickArrayKeys() draws into a bit set, retrying
// repeats up to 50 times in a row; more than half picked means drawing the
// keys to leave out.
#[test]
fn pick_semantics() {
    for (dups, ok, calls) in [(49, true, 51), (50, true, 52), (51, false, 52)] {
        let mut v = rep("03000000", dups + 1);
        v.push("04000000".into());
        let mut e = script_owned(v);
        let r = rz::pick_array_keys(&mut e, 10, 2);
        assert_eq!(r.is_ok(), ok);
        if ok {
            assert_eq!(r.unwrap(), [3, 4]);
        }
        assert_eq!(e.calls, calls);
    }
    let mut v = rep("03000000", 31);
    v.push("05000000".into());
    v.extend(rep("03000000", 30));
    v.push("06000000".into());
    let mut e = script_owned(v);
    assert_eq!(rz::pick_array_keys(&mut e, 10, 3).unwrap(), [3, 5, 6]);
    assert_eq!(e.calls, 63);

    let seq = ["07000000", "02000000", "05000000", "01000000", "00000000"];
    for (num, want, calls) in [
        (3, vec![2, 5, 7], 3),
        (8, vec![0, 1, 3, 4, 5, 6, 8, 9], 2),
        (1, vec![7], 1),
        (10, (0..10).collect(), 0),
        (5, vec![0, 1, 2, 5, 7], 5),
        (6, vec![0, 3, 4, 6, 8, 9], 4),
    ] {
        let mut e = script(&seq);
        assert_eq!(rz::pick_array_keys(&mut e, 10, num).unwrap(), want, "{num}");
        assert_eq!(e.calls, calls);
    }
}

// Observed on PHP 8.5: the getBytesFromString() fast path masks one byte at
// a time; the slow path draws a range per byte.
#[test]
fn bytes_from_string() {
    /// Engine results, source, length, expected output, generate() calls.
    type Case<'a> = (&'a [&'a str], &'a [u8], i64, &'a str, usize);
    let cases: &[Case] = &[
        (
            &["0303000102030301", "0001020300010203"],
            b"abc",
            6,
            "abcbab",
            2,
        ),
        (&["030102", "000102"], b"abc", 4, "bcab", 2),
        (&["030302", "000102"], b"abc", 4, "cabc", 2),
        (&["0201ff00"], b"ab", 4, "abba", 1),
        (&["0806050403020100"], b"abcde", 4, "aedc", 1),
    ];
    for &(vals, src, n, want, calls) in cases {
        let mut e = script(vals);
        assert_eq!(
            rz::get_bytes_from_string(&mut e, src, n).unwrap(),
            want.as_bytes()
        );
        assert_eq!(e.calls, calls);
    }
    let mut e = script(
        &rep("0303030303030303", 60)
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
    );
    assert_eq!(rz::get_bytes_from_string(&mut e, b"abc", 2), Err(broken()));
    assert_eq!(e.calls, 7);

    let mut src = vec![b'x'; 299];
    src.push(b'y');
    let mut e = script(&["2c01000000000000", "2b01000000000000"]);
    assert_eq!(rz::get_bytes_from_string(&mut e, &src, 2).unwrap(), b"xy");

    // test 03_randomizer/methods/getBytesFromString_fast_path.phpt
    struct Counting(Xoshiro256StarStar, usize);
    impl Engine for Counting {
        fn generate(&mut self) -> Result<Generated, RandomError> {
            self.1 += 1;
            self.0.generate()
        }
    }
    let all: Vec<u8> = (0..=255).collect();
    let mut i = 1;
    while i <= 256 {
        let mut e = Counting(Xoshiro256StarStar::from_int(i as i64), 0);
        let out = rz::get_bytes_from_string(&mut e, &all[..i], 20000).unwrap();
        assert_eq!(e.1, 2500);
        let mut seen = [false; 256];
        out.iter().for_each(|&b| seen[b as usize] = true);
        assert_eq!(seen.iter().filter(|&&s| s).count(), i);
        i *= 2;
    }
    let mut both = all.clone();
    both.extend_from_slice(&all);
    let mut e = Counting(Xoshiro256StarStar::from_int(5), 0);
    rz::get_bytes_from_string(&mut e, &both, 20000).unwrap();
    assert_eq!(e.1, 20000);

    // test methods/getBytesFromString_error.phpt
    let mut e = Secure;
    assert_eq!(
        rz::get_bytes_from_string(&mut e, b"", 2).unwrap_err(),
        RandomError::ValueError(
            "Random\\Randomizer::getBytesFromString(): Argument #1 ($string) must not be empty"
                .into()
        )
    );
    assert_eq!(
        rz::get_bytes_from_string(&mut e, b"abc", 0).unwrap_err(),
        RandomError::ValueError(
            "Random\\Randomizer::getBytesFromString(): Argument #2 ($length) must be greater than 0".into()
        )
    );
}

// Tests getInt_expansion_32.phpt, getInt_expansion_64.phpt,
// getBytes_expansion.phpt, user_reference_return.phpt; observed on PHP 8.5
// for the short and long results of nextInt() and nextFloat().
#[test]
fn result_sizes() {
    let bytes = ["01", "02", "03", "04", "05", "06", "07", "08"];
    let mut e = script(&bytes);
    let v = rz::get_int(&mut e, 0, 0x00FF_FFFF).unwrap();
    assert_eq!(hex(&(v as u32).to_le_bytes()), "01020300");
    let mut e = script(&bytes);
    let v = rz::get_int(&mut e, 0, 0x00FF_FFFF_FFFF_FFFF).unwrap();
    assert_eq!(hex(&v.to_le_bytes()), "0102030405060700");

    let mut e = Script {
        vals: [
            "H",
            "e",
            "ll",
            "o",
            "abcdefghijklmnopqrstuvwxyz",
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "success",
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect(),
        fallback: vec![],
        calls: 0,
    };
    assert_eq!(rz::get_bytes(&mut e, 5).unwrap(), b"Hello");
    assert_eq!(rz::get_bytes(&mut e, 11).unwrap(), b"abcdefghABC");
    assert_eq!(rz::get_bytes(&mut e, 7).unwrap(), b"success");
    assert_eq!(
        rz::get_bytes(&mut e, 1).unwrap_err(),
        RandomError::BrokenEngine(MSG_EMPTY_STRING)
    );

    let mut e = Constant(b"abcdef".to_vec());
    assert_eq!(
        rz::get_bytes(&mut e, 64).unwrap(),
        b"abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd"
    );

    assert_eq!(rz::next_int(&mut script(&["010203"])).unwrap(), 98560);
    assert_eq!(
        rz::next_int(&mut script(&["0102030405060708090a0b0c"])).unwrap(),
        289218847876153600
    );
    assert_eq!(
        rz::next_int(&mut script(&["ffffffffffffffff"])).unwrap(),
        i64::MAX
    );
    let mut e = script(&["010203", "040506", "070809"]);
    assert_eq!(
        rz::next_float(&mut e).unwrap().to_bits(),
        0x3fa00e0c0a080600
    );
    assert_eq!(e.calls, 3);
    assert_eq!(
        rz::get_int(&mut script(&["0102030405", "060708090a"]), 0, i64::MAX).unwrap(),
        578437695752307201
    );
    assert_eq!(
        rz::get_int(&mut script(&["0102030405060708"]), 0, 1000000).unwrap(),
        305918
    );
    assert_eq!(
        hex(&rz::get_bytes(&mut script(&["0102030405060708090a0b0c", "0d0e0f"]), 12).unwrap()),
        "01020304050607080d0e0fff"
    );
    let mut s = *b"abc";
    rz::shuffle_bytes(&mut script(&["01000000", "00000000"]), &mut s).unwrap();
    assert_eq!(&s, b"cab");
}

// Tests 03_randomizer/engine_unsafe_biased.phpt, engine_unsafe_nul.phpt,
// engine_unsafe_empty_string.phpt.
#[test]
fn unsafe_engines() {
    let size = 8;
    let biased = || Constant(vec![0xff; size]);
    assert_eq!(rz::get_int(&mut biased(), 0, 1234), Err(broken()));
    assert!(rz::next_int(&mut biased()).is_ok());
    assert_eq!(hex(&rz::get_bytes(&mut biased(), 1).unwrap()), "ff");
    let mut v: Vec<i32> = (1..=1234).collect();
    assert_eq!(rz::shuffle_array(&mut biased(), &mut v), Err(broken()));
    assert_eq!(rz::pick_array_keys(&mut biased(), 1234, 1), Err(broken()));
    assert_eq!(rz::pick_array_keys(&mut biased(), 1234, 10), Err(broken()));
    assert_eq!(
        rz::shuffle_bytes(&mut biased(), &mut b"foobar".to_vec()),
        Err(broken())
    );
    assert_eq!(
        rz::get_bytes_from_string(&mut biased(), b"123", 10),
        Err(broken())
    );
    assert_eq!(
        rz::get_bytes_from_string(&mut biased(), &[b'a'; 500], 10),
        Err(broken())
    );

    let nul = || Constant(vec![0; size]);
    assert_eq!(rz::get_int(&mut nul(), 0, 1234).unwrap(), 0);
    assert_eq!(rz::next_int(&mut nul()).unwrap(), 0);
    assert_eq!(hex(&rz::get_bytes(&mut nul(), 1).unwrap()), "00");
    let mut v: Vec<i32> = (1..=123).collect();
    rz::shuffle_array(&mut nul(), &mut v).unwrap();
    let mut want: Vec<i32> = (2..=123).collect();
    want.push(1);
    assert_eq!(v, want);
    assert_eq!(rz::pick_array_keys(&mut nul(), 123, 1).unwrap(), [0]);
    assert_eq!(rz::pick_array_keys(&mut nul(), 123, 10), Err(broken()));
    let mut s = b"foobar".to_vec();
    rz::shuffle_bytes(&mut nul(), &mut s).unwrap();
    assert_eq!(s, b"oobarf");
    assert_eq!(
        rz::get_bytes_from_string(&mut nul(), b"123", 10).unwrap(),
        b"1111111111"
    );
    assert_eq!(
        rz::get_bytes_from_string(&mut nul(), &[b'a'; 500], 10).unwrap(),
        b"aaaaaaaaaa"
    );

    let empty = || Constant(vec![]);
    let e = RandomError::BrokenEngine(MSG_EMPTY_STRING);
    assert_eq!(rz::get_int(&mut empty(), 0, 1234).unwrap_err(), e);
    assert_eq!(rz::next_int(&mut empty()).unwrap_err(), e);
    assert_eq!(rz::get_bytes(&mut empty(), 1).unwrap_err(), e);
    assert_eq!(rz::pick_array_keys(&mut empty(), 1234, 10).unwrap_err(), e);
    assert_eq!(
        rz::get_bytes_from_string(&mut empty(), b"123", 10).unwrap_err(),
        e
    );
}

// Tests 03_randomizer/compatibility_mt_rand.phpt, compatibility_array_rand.phpt,
// methods/getInt_gh9415.phpt.
#[test]
fn mt_compatibility() {
    for mode in [MtMode::Php, MtMode::Mt19937] {
        let mut r = Mt19937::new(1234, mode);
        let mut l = LegacyRandom::new();
        l.mt_srand(Some(1234), mode.to_int());
        for _ in 0..10_000 {
            assert_eq!(rz::next_int(&mut r).unwrap(), l.mt_rand());
        }
        for i in 0..10_000 {
            assert_eq!(
                rz::get_int(&mut r, 0, i).unwrap(),
                l.mt_rand_range(0, i).unwrap()
            );
        }
    }
    let mut l = LegacyRandom::new();
    l.mt_srand(Some(1234), 0);
    let mut r = Mt19937::new(1234, MtMode::Mt19937);
    for (n, num) in [(3, 1), (3, 2), (10, 1), (10, 2)] {
        assert_eq!(
            l.array_rand(&n, num).unwrap(),
            rz::pick_array_keys(&mut r, n, num).unwrap()
        );
    }
    let mut r = Mt19937::new(1234, MtMode::Mt19937);
    assert_eq!(
        rz::get_int(&mut r, -2147483648, 2147483647).unwrap(),
        -1324913873
    );
    let mut r = Mt19937::new(4321, MtMode::Mt19937);
    assert_eq!(
        rz::get_int(&mut r, -2147483648, 2147483647).unwrap(),
        -1843387587
    );
}

// Observed on PHP 8.5: array_rand() with one key draws storage slots while
// at most half of them are deleted, else an ordinal position.
#[test]
fn array_rand_holes() {
    /// Slots `0..slots`, live where `live(i)`.
    struct Holes(Vec<bool>);
    impl PickSource for Holes {
        fn count(&self) -> usize {
            self.0.iter().filter(|&&l| l).count()
        }
        fn slots(&self) -> usize {
            self.0.len()
        }
        fn slot_live(&self, i: usize) -> bool {
            self.0[i]
        }
        fn slot_to_index(&self, i: usize) -> usize {
            self.0[..i].iter().filter(|&&l| l).count()
        }
    }
    let pick = |live: Vec<bool>, num: i64| -> (Vec<usize>, i64) {
        let h = Holes(live);
        let mut l = LegacyRandom::new();
        l.mt_srand(Some(42), 0);
        let r = l.array_rand(&h, num).unwrap();
        (r, l.mt_rand())
    };
    // keys 1, 3, 5, 7, 9 of range(0, 9): key 7 is ordinal 3
    let odd: Vec<bool> = (0..10).map(|i| i % 2 == 1).collect();
    assert_eq!(pick(odd.clone(), 1), (vec![3], 2041643438));
    assert_eq!(pick(odd, 3), (vec![0, 3, 4], 393923207));
    // range(0, 9) without key 0: key 2 (ordinal 1)
    assert_eq!(
        pick((0..10).map(|i| i != 0).collect(), 1),
        (vec![1], 1710563033)
    );
    // range(0, 99) without keys 0..89: key 92 (ordinal 2)
    assert_eq!(
        pick((0..100).map(|i| i >= 90).collect(), 1),
        (vec![2], 1710563033)
    );
    assert_eq!(
        pick((0..100).map(|i| i >= 90).collect(), 2),
        (vec![2, 7], 2041643438)
    );
    // range(0, 9) without keys 0..h-1: key 7 for h=3..5 (two draws), key 8 for h=6
    for (h, key, draws_next) in [(3, 7, 2041643438), (5, 7, 2041643438), (6, 8, 1710563033)] {
        let (r, next) = pick((0..10).map(|i| i >= h).collect(), 1);
        assert_eq!(r, vec![key - h], "{h}");
        assert_eq!(next, draws_next, "{h}");
    }

    let mut l = LegacyRandom::new();
    assert_eq!(
        l.array_rand(&0usize, 1).unwrap_err(),
        RandomError::ValueError("array_rand(): Argument #1 ($array) must not be empty".into())
    );
    let msg = "array_rand(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)";
    for num in [0, 6, -1] {
        assert_eq!(
            l.array_rand(&5usize, num).unwrap_err(),
            RandomError::ValueError(msg.into())
        );
    }
    // test methods/pickArrayKeys_error.phpt
    let mut e = Secure;
    assert_eq!(
        rz::pick_keys(&mut e, PickCaller::PickArrayKeys, &0usize, 0).unwrap_err(),
        RandomError::ValueError(
            "Random\\Randomizer::pickArrayKeys(): Argument #1 ($array) must not be empty".into()
        )
    );
    for num in [0, -1, 10] {
        assert_eq!(
            rz::pick_array_keys(&mut e, 3, num).unwrap_err(),
            RandomError::ValueError(
                "Random\\Randomizer::pickArrayKeys(): Argument #2 ($num) must be between 1 and the number of elements in argument #1 ($array)".into()
            )
        );
    }
}

// Tests methods/getFloat_equal_steps.phpt, getFloat_extreme_range.phpt,
// getFloat_opposite_signs.phpt, getFloat_error.phpt.
#[test]
fn get_float_tests() {
    struct Counting64(u64);
    impl Engine for Counting64 {
        fn generate(&mut self) -> Result<Generated, RandomError> {
            self.0 += 1;
            Ok(Generated::u64(self.0 - 1))
        }
    }
    let mut e = Counting64(0);
    let (a, b) = (1.99999999999999, 2.00000000000001);
    let v: Vec<f64> = (0..48)
        .map(|_| rz::get_float(&mut e, a, b, B::ClosedClosed).unwrap())
        .collect();
    assert_eq!(format!("{:.17}", v[0]), "2.00000000000001021");
    assert_eq!(format!("{:.17}", v[1]), "2.00000000000000977");
    assert_eq!(format!("{:.17}", v[46]), "1.99999999999999001");
    assert_eq!(v[47], v[0]);

    let mut e = script(&["64e858793ef63800", "65e858793ef63800", "0000000000000000"]);
    let got: Vec<String> = (0..3)
        .map(|_| {
            format!(
                "{:e}",
                rz::get_float(&mut e, -1.6e308, 1.6e308, B::ClosedOpen).unwrap()
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            "-1.5999999999999998e308",
            "-1.6e308",
            "1.5999999999999998e308"
        ]
    );

    let mut e = script(&["63e858793ef63800", "64e858793ef63800", "65e858793ef63800"]);
    let got: Vec<String> = (0..3)
        .map(|_| {
            format!(
                "{:.17}",
                rz::get_float(&mut e, -1.0, 1.0, B::ClosedOpen).unwrap()
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            "-0.78005908680576086",
            "-0.78005908680576097",
            "-0.78005908680576108"
        ]
    );

    let mut e = Secure;
    let err = |r: Result<f64, RandomError>| match r {
        Err(RandomError::ValueError(m)) => m,
        other => panic!("{other:?}"),
    };
    let fin1 = "Random\\Randomizer::getFloat(): Argument #1 ($min) must be finite";
    let fin2 = "Random\\Randomizer::getFloat(): Argument #2 ($max) must be finite";
    for b in [B::ClosedClosed, B::ClosedOpen, B::OpenClosed, B::OpenOpen] {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(err(rz::get_float(&mut e, x, 0.0, b)), fin1);
            assert_eq!(err(rz::get_float(&mut e, 0.0, x, b)), fin2);
        }
    }
    assert_eq!(
        err(rz::get_float(&mut e, 0.0, -0.1, B::ClosedClosed)),
        "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than or equal to argument #1 ($min)"
    );
    assert_eq!(
        rz::get_float(&mut e, 0.0, 0.0, B::ClosedClosed).unwrap(),
        0.0
    );
    assert_eq!(
        rz::get_float(&mut e, 1e17, 1e17 + 0.1, B::ClosedClosed).unwrap(),
        1e17
    );
    let eps = f64::EPSILON;
    assert_eq!(
        rz::get_float(&mut e, 1.0, 1.0 + eps, B::ClosedOpen).unwrap(),
        1.0
    );
    assert_eq!(
        rz::get_float(&mut e, 1.0, 1.0 + eps, B::OpenClosed).unwrap(),
        1.0 + eps
    );
    assert_eq!(
        err(rz::get_float(&mut e, 1.0, 1.0 + eps, B::OpenOpen)),
        "The given interval is empty, there are no floats between argument #1 ($min) and argument #2 ($max)"
    );
    for b in [B::ClosedOpen, B::OpenClosed, B::OpenOpen] {
        for (x, y) in [(0.0, -0.1), (0.0, 0.0), (1e17, 1e17 + 0.1)] {
            assert_eq!(
                err(rz::get_float(&mut e, x, y, b)),
                "Random\\Randomizer::getFloat(): Argument #2 ($max) must be greater than argument #1 ($min)"
            );
        }
    }
}

#[test]
fn float_edges_fixture() {
    let data = include_str!("fixtures/float_edges.txt");
    let signed = |s: &str| -> f64 {
        match s.strip_prefix('-') {
            Some(r) => -f64::from_bits(r.parse().unwrap()),
            None => f64::from_bits(s.parse().unwrap()),
        }
    };
    let bits = |f: f64| -> String {
        let b = f.to_bits();
        if b >> 63 == 1 {
            format!("-{}", b & (u64::MAX >> 1))
        } else {
            b.to_string()
        }
    };
    let bound = |s: &str| match s {
        "CC" => B::ClosedClosed,
        "CO" => B::ClosedOpen,
        "OC" => B::OpenClosed,
        _ => B::OpenOpen,
    };
    let mut n = 0;
    for line in data.lines() {
        let (head, want) = line.split_once(": ").unwrap();
        let h: Vec<&str> = head.split(' ').collect();
        let got = if let Some(k) = h[0].strip_prefix("k=") {
            let k: u64 = k.parse().unwrap();
            let mut e = Constant(k.to_le_bytes().to_vec());
            if h.len() == 2 {
                let f = rz::get_float(&mut e, -1.0, 1.0, bound(h[1])).unwrap();
                format!("{} {:016x}", php_g17(f), f.to_bits())
            } else {
                let (a, b) = if h[1] == "[-2,1]" {
                    (-2.0, 1.0)
                } else {
                    (-1.0, 2.0)
                };
                format!(
                    "{:016x}",
                    rz::get_float(&mut e, a, b, B::ClosedClosed)
                        .unwrap()
                        .to_bits()
                )
            }
        } else {
            let (a, b) = (signed(h[0]), signed(h[1]));
            (0..6u64)
                .map(|k| {
                    let mut e = Constant(k.to_le_bytes().to_vec());
                    match rz::get_float(&mut e, a, b, bound(h[2])) {
                        Ok(f) => bits(f),
                        Err(_) => "E".into(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(got, want, "{head}");
        n += 1;
    }
    assert_eq!(n, 72);
}

/// `printf("%.17g")` for the few values in float_edges.txt.
fn php_g17(f: f64) -> String {
    if f == 0.0 {
        return "0".into();
    }
    if f == -1.0 {
        return "-1".into();
    }
    let s = format!("{:.16e}", f);
    let (m, e) = s.split_once('e').unwrap();
    let e: i32 = e.parse().unwrap();
    if (-5..17).contains(&e) {
        let digits = (16 - e).max(0) as usize;
        let t = format!("{:.*}", digits, f);
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        let m = m.trim_end_matches('0').trim_end_matches('.');
        format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
    }
}
