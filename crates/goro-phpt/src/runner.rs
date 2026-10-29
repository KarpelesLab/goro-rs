//! Running one .phpt test.
//!
//! Conventions, with their sources:
//! - The script, SKIPIF and CLEAN code are written next to the .phpt as
//!   `NAME.php`, `NAME.skip.php` and `NAME.clean.php` (documentation:
//!   "Executed scripts are foo.php and foo.clean.php"; tests use `__DIR__`
//!   to find their fixtures). They are removed afterwards unless `keep`.
//! - The working directory is the test-suite root (the php-src checkout):
//!   ext/pdo_odbc/tests/*.phpt `require 'ext/pdo/tests/pdo_test.inc'`.
//! - SKIPIF output starting with "skip" skips the test, "xfail" marks it as
//!   an expected failure and "flaky" lets it be retried (documentation).
//!   SKIPIF and CLEAN run with the same INI and ENV as the test, so a SKIPIF
//!   can inspect the test's settings (ext/opcache/tests/file_cache_error.phpt
//!   reads opcache.jit_buffer_size).
//! - INI lines are `key=value`, split at the first `=` (tests/run-test/
//!   test004.phpt sets `arg_separator.input==`). Lines starting with `;` are
//!   comments (the corpus has `;opcache.jit_debug=257`). `{PWD}` is the
//!   test's directory, `{TMP}` the system temp directory (documentation),
//!   and `{ENV:NAME}` an environment variable (used by
//!   ext/opcache/tests/preload_user_*.phpt).
//! - ARGS go after the script: `-f SCRIPT -- ARGS`, the CLI's documented
//!   form; ext/standard/tests/general_functions/getopt_007.phpt expects a
//!   `--` inside ARGS to reach the script.
//! - FLAKY tests (section or SKIPIF answer) are retried once on failure.
//! - XFAIL tests that fail report XFAIL; ones that pass report PASS.
//! - Tests needing a web SAPI or phpdbg are skipped until those exist.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::expect::{Kind, Matcher, normalize};
use crate::probe::{ExtAction, Extensions};
use crate::proc::{Capture, Exit, Run, RunOutput, run};
use crate::section::{Phpt, list_lines, trim_ws};

/// The ini settings every test run gets after `-n`, and why.
///
/// `-n` itself: no php.ini is read, so the host's configuration cannot leak
/// into results (the reference binary's production php.ini visibly changes
/// error output).
pub const DEFAULT_INI: &[(&str, &str)] = &[
    // Documentation: run-tests defaults to error_reporting(E_ALL) and
    // display_errors=1.
    ("error_reporting", "E_ALL"),
    ("display_errors", "1"),
    // Startup warnings are expected on stdout: Zend/tests/exit/
    // disabling_die.phpt expects "Warning: Cannot disable function die() in
    // Unknown on line 0".
    ("display_startup_errors", "1"),
    // With log_errors on, the CLI also logs each message to stderr ("PHP
    // Warning:  ...", observed on PHP 8.5), which is captured with stdout;
    // the same test expects the message once.
    ("log_errors", "0"),
    // Tests expect plain-text messages ("Warning: ... in %s on line %d").
    ("html_errors", "0"),
    // PHP 8.5 prints a stack trace after fatal errors by default (observed),
    // but tests expect none (ext/standard/tests/strings/
    // wordwrap_memory_limit.phpt), and the tests that want one set
    // fatal_error_backtraces=On themselves (Zend/tests/backtrace/
    // fatal_error_backtraces_001.phpt).
    ("fatal_error_backtraces", "0"),
    // assert() tests expect AssertionError to be thrown; production
    // php.ini files turn assertions off (zend.assertions=-1).
    ("zend.assertions", "1"),
    // ext/standard/tests/strings/wordwrap_memory_limit.phpt and others expect
    // the 128M limit ("Allowed memory size of 134217728 bytes").
    ("memory_limit", "128M"),
    // Output must reach the pipe unbuffered and unfiltered.
    ("output_buffering", "0"),
    ("output_handler", ""),
    ("implicit_flush", "1"),
    // No path restrictions, disabled functions or prepended code.
    ("open_basedir", ""),
    ("disable_functions", ""),
    ("auto_prepend_file", ""),
    ("auto_append_file", ""),
    ("error_prepend_string", ""),
    ("error_append_string", ""),
    ("max_execution_time", "0"),
];

