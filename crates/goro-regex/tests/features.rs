//! Feature tests, mostly from the examples in pcre2pattern (expected
//! results checked against PHP 8.5).

use goro_regex::php::{self, GlobalMatcher};
use goro_regex::{Captures, ErrorKind, Limits, MatchError, MatchOptions, Regex, Scratch};

fn re(p: &str) -> Regex {
    match php::compile(p.as_bytes()) {
        Ok(r) => r,
        Err(e) => panic!("{p}: {}", e.message()),
    }
}

/// First match: group texts (None for unset), trailing unset groups dropped.
fn m(p: &str, s: &str) -> Option<Vec<Option<String>>> {
    let r = re(p);
    let mut mt = r.matcher();
    if !mt.exec(s.as_bytes(), 0).unwrap() {
        return None;
    }
    let c = mt.captures();
    Some(
        (0..c.highest_set())
            .map(|i| c.get(i).map(|(a, b)| s[a..b].to_string()))
            .collect(),
    )
}

/// The whole-match texts of a global match.
fn all(p: &str, s: &str) -> Vec<String> {
    let r = re(p);
    let mut g = GlobalMatcher::new(&r, s.as_bytes(), 0, Limits::default());
    let mut v = Vec::new();
    while g.next_match().unwrap() {
        let (a, b) = g.caps.get(0).unwrap();
        v.push(s[a..b].to_string());
    }
    v
}

fn g0(p: &str, s: &str) -> Option<String> {
    m(p, s).map(|v| v[0].clone().unwrap())
}

fn err(p: &str) -> (ErrorKind, usize) {
    match php::compile(p.as_bytes()) {
        Err(php::PhpCompileError::Compile(e)) => (e.kind, e.offset),
        _ => panic!("{p} compiled"),
    }
}

fn s(x: &str) -> Option<String> {
    Some(x.to_string())
}

#[test]
fn literals_and_escapes() {
    assert_eq!(
        g0("/The quick brown fox/", "xThe quick brown fox"),
        s("The quick brown fox")
    );
    assert_eq!(g0(r"/\x41\x{42}\103\o{104}\cE/", "ABCD\x05"), s("ABCD\x05"));
    assert_eq!(g0(r"/\Qabc$xyz\E/", "abc$xyz"), s("abc$xyz"));
    assert_eq!(g0(r"/\Qabc\$xyz\E/", "abc\\$xyz"), s("abc\\$xyz"));
    assert_eq!(g0(r"/\Qabc\E\$\Qxyz\E/", "abc$xyz"), s("abc$xyz"));
    assert_eq!(g0(r"/a\Q\E+/", "aa"), s("aa"));
    assert_eq!(g0(r"/a{1\Q\E}/", "a{1}"), s("a{1}"));
    // \40 is octal when there are fewer than 40 groups; \11 is a tab.
    assert_eq!(g0(r"/a\40b/", "a b"), s("a b"));
    assert_eq!(g0(r"/\11/", "\t"), s("\t"));
    assert_eq!(g0(r"/\0113/", "\t3"), s("\t3"));
    assert_eq!(g0(r"/[\8]/", "8"), s("8"));
}

#[test]
fn character_types_and_classes() {
    assert_eq!(all(r"/\d+/", "ab123cd45"), ["123", "45"]);
    assert_eq!(all(r"/\h+/u", "a \t\u{a0}b"), [" \t\u{a0}"]);
    assert_eq!(all(r"/\v/", "a\n\x0bb"), ["\n", "\x0b"]);
    assert_eq!(g0(r"/\N+/", "ab\ncd"), s("ab"));
    assert_eq!(g0(r"/[^\W_]+/", "_ab1_"), s("ab1"));
    assert_eq!(g0("/[W-c]+/i", "W[\\]^_`abcwD"), s("W[\\]^_`abcw"));
    assert_eq!(g0("/[b-d-z]+/", "z-cb"), s("z-cb"));
    assert_eq!(g0("/[]a]+/", "a]]a"), s("a]]a"));
    assert_eq!(g0("/[W-]46]/", "-46]"), s("-46]"));
    assert_eq!(g0(r"/[W-\]46]+/", "X]4"), s("X]4"));
    assert_eq!(g0("/[01[:alpha:]%]+/", "1a%b"), s("1a%b"));
    assert_eq!(g0("/[12[:^digit:]]+/", "12ab3"), s("12ab"));
    // [[:<:]] and [[:>:]] (start and end of word).
    assert_eq!(all("/[[:<:]]a/", "ba a"), ["a"]);
    assert_eq!(err("/[a[:<:]b]/"), (ErrorKind::UnknownPosixClass, 7));
}

