//! `goro --test` and `goro --test-compare`.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::probe::Extensions;
use crate::results;
use crate::runner::{self, Config, Status, TestResult};
use crate::schedule;

pub const TEST_USAGE: &str = "\
usage: goro --test [options] <file-or-dir>...

Runs .phpt tests (directories recursively) and prints one line per test.

options:
  -v, --verbose       print an expected/actual diff for each failure
  -q, --quiet         print only FAIL and BORK lines (and the summary)
  -j N                parallel workers (default: half the CPUs, at most 8)
  --php PATH          PHP binary under test (default: this goro binary)
  --timeout SECS      per-run timeout (default 60)
  -d KEY=VALUE        extra ini setting for every run (after the defaults)
  --root DIR          test-suite root: working directory of the runs and
                      base of test names (default: the nearest parent of the
                      first path holding both Zend/ and ext/, else .)
  --copy-to DIR       copy the root to DIR (which must not exist), run the
                      tests there, then delete DIR, leaving the checkout as it
                      was; with --keep, DIR is kept
  --keep              keep generated NAME.php/.skip.php/.clean.php files and
                      write NAME.out/.exp/.diff for failures
  --results FILE      write a result file (one STATUS<TAB>test line per test)
  --guard BASELINE    compare with a baseline result file and exit 1 if a test
                      that passed there does not pass now

usage: goro --test-compare [--fixed] BASELINE NEW
  lists tests that went from PASS to anything else; exits 1 if any did
";

struct Options {
    verbose: bool,
    quiet: bool,
    jobs: usize,
    php: Option<PathBuf>,
    timeout: u64,
    ini: Vec<(String, String)>,
    root: Option<PathBuf>,
    copy_to: Option<PathBuf>,
    keep: bool,
    results: Option<PathBuf>,
    guard: Option<PathBuf>,
    paths: Vec<PathBuf>,
}

fn default_jobs() -> usize {
    let n = std::thread::available_parallelism().map_or(2, |n| n.get());
    (n / 2).clamp(1, 8)
}

fn parse_options(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options {
        verbose: false,
        quiet: false,
        jobs: default_jobs(),
        php: None,
        timeout: 60,
        ini: Vec::new(),
        root: None,
        copy_to: None,
        keep: false,
        results: None,
        guard: None,
        paths: Vec::new(),
    };
    let mut it = args.iter();
    let value = |it: &mut std::slice::Iter<'_, OsString>, opt: &str| {
        it.next()
            .cloned()
            .ok_or_else(|| format!("{opt} needs a value"))
    };
    while let Some(a) = it.next() {
        let s = a.to_string_lossy();
        match s.as_ref() {
            "-v" | "--verbose" => o.verbose = true,
            "-q" | "--quiet" => o.quiet = true,
            "--keep" => o.keep = true,
            "-h" | "--help" => return Err(String::new()),
            "-j" => {
                let v = value(&mut it, "-j")?;
                o.jobs = v
                    .to_string_lossy()
                    .parse()
                    .map_err(|_| "bad -j value".to_string())?;
            }
            "--php" => o.php = Some(value(&mut it, "--php")?.into()),
            "--timeout" => {
                let v = value(&mut it, "--timeout")?;
                o.timeout = v
                    .to_string_lossy()
                    .parse()
                    .map_err(|_| "bad --timeout value".to_string())?;
            }
            "-d" => {
                let v = value(&mut it, "-d")?.to_string_lossy().into_owned();
                let (k, val) = v.split_once('=').unwrap_or((&v, "1"));
                o.ini.push((k.to_string(), val.to_string()));
            }
            "--root" => o.root = Some(value(&mut it, "--root")?.into()),
            "--copy-to" => o.copy_to = Some(value(&mut it, "--copy-to")?.into()),
            "--results" => o.results = Some(value(&mut it, "--results")?.into()),
            "--guard" => o.guard = Some(value(&mut it, "--guard")?.into()),
            _ if s.starts_with("-j") && s.len() > 2 => {
                o.jobs = s[2..].parse().map_err(|_| "bad -j value".to_string())?;
            }
            _ if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option {s}")),
            _ => o.paths.push(a.into()),
        }
    }
    if o.paths.is_empty() {
        return Err("no tests given".into());
    }
    o.jobs = o.jobs.max(1);
    Ok(o)
}

fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(p)
        }
    })
}

