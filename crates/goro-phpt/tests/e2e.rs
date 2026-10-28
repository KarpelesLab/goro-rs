//! End-to-end tests driving the runner with the fake php binary
//! (src/bin/phpt-fake-php.rs).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use goro_phpt::probe::Extensions;
use goro_phpt::runner::{Config, Status, TestResult, run_test};

const FAKE: &str = env!("CARGO_BIN_EXE_phpt-fake-php");

fn suite(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("phpt-e2e")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    dir
}

fn config(root: &Path) -> Config {
    Config {
        php: PathBuf::from(FAKE),
        root: root.to_path_buf(),
        timeout: Duration::from_secs(20),
        extra_ini: vec![("extra.setting".into(), "on".into())],
        extensions: Extensions::probe(Path::new(FAKE), root),
        keep: false,
        tmp: std::env::temp_dir(),
        mem_limit: None,
        heavy: false,
    }
}

fn write(root: &Path, rel: &str, body: &str) -> PathBuf {
    let p = root.join(rel);
    std::fs::write(&p, body).unwrap();
    p
}

fn run(cfg: &Config, path: &Path) -> TestResult {
    let mut r = run_test(cfg, path);
    assert_eq!(r.len(), 1);
    r.pop().unwrap()
}

/// Names of files in `dir` other than the given .phpt sources.
fn leftovers(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".phpt") && !n.ends_with(".inc") && !n.ends_with(".txt"))
        .collect();
    v.sort();
    v
}

#[test]
fn expect_kinds() {
    let root = suite("expect");
    let cfg = config(&root);
    let pass = write(
        &root,
        "tests/pass.phpt",
        "--TEST--\nok\n--FILE--\necho hello\n--EXPECT--\nhello\n",
    );
    let fail = write(
        &root,
        "tests/fail.phpt",
        "--TEST--\nko\n--FILE--\necho hello\n--EXPECT--\nbye\n",
    );
    let fmt = write(
        &root,
        "tests/fmt.phpt",
        "--TEST--\nf\n--FILE--\necho\necho Warning: x in /a/b.php on line 3\n--EXPECTF--\nWarning: x in %s on line %d\n",
    );
    let re = write(
        &root,
        "tests/re.phpt",
        "--TEST--\nr\n--FILE--\necho abc123\n--EXPECTREGEX--\n[a-c]+\\d{3}\n",
    );
    write(&root, "tests/ext.txt", "Warning: %s\n");
    let ext = write(
        &root,
        "tests/ext.phpt",
        "--TEST--\ne\n--FILE--\necho Warning: hi\n--EXPECTF_EXTERNAL--\next.txt\n",
    );
    write(&root, "tests/body.inc", "echo from external\n");
    let fe = write(
        &root,
        "tests/fe.phpt",
        "--TEST--\nfe\n--FILE_EXTERNAL--\nbody.inc\n--EXPECT--\nfrom external\n",
    );
    let eof = write(
        &root,
        "tests/eof.phpt",
        "--TEST--\neof\n--FILEEOF--\necho a\n\n\n--EXPECT--\na\n",
    );
    let bork = write(&root, "tests/bork.phpt", "--TEST--\nb\n--EXPECT--\nx\n");
    let escape = write(
        &root,
        "tests/escape.phpt",
        "--TEST--\nx\n--FILE_EXTERNAL--\n../outside.inc\n--EXPECT--\nx\n",
    );
    assert_eq!(run(&cfg, &pass).status, Status::Pass);
    let f = run(&cfg, &fail);
    assert_eq!(f.status, Status::Fail);
    let d = f.detail.unwrap();
    assert_eq!(d.expected, b"bye");
    assert_eq!(d.actual, b"hello");
    assert_eq!(run(&cfg, &fmt).status, Status::Pass);
    assert_eq!(run(&cfg, &re).status, Status::Pass);
    assert_eq!(run(&cfg, &ext).status, Status::Pass);
    assert_eq!(run(&cfg, &fe).status, Status::Pass);
    assert_eq!(run(&cfg, &eof).status, Status::Pass);
    assert_eq!(run(&cfg, &bork).status, Status::Bork);
    assert_eq!(run(&cfg, &escape).status, Status::Bork);
    assert!(
        leftovers(&root.join("tests")).is_empty(),
        "{:?}",
        leftovers(&root.join("tests"))
    );
}

