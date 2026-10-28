//! date_parse() against a corpus run on PHP 8.5.
//!
//! The fixture lines are: hex(input), the canonical date_parse() result,
//! then strtotime()/modify() results (see tools/date_fixture_canon.py).

mod common;

use goro_ext_date::parse;

fn corpus_path() -> String {
    std::env::var("GORO_DATE_CORPUS").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/php85_date_parse.tsv"
        )
        .to_string()
    })
}

fn legacy_name_only(p: &parse::Parsed, php: &str) -> bool {
    use goro_ext_date::TimeZone;
    use goro_ext_date::parse::ParsedZone;
    php.contains("could not be found")
        && matches!(&p.zone, Some(ParsedZone::Zone(TimeZone::Id(z))) if z.zone.is_link())
}

#[test]
fn date_parse_corpus() {
    let Ok(data) = std::fs::read_to_string(corpus_path()) else {
        eprintln!("no corpus");
        return;
    };
    let mut bad = 0;
    let mut total = 0;
    let verbose = std::env::var("GORO_DATE_VERBOSE").is_ok();
    for line in data.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let input = common::unhex(cols[0]);
        let parsed = parse::parse(&input);
        let got = common::canon(&parsed.date_parse());
        total += 1;
        if got != cols[1] && legacy_name_only(&parsed, cols[1]) {
            // The reference build reads the system database, which lacks
            // the backward-compatibility names ("Asia/Calcutta", "GB").
            continue;
        }
        if got != cols[1] {
            bad += 1;
            if verbose {
                println!(
                    "{:?}\n   php: {}\n   got: {}",
                    String::from_utf8_lossy(&input),
                    cols[1],
                    got
                );
            }
        }
    }
    println!("date_parse corpus: {} of {} differ", bad, total);
    assert_eq!(bad, 0);
}