#[derive(Debug, Clone)]
pub struct Config {
    /// The PHP binary under test.
    pub php: PathBuf,
    /// The test-suite root: working directory of every run, and the base of
    /// reported test names.
    pub root: PathBuf,
    pub timeout: Duration,
    /// Extra `-d` settings from the command line, applied after the defaults.
    pub extra_ini: Vec<(String, String)>,
    pub extensions: Extensions,
    /// Keep the generated scripts, and write NAME.out/.exp/.diff on failure.
    pub keep: bool,
    /// Directory substituted for `{TMP}`.
    pub tmp: PathBuf,
    /// Address-space limit of each test process (bytes).
    pub mem_limit: Option<u64>,
    /// Run tests needing large resources (see [`HEAVY_TESTS`]).
    pub heavy: bool,
    /// Run tests reaching hosts on the Internet (see [`ONLINE_TESTS`]).
    pub online: bool,
}

/// Tests that reach hosts on the Internet without checking
/// SKIP_ONLINE_TESTS (connections, DNS lookups of real or `.invalid`
/// names, in their SKIPIF or FILE sections): skipped unless asked for
/// (`--online`).
pub const ONLINE_TESTS: &[&str] = &[
    "ext/curl/tests/bug52202.phpt",
    "ext/curl/tests/bug71523.phpt",
    "ext/curl/tests/curl_basic_008.phpt",
    "ext/curl/tests/curl_basic_010.phpt",
    "ext/standard/tests/file/file_get_contents_error002.phpt",
    "ext/standard/tests/file/file_get_contents_with_custom_uri_parser.phpt",
    "ext/standard/tests/network/gethostbyname_basic001.phpt",
    // DNS queries for unknown names ("asdfasdf", ".toto.toto.toto")
    "ext/standard/tests/network/gethostbyname_error003.phpt",
    "ext/standard/tests/network/gethostbyname_error006.phpt",
];

/// Tests that need several gigabytes of memory or disk: skipped unless
/// asked for (`--heavy`).
pub const HEAVY_TESTS: &[&str] = &[
    "file_get_contents_file_put_contents_5gb.phpt",
    // a 2 GB string
    "gh15613.phpt",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Pass,
    Fail,
    Skip,
    Xfail,
    Bork,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Skip => "SKIP",
            Status::Xfail => "XFAIL",
            Status::Bork => "BORK",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        Some(match s {
            "PASS" => Status::Pass,
            "FAIL" => Status::Fail,
            "SKIP" => Status::Skip,
            "XFAIL" => Status::Xfail,
            "BORK" => Status::Bork,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TestResult {
    /// The test path relative to the root, with `[via REDIRECT]` appended for
    /// tests run through a REDIRECTTEST.
    pub name: String,
    pub title: String,
    pub status: Status,
    pub reason: String,
    /// Normalized expected and actual output, for failures.
    pub detail: Option<FailDetail>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct FailDetail {
    pub kind: Kind,
    pub expected: Vec<u8>,
    pub actual: Vec<u8>,
}

/// Environment added to every run of one test.
type Env = Vec<(OsString, OsString)>;

/// A test's name relative to the root.
pub fn display_name(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// Quotes a string for a POSIX shell.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+=:,@%".contains(&b))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Splits ARGS like a shell would for simple cases: whitespace separates
/// words; single and double quotes group.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                have = true;
            }
            None if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            None => {
                cur.push(c);
                have = true;
            }
        }
    }
    if have {
        out.push(cur);
    }
    out
}

/// Replaces `{PWD}`, `{TMP}` and `{ENV:NAME}` in an INI or ENV value.
pub fn substitute(value: &str, pwd: &Path, tmp: &Path) -> String {
    let mut s = value
        .replace("{PWD}", &pwd.to_string_lossy())
        .replace("{TMP}", &tmp.to_string_lossy());
    // `{MAIL:FILE}`: a sendmail_path storing the message in FILE (relative
    // to the working directory, where ext/standard/tests/mail/*.phpt read
    // it back)
    while let Some(start) = s.find("{MAIL:") {
        let Some(len) = s[start..].find('}') else {
            break;
        };
        let quoted = s[start + 6..start + len].replace('\'', "'\\''");
        s.replace_range(start..start + len + 1, &format!("cat > '{quoted}'"));
    }
    while let Some(start) = s.find("{ENV:") {
        let Some(len) = s[start..].find('}') else {
            break;
        };
        let name = &s[start + 5..start + len];
        let val = std::env::var(name).unwrap_or_default();
        s.replace_range(start..start + len + 1, &val);
    }
    s
}

/// Parses INI section lines into key/value pairs.
pub fn parse_ini(lines: &[String], pwd: &Path, tmp: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in lines {
        if l.starts_with(';') {
            continue;
        }
        let (k, v) = match l.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (l.as_str(), ""),
        };
        if k.is_empty() {
            continue;
        }
        out.push((k.to_string(), substitute(v, pwd, tmp)));
    }
    out
}