#[test]
fn skipif_ini_env_args_stdin() {
    let root = suite("inputs");
    let cfg = config(&root);
    let skip = write(
        &root,
        "tests/skip.phpt",
        "--TEST--\ns\n--SKIPIF--\necho skip not today\n--FILE--\necho x\n--EXPECT--\nx\n",
    );
    let r = run(&cfg, &skip);
    assert_eq!((r.status, r.reason.as_str()), (Status::Skip, "not today"));

    let noskip = write(
        &root,
        "tests/noskip.phpt",
        "--TEST--\ns\n--SKIPIF--\necho fine\n--FILE--\necho x\n--EXPECT--\nx\n",
    );
    assert_eq!(run(&cfg, &noskip).status, Status::Pass);

    let ini = write(
        &root,
        "tests/ini.phpt",
        "--TEST--\ni\n--INI--\nfoo.bar=baz\narg_separator.input==\n;comment=1\npath={PWD}/x\n\
         --FILE--\nini foo.bar\nini arg_separator.input\nini comment\nini path\nini display_errors\nini log_errors\nini extra.setting\n\
         --EXPECTF--\nbaz\n=\n(unset)\n%stests/x\n1\n0\non\n",
    );
    let r = run(&cfg, &ini);
    assert_eq!(r.status, Status::Pass, "{:?}", r.detail);

    // The SKIPIF sees the test's INI and ENV.
    let skipini = write(
        &root,
        "tests/skipini.phpt",
        "--TEST--\ns\n--INI--\nwant=skip\n--ENV--\nWHY=env works\n--SKIPIF--\necho skip\nenv WHY\n--FILE--\necho x\n--EXPECT--\nx\n",
    );
    let r = run(&cfg, &skipini);
    assert_eq!((r.status, r.reason.as_str()), (Status::Skip, "env works"));

    let env = write(
        &root,
        "tests/env.phpt",
        "--TEST--\ne\n--ENV--\nFOO=bar\nDIR={PWD}\n--FILE--\nenv FOO\nenv DIR\nenv TEST_PHP_EXECUTABLE\ncwd\n--EXPECTF--\nbar\n%stests\n%sphpt-fake-php\n%sinputs\n",
    );
    let r = run(&cfg, &env);
    assert_eq!(r.status, Status::Pass, "{:?}", r.detail);

    let args = write(
        &root,
        "tests/args.phpt",
        "--TEST--\na\n--ARGS--\n-a 1 -- -c 'x y'\n--FILE--\nargs\n--EXPECT--\n-a\n1\n--\n-c\nx y\n",
    );
    let r = run(&cfg, &args);
    assert_eq!(r.status, Status::Pass, "{:?}", r.detail);

    let stdin = write(
        &root,
        "tests/stdin.phpt",
        "--TEST--\ni\n--STDIN--\nline one\nline two\n--FILE--\nstdin\necho done\n--EXPECT--\nline one\nline two\ndone\n",
    );
    let r = run(&cfg, &stdin);
    assert_eq!(r.status, Status::Pass, "{:?}", r.detail);

    // stderr is captured with stdout by default, and left alone when
    // CAPTURE_STDIO omits it.
    let both = write(
        &root,
        "tests/both.phpt",
        "--TEST--\nb\n--FILE--\necho out\nstderr err\necho out2\n--EXPECT--\nout\nerr\nout2\n",
    );
    assert_eq!(run(&cfg, &both).status, Status::Pass);
    let only_out = write(
        &root,
        "tests/only_out.phpt",
        "--TEST--\nb\n--CAPTURE_STDIO--\nSTDIN STDOUT\n--FILE--\necho out\nstderr (stderr of a CAPTURE_STDIO test, expected in the test log)\n--EXPECT--\nout\n",
    );
    assert_eq!(run(&cfg, &only_out).status, Status::Pass);
    assert!(
        leftovers(&root.join("tests")).is_empty(),
        "{:?}",
        leftovers(&root.join("tests"))
    );
}