#[test]
fn unicode() {
    assert_eq!(g0("/./u", "é"), s("é"));
    assert_eq!(g0(r"/\w+/u", "héllo"), s("héllo"));
    assert_eq!(g0(r"/(*UTF)\w+/", "héllo"), s("h"));
    assert_eq!(g0(r"/\p{Greek}+/u", "abcαβγ"), s("αβγ"));
    assert_eq!(g0(r"/\p{sc:Latin}+/u", "abc"), s("abc"));
    assert_eq!(g0(r"/\p{ Bi-di class = Al }/u", "\u{627}"), s("\u{627}"));
    assert_eq!(g0(r"/\P{ ^ Bi-di class = Al }/u", "\u{627}"), s("\u{627}"));
    assert_eq!(g0(r"/\pL+/u", "ab1"), s("ab"));
    assert_eq!(g0(r"/\p{Xwd}+/u", "a_b-"), s("a_b"));
    // Lu, Ll and Lt are Lc when caseless (10.45).
    assert_eq!(g0(r"/\p{Lu}/iu", "a"), s("a"));
    assert_eq!(g0("/K/iu", "\u{212a}"), s("\u{212a}"));
    assert_eq!(m("/(?r)k/iu", "\u{212a}"), None);
    assert_eq!(g0("/σ+/iu", "ΣςΣ"), s("ΣςΣ"));
    assert_eq!(g0(r"/^\X$/u", "e\u{301}"), s("e\u{301}"));
    assert_eq!(
        g0(r"/^\X$/u", "\u{1F1EB}\u{1F1F7}"),
        s("\u{1F1EB}\u{1F1F7}")
    );
    assert_eq!(m(r"/^\X$/u", "\u{915}\u{94D}\u{937}"), None);
    // Script runs: Latin with a Cyrillic "а" is not a run.
    assert_eq!(m(r"/^(*sr:\S+)$/u", "pаypal"), None);
    assert_eq!(
        g0(r"/^(*sr:.+)$/u", "日本語ひらがなカタカナ"),
        s("日本語ひらがなカタカナ")
    );
    assert_eq!(m(r"/^(*sr:.+)$/u", "한ㄅ"), None);
}

#[test]
fn anchors_and_newlines() {
    assert_eq!(g0("/^abc$/m", "def\nabc"), s("abc"));
    assert_eq!(m("/^abc$/", "def\nabc"), None);
    assert_eq!(g0("/a$/", "a\n"), s("a"));
    assert_eq!(m("/a$/D", "a\n"), None);
    assert_eq!(g0(r"/a\Z/", "a\n"), s("a"));
    assert_eq!(m(r"/a\z/", "a\n"), None);
    assert_eq!(g0("/(*CR)a.b/", "a\nb"), s("a\nb"));
    assert_eq!(all("/(*ANY)^./m", "a\x0bb"), ["a", "b"]);
    assert_eq!(g0(r"/\R/", "\r\n"), s("\r\n"));
    assert_eq!(m(r"/\R{2}/", "\r\n"), None);
    // CRLF bumpalong (pcre2api): .+A does not match \r\nA, [\r\n]A does.
    assert_eq!(m("/(*CRLF).+A/", "\r\nA"), None);
    assert_eq!(g0("/(*CRLF)[\\r\\n]A/", "\r\nA"), s("\nA"));
}