struct Prepared {
    dir: PathBuf,
    stem: PathBuf,
    ini_args: Vec<OsString>,
    env: Env,
}

impl Prepared {
    fn file(&self, suffix: &str) -> PathBuf {
        let mut s = self.stem.clone().into_os_string();
        s.push(suffix);
        PathBuf::from(s)
    }
}

fn result(name: &str, title: &str, status: Status, reason: impl Into<String>) -> TestResult {
    TestResult {
        name: name.to_string(),
        title: title.to_string(),
        status,
        reason: reason.into(),
        detail: None,
        elapsed: Duration::ZERO,
    }
}

/// What SKIPIF said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipAnswer {
    Run,
    Skip(String),
    Xfail(String),
    Flaky,
}

/// Interprets SKIPIF output. The documentation: output starting with "skip"
/// skips (the rest is the reason), "xfail" marks an expected failure,
/// "flaky" a flaky test.
pub fn skip_answer(output: &[u8]) -> SkipAnswer {
    let t = trim_ws(output);
    let lower: Vec<u8> = t.iter().take(5).map(|b| b.to_ascii_lowercase()).collect();
    let rest = |n: usize| {
        let r = String::from_utf8_lossy(&t[n..]).into_owned();
        r.trim_start_matches(|c: char| c == ':' || c == '-' || c.is_whitespace())
            .trim_end()
            .to_string()
    };
    if lower.starts_with(b"skip") {
        SkipAnswer::Skip(rest(4))
    } else if lower.starts_with(b"xfail") {
        SkipAnswer::Xfail(rest(5))
    } else if lower.starts_with(b"flaky") {
        SkipAnswer::Flaky
    } else {
        SkipAnswer::Run
    }
}

fn capture_spec(p: &Phpt) -> Capture {
    let Some(body) = p.get("CAPTURE_STDIO") else {
        return Capture::ALL;
    };
    let text = String::from_utf8_lossy(body).to_ascii_uppercase();
    let words: Vec<&str> = text.split_whitespace().collect();
    Capture {
        stdin: words.contains(&"STDIN"),
        stdout: words.contains(&"STDOUT"),
        stderr: words.contains(&"STDERR"),
    }
}

/// Base environment of every test run (on top of the runner's own).
pub fn base_env(cfg: &Config) -> Env {
    let php = cfg.php.to_string_lossy().into_owned();
    let mut extra = vec!["-n".to_string()];
    for (k, v) in DEFAULT_INI.iter().map(|(k, v)| (*k, *v)) {
        extra.push("-d".into());
        extra.push(format!("{k}={v}"));
    }
    let extra: Vec<String> = extra.iter().map(|s| shell_quote(s)).collect();
    vec![
        ("TEST_PHP_EXECUTABLE".into(), php.clone().into()),
        (
            "TEST_PHP_EXECUTABLE_ESCAPED".into(),
            shell_quote(&php).into(),
        ),
        ("TEST_PHP_EXTRA_ARGS".into(), extra.join(" ").into()),
        // Tests needing the internet skip themselves (run-tests' variable):
        // runs here stay on the local machine, unless the environment
        // already says otherwise.
        (
            "SKIP_ONLINE_TESTS".into(),
            std::env::var_os("SKIP_ONLINE_TESTS").unwrap_or_else(|| "1".into()),
        ),
    ]
}

