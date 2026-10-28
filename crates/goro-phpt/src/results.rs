//! Result files and the guard comparison.
//!
//! A result file is plain text, one test per line, sorted by name so two
//! files diff cleanly with standard tools:
//!
//! ```text
//! # goro-phpt results v1
//! # php: /path/to/binary
//! PASS<TAB>Zend/tests/add_001.phpt
//! SKIP<TAB>ext/foo/tests/bar.phpt<TAB>extension foo is not available
//! ```
//!
//! The guard lists tests that passed in a baseline and don't in a new run.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::runner::{Status, TestResult};

pub const HEADER: &str = "# goro-phpt results v1";

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

pub fn format(php: &Path, results: &[TestResult]) -> String {
    let mut sorted: Vec<&TestResult> = results.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = String::new();
    let _ = writeln!(out, "{HEADER}");
    let _ = writeln!(out, "# php: {}", clean(&php.to_string_lossy()));
    for r in sorted {
        if r.reason.is_empty() || r.status == Status::Pass {
            let _ = writeln!(out, "{}\t{}", r.status.as_str(), clean(&r.name));
        } else {
            let _ = writeln!(
                out,
                "{}\t{}\t{}",
                r.status.as_str(),
                clean(&r.name),
                clean(&r.reason)
            );
        }
    }
    out
}

/// Parses a result file into test name -> status.
pub fn parse(text: &str) -> Result<BTreeMap<String, Status>, String> {
    let mut map = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let st = parts.next().unwrap_or_default();
        let name = parts
            .next()
            .ok_or_else(|| format!("line {}: no test name", n + 1))?;
        let st =
            Status::parse(st).ok_or_else(|| format!("line {}: unknown status {st:?}", n + 1))?;
        map.insert(name.to_string(), st);
    }
    Ok(map)
}

#[derive(Debug, Default)]
pub struct Comparison {
    /// PASS in the baseline, something else (or absent) now.
    pub regressions: Vec<(String, String)>,
    /// Not PASS in the baseline (or absent), PASS now.
    pub fixed: Vec<String>,
    pub base_pass: usize,
    pub new_pass: usize,
}

pub fn compare(base: &BTreeMap<String, Status>, new: &BTreeMap<String, Status>) -> Comparison {
    let mut c = Comparison {
        base_pass: base.values().filter(|s| **s == Status::Pass).count(),
        new_pass: new.values().filter(|s| **s == Status::Pass).count(),
        ..Default::default()
    };
    for (name, st) in base {
        if *st == Status::Pass {
            match new.get(name) {
                Some(Status::Pass) => {}
                Some(other) => c
                    .regressions
                    .push((name.clone(), other.as_str().to_string())),
                None => c.regressions.push((name.clone(), "MISSING".to_string())),
            }
        }
    }
    for (name, st) in new {
        if *st == Status::Pass && base.get(name) != Some(&Status::Pass) {
            c.fixed.push(name.clone());
        }
    }
    c
}

impl Comparison {
    pub fn report(&self, list_fixed: bool) -> String {
        let mut out = String::new();
        for (name, st) in &self.regressions {
            let _ = writeln!(out, "PASS -> {st}\t{name}");
        }
        if list_fixed {
            for name in &self.fixed {
                let _ = writeln!(out, "now PASS\t{name}");
            }
        }
        let _ = writeln!(
            out,
            "guard: {} regression(s), {} newly passing; passing {} -> {}",
            self.regressions.len(),
            self.fixed.len(),
            self.base_pass,
            self.new_pass
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn r(name: &str, status: Status, reason: &str) -> TestResult {
        TestResult {
            name: name.into(),
            title: String::new(),
            status,
            reason: reason.into(),
            detail: None,
            elapsed: Duration::ZERO,
        }
    }

    #[test]
    fn round_trip_and_compare() {
        let base = format(
            Path::new("/bin/php"),
            &[
                r("b.phpt", Status::Pass, ""),
                r("a.phpt", Status::Pass, ""),
                r("c.phpt", Status::Fail, "output\tdiffers"),
                r("d.phpt", Status::Pass, ""),
            ],
        );
        assert!(base.starts_with(HEADER));
        assert!(base.contains("PASS\ta.phpt\nPASS\tb.phpt\nFAIL\tc.phpt\toutput differs\n"));
        let new = format(
            Path::new("/bin/php"),
            &[
                r("a.phpt", Status::Fail, "x"),
                r("b.phpt", Status::Pass, ""),
                r("c.phpt", Status::Pass, ""),
            ],
        );
        let c = compare(&parse(&base).unwrap(), &parse(&new).unwrap());
        assert_eq!(
            c.regressions,
            vec![
                ("a.phpt".to_string(), "FAIL".to_string()),
                ("d.phpt".to_string(), "MISSING".to_string())
            ]
        );
        assert_eq!(c.fixed, vec!["c.phpt".to_string()]);
        assert_eq!((c.base_pass, c.new_pass), (3, 2));
        assert!(parse("BOGUS\tx").is_err());
    }
}
