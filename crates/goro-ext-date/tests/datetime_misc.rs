//! The DateTime constructor's zone rules, abbreviation-zone arithmetic and
//! error messages (observed on PHP 8.5, default zone Europe/Paris).

use goro_ext_date::interval::{self, Interval};
use goro_ext_date::{DateTime, TimeZone, resolve};

fn z(name: &str) -> TimeZone {
    TimeZone::parse(name).unwrap()
}

fn show(d: &DateTime) -> String {
    String::from_utf8(d.format(b"c T e Z I U")).unwrap()
}

const NOW: i64 = 1_793_160_715;

#[test]
fn constructor_zones() {
    let paris = z("Europe/Paris");
    for (s, zone, want) in [
        (
            "2021-06-15 10:00",
            None,
            "2021-06-15T10:00:00+02:00 CEST Europe/Paris 7200 1 1623744000",
        ),
        (
            "2021-06-15 10:00",
            Some("Asia/Tokyo"),
            "2021-06-15T10:00:00+09:00 JST Asia/Tokyo 32400 0 1623718800",
        ),
        (
            "2021-06-15 10:00 UTC",
            Some("Asia/Tokyo"),
            "2021-06-15T10:00:00+00:00 UTC UTC 0 0 1623751200",
        ),
        (
            "2021-06-15 10:00 +02:00",
            Some("Asia/Tokyo"),
            "2021-06-15T10:00:00+02:00 GMT+0200 +02:00 7200 0 1623744000",
        ),
        (
            "2021-06-15 10:00 EDT",
            None,
            "2021-06-15T10:00:00-04:00 EDT EDT -14400 1 1623765600",
        ),
        (
            "@0",
            Some("Asia/Tokyo"),
            "1970-01-01T00:00:00+00:00 GMT+0000 +00:00 0 0 0",
        ),
        (
            "2021-06-15 10:00",
            Some("+05:00"),
            "2021-06-15T10:00:00+05:00 GMT+0500 +05:00 18000 0 1623733200",
        ),
        (
            "2021-06-15 10:00",
            Some("CEST"),
            "2021-06-15T10:00:00+02:00 CEST CEST 7200 1 1623744000",
        ),
    ] {
        let zone = zone.map(z).unwrap_or_else(|| paris.clone());
        let d = resolve::create(s.as_bytes(), NOW, 0, &zone).unwrap();
        assert_eq!(show(&d), want, "{s}");
    }
    let d = resolve::create(b"now", NOW, 0, &z("EST")).unwrap();
    assert_eq!(
        String::from_utf8(d.format(b"T e Z I")).unwrap(),
        "EST EST -18000 0"
    );
}

#[test]
fn abbreviation_zone_arithmetic() {
    let paris = z("Europe/Paris");
    let est = resolve::create(b"2021-01-15 10:00 EST", NOW, 0, &paris).unwrap();
    let f = |d: &DateTime| String::from_utf8(d.format(b"c T e I")).unwrap();
    for (m, want) in [
        ("+6 months", "2021-07-15T10:00:00-05:00 EST EST 0"),
        ("+1 day", "2021-01-16T10:00:00-05:00 EST EST 0"),
        ("next sunday", "2021-01-17T00:00:00-05:00 EST EST 0"),
        ("+1 hour", "2021-01-15T11:00:00-05:00 EST EST 0"),
    ] {
        let d = resolve::modify(m.as_bytes(), est.ts, est.us, &est.zone).unwrap();
        assert_eq!(f(&d), want, "{m}");
    }
    let p6m = Interval::parse_iso(b"P6M").unwrap();
    assert_eq!(
        f(&interval::add(&est, &p6m, 1)),
        "2021-07-15T10:00:00-05:00 EST EST 0"
    );
    let edt = resolve::create(b"2021-07-15 10:00 EDT", NOW, 0, &paris).unwrap();
    let d = resolve::modify(b"+6 months", edt.ts, edt.us, &edt.zone).unwrap();
    assert_eq!(f(&d), "2022-01-15T10:00:00-04:00 EDT EDT 1");
    assert_eq!(
        f(&edt.set_timezone(z("America/New_York"))),
        "2021-07-15T10:00:00-04:00 EDT America/New_York 1"
    );
    let ny = resolve::create(b"2021-01-15 10:00", NOW, 0, &z("America/New_York")).unwrap();
    assert_eq!(
        f(&ny.set_timezone(z("EDT"))),
        "2021-01-15T11:00:00-04:00 EDT EDT 1"
    );
    assert_eq!(z("EDT").offset_at(0), -14400);
    assert_eq!(z("edt").name(), "EDT");
    assert_eq!(z("+0530").name(), "+05:30");
}

