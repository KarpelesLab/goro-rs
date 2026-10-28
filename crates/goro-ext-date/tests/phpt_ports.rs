//! Hand ports of ext/date tests (inputs and expected output of the named
//! .phpt files), driving the library the way the PHP functions will.

use goro_ext_date::format::consts;
use goro_ext_date::{TimeZone, parse, resolve, time};

fn tz(name: &str) -> TimeZone {
    TimeZone::parse(name).unwrap()
}

fn date(fmt: &str, ts: i64, zone: &TimeZone) -> String {
    String::from_utf8(time::date(fmt.as_bytes(), ts, zone)).unwrap()
}

fn strtotime(s: &str, now: i64, zone: &TimeZone) -> Option<i64> {
    resolve::strtotime(s.as_bytes(), now, zone)
}

#[test]
fn strtotime_phpt() {
    // test strtotime.phpt
    let oslo = tz("Europe/Oslo");
    let got: Vec<String> = [
        "2005-07-14 22:30:41",
        "2005-07-14 22:30:41 GMT",
        "@1121373041",
        "@1121373041 CEST",
    ]
    .iter()
    .map(|s| date(consts::ISO8601, strtotime(s, 0, &oslo).unwrap(), &oslo))
    .collect();
    assert_eq!(
        got,
        [
            "2005-07-14T22:30:41+0200",
            "2005-07-15T00:30:41+0200",
            "2005-07-14T22:30:41+0200",
            "2005-07-14T22:30:41+0200"
        ]
    );
}

#[test]
fn strtotime2_phpt() {
    // test strtotime2.phpt: date(CONST) reads back with strtotime
    let oslo = tz("Europe/Oslo");
    for now in [
        1_793_160_715i64,
        1_150_494_719,
        0,
        1_616_893_199,
        1_635_642_000,
    ] {
        for c in [
            consts::ATOM,
            consts::COOKIE,
            consts::ISO8601,
            consts::ISO8601_EXPANDED,
            consts::RFC822,
            consts::RFC850,
            consts::RFC1036,
            consts::RFC1123,
            consts::RFC2822,
            consts::RFC3339,
            consts::RFC3339_EXTENDED,
            consts::RSS,
            consts::W3C,
        ] {
            let s = date(c, now, &oslo);
            assert_eq!(strtotime(&s, 12345, &oslo), Some(now), "{c}: {s}");
        }
    }
}

#[test]
fn strtotime3_64bit_phpt() {
    // test strtotime3-64bit.phpt
    let lisbon = tz("Europe/Lisbon");
    let time = 1_150_494_719;
    let cases: &[(&str, Option<&str>)] = &[
        ("", None),
        (" \t\r\n000", None),
        ("yesterday", Some("Thu, 15 Jun 2006 00:00:00 +0100")),
        ("22:49:12", Some("Fri, 16 Jun 2006 22:49:12 +0100")),
        ("22:49:12 bogusTZ", None),
        ("22.49.12.42GMT", Some("Fri, 16 Jun 2006 23:49:12 +0100")),
        ("22.49.12.42bogusTZ", None),
        ("t0222", Some("Fri, 16 Jun 2006 02:22:00 +0100")),
        ("t0222 t0222", Some("Sun, 16 Jun 0222 02:22:00 -0036")),
        ("022233", Some("Fri, 16 Jun 2006 02:22:33 +0100")),
        ("022233 bogusTZ", None),
        ("2-3-2004", Some("Tue, 02 Mar 2004 00:00:00 +0000")),
        ("2.3.2004", Some("Tue, 02 Mar 2004 00:00:00 +0000")),
        (
            "20060212T23:12:23UTC",
            Some("Sun, 12 Feb 2006 23:12:23 +0000"),
        ),
        ("20060212T23:12:23 bogusTZ", None),
        ("2006167", Some("Fri, 16 Jun 2006 00:00:00 +0100")),
        ("Jan-15-2006", Some("Sun, 15 Jan 2006 00:00:00 +0000")),
        ("2006-Jan-15", Some("Sun, 15 Jan 2006 00:00:00 +0000")),
        (
            "10/Oct/2000:13:55:36 +0100",
            Some("Tue, 10 Oct 2000 13:55:36 +0100"),
        ),
        ("10/Oct/2000:13:55:36 +00100", None),
        ("2006", Some("Fri, 16 Jun 2006 20:06:00 +0100")),
        ("1986", Some("Mon, 16 Jun 1986 22:51:59 +0100")),
        ("JAN", Some("Mon, 16 Jan 2006 00:00:00 +0000")),
        ("January", Some("Mon, 16 Jan 2006 00:00:00 +0000")),
    ];
    for (s, want) in cases {
        let got = strtotime(s, time, &lisbon).map(|t| date(consts::RFC2822, t, &lisbon));
        assert_eq!(got.as_deref(), *want, "{s:?}");
    }
}

#[test]
fn strtotime_relative_phpt() {
    // test strtotime-relative.phpt (first lines of each group)
    let utc = tz("UTC");
    let base = 1_204_200_000;
    for (s, want) in [
        ("+80412 seconds", "2008-02-29T10:20:12+0000"),
        ("-80412 seconds", "2008-02-27T13:39:48+0000"),
        ("+86399 seconds", "2008-02-29T11:59:59+0000"),
        ("-86401 seconds", "2008-02-27T11:59:59+0000"),
        ("+134 hours", "2008-03-05T02:00:00+0000"),
        ("-169 hours", "2008-02-21T11:00:00+0000"),
        ("+178 days", "2008-08-24T12:00:00+0000"),
        ("-184 days", "2007-08-28T12:00:00+0000"),
        ("+115 months", "2017-09-28T12:00:00+0000"),
        ("-128 months", "1997-06-28T12:00:00+0000"),
        ("+24 years", "2032-02-28T12:00:00+0000"),
        ("-26 years", "1982-02-28T12:00:00+0000"),
    ] {
        assert_eq!(
            date(consts::ISO8601, strtotime(s, base, &utc).unwrap(), &utc),
            want,
            "{s}"
        );
    }
}