/// Runs the test at `path` (and, for a REDIRECTTEST, the tests it points
/// to). Returns one result per test run.
pub fn run_test(cfg: &Config, path: &Path) -> Vec<TestResult> {
    let name = display_name(&cfg.root, path);
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => return vec![result(&name, "", Status::Bork, format!("cannot read: {e}"))],
    };
    let phpt = match Phpt::parse(&data) {
        Ok(p) => p,
        Err(e) => return vec![result(&name, "", Status::Bork, e.to_string())],
    };
    if phpt.has("REDIRECTTEST") {
        return run_redirect(cfg, path, &name, &phpt);
    }
    vec![run_single(cfg, path, &name, &phpt, &[])]
}

fn prepare(
    cfg: &Config,
    path: &Path,
    phpt: &Phpt,
    extra_env: &[(OsString, OsString)],
) -> Result<Prepared, (Status, String)> {
    let dir = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = path.with_extension("");
    let mut ini_args: Vec<OsString> = vec!["-n".into()];
    let mut push = |k: &str, v: &str| {
        ini_args.push("-d".into());
        ini_args.push(format!("{k}={v}").into());
    };
    for (k, v) in DEFAULT_INI {
        push(k, v);
    }
    for (k, v) in &cfg.extra_ini {
        push(k, v);
    }
    for ext in phpt.get_list("EXTENSIONS", false) {
        match cfg.extensions.action(&ext) {
            ExtAction::Present => {}
            ExtAction::Load(setting) => {
                let (k, v) = setting.split_once('=').unwrap_or((&setting, ""));
                push(k, v);
            }
            ExtAction::Missing => {
                return Err((Status::Skip, format!("extension {ext} is not available")));
            }
        }
    }
    let ini = parse_ini(&phpt.get_list("INI", false), &dir, &cfg.tmp);
    for (k, v) in &ini {
        push(k, v);
    }
    let mut env = base_env(cfg);
    env.extend(extra_env.iter().cloned());
    for line in phpt.get_list("ENV", false) {
        let (k, v) = line.split_once('=').unwrap_or((&line, ""));
        env.push((k.trim().into(), substitute(v.trim(), &dir, &cfg.tmp).into()));
    }
    Ok(Prepared {
        dir,
        stem,
        ini_args,
        env,
    })
}

fn exec(
    cfg: &Config,
    prep: &Prepared,
    script: &Path,
    args: &[String],
    stdin: Option<&[u8]>,
    capture: Capture,
) -> std::io::Result<RunOutput> {
    let mut argv = prep.ini_args.clone();
    argv.push("-f".into());
    argv.push(script.into());
    if !args.is_empty() {
        argv.push("--".into());
        argv.extend(args.iter().map(OsString::from));
    }
    run(&Run {
        program: &cfg.php,
        args: &argv,
        cwd: &cfg.root,
        env: &prep.env,
        stdin,
        capture,
        timeout: cfg.timeout,
        mem_limit: cfg.mem_limit,
    })
}

/// Runs a SKIPIF section. Returns its answer.
fn run_skipif(cfg: &Config, prep: &Prepared, code: &[u8]) -> Result<SkipAnswer, String> {
    let file = prep.file(".skip.php");
    std::fs::write(&file, code).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    let out = exec(cfg, prep, &file, &[], None, Capture::ALL);
    if !cfg.keep {
        let _ = std::fs::remove_file(&file);
    }
    let out = out.map_err(|e| format!("cannot run {}: {e}", cfg.php.display()))?;
    if out.exit == Exit::Timeout {
        return Err("SKIPIF timed out".into());
    }
    Ok(skip_answer(&out.output))
}

fn run_clean(cfg: &Config, prep: &Prepared, code: &[u8]) {
    let file = prep.file(".clean.php");
    if std::fs::write(&file, code).is_ok() {
        let _ = exec(cfg, prep, &file, &[], None, Capture::ALL);
        if !cfg.keep {
            let _ = std::fs::remove_file(&file);
        }
    }
}

/// Reads a file named by an *_EXTERNAL section, relative to the test's
/// directory. The documentation: it must be in the same directory or a
/// subdirectory.
fn read_external(dir: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let rel_path = Path::new(rel);
    if rel.is_empty()
        || rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "external file {rel:?} is not inside the test directory"
        ));
    }
    std::fs::read(dir.join(rel_path)).map_err(|e| format!("cannot read external file {rel}: {e}"))
}

