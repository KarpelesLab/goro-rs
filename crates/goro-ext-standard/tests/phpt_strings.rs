//! Cases ported from php-src's ext/standard/tests/strings/*.phpt: inputs
//! and their expected outputs.

use goro_ext_standard::error::Diag;
use goro_ext_standard::natcmp::strnatcmp;
use goro_ext_standard::phonetic::{levenshtein, similar_text};
use goro_ext_standard::strings::*;

// strnatcmp_leftalign.phpt
#[test]
fn strnatcmp_leftalign() {
    assert_eq!(strnatcmp(b" 00", b" 0", false), 1);
    assert_eq!(strnatcmp(b" 0", b" 00", false), -1);
}

// similar_text_basic.phpt
#[test]
fn similar_text_basic() {
    assert_eq!(similar_text(b"abcdefgh", b"efg"), (3, 54.54545454545455));
    assert_eq!(similar_text(b"abcdefgh", b"mno"), (0, 0.0));
    assert_eq!(similar_text(b"abcdefghcc", b"c"), (1, 18.181818181818183));
    assert_eq!(
        similar_text(b"abcdefghabcdef", b"zzzzabcdefggg"),
        (7, 51.851851851851855)
    );
}

// levenshtein.phpt, levenshtein_bug_16473.phpt, levenshtein_bug_6562.phpt,
// levenshtein_bug_7368.phpt
#[test]
fn levenshtein_tests() {
    let l = |a: &str, b: &str| levenshtein(a.as_bytes(), b.as_bytes(), 1, 1, 1);
    assert_eq!(l("12345", "12345"), 0);
    assert_eq!(l("", "xyz"), 3);
    assert_eq!(l("xyz", ""), 3);
    assert_eq!(l("", ""), 0);
    assert_eq!(levenshtein(b"", b"", 10, 10, 10), 0);
    assert_eq!(l("1", "2"), 1);
    assert_eq!(l("12", "21"), 2);
    assert_eq!(levenshtein(b"2121", b"11", 2, 1, 1), 2);
    assert_eq!(levenshtein(b"2121", b"11", 2, 1, 5), 10);
    assert_eq!(l("11", "2121"), 2);
    assert_eq!(levenshtein(b"11", b"2121", 5, 1, 1), 10);
    assert_eq!(levenshtein(b"111", b"121", 2, 3, 2), 3);
    assert_eq!(levenshtein(b"111", b"121", 2, 9, 2), 4);
    assert_eq!(l("a", "bc"), 2);
    assert_eq!(l("xa", "xbc"), 2);
    assert_eq!(l("xax", "xbcx"), 2);
    assert_eq!(l("ax", "bcx"), 2);
    assert_eq!(l("debugg", "debug"), 1);
    assert_eq!(l("ddebug", "debug"), 1);
    assert_eq!(l("debbbug", "debug"), 2);
    assert_eq!(l("debugging", "debuging"), 1);
    assert_eq!(l("13458", "12345"), 2);
    assert_eq!(l("1345", "1234"), 2);
}

// trim.phpt-style ranges and their warnings (trim_basic.phpt,
// trim1.phpt).
#[test]
fn trim_ranges() {
    let mut d = Vec::new();
    assert_eq!(
        trim(b"  ABC  ", Some(b" A..B"), TrimMode::Both, &mut d),
        b"C"
    );
    assert!(d.is_empty());
    trim(b"x", Some(b"a.."), TrimMode::Both, &mut d);
    assert_eq!(
        d,
        vec![Diag::Warning(
            "Invalid '..'-range, no character to the right of '..'".into()
        )]
    );
}