#[test]
fn constructor_errors() {
    let paris = z("Europe/Paris");
    for (s, want) in [
        (
            &b"foo"[..],
            "Failed to parse time string (foo) at position 0 (f): The timezone could not be found in the database",
        ),
        (
            b"2021-13-45",
            "Failed to parse time string (2021-13-45) at position 6 (3): Unexpected character",
        ),
        (
            b"next foo",
            "Failed to parse time string (next foo) at position 0 (n): The timezone could not be found in the database",
        ),
        (
            b"10:00 Mars/Olympus",
            "Failed to parse time string (10:00 Mars/Olympus) at position 6 (M): The timezone could not be found in the database",
        ),
        (
            b"2021-01-01\0junk",
            "Failed to parse time string (2021-01-01) at position 11 (j): The timezone could not be found in the database",
        ),
    ] {
        assert_eq!(resolve::create(s, NOW, 0, &paris), Err(want.to_string()));
    }
    // a warning only: the date is accepted (test date_get_last_errors_basic)
    let p = goro_ext_date::parse::parse(b"2021-02-30");
    assert_eq!(
        p.warnings,
        [(11, "The parsed date was invalid".to_string())]
    );
    assert!(resolve::create(b"2021-02-30", NOW, 0, &paris).is_ok());
}

#[test]
fn overlap_reading_by_zone_source() {
    // observed on PHP 8.5 (default zone UTC): (string, zone, unix time with
    // the zone in the string, unix time with the zone as argument)
    let utc = z("UTC");
    for (s, zone, in_string, as_arg) in [
        ("2021-10-31 02:30", "Europe/Paris", 1635640200, 1635643800),
        ("2021-10-31 01:30", "Europe/London", 1635640200, 1635643800),
        (
            "2021-11-07 01:30",
            "America/New_York",
            1636263000,
            1636263000,
        ),
        (
            "2021-04-04 01:45",
            "Australia/Lord_Howe",
            1617461100,
            1617462900,
        ),
        (
            "2021-04-04 02:30",
            "Australia/Sydney",
            1617463800,
            1617467400,
        ),
        (
            "2018-02-17 23:30",
            "America/Sao_Paulo",
            1518917400,
            1518917400,
        ),
        ("2021-10-31 01:30", "Europe/Dublin", 1635643800, 1635643800),
        ("2021-03-28 02:30", "Europe/Paris", 1616895000, 1616895000),
        (
            "2021-11-07 02:00",
            "America/New_York",
            1636268400,
            1636268400,
        ),
    ] {
        let with = format!("{s} {zone}");
        assert_eq!(
            resolve::create(with.as_bytes(), NOW, 0, &utc).unwrap().ts,
            in_string,
            "{with}"
        );
        assert_eq!(
            resolve::strtotime(with.as_bytes(), NOW, &utc),
            Some(in_string),
            "{with}"
        );
        assert_eq!(
            resolve::create(s.as_bytes(), NOW, 0, &z(zone)).unwrap().ts,
            as_arg,
            "{s} in {zone}"
        );
        assert_eq!(
            resolve::strtotime(s.as_bytes(), NOW, &z(zone)),
            Some(as_arg),
            "{s} in {zone}"
        );
    }
}

#[test]
fn interval_dump_fields() {
    let paris = z("Europe/Paris");
    let a = resolve::create(b"2021-01-01", NOW, 0, &paris).unwrap();
    let b = resolve::create(b"2021-02-01 01:00", NOW, 0, &paris).unwrap();
    let d = interval::diff(&a, &b, false);
    assert_eq!(
        (d.y, d.m, d.d, d.h, d.i, d.s, d.us, d.invert, d.days),
        (0, 1, 0, 1, 0, 0, 0, false, Some(31))
    );
}