fn expectation(phpt: &Phpt, dir: &Path) -> Result<(Kind, Vec<u8>), String> {
    let sec = phpt.expect_section().ok_or("no EXPECT section")?;
    let kind = match sec {
        "EXPECT" | "EXPECT_EXTERNAL" => Kind::Exact,
        "EXPECTF" | "EXPECTF_EXTERNAL" => Kind::Format,
        _ => Kind::Regex,
    };
    let body = if sec.ends_with("_EXTERNAL") {
        read_external(dir, &phpt.get_line(sec).unwrap_or_default())?
    } else {
        phpt.get(sec).unwrap_or_default().to_vec()
    };
    Ok((kind, normalize(&body)))
}

fn script_body(phpt: &Phpt, dir: &Path) -> Result<Vec<u8>, String> {
    match phpt.file_section() {
        Some("FILE") => Ok(phpt.get("FILE").unwrap_or_default().to_vec()),
        Some("FILEEOF") => {
            // Documentation: like FILE, with trailing line breaks removed.
            let b = phpt.get("FILEEOF").unwrap_or_default();
            let end = b
                .iter()
                .rposition(|&c| c != b'\n' && c != b'\r')
                .map_or(0, |e| e + 1);
            Ok(b[..end].to_vec())
        }
        Some("FILE_EXTERNAL") => {
            read_external(dir, &phpt.get_line("FILE_EXTERNAL").unwrap_or_default())
        }
        _ => Err("no FILE section".into()),
    }
}

fn run_single(
    cfg: &Config,
    path: &Path,
    name: &str,
    phpt: &Phpt,
    extra_env: &[(OsString, OsString)],
) -> TestResult {
    let start = std::time::Instant::now();
    let mut r = run_single_inner(cfg, path, name, phpt, extra_env);
    r.elapsed = start.elapsed();
    r
}

fn run_single_inner(
    cfg: &Config,
    path: &Path,
    name: &str,
    phpt: &Phpt,
    extra_env: &[(OsString, OsString)],
) -> TestResult {
    let title = phpt.title();
    if !cfg.heavy
        && path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| HEAVY_TESTS.contains(&f))
    {
        return result(
            name,
            &title,
            Status::Skip,
            "needs large resources (run with --heavy)",
        );
    }
    if !cfg.online && ONLINE_TESTS.contains(&name) {
        return result(
            name,
            &title,
            Status::Skip,
            "reaches the Internet (run with --online)",
        );
    }
    if let Some(sec) = phpt.cgi_section() {
        return result(
            name,
            &title,
            Status::Skip,
            format!("needs the CGI SAPI (--{sec}--)"),
        );
    }
    if phpt.has("PHPDBG") {
        return result(name, &title, Status::Skip, "needs phpdbg (--PHPDBG--)");
    }
    let prep = match prepare(cfg, path, phpt, extra_env) {
        Ok(p) => p,
        Err((st, why)) => return result(name, &title, st, why),
    };
    let mut flaky = phpt.has("FLAKY");
    let mut xfail = phpt.get_line("XFAIL");
    if let Some(code) = phpt.get("SKIPIF") {
        match run_skipif(cfg, &prep, code) {
            Ok(SkipAnswer::Run) => {}
            Ok(SkipAnswer::Skip(why)) => return result(name, &title, Status::Skip, why),
            Ok(SkipAnswer::Xfail(why)) => xfail = Some(why),
            Ok(SkipAnswer::Flaky) => flaky = true,
            Err(e) => return result(name, &title, Status::Bork, e),
        }
    }
    let (kind, expected) = match expectation(phpt, &prep.dir) {
        Ok(x) => x,
        Err(e) => return result(name, &title, Status::Bork, e),
    };
    let matcher = match Matcher::new(kind, &expected) {
        Ok(m) => m,
        Err(e) => return result(name, &title, Status::Bork, e),
    };
    let script = match script_body(phpt, &prep.dir) {
        Ok(s) => s,
        Err(e) => return result(name, &title, Status::Bork, e),
    };
    let script_path = prep.file(".php");
    if let Err(e) = std::fs::write(&script_path, &script) {
        return result(
            name,
            &title,
            Status::Bork,
            format!("cannot write {}: {e}", script_path.display()),
        );
    }
    let args = phpt
        .get_line("ARGS")
        .map(|a| split_args(&a))
        .unwrap_or_default();
    let stdin = phpt.get("STDIN");
    let capture = capture_spec(phpt);

    let mut attempts = if flaky { 2 } else { 1 };
    let outcome = loop {
        attempts -= 1;
        let out = match exec(cfg, &prep, &script_path, &args, stdin, capture) {
            Ok(o) => o,
            Err(e) => break Err(format!("cannot run {}: {e}", cfg.php.display())),
        };
        let actual = normalize(&out.output);
        let ok = matches!(out.exit, Exit::Code(_)) && matcher.is_match(&actual);
        if ok || attempts == 0 {
            break Ok((ok, out.exit, actual));
        }
    };
    if let Some(code) = phpt.get("CLEAN") {
        run_clean(cfg, &prep, code);
    }
    if !cfg.keep {
        let _ = std::fs::remove_file(&script_path);
    }
    let (ok, exit, actual) = match outcome {
        Ok(x) => x,
        Err(e) => return result(name, &title, Status::Bork, e),
    };
    if ok {
        return result(name, &title, Status::Pass, "");
    }
    let reason = match exit {
        Exit::Timeout => format!("timed out after {}s", cfg.timeout.as_secs()),
        Exit::Signal(s) => format!("killed by signal {s}"),
        Exit::TooMuchOutput => format!("more than {} bytes of output", crate::proc::OUTPUT_LIMIT),
        Exit::Code(_) => "output differs".to_string(),
    };
    if cfg.keep {
        let _ = std::fs::write(prep.file(".out"), &actual);
        let _ = std::fs::write(prep.file(".exp"), &expected);
        let d = crate::diff::diff(kind, &expected, &actual, 3, usize::MAX);
        let _ = std::fs::write(prep.file(".diff"), d);
    }
    let (status, reason) = match xfail {
        Some(why) => (Status::Xfail, why),
        None => (Status::Fail, reason),
    };
    let mut r = result(name, &title, status, reason);
    r.detail = Some(FailDetail {
        kind,
        expected,
        actual,
    });
    r
}