#[test]
fn clean_xfail_flaky_crash() {
    let root = suite("lifecycle");
    let cfg = config(&root);
    let clean = write(
        &root,
        "tests/clean.phpt",
        "--TEST--\nc\n--FILE--\ntouch clean_marker.tmp\nexists clean_marker.tmp\n--CLEAN--\nrm clean_marker.tmp\n--EXPECT--\nyes\n",
    );
    assert_eq!(run(&cfg, &clean).status, Status::Pass);
    assert!(!root.join("tests/clean_marker.tmp").exists());

    // CLEAN runs even when the test fails.
    let clean_fail = write(
        &root,
        "tests/clean_fail.phpt",
        "--TEST--\nc\n--FILE--\ntouch clean_marker2.tmp\n--CLEAN--\nrm clean_marker2.tmp\n--EXPECT--\nnope\n",
    );
    assert_eq!(run(&cfg, &clean_fail).status, Status::Fail);
    assert!(!root.join("tests/clean_marker2.tmp").exists());

    let xfail = write(
        &root,
        "tests/xfail.phpt",
        "--TEST--\nx\n--XFAIL--\nknown bug\n--FILE--\necho a\n--EXPECT--\nb\n",
    );
    let r = run(&cfg, &xfail);
    assert_eq!((r.status, r.reason.as_str()), (Status::Xfail, "known bug"));
    let xfail_skipif = write(
        &root,
        "tests/xfail2.phpt",
        "--TEST--\nx\n--SKIPIF--\necho xfail not yet\n--FILE--\necho a\n--EXPECT--\nb\n",
    );
    let r = run(&cfg, &xfail_skipif);
    assert_eq!((r.status, r.reason.as_str()), (Status::Xfail, "not yet"));

    let flaky = write(
        &root,
        "tests/flaky.phpt",
        "--TEST--\nf\n--FLAKY--\nfirst run fails\n--FILE--\nflaky flaky.tmp\n--CLEAN--\nrm flaky.tmp\n--EXPECT--\nsecond\n",
    );
    assert_eq!(run(&cfg, &flaky).status, Status::Pass);
    let not_flaky = write(
        &root,
        "tests/notflaky.phpt",
        "--TEST--\nf\n--FILE--\nflaky notflaky.tmp\n--CLEAN--\nrm notflaky.tmp\n--EXPECT--\nsecond\n",
    );
    assert_eq!(run(&cfg, &not_flaky).status, Status::Fail);

    let crash = write(
        &root,
        "tests/crash.phpt",
        "--TEST--\nc\n--FILE--\necho ok\ncrash\n--EXPECT--\nok\n",
    );
    let r = run(&cfg, &crash);
    assert_eq!(r.status, Status::Fail);
    assert!(r.reason.contains("signal"), "{}", r.reason);

    // A non-zero exit code is fine (fatal errors exit with 255).
    let code = write(
        &root,
        "tests/code.phpt",
        "--TEST--\nc\n--FILE--\necho ok\nexit 255\n--EXPECT--\nok\n",
    );
    assert_eq!(run(&cfg, &code).status, Status::Pass);
    assert!(
        leftovers(&root.join("tests")).is_empty(),
        "{:?}",
        leftovers(&root.join("tests"))
    );
}

#[test]
fn memory_limit_applies_to_the_test() {
    // The child's address space is capped (RLIMIT_AS): an allocation past
    // it fails instead of growing the process.
    let root = suite("memlimit");
    let mut cfg = config(&root);
    cfg.mem_limit = Some(256 << 20);
    let t = write(
        &root,
        "tests/big.phpt",
        "--TEST--\nb\n--FILE--\necho started\nalloc 1024\n--EXPECT--\nstarted\n",
    );
    let r = run(&cfg, &t);
    let out = String::from_utf8_lossy(
        &r.detail
            .as_ref()
            .map(|d| d.actual.clone())
            .unwrap_or_default(),
    )
    .into_owned();
    assert!(!out.contains("allocated"), "{out}");
    // Under the limit the same allocation works.
    let t = write(
        &root,
        "tests/small.phpt",
        "--TEST--\ns\n--FILE--\nalloc 16\n--EXPECT--\nallocated 4096\n",
    );
    let r = run(&cfg, &t);
    assert_eq!(r.status, Status::Pass, "{}", r.reason);
}

#[test]
fn timeout_kills_the_test() {
    let root = suite("timeout");
    let mut cfg = config(&root);
    cfg.timeout = Duration::from_secs(1);
    let t = write(
        &root,
        "tests/slow.phpt",
        "--TEST--\ns\n--FILE--\necho started\nsleep 30\n--EXPECT--\nstarted\n",
    );
    let start = Instant::now();
    let r = run(&cfg, &t);
    assert!(start.elapsed() < Duration::from_secs(10));
    assert_eq!(r.status, Status::Fail);
    assert!(r.reason.contains("timed out"), "{}", r.reason);
    assert_eq!(r.detail.unwrap().actual, b"started");
}