#[test]
fn quantifiers() {
    assert_eq!(g0("/z{2,4}/", "zzzzz"), s("zzzz"));
    assert_eq!(g0("/x{,4}/", "xxxxx"), s("xxxx"));
    assert_eq!(g0("/{,}/", "{,}"), s("{,}"));
    assert_eq!(
        g0(r"/\/\*.*\*\//", "/* first */ x /* second */"),
        s("/* first */ x /* second */")
    );
    assert_eq!(
        g0(r"/\/\*.*?\*\//", "/* first */ x /* second */"),
        s("/* first */")
    );
    assert_eq!(g0(r"/\d??\d/", "12"), s("1"));
    assert_eq!(g0("/(a?)*/", "aaa"), s("aaa"));
    assert_eq!(
        m("/(tweedle[dume]{3}\\s*)+/", "tweedledum tweedledee").unwrap()[1],
        s("tweedledee")
    );
    assert_eq!(
        m("/(a|(b))+/", "aba").unwrap(),
        vec![s("aba"), s("a"), s("b")]
    );
    assert_eq!(m(r"/\d+foo/", "123456bar"), None);
    assert_eq!(m(r"/(?>\d+)foo/", "123456bar"), None);
    assert_eq!(m(r"/\d++\d/", "123"), None);
    assert_eq!(g0("/(abc|xyz){2,3}+/", "abcxyzabc"), s("abcxyzabc"));
    assert_eq!(g0("/(?>.*?a)b/", "aab"), s("ab"));
}

#[test]
fn groups_and_references() {
    assert_eq!(
        m("/the ((red|white) (king|queen))/", "the red king").unwrap(),
        vec![s("the red king"), s("red king"), s("red"), s("king")]
    );
    assert_eq!(
        m("/(?|(Sat)ur|(Sun))day/", "Sunday").unwrap(),
        vec![s("Sunday"), s("Sun")]
    );
    assert_eq!(
        m(r"/(a)(?|x(y)z|(p(q)r)|(t)u(v))(z)/", "apqrz").unwrap(),
        vec![s("apqrz"), s("a"), s("pqr"), s("q"), s("z")]
    );
    assert_eq!(g0(r"/(?|(abc)|(def))\1/", "defdef"), s("defdef"));
    assert_eq!(g0("/(?|(abc)|(def))(?1)/", "defabc"), s("defabc"));
    assert_eq!(
        g0(r"/(sens|respons)e and \1ibility/", "sense and sensibility"),
        s("sense and sensibility")
    );
    assert_eq!(
        m(
            r"/(sens|respons)e and \1ibility/",
            "sense and responsibility"
        ),
        None
    );
    assert_eq!(g0(r"/((?i)rah)\s+\1/", "RAH RAH"), s("RAH RAH"));
    assert_eq!(m(r"/((?i)rah)\s+\1/", "RAH rah"), None);
    for p in [
        r"/(?<p1>(?i)rah)\s+\k<p1>/",
        r"/(?'p1'(?i)rah)\s+\k{p1}/",
        r"/(?P<p1>(?i)rah)\s+(?P=p1)/",
        r"/(?<p1>(?i)rah)\s+\g{p1}/",
    ] {
        assert_eq!(g0(p, "rah rah"), s("rah rah"), "{p}");
    }
    assert_eq!(
        g0(r"/(abc(def)ghi)\g{-1}/", "abcdefghidef"),
        s("abcdefghidef")
    );
    assert_eq!(m(r"/(a|(bc))\2/", "aa"), None);
    assert_eq!(g0(r"/^(a|b\1)+$/", "aba"), s("aba"));
    assert_eq!(
        all(
            r"/(?J)(?:(?<n>foo)|(?<n>bar))\k<n>/",
            "foofoo barbar foobar"
        ),
        ["foofoo", "barbar"]
    );
    assert_eq!(
        err("/(?<n>a)(?<n>b)/"),
        (ErrorKind::DuplicateSubpatternName, 12)
    );
    assert_eq!(
        err("/(?|(?<AA>aa)|(?<BB>bb))/"),
        (ErrorKind::SubpatternNamesMismatch, 19)
    );
}

