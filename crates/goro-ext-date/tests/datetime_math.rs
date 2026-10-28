//! The DateTime diff/days/add/sub tests of php-src's ext/date
//! (DateTime_{diff,days,add,sub}-*.phpt with their DateTime_data-*.inc),
//! ported: tools/date_corpus/extract_math_tests.py extracts the inputs and
//! the expected lines into tests/fixtures/phpt_datetime_math.txt, and this
//! test replays examine_diff.inc's logic.

use goro_ext_date::interval::{self, Interval};
use goro_ext_date::{DateTime, TimeZone, parse, resolve};

fn make(s: &str, zone: &TimeZone) -> DateTime {
    let (s, moved) = match s.split_once("@@") {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let p = parse::parse(s.as_bytes());
    assert!(p.errors.is_empty(), "{s}: {:?}", p.errors);
    let mut d = resolve::resolve(&p, 0, 0, zone, resolve::Mode::Create);
    if let Some(z) = moved {
        d.zone = TimeZone::from_id(z).unwrap();
    }
    d
}

fn fmt(d: &DateTime) -> String {
    String::from_utf8(d.format(b"Y-m-d H:i:s T")).unwrap()
}

#[test]
fn phpt_datetime_math() {
    let data = include_str!("fixtures/phpt_datetime_math.txt");
    let verbose = std::env::var("GORO_DATE_VERBOSE").is_ok();
    let (mut bad, mut total) = (0, 0);
    for line in data.lines() {
        let c: Vec<&str> = line.split('\t').collect();
        let (kind, tz, end_s, start_s, spec, _days, absolute, expected) =
            (c[0], c[1], c[2], c[3], c[4], c[5], c[6] == "true", c[7]);
        let zone = TimeZone::from_id(tz).unwrap();
        let start = make(start_s, &zone);
        let end = make(end_s, &zone);
        let name = &expected[..expected.find(": ").unwrap()];
        let mut want_iv = Interval::parse_iso(format!("P{}", &spec[2..]).as_bytes()).unwrap();
        want_iv.invert = &spec[1..2] == "-";
        let got = match kind {
            "diff" => {
                let iv = interval::diff(&start, &end, absolute);
                let spec = String::from_utf8(iv.format(b"P%R%yY%mM%dDT%hH%iM%sS")).unwrap();
                format!("{name}: DIFF: {} - {} = **{spec}**", fmt(&end), fmt(&start))
            }
            "days" => {
                let iv = interval::diff(&start, &end, absolute);
                let days = String::from_utf8(iv.format(b"%a")).unwrap();
                format!("{name}: DAYS: **{days}**")
            }
            "add" => {
                let r = interval::add(&start, &want_iv, 1);
                format!("{name}: ADD: {} + {spec} = **{}**", fmt(&start), fmt(&r))
            }
            _ => {
                let r = interval::add(&end, &want_iv, -1);
                format!("{name}: SUB: {} - {spec} = **{}**", fmt(&end), fmt(&r))
            }
        };
        total += 1;
        if got != expected {
            bad += 1;
            if verbose {
                println!("php: {expected}\ngot: {got}");
            }
        }
    }
    println!("datetime math: {bad} of {total} differ");
    assert_eq!(bad, 0);
}