#[test]
fn extensions_and_web_sapis() {
    let root = suite("ext");
    let cfg = config(&root);
    let builtin = write(
        &root,
        "tests/b.phpt",
        "--TEST--\nb\n--EXTENSIONS--\nFakeExt\nstandard\n--FILE--\nini extension\n--EXPECT--\n(unset)\n",
    );
    assert_eq!(run(&cfg, &builtin).status, Status::Pass);
    let shared = write(
        &root,
        "tests/s.phpt",
        "--TEST--\ns\n--EXTENSIONS--\nsharedext\n--FILE--\nini extension\n--EXPECT--\nsharedext\n",
    );
    assert_eq!(run(&cfg, &shared).status, Status::Pass);
    let missing = write(
        &root,
        "tests/m.phpt",
        "--TEST--\nm\n--EXTENSIONS--\nnosuchext\n--FILE--\necho x\n--EXPECT--\nx\n",
    );
    let r = run(&cfg, &missing);
    assert_eq!(r.status, Status::Skip);
    assert!(r.reason.contains("nosuchext"));
    let post = write(
        &root,
        "tests/p.phpt",
        "--TEST--\np\n--POST--\na=1\n--FILE--\necho x\n--EXPECT--\nx\n",
    );
    let r = run(&cfg, &post);
    assert_eq!(r.status, Status::Skip);
    assert!(r.reason.contains("CGI"));
    let dbg = write(
        &root,
        "tests/d.phpt",
        "--TEST--\nd\n--PHPDBG--\nq\n--EXPECT--\nx\n",
    );
    assert_eq!(run(&cfg, &dbg).status, Status::Skip);
}

#[test]
fn redirect_test() {
    let root = suite("redirect");
    let cfg = config(&root);
    std::fs::create_dir_all(root.join("shared/tests")).unwrap();
    write(
        &root,
        "shared/tests/one.phpt",
        "--TEST--\none\n--FILE--\nenv DRIVER\nenv REDIR_TEST_DIR\n--EXPECTF--\nfake\n%sshared/tests/\n",
    );
    let redir = write(
        &root,
        "tests/common.phpt",
        "--TEST--\nr\n--REDIRECTTEST--\necho TESTS=shared/tests\necho ENV=DRIVER=fake\n",
    );
    let rs = run_test(&cfg, &redir);
    assert_eq!(rs.len(), 1);
    assert_eq!(rs[0].name, "shared/tests/one.phpt [via tests/common.phpt]");
    assert_eq!(rs[0].status, Status::Pass, "{:?}", rs[0].detail);
}

#[test]
fn cli_results_guard_and_copy() {
    let root = suite("cli");
    // Looks like a php-src checkout so the root is detected.
    std::fs::create_dir_all(root.join("Zend")).unwrap();
    std::fs::create_dir_all(root.join("ext")).unwrap();
    write(
        &root,
        "tests/a.phpt",
        "--TEST--\na\n--FILE--\necho a\n--EXPECT--\na\n",
    );
    write(
        &root,
        "tests/b.phpt",
        "--TEST--\nb\n--CONFLICTS--\nall\n--FILE--\necho b\n--EXPECT--\nb\n",
    );
    write(
        &root,
        "tests/c.phpt",
        "--TEST--\nc\n--FILE--\necho c\ntouch stray.tmp\n--EXPECT--\nc\n",
    );
    let base = root.join("base.txt");
    let os = |s: &str| OsString::from(s);
    let path = |p: &Path| p.as_os_str().to_owned();
    let copy = root.with_file_name("cli-copy");
    let _ = std::fs::remove_dir_all(&copy);
    let code = goro_phpt::cli::test_main(&[
        os("-q"),
        os("-j"),
        os("3"),
        os("--php"),
        os(FAKE),
        os("--results"),
        path(&base),
        os("--copy-to"),
        path(&copy),
        path(&root.join("tests")),
    ]);
    assert_eq!(code, 0);
    // The copy is removed and the stray file a test made stays out of the
    // original tree.
    assert!(!copy.exists());
    assert!(!root.join("tests/stray.tmp").exists());
    let text = std::fs::read_to_string(&base).unwrap();
    assert!(
        text.contains("PASS\ttests/a.phpt\nPASS\ttests/b.phpt\nPASS\ttests/c.phpt\n"),
        "{text}"
    );

    // Break a test: the guard reports it and fails.
    write(
        &root,
        "tests/b.phpt",
        "--TEST--\nb\n--FILE--\necho changed\n--EXPECT--\nb\n",
    );
    let new = root.join("new.txt");
    let code = goro_phpt::cli::test_main(&[
        os("-q"),
        os("--php"),
        os(FAKE),
        os("--results"),
        path(&new),
        os("--guard"),
        path(&base),
        path(&root.join("tests")),
    ]);
    assert_eq!(code, 1);
    assert_eq!(goro_phpt::cli::compare_main(&[path(&base), path(&new)]), 1);
    assert_eq!(goro_phpt::cli::compare_main(&[path(&new), path(&base)]), 0);
    let _ = std::fs::remove_file(root.join("tests/stray.tmp"));
}
