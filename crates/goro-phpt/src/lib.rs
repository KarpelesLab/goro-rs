//! Runner for PHP's .phpt test format.
//!
//! Written from the public documentation of the format (the "PHPT Test File
//! Layout" pages and the PHP Internals Book chapter on tests), the test
//! files used as data, and observation of a reference PHP binary.
//!
//! - [`section`]: the file layout parser.
//! - [`expect`]: EXPECT / EXPECTF / EXPECTREGEX matching.
//! - [`runner`]: running one test (SKIPIF, INI, ENV, ARGS, STDIN, CLEAN...).
//! - [`schedule`]: parallel workers honoring CONFLICTS.
//! - [`results`]: result files and the PASS-to-FAIL guard.
//! - [`cli`]: `goro --test` and `goro --test-compare`.

pub mod cli;
pub mod diff;
pub mod expect;
pub mod probe;
pub mod proc;
pub mod results;
pub mod runner;
pub mod schedule;
pub mod section;

use std::path::{Path, PathBuf};

/// Collects the .phpt files under `path` (or `path` itself), sorted.
pub fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    let mut found = Vec::new();
    walk(path, &mut found);
    found.sort();
    out.extend(found);
}

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.is_file() {
        if path.extension().is_some_and(|e| e == "phpt") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(t) if t.is_file() => {
                if p.extension().is_some_and(|e| e == "phpt") {
                    out.push(p);
                }
            }
            _ => {}
        }
    }
}