#[test]
fn lookaround() {
    assert_eq!(g0(r"/\w+(?=;)/", "foo; bar"), s("foo"));
    assert_eq!(all("/foo(?!bar)/", "foobar foobaz"), ["foo"]);
    assert_eq!(all("/(?<!foo)bar/", "foobar xbar"), ["bar"]);
    assert_eq!(all(r"/(?<=\d{3})(?<!999)foo/", "123foo 999foo"), ["foo"]);
    assert_eq!(m(r"/(?<=\d{3})(?<!999)foo/", "123abcfoo"), None);
    assert_eq!(g0(r"/(?<=\d{3}...)(?<!999)foo/", "123abcfoo"), s("foo"));
    assert_eq!(g0("/(?<=(?<!foo)bar)baz/", "barbaz"), s("baz"));
    assert_eq!(g0("/(?<=colou?r)x/", "colourx"), s("x"));
    // Variable lookbehind branches try the longest length first.
    assert_eq!(m(r"/(?<=(\w{1,3}))x/", "abcx").unwrap()[1], s("abc"));
    assert_eq!(g0(r"/\b(\w)\w++(?<=\1)/", "level"), s("level"));
    assert_eq!(
        m(
            r"/^(?x)(*napla: .* \b(\w++)) (?> .*? \b\1\b ){2}/",
            "word1 word2 word3 word2 word3 word4"
        )
        .unwrap()[1],
        s("word3")
    );
    assert_eq!(
        g0(r"/\b(\w++)(*scs:(1).+rh)/", "the rhino shrhx"),
        s("shrhx")
    );
    assert_eq!(err("/(?<=a+)b/"), (ErrorKind::LookbehindNotFixedLength, 0));
    assert_eq!(err("/x(?<=a+)b/"), (ErrorKind::LookbehindNotFixedLength, 1));
    assert_eq!(
        err("/(?<=a{1,300})b/"),
        (ErrorKind::VariableLengthLookbehindTooLong, 0)
    );
}

#[test]
fn conditionals_recursion_subroutines() {
    assert_eq!(
        all(r"/( \( )? [^()]+ (?(1) \) )/x", "(abc) def"),
        ["(abc)", " def"]
    );
    assert_eq!(
        g0(r"/(?<OPEN> \( )? [^()]+ (?(<OPEN>) \) )/x", "(abc)"),
        s("(abc)")
    );
    assert_eq!(g0("/((?(R1)a+|(?1)b))/", "aab"), s("aab"));
    assert_eq!(
        g0(
            r"/(?(DEFINE) (?<byte> 2[0-4]\d | 25[0-5] | 1\d\d | [1-9]?\d) ) \b (?&byte) (\.(?&byte)){3} \b/x",
            "ip 192.168.23.245"
        ),
        s("192.168.23.245")
    );
    assert_eq!(
        g0(
            r"/(?(?=[^a-z]*[a-z]) \d{2}-[a-z]{3}-\d{2} | \d{2}-\d{2}-\d{2} )/x",
            "12-34-56"
        ),
        s("12-34-56")
    );
    assert_eq!(
        g0(r"/\( ( [^()]++ | (?R) )* \)/x", "x(a(b)c)y"),
        s("(a(b)c)")
    );
    assert_eq!(
        g0(r"/(?<pn> \( ( [^()]++ | (?&pn) )* \) )/x", "((a))"),
        s("((a))")
    );
    assert_eq!(m(r"/^((.)(?1)\2|.?)$/", "abcba").unwrap()[0], s("abcba"));
    assert_eq!(
        m(r"/^(.)(\1|a(?2))/", "bab").unwrap(),
        vec![s("bab"), s("b"), s("ab")]
    );
    assert_eq!(
        g0(
            "/(sens|respons)e and (?1)ibility/",
            "sense and responsibility"
        ),
        s("sense and responsibility")
    );
    assert_eq!(m("/(abc)(?i:(?-1))/", "abcABC"), None);
    assert_eq!(
        g0(r"/< (?: (?(R) \d++ | [^<>]*+) | (?R)) * >/x", "<ab<12>c>"),
        s("<ab<12>c>")
    );
    assert_eq!(
        g0(r"/(?<pn> \( ( (?>[^()]+) | \g<pn> )* \) )/x", "(x(y))"),
        s("(x(y))")
    );
    assert_eq!(
        err("/(?(?=a)a|b|c)/"),
        (ErrorKind::TooManyConditionBranches, 0)
    );
    assert_eq!(
        err("/(?(DEFINE)a|b)/"),
        (ErrorKind::DefineTooManyBranches, 3)
    );
}