// str_word_count.phpt
#[test]
fn str_word_count_phpt() {
    let mut d = Vec::new();
    let s = b"Hello friend, you're\n    looking          good today!";
    assert_eq!(
        str_word_count(s, 1, None, &mut d).unwrap(),
        WordCount::Words(vec![
            b"Hello".to_vec(),
            b"friend".to_vec(),
            b"you're".to_vec(),
            b"looking".to_vec(),
            b"good".to_vec(),
            b"today".to_vec(),
        ])
    );
    assert_eq!(
        str_word_count(s, 0, None, &mut d).unwrap(),
        WordCount::Count(6)
    );
    for bad in [3, 123, -1, 999999999] {
        assert!(str_word_count(s, bad, None, &mut d).is_err());
    }
    let s2 = b"F0o B4r 1s bar foo";
    assert_eq!(
        str_word_count(s2, 0, Some(b"04"), &mut d).unwrap(),
        WordCount::Count(5)
    );
    assert_eq!(
        str_word_count(s2, 0, Some(b"01"), &mut d).unwrap(),
        WordCount::Count(6)
    );
    assert_eq!(
        str_word_count(s2, 0, Some(b"014"), &mut d).unwrap(),
        WordCount::Count(5)
    );
    assert_eq!(
        str_word_count(s2, 0, Some(b""), &mut d).unwrap(),
        WordCount::Count(7)
    );
    let p = |s: &[u8], cl: Option<&[u8]>| str_word_count(s, 2, cl, &mut Vec::new()).unwrap();
    assert_eq!(
        p(b"F0o B4r 1s bar foo", Some(b"01")),
        WordCount::Positions(vec![
            (0, b"F0o".to_vec()),
            (4, b"B".to_vec()),
            (6, b"r".to_vec()),
            (8, b"1s".to_vec()),
            (11, b"bar".to_vec()),
            (15, b"foo".to_vec()),
        ])
    );
    assert_eq!(
        p(b"foo'0 bar-0var", Some(b"0")),
        WordCount::Positions(vec![(0, b"foo'0".to_vec()), (6, b"bar-0var".to_vec())])
    );
    assert_eq!(
        p(b"'foo'", None),
        WordCount::Positions(vec![(1, b"foo'".to_vec())])
    );
    assert_eq!(
        p(b"'foo'", Some(b"'")),
        WordCount::Positions(vec![(0, b"'foo'".to_vec())])
    );
    assert_eq!(
        p(b"-foo-", None),
        WordCount::Positions(vec![(1, b"foo".to_vec())])
    );
    assert_eq!(
        p(b"-foo-", Some(b"-")),
        WordCount::Positions(vec![(0, b"-foo-".to_vec())])
    );
}

// wordwrap_basic.phpt
#[test]
fn wordwrap_basic() {
    let w = |s: &str, n, b: &str, c| {
        String::from_utf8(wordwrap(s.as_bytes(), n, b.as_bytes(), c).unwrap()).unwrap()
    };
    let s = "The quick brown foooooooooox jummmmmmmmmmmmped over the lazzzzzzzzzzzy doooooooooooooooooooooog.";
    assert_eq!(
        w(s, 75, "\n", false),
        "The quick brown foooooooooox jummmmmmmmmmmmped over the lazzzzzzzzzzzy\ndoooooooooooooooooooooog."
    );
    assert_eq!(
        w(
            "The quick brown fox jumped over the lazy dog.",
            20,
            "<br />\n",
            false
        ),
        "The quick brown fox<br />\njumped over the lazy<br />\ndog."
    );
    assert_eq!(
        w("A very long woooooooooooord.", 8, "\n", true),
        "A very\nlong\nwooooooo\nooooord."
    );
    assert_eq!(
        w("A very long woooooooooooord.", 8, "\n", false),
        "A very\nlong\nwoooooooooooord."
    );
}

// explode.phpt (limits)
#[test]
fn explode_limits() {
    let e = |s: &str, l| {
        explode(b" ", s.as_bytes(), l)
            .unwrap()
            .into_iter()
            .map(|p| String::from_utf8(p).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(e("1 2 3 4", 2), ["1", "2 3 4"]);
    assert_eq!(e("1 2 3 4", -1), ["1", "2", "3"]);
    assert_eq!(e("1 2 3 4", 0), ["1 2 3 4"]);
    assert_eq!(e("", -1), Vec::<String>::new());
    assert_eq!(e("", 1), [""]);
}
