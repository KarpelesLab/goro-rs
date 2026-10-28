//! strtotime() and modify() against a corpus run on PHP 8.5 (see
//! tools/date_corpus). Columns: hex(input); strtotime() with base
//! 1623764756 in UTC and with base 1582950615 in America/New_York; then
//! modify() on 2021-06-15 13:45:56.654321 Europe/Paris, 2020-02-29 23:30:15
//! America/New_York and 2021-01-31 00:00:00 UTC, each as "U.u T e" or
//! "E position message".

mod common;

use goro_ext_date::parse::{self, ParsedZone};
use goro_ext_date::{TimeZone, resolve};

fn corpus_path() -> String {
    std::env::var("GORO_DATE_RESOLVE").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/php85_resolve.tsv"
        )
        .to_string()
    })
}

fn fmt_modify(input: &[u8], ts: i64, us: u32, zone: &TimeZone) -> String {
    let p = parse::parse(input);
    if let Some((pos, msg)) = p.errors.first() {
        return format!("E {pos} {msg}");
    }
    let d = resolve::resolve(&p, ts, us, zone, resolve::Mode::Modify);
    String::from_utf8(d.format(b"U.u T e")).unwrap()
}

/// Names the reference build's system database lacks (backward links).
fn legacy_name(p: &parse::Parsed, php: &str) -> bool {
    php.contains("could not be found")
        && matches!(&p.zone, Some(ParsedZone::Zone(TimeZone::Id(z))) if z.zone.is_link())
}

/// Instants beyond what PHP's offset lookup handles (it reports LMT there).
fn extreme(p: &parse::Parsed) -> bool {
    p.y.is_some_and(|y| y.abs() > 100_000_000_000)
}

#[test]
fn resolve_corpus() {
    let Ok(data) = std::fs::read_to_string(corpus_path()) else {
        eprintln!("no corpus");
        return;
    };
    let verbose = std::env::var("GORO_DATE_VERBOSE").is_ok();
    let utc = TimeZone::from_id("UTC").unwrap();
    let ny = TimeZone::from_id("America/New_York").unwrap();
    let paris = TimeZone::from_id("Europe/Paris").unwrap();
    let bases = [
        (1_623_757_556i64, 654_321u32, paris),
        (1_583_037_015, 0, ny.clone()),
        (1_612_051_200, 0, utc.clone()),
    ];
    let (mut bad, mut total) = (0, 0);
    for line in data.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let input = common::unhex(cols[0]);
        let parsed = parse::parse(&input);
        if legacy_name(&parsed, cols[3]) || extreme(&parsed) {
            continue;
        }
        let mut fails = Vec::new();
        let t1 = resolve::strtotime(&input, 1_623_764_756, &utc)
            .map_or("F".to_string(), |v| v.to_string());
        if t1 != cols[1] {
            fails.push(format!("t1 php {} got {}", cols[1], t1));
        }
        let t2 = resolve::strtotime(&input, 1_582_950_615, &ny)
            .map_or("F".to_string(), |v| v.to_string());
        if t2 != cols[2] {
            fails.push(format!("t2 php {} got {}", cols[2], t2));
        }
        for (k, (ts, us, z)) in bases.iter().enumerate() {
            // the fixture has tabs and newlines turned into spaces
            let got = fmt_modify(&input, *ts, *us, z).replace(['\t', '\n'], " ");
            if got != cols[3 + k] {
                fails.push(format!("m{} php {} got {}", k + 1, cols[3 + k], got));
            }
        }
        total += 1;
        if !fails.is_empty() {
            bad += 1;
            if verbose {
                println!("{:?}", String::from_utf8_lossy(&input));
                for f in fails {
                    println!("   {f}");
                }
            }
        }
    }
    println!("resolve corpus: {bad} of {total} differ");
    assert_eq!(bad, 0);
}