#[test]
fn verbs() {
    assert_eq!(g0("/a+(*COMMIT)b/", "xxaab"), s("aab"));
    assert_eq!(m("/a+(*COMMIT)b/", "aacaab"), None);
    assert_eq!(g0("/(*COMMIT)abc/", "xyzabc"), s("abc"));
    assert_eq!(m("/(*NO_START_OPT)(*COMMIT)abc/", "xyzabc"), None);
    assert_eq!(g0("/a+(*SKIP)b|c/", "aaaacb"), s("c"));
    assert_eq!(
        m("/a(?>(*MARK:X))(*SKIP:X)(*F)|(.)/", "abc").unwrap()[1],
        s("a")
    );
    assert_eq!(
        m("/a(?:(*MARK:X))(*SKIP:X)(*F)|(.)/", "abc").unwrap()[1],
        s("b")
    );
    assert_eq!(
        m("/A((?:A|B(*ACCEPT)|C)D)/", "AB").unwrap(),
        vec![s("AB"), s("B")]
    );
    assert_eq!(m("/(A(*ACCEPT)??B)C/", "AB").unwrap(), vec![s("A"), s("A")]);
    assert_eq!(m("/(a(*COMMIT)b)+ac/", "abac"), None);
    assert_eq!(m(r"/^.*? (?(?=a) a | b(*THEN)c )/x", "ba"), None);
    let r = re("/X(*MARK:A)Y|X(*MARK:B)Z/");
    let mut mt = r.matcher();
    assert!(mt.exec(b"XZ", 0).unwrap());
    assert_eq!(mt.captures().mark(), Some(&b"B"[..]));
    assert_eq!(err("/(*MARK)/"), (ErrorKind::MarkMissingArgument, 6));
    assert_eq!(err("/(*FOO)/"), (ErrorKind::VerbUnknown, 5));
}

#[test]
fn keep_and_options() {
    assert_eq!(g0(r"/foo\Kbar/", "foobar"), s("bar"));
    assert_eq!(
        m(r"/(foo)\Kbar/", "foobar").unwrap(),
        vec![s("bar"), s("foo")]
    );
    assert_eq!(err(r"/(?=a\K)/"), (ErrorKind::BackslashKInLookaround, 7));
    assert_eq!(all("/(a(?i)b)c/", "abc aBc abC"), ["abc", "aBc"]);
    assert_eq!(all("/(a(?i)b|c)/", "C"), ["C"]);
    assert_eq!(
        all("/(?i:saturday|sunday)/", "SUNDAY Saturday"),
        ["SUNDAY", "Saturday"]
    );
    assert_eq!(g0("/(?xx)[a b]+/", "a b"), s("a"));
    assert_eq!(g0("/(?x)[a b]+/", "a b"), s("a b"));
    assert_eq!(g0(r"/abc #comment \n still comment/x", "abc"), s("abc"));
    assert_eq!(m("/(?n)(a)(?<x>b)/", "ab").unwrap().len(), 2);
    assert_eq!(g0("/a+/U", "aaa"), s("a"));
    assert_eq!(g0("/a+?/U", "aaa"), s("aaa"));
    assert_eq!(g0("/(?[ [a-c] - [b] ])+/", "abc"), s("a"));
    assert_eq!(all(r"/(?[ \d & [1-2] ])/", "123"), ["1", "2"]);
}