/// REDIRECTTEST (documentation): PHP code returning an array whose `TESTS`
/// names a directory, relative to the source root, of tests to run with the
/// array's `ENV` added to the environment. The tests find their includes
/// through REDIR_TEST_DIR (ext/pdo/tests/*.phpt read
/// `getenv('REDIR_TEST_DIR')`).
fn run_redirect(cfg: &Config, path: &Path, name: &str, phpt: &Phpt) -> Vec<TestResult> {
    let title = phpt.title();
    let prep = match prepare(cfg, path, phpt, &[]) {
        Ok(p) => p,
        Err((st, why)) => return vec![result(name, &title, st, why)],
    };
    if let Some(code) = phpt.get("SKIPIF") {
        match run_skipif(cfg, &prep, code) {
            Ok(SkipAnswer::Skip(why)) => return vec![result(name, &title, Status::Skip, why)],
            Err(e) => return vec![result(name, &title, Status::Bork, e)],
            Ok(_) => {}
        }
    }
    let body = phpt.get("REDIRECTTEST").unwrap_or_default();
    // The runner evaluates the code itself, so `__DIR__` is the suite root
    // (ext/pdo_sqlite/tests/common.phpt: `__DIR__ . '/ext/pdo/tests'`).
    let root_lit = {
        let r = cfg
            .root
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        format!("'{r}'")
    };
    let body = String::from_utf8_lossy(body)
        .replace("__DIR__", &root_lit)
        .into_bytes();
    let mut code = b"<?php\n$__redirect_config = (function () {\n".to_vec();
    code.extend_from_slice(&body);
    code.extend_from_slice(
        b"\n})();\necho \"TESTS=\", $__redirect_config['TESTS'] ?? '', \"\\n\";\n\
          foreach (($__redirect_config['ENV'] ?? []) as $__k => $__v) { echo \"ENV=\", $__k, \"=\", $__v, \"\\n\"; }\n",
    );
    let file = prep.file(".php");
    if let Err(e) = std::fs::write(&file, &code) {
        return vec![result(
            name,
            &title,
            Status::Bork,
            format!("cannot write {}: {e}", file.display()),
        )];
    }
    let out = exec(cfg, &prep, &file, &[], None, Capture::ALL);
    if !cfg.keep {
        let _ = std::fs::remove_file(&file);
    }
    let out = match out {
        Ok(o) if o.exit == Exit::Code(0) => o.output,
        Ok(o) => {
            return vec![result(
                name,
                &title,
                Status::Fail,
                format!(
                    "REDIRECTTEST code failed: {}",
                    String::from_utf8_lossy(trim_ws(&o.output))
                ),
            )];
        }
        Err(e) => return vec![result(name, &title, Status::Bork, e.to_string())],
    };
    let mut tests_dir = None;
    let mut env: HashMap<String, String> = HashMap::new();
    for line in list_lines(&out, false) {
        if let Some(t) = line.strip_prefix("TESTS=") {
            tests_dir = Some(t.trim().to_string());
        } else if let Some(kv) = line.strip_prefix("ENV=")
            && let Some((k, v)) = kv.split_once('=')
        {
            env.insert(k.to_string(), v.to_string());
        }
    }
    let Some(tests_dir) = tests_dir.filter(|t| !t.is_empty()) else {
        return vec![result(
            name,
            &title,
            Status::Bork,
            "REDIRECTTEST returned no TESTS",
        )];
    };
    let target = cfg.root.join(&tests_dir);
    let mut files = Vec::new();
    crate::collect(&target, &mut files);
    if files.is_empty() {
        return vec![result(
            name,
            &title,
            Status::Bork,
            format!("no tests in {tests_dir}"),
        )];
    }
    let mut extra: Env = prep.env.iter().skip(base_env(cfg).len()).cloned().collect();
    let mut redir = target.into_os_string();
    redir.push("/");
    extra.push(("REDIR_TEST_DIR".into(), redir));
    let mut keys: Vec<_> = env.into_iter().collect();
    keys.sort();
    for (k, v) in keys {
        extra.push((k.into(), v.into()));
    }
    let mut results = Vec::new();
    for f in files {
        let sub_name = format!("{} [via {name}]", display_name(&cfg.root, &f));
        let r = match std::fs::read(&f)
            .map_err(|e| e.to_string())
            .and_then(|d| Phpt::parse(&d).map_err(|e| e.to_string()))
        {
            Ok(p) if p.has("REDIRECTTEST") => {
                result(&sub_name, &p.title(), Status::Bork, "nested REDIRECTTEST")
            }
            Ok(p) => run_single(cfg, &f, &sub_name, &p, &extra),
            Err(e) => result(&sub_name, "", Status::Bork, e),
        };
        results.push(r);
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipif_answers() {
        assert_eq!(
            skip_answer(b"skip no network"),
            SkipAnswer::Skip("no network".into())
        );
        assert_eq!(
            skip_answer(b"\n Skip: 64-bit only\n"),
            SkipAnswer::Skip("64-bit only".into())
        );
        assert_eq!(skip_answer(b"skip"), SkipAnswer::Skip(String::new()));
        assert_eq!(
            skip_answer(b"xfail bug 123"),
            SkipAnswer::Xfail("bug 123".into())
        );
        assert_eq!(skip_answer(b"flaky"), SkipAnswer::Flaky);
        assert_eq!(skip_answer(b""), SkipAnswer::Run);
        assert_eq!(skip_answer(b"Warning: foo"), SkipAnswer::Run);
    }

    #[test]
    fn ini_parsing() {
        let pwd = Path::new("/t/dir");
        let tmp = Path::new("/tmp");
        let lines: Vec<String> = [
            "arg_separator.input==",
            ";opcache.jit_debug=257",
            "open_basedir={PWD}",
            "opcache.file_cache=\"{TMP}\"",
            "error_reporting = E_ALL ^ E_NOTICE",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            parse_ini(&lines, pwd, tmp),
            vec![
                ("arg_separator.input".to_string(), "=".to_string()),
                ("open_basedir".into(), "/t/dir".into()),
                ("opcache.file_cache".into(), "\"/tmp\"".into()),
                ("error_reporting".into(), "E_ALL ^ E_NOTICE".into()),
            ]
        );
    }

    #[test]
    fn args_and_quoting() {
        assert_eq!(
            split_args("-a 1 -b 2 -- -c nope test"),
            vec!["-a", "1", "-b", "2", "--", "-c", "nope", "test"]
        );
        assert_eq!(split_args(" 'a b' \"c\" '' "), vec!["a b", "c", ""]);
        assert_eq!(shell_quote("/usr/bin/php"), "/usr/bin/php");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
