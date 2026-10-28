//! Differential check against the `strtotime` crate (the maintainer's
//! parser of PHP-style expressions) on the PHP 8.5 corpus, UTC base
//! 1623764756: where the crate agrees with PHP, goro must too, and the
//! crate's disagreements are listed for an upstream report.
//!
//! `GORO_DATE_VERBOSE=1 cargo test -p goro-ext-date --test strtotime_crate
//! -- --nocapture` prints them.

mod common;

use goro_ext_date::{TimeZone, resolve};

#[test]
fn strtotime_crate_comparison() {
    std::panic::set_hook(Box::new(|_| {}));
    let data = include_str!("fixtures/php85_resolve.tsv");
    let verbose = std::env::var("GORO_DATE_VERBOSE").is_ok();
    let utc = TimeZone::from_id("UTC").unwrap();
    let base = 1_623_764_756;
    let (mut total, mut crate_bad, mut crate_ok) = (0, 0, 0);
    for line in data.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        let input = common::unhex(cols[0]);
        let Ok(s) = std::str::from_utf8(&input) else {
            continue;
        };
        let php = cols[1];
        // the crate's arithmetic can overflow (a debug-build panic)
        let theirs =
            match std::panic::catch_unwind(|| strtotime::strtotime(s, base, strtotime::Tz::Utc)) {
                Ok(r) => r.map_or("F".to_string(), |v| v.to_string()),
                Err(_) => "PANIC".to_string(),
            };
        total += 1;
        if theirs == php {
            crate_ok += 1;
            let ours =
                resolve::strtotime(&input, base, &utc).map_or("F".to_string(), |v| v.to_string());
            // (names the reference build's database lacks fail there only)
            let legacy = php == "F" && goro_ext_date::parse::parse(&input).errors.is_empty();
            if !legacy {
                assert_eq!(ours, php, "{s:?}");
            }
        } else {
            crate_bad += 1;
            if verbose {
                println!("{s:?}: php {php} crate {theirs}");
            }
        }
    }
    println!("strtotime crate: {crate_ok} of {total} agree with PHP 8.5, {crate_bad} differ");
}