#[test]
fn errors_and_offsets() {
    // Observed on PHP 8.5 (ext/pcre/tests and spot checks).
    assert_eq!(err("/.++\\d*+[/"), (ErrorKind::MissingSquareBracket, 8));
    assert_eq!(err("/(?=xyz\\K)/"), (ErrorKind::BackslashKInLookaround, 9));
    assert_eq!(err("/\\y/"), (ErrorKind::UnknownEscape, 1));
    assert_eq!(err("/a)/"), (ErrorKind::UnmatchedClosingParenthesis, 1));
    assert_eq!(err("/a**/"), (ErrorKind::QuantifierInvalid, 2));
    assert_eq!(err("/a{2,1}/"), (ErrorKind::QuantifierOutOfOrder, 5));
    assert_eq!(err("/\\x{110000}/u"), (ErrorKind::CodePointTooBig, 9));
    assert_eq!(err("/[z-a]/"), (ErrorKind::ClassRangeOrder, 3));
    assert_eq!(err("/(?<1n>a)/"), (ErrorKind::InvalidSubpatternName, 3));
    assert_eq!(err("/\\p{Foo}/"), (ErrorKind::UnknownUnicodeProperty, 7));
    // UTF-8 errors in the pattern.
    match php::compile(b"/a\xc3/u") {
        Err(e) => assert_eq!(
            e.message(),
            "Compilation failed: UTF-8 error: 1 byte missing at end at offset 1"
        ),
        _ => panic!(),
    }
    // ext/pcre/tests/bug74183.phpt
    let alts = "[A-Z]|[a-z]|[0-9]| |Ñ|ñ|!|&quot;|%|&amp;|'|´|-|:|;|>|=|&lt;|@|_|,|\\{|\\}|`|~|á|é|í|ó|ú|Á|É|Í|Ó|Ú|ü|Ü";
    assert_eq!(
        err(&format!("/({alts}){{1,300}}/")).0,
        ErrorKind::PatternTooLarge
    );
}

/// The largest repetition counts that still compile, measured on PHP 8.5
/// (tools/regex_fixtures/big.php).
#[test]
fn pattern_too_large_thresholds() {
    let cases: &[(&str, usize)] = &[
        ("/(a){1,N}/", 3855),
        ("/(?:a){1,N}/", 4369),
        ("/(?:ab|cd){N}/", 3854),
        ("/(a|b|c){1,N}/", 2427),
        ("/([a-z]){N}/", 1598),
        ("/(?:\\d+\\.){N}/", 6552),
        ("/(?:abcdefgh){N}/", 2978),
        ("LIT", 32764),
        ("/(?:[a-z]|é){N}/u", 1456),
        ("/(?:\\p{L}){N}/u", 7281),
    ];
    let mk = |t: &str, n: usize| {
        if t == "LIT" {
            format!("/{}/", "a".repeat(n))
        } else {
            t.replace('N', &n.to_string())
        }
    };
    let mut bad = Vec::new();
    for &(t, n) in cases {
        let ok = php::compile(mk(t, n).as_bytes()).is_ok();
        let over = mk(t, n + 1);
        let len = over.len() - 2 - over.rfind('/').map_or(0, |i| over.len() - 1 - i);
        let e = match php::compile(over.as_bytes()) {
            Err(php::PhpCompileError::Compile(e)) => Some((e.kind, e.offset)),
            _ => None,
        };
        if !ok || e != Some((ErrorKind::PatternTooLarge, len)) {
            bad.push(format!(
                "{t}: {n} ok={ok}, {} -> {e:?} (pattern length {len})",
                n + 1
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn match_errors() {
    let r = re("/a/u");
    let mut sc = Scratch::new();
    let mut caps = Captures::default();
    let run = |s: &[u8], off: usize, sc: &mut Scratch, caps: &mut Captures| {
        r.exec_with(sc, s, off, MatchOptions::default(), Limits::default(), caps)
    };
    assert!(matches!(
        run(b"a\xff", 0, &mut sc, &mut caps),
        Err(MatchError::BadUtf8 { offset: 1, .. })
    ));
    assert_eq!(
        run("\u{30a2}".as_bytes(), 1, &mut sc, &mut caps),
        Err(MatchError::BadUtf8Offset)
    );
    assert_eq!(
        run(b"abc", 5, &mut sc, &mut caps),
        Err(MatchError::BadOffset)
    );
    // Only the part of the subject reachable from the offset is checked.
    let r2 = re("/(?<=a)b/u");
    assert_eq!(
        r2.exec_with(
            &mut sc,
            b"\xffab",
            2,
            MatchOptions::default(),
            Limits::default(),
            &mut caps
        ),
        Ok(true)
    );
    assert_eq!(re("/(?0)/").is_match(b"a"), Err(MatchError::RecurseLoop));
}