#[test]
fn bug33578_phpt() {
    // test bug33578.phpt (the %d year is the base's)
    let utc = tz("UTC");
    let now = 1_623_764_756; // 2021
    for (s, want) in [
        ("Oct 11", "10/11/2021"),
        ("11 Oct", "10/11/2021"),
        ("11 Oct 2005", "10/11/2005"),
        ("Oct11", "10/11/2021"),
        ("11Oct", "10/11/2021"),
        ("11Oct 2005", "10/11/2005"),
        ("11Oct2005", "10/11/2005"),
    ] {
        assert_eq!(
            date("m/d/Y", strtotime(s, now, &utc).unwrap(), &utc),
            want,
            "{s}"
        );
    }
}

#[test]
fn bug41523_64bit_phpt() {
    // test bug41523-64bit.phpt
    let utc = tz("UTC");
    let p = parse::parse(b"0000-00-00 00:00:00").date_parse();
    assert_eq!(
        (
            p.year,
            p.month,
            p.day,
            p.hour,
            p.minute,
            p.second,
            p.fraction_us
        ),
        (
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0)
        )
    );
    assert_eq!(
        p.warnings,
        [(20, "The parsed date was invalid".to_string())]
    );
    assert!(p.errors.is_empty() && !p.is_localtime);
    assert_eq!(
        strtotime("0000-00-00 00:00:00", 0, &utc),
        Some(-62169984000)
    );
    let d = resolve::create(b"0000-00-00 00:00:00", 0, 0, &utc).unwrap();
    assert_eq!(d.dump_date(), "-0001-11-30 00:00:00.000000");
    assert_eq!(
        String::from_utf8(d.format(consts::ISO8601.as_bytes())).unwrap(),
        "-0001-11-30T00:00:00+0000"
    );
}

#[test]
fn bug26317_phpt() {
    // test bug26317.phpt (default zone GMT0): military zones
    let gmt0 = tz("GMT0");
    for s in [
        "2003-11-19 16:20:42 Z",
        "2003-11-19 09:20:42 T",
        "2003-11-19 19:20:42 C",
    ] {
        assert_eq!(
            date("Y-m-d H:i:s", strtotime(s, 0, &gmt0).unwrap(), &gmt0),
            "2003-11-19 16:20:42",
            "{s}"
        );
    }
}

#[test]
fn bug28088_phpt() {
    // test bug28088.phpt
    let utc = tz("UTC");
    assert_eq!(
        date(
            "m/d/y Hi",
            strtotime("04/04/04 2345", 0, &utc).unwrap(),
            &utc
        ),
        "04/04/04 2345"
    );
    assert_eq!(
        date(
            "m/d/y Hi",
            strtotime("04/04/04 0045", 0, &utc).unwrap(),
            &utc
        ),
        "04/04/04 0045"
    );
}

#[test]
fn date_diff1_phpt() {
    // test date_diff1.phpt (default zone Europe/London, EDT zones)
    let london = tz("Europe/London");
    let start = resolve::create(b"2010-10-04 02:18:48 EDT", 0, 0, &london).unwrap();
    let end = resolve::create(b"2010-11-06 18:38:28 EDT", 0, 0, &london).unwrap();
    assert_eq!(
        (
            start.dump_date().as_str(),
            start.zone.kind(),
            start.zone.name().as_str()
        ),
        ("2010-10-04 02:18:48.000000", 2, "EDT")
    );
    assert_eq!(end.dump_date(), "2010-11-06 18:38:28.000000");
    let i = goro_ext_date::interval::diff(&start, &end, false);
    assert_eq!(
        (i.y, i.m, i.d, i.i, i.s, i.us, i.invert, i.days),
        (0, 1, 2, 19, 40, 0, false, Some(33))
    );
}

#[test]
fn dateinterval_format_phpt() {
    // test DateInterval_format.phpt
    let utc = tz("UTC");
    let d1 = resolve::create(b"2000-01-01 00:00:00", 0, 0, &utc).unwrap();
    let d2 = resolve::create(b"2001-03-04 04:05:06", 0, 0, &utc).unwrap();
    let i = goro_ext_date::interval::diff(&d1, &d2, false);
    let f = |i: &goro_ext_date::interval::Interval, s: &str| {
        String::from_utf8(i.format(s.as_bytes())).unwrap()
    };
    let got: Vec<String> = [
        "Y=%Y", "M=%M", "D=%D", "H=%H", "I=%I", "S=%S", "R=%R", "y=%y", "m=%m", "d=%d", "h=%h",
        "i=%i", "s=%s", "r=%r",
    ]
    .iter()
    .map(|s| f(&i, s))
    .collect();
    assert_eq!(
        got,
        [
            "Y=01", "M=02", "D=03", "H=04", "I=05", "S=06", "R=+", "y=1", "m=2", "d=3", "h=4",
            "i=5", "s=6", "r="
        ]
    );
    let inv = goro_ext_date::interval::diff(&d2, &d1, false);
    assert_eq!(f(&inv, "inverted R=%R"), "inverted R=-");
    assert_eq!(f(&inv, "inverted r=%r"), "inverted r=-");
    assert_eq!(f(&inv, "%=%%"), "%=%");
    assert_eq!(f(&inv, "x=%x"), "x=%x");
}
