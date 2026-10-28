//! Parses every .phpt of a php-src test checkout and compiles its
//! expectation. Run with
//! `GORO_PHPT_CORPUS=/path/to/php-tests cargo test -p goro-phpt --test corpus -- --ignored --nocapture`.

use goro_phpt::expect::{Kind, Matcher, normalize};
use goro_phpt::section::Phpt;

#[test]
#[ignore]
fn whole_corpus_parses() {
    let Some(root) = std::env::var_os("GORO_PHPT_CORPUS") else {
        eprintln!("GORO_PHPT_CORPUS not set; nothing to do");
        return;
    };
    let mut files = Vec::new();
    goro_phpt::collect(std::path::Path::new(&root), &mut files);
    let start = std::time::Instant::now();
    let mut bad = Vec::new();
    let mut compiled = 0;
    for f in &files {
        let data = std::fs::read(f).unwrap();
        let p = match Phpt::parse(&data) {
            Ok(p) => p,
            Err(e) => {
                bad.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let kind = match p.expect_section() {
            Some("EXPECTF") => Kind::Format,
            Some("EXPECTREGEX") => Kind::Regex,
            Some("EXPECT") => Kind::Exact,
            _ => continue,
        };
        let body = normalize(p.get(p.expect_section().unwrap()).unwrap());
        match Matcher::new(kind, &body) {
            Ok(_) => compiled += 1,
            Err(e) => bad.push(format!("{}: {e}", f.display())),
        }
    }
    eprintln!(
        "{} files, {compiled} expectations compiled, {} problems, {:.2}s",
        files.len(),
        bad.len(),
        start.elapsed().as_secs_f64()
    );
    for b in &bad {
        eprintln!("  {b}");
    }
}
