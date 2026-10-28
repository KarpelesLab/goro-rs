//! date() / DateTime::format() against PHP 8.5 output
//! (tests/fixtures/php85_format.php run once on php85 with UTC as the
//! default zone; its output is php85_format.txt).

use goro_ext_date::format::consts;
use goro_ext_date::{DateTime, TimeZone, parse, resolve, time};

fn json_str(s: &str) -> Vec<u8> {
    // the fixture's strings are JSON encoded ASCII
    let b = s
        .trim()
        .trim_start_matches('"')
        .trim_end_matches('"')
        .as_bytes();
    let mut out = Vec::new();
    let mut k = 0;
    while k < b.len() {
        if b[k] == b'\\' {
            match b[k + 1] {
                b'u' => {
                    let cp =
                        u32::from_str_radix(std::str::from_utf8(&b[k + 2..k + 6]).unwrap(), 16)
                            .unwrap();
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(
                        char::from_u32(cp).unwrap().encode_utf8(&mut buf).as_bytes(),
                    );
                    k += 6;
                }
                c => {
                    out.push(match c {
                        b'n' => b'\n',
                        b't' => b'\t',
                        other => other,
                    });
                    k += 2;
                }
            }
        } else {
            out.push(b[k]);
            k += 1;
        }
    }
    out
}

fn mk(s: &str, zone: &TimeZone) -> DateTime {
    let p = parse::parse(s.as_bytes());
    resolve::resolve(&p, 0, 0, zone, resolve::Mode::Create)
}

#[test]
fn php85_format() {
    let data = include_str!("fixtures/php85_format.txt");
    let utc = TimeZone::from_id("UTC").unwrap();
    let mut lines = data.lines().peekable();
    let mut checked = 0;
    let mut cur: Option<DateTime> = None;
    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix("  ") {
            // "  c=value" for the current date
            let (c, v) = rest.split_at(1);
            let got = cur.as_ref().unwrap().format(c.as_bytes());
            assert_eq!(
                String::from_utf8(got).unwrap(),
                &v[1..],
                "{c} for {:?}",
                cur.as_ref().map(|d| d.ts)
            );
            checked += 1;
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() == 7 && parts.iter().all(|p| p.parse::<i64>().is_ok()) {
            let n: Vec<i64> = parts.iter().map(|p| p.parse().unwrap()).collect();
            let d = mk("2000-01-01", &utc)
                .set_date(n[0], n[1], n[2])
                .set_time(n[3], n[4], n[5], n[6]);
            cur = Some(d);
            continue;
        }
        if let Some((head, vals)) = line.split_once(": ")
            && vals.contains('|')
            && head.contains(' ')
        {
            // "Zone date: e|T|O|P|p|Z|I|c|r|U|B"
            let (zone, date) = head.split_once(' ').unwrap();
            let z = TimeZone::parse(zone).unwrap();
            let d = mk(date, &z);
            let got = String::from_utf8(d.format(b"e|T|O|P|p|Z|I|c|r|U|B")).unwrap();
            assert_eq!(got, vals, "{head}");
            checked += 1;
            continue;
        }
        if let Some((f, want)) = line.split_once(" => ")
            && f.starts_with('"')
        {
            let d = mk("2021-07-04 13:05:09.123456", &utc);
            assert_eq!(d.format(&json_str(f)), json_str(want), "{f}");
            checked += 1;
            continue;
        }
        if let Some((ts, want)) = line.split_once(" => ")
            && let Ok(ts) = ts.parse::<i64>()
        {
            let got = time::date(b"c|r|D|N|z|W|o|Y|y|X|x|B", ts, &utc);
            assert_eq!(String::from_utf8(got).unwrap(), want, "{ts}");
            checked += 1;
            continue;
        }
    }
    assert!(checked > 1500, "{checked}");
    // the DATE_* constants (PHP manual: DateTimeInterface constants)
    let d = mk("2021-07-04 13:05:09.123456", &utc);
    let f = |c: &str| String::from_utf8(d.format(c.as_bytes())).unwrap();
    assert_eq!(f(consts::ATOM), "2021-07-04T13:05:09+00:00");
    assert_eq!(f(consts::COOKIE), "Sunday, 04-Jul-2021 13:05:09 UTC");
    assert_eq!(f(consts::ISO8601), "2021-07-04T13:05:09+0000");
    assert_eq!(f(consts::ISO8601_EXPANDED), "+2021-07-04T13:05:09+00:00");
    assert_eq!(f(consts::RFC822), "Sun, 04 Jul 21 13:05:09 +0000");
    assert_eq!(f(consts::RFC850), "Sunday, 04-Jul-21 13:05:09 UTC");
    assert_eq!(f(consts::RFC1036), "Sun, 04 Jul 21 13:05:09 +0000");
    assert_eq!(f(consts::RFC1123), "Sun, 04 Jul 2021 13:05:09 +0000");
    assert_eq!(f(consts::RFC7231), "Sun, 04 Jul 2021 13:05:09 GMT");
    assert_eq!(f(consts::RFC2822), "Sun, 04 Jul 2021 13:05:09 +0000");
    assert_eq!(f(consts::RFC3339), "2021-07-04T13:05:09+00:00");
    assert_eq!(f(consts::RFC3339_EXTENDED), "2021-07-04T13:05:09.123+00:00");
    assert_eq!(f(consts::RSS), "Sun, 04 Jul 2021 13:05:09 +0000");
    assert_eq!(f(consts::W3C), "2021-07-04T13:05:09+00:00");
    let ny = TimeZone::from_id("America/New_York").unwrap();
    let d = mk("2021-07-04 13:05:09.123456", &ny);
    assert_eq!(
        String::from_utf8(d.format(consts::RFC7231.as_bytes())).unwrap(),
        "Sun, 04 Jul 2021 13:05:09 GMT"
    );
    assert_eq!(
        String::from_utf8(d.format(consts::COOKIE.as_bytes())).unwrap(),
        "Sunday, 04-Jul-2021 13:05:09 EDT"
    );
}