/// Finds a program given without a slash in PATH, like a shell would.
fn find_program(p: &Path) -> PathBuf {
    if p.components().count() > 1 {
        return absolute(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let c = dir.join(p);
            if c.is_file() {
                return c;
            }
        }
    }
    absolute(p)
}

/// The nearest ancestor of `start` holding both `Zend` and `ext`: the layout
/// of a php-src checkout.
fn detect_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join("Zend").is_dir() && dir.join("ext").is_dir() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst = to.join(entry.file_name());
        if ty.is_dir() {
            copy_tree(&entry.path(), &dst)?;
        } else if ty.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(entry.path())?, &dst)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    // Keep the directory's own mode (some tests ship read-only fixtures).
    let perm = std::fs::metadata(from)?.permissions();
    std::fs::set_permissions(to, perm)?;
    Ok(())
}

/// Makes everything under `p` writable so it can be deleted.
fn remove_tree(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fn chmod_all(p: &Path) {
        if let Ok(meta) = std::fs::symlink_metadata(p)
            && meta.is_dir()
        {
            let mut perm = meta.permissions();
            perm.set_mode(perm.mode() | 0o700);
            let _ = std::fs::set_permissions(p, perm);
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    chmod_all(&e.path());
                }
            }
        }
    }
    chmod_all(p);
    let _ = std::fs::remove_dir_all(p);
}

struct Counts {
    by_status: [usize; 5],
}

fn idx(s: Status) -> usize {
    match s {
        Status::Pass => 0,
        Status::Fail => 1,
        Status::Skip => 2,
        Status::Xfail => 3,
        Status::Bork => 4,
    }
}

/// `goro --test ARGS...`: returns the process exit code.
pub fn test_main(args: &[OsString]) -> i32 {
    let o = match parse_options(args) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("goro --test: {e}");
            }
            eprint!("{TEST_USAGE}");
            return if e.is_empty() { 0 } else { 2 };
        }
    };
    let php = match &o.php {
        Some(p) => find_program(p),
        None => match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("goro --test: cannot find own binary: {e}");
                return 2;
            }
        },
    };
    let paths: Vec<PathBuf> = o.paths.iter().map(|p| absolute(p)).collect();
    for (p, orig) in paths.iter().zip(&o.paths) {
        if !p.exists() {
            eprintln!("goro --test: {} does not exist", orig.display());
            return 2;
        }
    }
    let mut root = match &o.root {
        Some(r) => absolute(r),
        None => detect_root(&paths[0]).unwrap_or_else(|| absolute(Path::new("."))),
    };
    let mut paths = paths;
    let mut copied: Option<PathBuf> = None;
    if let Some(dest) = &o.copy_to {
        let dest = if dest.is_absolute() {
            dest.clone()
        } else {
            std::env::current_dir().unwrap_or_default().join(dest)
        };
        if dest.exists() {
            eprintln!("goro --test: --copy-to {} already exists", dest.display());
            return 2;
        }
        if dest.starts_with(&root) {
            eprintln!(
                "goro --test: --copy-to {} is inside the root {}",
                dest.display(),
                root.display()
            );
            return 2;
        }
        let mut mapped = Vec::new();
        for p in &paths {
            match p.strip_prefix(&root) {
                Ok(rel) => mapped.push(dest.join(rel)),
                Err(_) => {
                    eprintln!(
                        "goro --test: {} is outside the root {}",
                        p.display(),
                        root.display()
                    );
                    return 2;
                }
            }
        }
        let t = Instant::now();
        if let Err(e) = copy_tree(&root, &dest) {
            eprintln!(
                "goro --test: copying {} to {}: {e}",
                root.display(),
                dest.display()
            );
            remove_tree(&dest);
            return 2;
        }
        if !o.quiet {
            eprintln!(
                "copied {} to {} in {:.1}s",
                root.display(),
                dest.display(),
                t.elapsed().as_secs_f64()
            );
        }
        paths = mapped;
        root = dest.clone();
        copied = Some(dest);
    }

    let code = run_and_report(&o, php, root, &paths);
    if let Some(dir) = copied
        && !o.keep
    {
        remove_tree(&dir);
    }
    code
}

fn run_and_report(o: &Options, php: PathBuf, root: PathBuf, paths: &[PathBuf]) -> i32 {
    let start = Instant::now();
    let mut files = Vec::new();
    for p in paths {
        crate::collect(p, &mut files);
    }
    files.dedup();
    let extensions = Extensions::probe(&php, &root);
    let cfg = Config {
        php: php.clone(),
        root,
        timeout: Duration::from_secs(o.timeout),
        extra_ini: o.ini.clone(),
        extensions,
        keep: o.keep,
        tmp: std::env::temp_dir(),
    };
    let jobs = schedule::jobs(&files);
    let all: Mutex<(Vec<TestResult>, Counts)> =
        Mutex::new((Vec::new(), Counts { by_status: [0; 5] }));
    let total = jobs.len();
    schedule::run_all(&jobs, o.jobs, |job| {
        let rs = runner::run_test(&cfg, &job.path);
        let mut g = all.lock().unwrap();
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        for r in rs {
            g.1.by_status[idx(r.status)] += 1;
            let show = !o.quiet || matches!(r.status, Status::Fail | Status::Bork);
            if show {
                let reason = if r.reason.is_empty() || r.status == Status::Pass {
                    String::new()
                } else {
                    format!(" ({})", r.reason)
                };
                let _ = writeln!(out, "{} {}{}", r.status.as_str(), r.name, reason);
                if o.verbose
                    && r.status == Status::Fail
                    && let Some(d) = &r.detail
                {
                    let _ = writeln!(out, "  {}", r.title);
                    let diff = crate::diff::diff(d.kind, &d.expected, &d.actual, 3, 200);
                    for l in diff.lines() {
                        let _ = writeln!(out, "    {l}");
                    }
                }
            }
            g.0.push(r);
        }
    });
    let (results_list, counts) = all.into_inner().unwrap();
    let elapsed = start.elapsed();
    let c = counts.by_status;
    let n: usize = c.iter().sum();
    let ran = n - c[2];
    let pct = |x: usize, of: usize| {
        if of == 0 {
            0.0
        } else {
            100.0 * x as f64 / of as f64
        }
    };
    println!("----");
    println!(
        "{n} tests from {total} files: PASS {} FAIL {} SKIP {} XFAIL {} BORK {}",
        c[0], c[1], c[2], c[3], c[4]
    );
    println!(
        "pass rate {:.2}% of tests run, {:.2}% of all; {:.1}s, {:.0} tests/s, {} workers",
        pct(c[0], ran),
        pct(c[0], n),
        elapsed.as_secs_f64(),
        n as f64 / elapsed.as_secs_f64().max(1e-9),
        o.jobs
    );
    let text = results::format(&php, &results_list);
    if let Some(file) = &o.results
        && let Err(e) = std::fs::write(file, &text)
    {
        eprintln!("goro --test: writing {}: {e}", file.display());
        return 2;
    }
    if let Some(base) = &o.guard {
        let base_text = match std::fs::read_to_string(base) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("goro --test: reading {}: {e}", base.display());
                return 2;
            }
        };
        let base_map = match results::parse(&base_text) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("goro --test: {}: {e}", base.display());
                return 2;
            }
        };
        let new_map = results::parse(&text).unwrap_or_default();
        // Only tests that were part of this run can regress.
        let base_map = base_map
            .into_iter()
            .filter(|(k, _)| new_map.contains_key(k))
            .collect();
        let cmp = results::compare(&base_map, &new_map);
        print!("{}", cmp.report(false));
        return if cmp.regressions.is_empty() { 0 } else { 1 };
    }
    if c[1] + c[4] > 0 { 1 } else { 0 }
}

/// `goro --test-compare [--fixed] BASELINE NEW`.
pub fn compare_main(args: &[OsString]) -> i32 {
    let mut fixed = false;
    let mut files = Vec::new();
    for a in args {
        match a.to_string_lossy().as_ref() {
            "--fixed" => fixed = true,
            _ => files.push(PathBuf::from(a)),
        }
    }
    if files.len() != 2 {
        eprint!("{TEST_USAGE}");
        return 2;
    }
    let mut maps = Vec::new();
    for f in &files {
        match std::fs::read_to_string(f)
            .map_err(|e| e.to_string())
            .and_then(|t| results::parse(&t))
        {
            Ok(m) => maps.push(m),
            Err(e) => {
                eprintln!("goro --test-compare: {}: {e}", f.display());
                return 2;
            }
        }
    }
    let cmp = results::compare(&maps[0], &maps[1]);
    print!("{}", cmp.report(fixed));
    if cmp.regressions.is_empty() { 0 } else { 1 }
}
