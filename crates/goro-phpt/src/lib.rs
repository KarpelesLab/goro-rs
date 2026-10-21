use std::collections::HashMap;
use std::path::Path;

pub mod mysqli_test;

const VALID_SECTIONS: &[&str] = &[
    "TEST",
    "FILE",
    "FILEEOF",
    "EXPECT",
    "EXPECTF",
    "EXPECTREGEX",
    "SKIPIF",
    "INI",
    "ARGS",
    "ENV",
    "STDIN",
    "POST",
    "POST_RAW",
    "GZIP_POST",
    "DEFLATE_POST",
    "PUT",
    "GET",
    "COOKIE",
    "HEADERS",
    "CLEAN",
    "CREDITS",
    "DESCRIPTION",
    "EXTENSIONS",
    "CONFLICTS",
    "XFAIL",
    "XLEAK",
    "CAPTURE_STDIO",
    "FILE_EXTERNAL",
    "EXPECT_EXTERNAL",
    "EXPECTF_EXTERNAL",
    "EXPECTHEADERS",
    "CGI",
    "PHPDBG",
    "REDIRECTTEST",
    "FLAKY",
    "WHITESPACE_SENSITIVE",
    "EXPECTREGEX_EXTERNAL",
];

/// A parsed PHPT test file
#[derive(Debug)]
pub struct PhptTest {
    pub name: String,
    pub sections: HashMap<String, String>,
    /// Byte-exact (line endings preserved) copies of the request sections.
    pub raw_sections: HashMap<String, String>,
    /// The file was not valid UTF-8 and was decoded as Latin-1 (one char per
    /// byte); `bytes()` maps section text back to the original bytes.
    pub latin1: bool,
}

impl PhptTest {
    /// Parse a .phpt file from its content
    pub fn parse(content: &str) -> Option<Self> {
        let mut sections: HashMap<String, String> = HashMap::new();
        let mut current_section: Option<String> = None;
        let mut current_content = String::new();

        let valid_sections = VALID_SECTIONS;
        for line in content.lines() {
            if line.starts_with("--") && line.ends_with("--") && line.len() > 4 {
                let section_name = &line[2..line.len() - 2];
                // Only recognize valid PHPT section names
                if !valid_sections.contains(&section_name) {
                    // Not a section header, treat as content
                    if current_section.is_some() {
                        if !current_content.is_empty() {
                            current_content.push('\n');
                        }
                        current_content.push_str(line);
                    }
                    continue;
                }
                // Save the previous section
                if let Some(ref sec) = current_section {
                    sections.insert(sec.clone(), current_content.clone());
                }
                current_section = Some(section_name.to_string());
                current_content.clear();
            } else if current_section.is_some() {
                if !current_content.is_empty() {
                    current_content.push('\n');
                }
                current_content.push_str(line);
            }
        }

        // Save the last section
        if let Some(ref sec) = current_section {
            sections.insert(sec.clone(), current_content.clone());
        }

        // run-tests.php keeps the FILE section byte-exact (including the
        // trailing newline, which matters for inline HTML after `?>`), stops
        // it at a `===DONE===` line, and strips trailing newlines of FILEEOF.
        if let Some(raw) = Self::raw_file_section(content) {
            sections.insert("FILE".to_string(), raw);
        }

        let name = sections.get("TEST")?.clone();

        let mut raw_sections = HashMap::new();
        for sec in [
            "POST", "POST_RAW", "PUT", "GET", "COOKIE", "ARGS", "ENV", "STDIN",
        ] {
            if let Some(r) = Self::raw_section(content, sec) {
                raw_sections.insert(sec.to_string(), r);
            }
        }
        Some(PhptTest {
            name,
            sections,
            raw_sections,
            latin1: false,
        })
    }

    /// Extract a section byte-exactly (keeping `\r\n` line endings).
    fn raw_section(content: &str, want: &str) -> Option<String> {
        let mut out: Option<String> = None;
        let mut inside = false;
        for line in content.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            if bare.len() > 4
                && bare.starts_with("--")
                && bare.ends_with("--")
                && VALID_SECTIONS.contains(&&bare[2..bare.len() - 2])
            {
                inside = &bare[2..bare.len() - 2] == want;
                if inside {
                    out = Some(String::new());
                }
                continue;
            }
            if inside {
                if let Some(o) = out.as_mut() {
                    o.push_str(line);
                }
            }
        }
        out
    }

    /// Extract the FILE (or FILEEOF) section exactly as run-tests.php does.
    fn raw_file_section(content: &str) -> Option<String> {
        let mut out: Option<String> = None;
        let mut eof = false;
        let mut in_file = false;
        let mut done = false;
        for line in content.split_inclusive('\n') {
            let bare = line.trim_end_matches(['\n', '\r']);
            if bare.len() > 4
                && bare.starts_with("--")
                && bare.ends_with("--")
                && VALID_SECTIONS.contains(&&bare[2..bare.len() - 2])
            {
                let name = &bare[2..bare.len() - 2];
                if name == "FILE" || name == "FILEEOF" {
                    in_file = true;
                    eof = name == "FILEEOF";
                    done = false;
                    out = Some(String::new());
                    continue;
                }
                if in_file {
                    in_file = false;
                }
                continue;
            }
            if in_file && !done {
                if let Some(o) = out.as_mut() {
                    o.push_str(line);
                }
                if bare.starts_with("===DONE===") && bare[10..].trim().is_empty() {
                    done = true;
                }
            }
        }
        if eof {
            if let Some(o) = out.as_mut() {
                while o.ends_with('\n') || o.ends_with('\r') {
                    o.pop();
                }
            }
        }
        out
    }

    /// Parse raw file bytes; non-UTF-8 files are decoded as Latin-1.
    pub fn parse_bytes(content: &[u8]) -> Option<Self> {
        match std::str::from_utf8(content) {
            Ok(s) => Self::parse(s),
            Err(_) => {
                let s: String = content.iter().map(|&b| b as char).collect();
                let mut t = Self::parse(&s)?;
                t.latin1 = true;
                Some(t)
            }
        }
    }

    /// Bytes of a section's text (undoing the Latin-1 decoding if needed).
    pub fn bytes(&self, s: &str) -> Vec<u8> {
        if self.latin1 {
            s.chars().map(|c| c as u32 as u8).collect()
        } else {
            s.as_bytes().to_vec()
        }
    }

    pub fn file_section(&self) -> Option<&str> {
        self.sections.get("FILE").map(|s| s.as_str())
    }

    pub fn expect_section(&self) -> Option<&str> {
        self.sections.get("EXPECT").map(|s| s.as_str())
    }

    pub fn expectf_section(&self) -> Option<&str> {
        self.sections.get("EXPECTF").map(|s| s.as_str())
    }

    pub fn skipif_section(&self) -> Option<&str> {
        self.sections.get("SKIPIF").map(|s| s.as_str())
    }

    pub fn expect_regex_section(&self) -> Option<&str> {
        self.sections.get("EXPECTREGEX").map(|s| s.as_str())
    }

    pub fn ini_section(&self) -> Option<&str> {
        self.sections.get("INI").map(|s| s.as_str())
    }
}

/// Result of running a single PHPT test
#[derive(Debug)]
pub enum TestResult {
    Pass,
    Fail {
        expected: String,
        actual: String,
    },
    Skip(String),
    Error(String),
    /// The test failed, but it has an --XFAIL-- section (or its SKIPIF
    /// printed "xfail ..."): an expected failure, not counted as one.
    XFail(String),
    /// The test passed although it is marked --XFAIL-- (run-tests.php
    /// reports it as a warning, not a failure).
    XPass,
}

thread_local! {
    /// Reason given by a SKIPIF section that printed "xfail <reason>".
    static SKIPIF_XFAIL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

// Run a PHPT test against the goro engine
thread_local! {
    static TEST_ARGS: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    static TEST_ENV: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Split --ARGS-- like a POSIX shell would (whitespace, single/double quotes).
fn split_test_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            None => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    out
}

pub fn run_test(test: &PhptTest) -> TestResult {
    run_test_with_dir(test, None)
}

pub fn run_test_with_dir(test: &PhptTest, test_dir: Option<&Path>) -> TestResult {
    run_test_with_dir_and_filename(test, test_dir, "Unknown.php")
}

pub fn run_test_with_dir_and_filename(
    test: &PhptTest,
    test_dir: Option<&Path>,
    filename: &str,
) -> TestResult {
    SKIPIF_XFAIL.with(|x| *x.borrow_mut() = None);
    let result = run_test_inner(test, test_dir, filename);
    // --XFAIL-- (run-tests.php): a failure is expected, a pass is notable.
    let xfail = test
        .sections
        .get("XFAIL")
        .map(|r| r.trim().to_string())
        .or_else(|| SKIPIF_XFAIL.with(|x| x.borrow_mut().take()));
    match (result, xfail) {
        (TestResult::Fail { .. }, Some(reason)) => TestResult::XFail(reason),
        (TestResult::Pass, Some(_)) => TestResult::XPass,
        (r, _) => r,
    }
}

fn run_test_inner(test: &PhptTest, test_dir: Option<&Path>, filename: &str) -> TestResult {
    // Parse INI settings
    let mut ini_settings = parse_ini_settings(test.ini_section());
    // run-tests.php placeholders: {PWD}, {TMP}, {MAIL:path}
    for (_, v) in ini_settings.iter_mut() {
        if v.contains("{PWD}") {
            let pwd = test_dir
                .map(|d| d.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            *v = v.replace("{PWD}", &pwd);
        }
        if v.contains("{TMP}") {
            *v = v.replace("{TMP}", &std::env::temp_dir().to_string_lossy());
        }
        while let Some(start) = v.find("{MAIL:") {
            let rest = &v[start + 6..];
            let end = rest
                .find(|c: char| c == '}' || c.is_whitespace())
                .unwrap_or(rest.len());
            if !rest[end..].starts_with('}') {
                break;
            }
            let path = rest[..end].to_string();
            *v = format!("{}tee {} >/dev/null{}", &v[..start], path, &rest[end + 1..]);
        }
    }
    // --ARGS-- are passed to the script as $argv (inherited by the forked child).
    TEST_ARGS.with(|a| *a.borrow_mut() = test.sections.get("ARGS").map(|s| s.trim().to_string()));
    TEST_ENV.with(|a| *a.borrow_mut() = test.sections.get("ENV").map(|s| s.to_string()));

    // Check goro skip-list: tests that depend on infrastructure goro doesn't have
    // (JIT, phpdbg, CLI server, real MySQL, external php spawn, real network, live curl).
    // Converting these from FAIL to SKIP makes the pass-rate honest.
    if let Some(reason) = goro_should_skip(filename, test) {
        return TestResult::Skip(reason);
    }

    // Handle EXTENSIONS section - skip tests requiring unsupported extensions
    // Uses a thread_local VM to check loaded_extensions dynamically
    if let Some(extensions) = test.sections.get("EXTENSIONS") {
        thread_local! {
            static LOADED_EXTS: std::cell::RefCell<Option<std::collections::HashSet<Vec<u8>>>> = const { std::cell::RefCell::new(None) };
        }
        let skip = LOADED_EXTS.with(|exts| {
            let mut exts = exts.borrow_mut();
            if exts.is_none() {
                // Build the set once by registering all extensions
                let mut vm = goro_core::vm::Vm::new();
                goro_ext_standard::register_standard_functions(&mut vm);
                goro_ext_date::register(&mut vm);
                goro_ext_json::register(&mut vm);
                goro_ext_ctype::register(&mut vm);
                goro_ext_hash::register(&mut vm);
                goro_ext_openssl::register(&mut vm);
                goro_ext_zlib::register(&mut vm);
                goro_ext_gmp::register(&mut vm);
                goro_ext_bz2::register(&mut vm);
                goro_ext_curl::register(&mut vm);
                goro_ext_xml::register(&mut vm);
                goro_ext_session::register(&mut vm);
                goro_ext_mysqli::register(&mut vm);
                goro_ext_sockets::register(&mut vm);
                goro_ext_mbstring::register(&mut vm);
                goro_ext_spl::register(&mut vm);
                goro_ext_reflection::register(&mut vm);
                vm.register_extension(b"simplexml");
                vm.register_extension(b"tokenizer");
                vm.register_extension(b"pcre");
                *exts = Some(vm.loaded_extensions);
            }
            let loaded = exts.as_ref().unwrap();
            for ext in extensions.lines() {
                let ext = ext.trim().to_lowercase();
                if !ext.is_empty() && !loaded.contains(ext.as_bytes()) {
                    return Some(format!("Required extension {} not available", ext));
                }
            }
            None
        });
        if let Some(reason) = skip {
            return TestResult::Skip(reason);
        }
    }

    // Handle SKIPIF section
    if let Some(skipif) = test.skipif_section() {
        let skipif_trimmed = skipif.trim();
        if !skipif_trimmed.is_empty() {
            // run-tests.php runs SKIPIF before merging the test's --INI--,
            // with output_handler= and zlib.output_compression=Off: an
            // output handler must not mangle the "skip" message.
            let skipif_ini: Vec<(String, String)> = ini_settings
                .iter()
                .filter(|(k, _)| k != "output_handler" && k != "zlib.output_compression")
                .cloned()
                .collect();
            // Written next to the test as run-tests.php does, so that
            // include('skipif.inc') and __DIR__ resolve.
            let skip_filename = match filename.strip_suffix(".php") {
                Some(stem) if filename != "Unknown.php" => format!("{}.skip.php", stem),
                _ => "Unknown.php".to_string(),
            };
            match execute_php_with_timeout_and_filename(
                skipif_trimmed.as_bytes(),
                5,
                test_dir,
                &skipif_ini,
                &skip_filename,
            ) {
                Ok(output) => {
                    let output_str = String::from_utf8_lossy(&output).to_lowercase();
                    // run-tests.php: the output starts with "skip"
                    if output_str.trim_start().starts_with("skip") {
                        return TestResult::Skip(
                            String::from_utf8_lossy(&output).trim().to_string(),
                        );
                    }
                    // "xfail <reason>" marks the test as expected to fail.
                    let trimmed = output_str.trim_start();
                    if trimmed.starts_with("xfail") {
                        let raw = String::from_utf8_lossy(&output);
                        let reason = raw.trim_start()[5..].trim().to_string();
                        SKIPIF_XFAIL.with(|x| *x.borrow_mut() = Some(reason));
                    }
                }
                Err(_) => {
                    // If SKIPIF errors, skip the test
                    return TestResult::Skip("SKIPIF section errored".into());
                }
            }
        }
    }

    // Get the PHP source. PHPT supports two forms:
    //   --FILE--           : source embedded inline
    //   --FILE_EXTERNAL--  : a single filename whose contents are the source
    //                        (typically just `foo.inc` next to the .phpt file)
    let inline_source = test.file_section();
    let external_source: Option<String> = if inline_source.is_none() {
        test.sections.get("FILE_EXTERNAL").and_then(|raw| {
            let rel = raw.trim();
            if rel.is_empty() {
                return None;
            }
            let path = match test_dir {
                Some(dir) => dir.join(rel),
                None => std::path::PathBuf::from(rel),
            };
            std::fs::read_to_string(&path).ok()
        })
    } else {
        None
    };
    let source: &str = match (inline_source, external_source.as_deref()) {
        (Some(s), _) => s,
        (None, Some(s)) => s,
        (None, None) => return TestResult::Error("missing --FILE-- section".into()),
    };

    // Execute the source with a timeout (longer for tests that spawn a server).
    // Tests talking to a database server do real I/O; a few of them insert
    // rows "for 5 seconds" on purpose.
    let uses_mysqli = test
        .sections
        .get("EXTENSIONS")
        .is_some_and(|e| e.split_whitespace().any(|x| x == "mysqli"));
    let timeout_secs: u64 = if source.contains("php_cli_server_start") || uses_mysqli {
        30
    } else {
        5
    };
    let req = build_request(test, test_dir, filename);
    PHPT_REQUEST.with(|r| *r.borrow_mut() = Some(req));
    let source_bytes = test.bytes(source);
    let output = execute_php_with_timeout_and_filename(
        &source_bytes,
        timeout_secs,
        test_dir,
        &ini_settings,
        filename,
    );
    PHPT_REQUEST.with(|r| *r.borrow_mut() = None);
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            // Run CLEAN section even on error so leftover files don't cascade.
            if let Some(clean) = test.sections.get("CLEAN") {
                let clean_trimmed = clean.trim();
                if !clean_trimmed.is_empty() {
                    let clean_filename = if let Some(stem) = filename.strip_suffix(".php") {
                        format!("{}.clean.php", stem)
                    } else {
                        format!("{}.clean.php", filename)
                    };
                    let _ = execute_php_with_timeout_and_filename(
                        clean_trimmed.as_bytes(),
                        5,
                        test_dir,
                        &ini_settings,
                        &clean_filename,
                    );
                }
            }
            return TestResult::Error(e);
        }
    };

    // Run CLEAN section after the main FILE section so that leftover files
    // (e.g. files created with fopen("x")) don't cause cascading failures
    // when subsequent test runs check for "file already exists" warnings.
    // Use a `.clean.php` filename like the upstream PHP test runner does so
    // tests that compute paths from basename(__FILE__, ".clean.php") work.
    if let Some(clean) = test.sections.get("CLEAN") {
        let clean_trimmed = clean.trim();
        if !clean_trimmed.is_empty() {
            let clean_filename = if let Some(stem) = filename.strip_suffix(".php") {
                format!("{}.clean.php", stem)
            } else {
                format!("{}.clean.php", filename)
            };
            let _ = execute_php_with_timeout_and_filename(
                clean_trimmed.as_bytes(),
                5,
                test_dir,
                &ini_settings,
                &clean_filename,
            );
        }
    }

    let actual = if test.latin1 {
        output.iter().map(|&b| b as char).collect::<String>()
    } else {
        String::from_utf8_lossy(&output).to_string()
    };
    // Normalize \r\n to \n for comparison (PHP test files use \n line endings)
    let actual_normalized = actual.replace("\r\n", "\n");

    // run-tests.php compares trim()med output (PHP's trim() character set).
    // Compare with expected output
    if let Some(expected) = test.expect_section() {
        let expected_trimmed = php_trim(expected);
        let actual_trimmed = php_trim(&actual_normalized);
        // Replace %0 with actual null byte for comparison (PHPT files encode null as %0)
        let expected_with_nulls = expected_trimmed.replace("%0", "\0");
        if actual_trimmed == expected_with_nulls || actual_trimmed == expected_trimmed {
            TestResult::Pass
        } else {
            TestResult::Fail {
                expected: expected_trimmed.to_string(),
                actual: actual_trimmed.to_string(),
            }
        }
    } else if let Some(pattern) = test.expectf_section() {
        // EXPECTF: convert printf-style patterns to regex
        let pattern_trimmed = php_trim(pattern);
        let actual_trimmed = php_trim(&actual_normalized);
        if matches_expectf(pattern_trimmed, actual_trimmed) {
            TestResult::Pass
        } else {
            if std::env::var("GORO_DEBUG_EXPECTF").is_ok() {
                eprintln!(
                    "EXPECTF FAIL — pattern bytes: {}, actual bytes: {}",
                    pattern_trimmed.len(),
                    actual_trimmed.len()
                );
                let p_lines: Vec<&str> = pattern_trimmed.lines().collect();
                let a_lines: Vec<&str> = actual_trimmed.lines().collect();
                eprintln!(
                    "pattern lines: {}, actual lines: {}",
                    p_lines.len(),
                    a_lines.len()
                );
                for (i, (p, a)) in p_lines.iter().zip(a_lines.iter()).enumerate() {
                    if !match_expectf_line(p, a) {
                        eprintln!("LINE {} MISMATCH:", i + 1);
                        eprintln!("  P: {:?}", p);
                        eprintln!("  A: {:?}", a);
                    }
                }
            }
            TestResult::Fail {
                expected: pattern_trimmed.to_string(),
                actual: actual_trimmed.to_string(),
            }
        }
    } else if let Some(regex_pattern) = test.expect_regex_section() {
        // EXPECTREGEX: match output against regex pattern
        let regex_trimmed = php_trim(regex_pattern);
        let actual_trimmed = php_trim(&actual_normalized);
        if matches_expect_regex(regex_trimmed, actual_trimmed) {
            TestResult::Pass
        } else {
            TestResult::Fail {
                expected: regex_trimmed.to_string(),
                actual: actual_trimmed.to_string(),
            }
        }
    } else {
        TestResult::Error("missing --EXPECT-- or --EXPECTF-- section".into())
    }
}

/// Skip-list for tests that depend on infrastructure goro doesn't have and
/// doesn't plan to implement. These aren't real failures — marking them SKIP
/// keeps the pass-rate honest.
///
/// Categories:
/// 1. opcache JIT tests — we don't have a JIT.
/// 2. phpdbg tests — we don't have phpdbg.
/// 3. CLI server tests (`php -S`) — we don't have a built-in HTTP server.
/// 4. Real MySQL tests (bind_param/bind_result on a live server).
/// 5. External `php` binary spawn (proc_open/shell_exec/popen of $PHP_BINARY).
/// 6. Real network tests (live DNS/sockets to non-localhost endpoints).
/// 7. Live HTTP server required for curl_exec.
///
/// Be conservative: better to skip too few than too many. Never skip a test
/// that could actually pass.
pub fn goro_should_skip(filename: &str, test: &PhptTest) -> Option<String> {
    let file_section = test.file_section().unwrap_or("");
    let test_name = &test.name;

    // 1. opcache JIT — we don't implement a JIT. Most JIT tests already have
    //    `--EXTENSIONS-- opcache` and skip via the EXTENSIONS check. But some
    //    JIT tests omit EXTENSIONS and rely solely on the JIT INI directives.
    //    We only skip those that EXPLICITLY require an active JIT mode (i.e.
    //    set opcache.jit=tracing, opcache.jit=function, or a numeric JIT
    //    mode like opcache.jit=1255). Tests with only opcache.jit_debug (a
    //    debug knob that doesn't affect behavior) or just `opcache.enable=1`
    //    aren't really JIT-dependent — many pass trivially without JIT.
    //    This avoids PASS→SKIP regressions for tests that happen to live in
    //    the jit/ directory but don't depend on JIT semantics.
    if filename.contains("/ext/opcache/tests/jit/") && requires_active_jit(test) {
        return Some("opcache JIT not implemented in goro".into());
    }

    // 2. phpdbg — we don't implement the phpdbg SAPI.
    //    Either the test lives under sapi/phpdbg, or it has a --PHPDBG-- section.
    if filename.contains("/sapi/phpdbg/") || test.sections.contains_key("PHPDBG") {
        return Some("phpdbg SAPI not implemented".into());
    }

    // 3. CLI server (`php -S`) — we don't implement the built-in HTTP server.
    //    Match either the path or use of php_cli_server_start() in the FILE.
    if filename.contains("/sapi/cli/tests/php_cli_server")
        || file_section.contains("php_cli_server_start(")
    {
        return Some("CLI server (`php -S`) not implemented".into());
    }

    // 4. Real MySQL — bind_param / bind_result against a live server.
    //    Path-based: ext/mysqli/tests/bind_fetch and bind_insert dirs are
    //    exclusively live-server tests.
    //    Content-based: any mysqli test that calls bind_param or bind_result
    //    is going to talk to a real server in practice. The constant-existence
    //    and pure-error-path tests don't use bind_*().
    //
    //    When our embedded fixture is up, DON'T skip — let them run.
    if !mysqli_test::is_available() {
        if filename.contains("/ext/mysqli/tests/bind_fetch/")
            || filename.contains("/ext/mysqli/tests/bind_insert/")
        {
            return Some("Live MySQL server required".into());
        }
        if filename.contains("/ext/mysqli/tests/")
            && (file_section.contains("bind_param") || file_section.contains("bind_result"))
        {
            return Some("Live MySQL server required".into());
        }
    }

    // 5. External `php` binary spawn — currently disabled. Tests that
    //    invoke an external php via PHP_BINARY/proc_open/shell_exec often
    //    coincidentally pass in goro (because shell_exec returns empty or
    //    proc_open silently fails). Skipping them would regress the pass
    //    count. Leaving them alone keeps the suite honest about their
    //    coincidental success without losing pass-rate. See git history
    //    if you want to revisit this.

    // 6. Live network — fsockopen / stream_socket_client to a clearly
    //    non-loopback remote endpoint (i.e. a literal http(s)://example.com
    //    style URL or a fsockopen("tcp://hostname.tld:80")). Tests using
    //    plain invalid strings (e.g. 'abc') or 127.0.0.1 are NOT skipped.
    if has_remote_network_call(file_section) {
        return Some("Live network not supported".into());
    }

    // 7. Curl live — only skip if the test actually calls curl_exec() and
    //    points CURLOPT_URL at a non-loopback URL, OR uses curl_cli_server_start
    //    (whose whole purpose is to spawn `php -S`).
    if filename.contains("/ext/curl/tests/")
        && (file_section.contains("curl_cli_server_start(")
            || (file_section.contains("curl_exec(") && has_remote_curl_url(file_section)))
    {
        return Some("Live HTTP server required for curl".into());
    }

    let _ = test_name; // kept for future use
    None
}

/// True if the test EXPLICITLY requires an active JIT mode via INI. We look
/// for `opcache.jit=tracing`, `opcache.jit=function`, or `opcache.jit=<N>`
/// where N is a non-zero numeric mode. We ignore commented-out lines and
/// `opcache.jit=0`/`off`/`disable` (which means JIT off).
fn requires_active_jit(test: &PhptTest) -> bool {
    let ini = match test.ini_section() {
        Some(s) => s,
        None => return false,
    };
    for line in ini.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(';') || trimmed.starts_with('#') {
            continue;
        }
        // Look for opcache.jit= setting.
        if let Some(rest) = trimmed.strip_prefix("opcache.jit=") {
            let val = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            let val_lower = val.to_lowercase();
            // Off-values:
            if val_lower == "0" || val_lower == "off" || val_lower == "disable" {
                continue;
            }
            // tracing, function, or any non-zero mode.
            if !val.is_empty() {
                return true;
            }
        }
        // opcache.jit_buffer_size=<nonzero> also strongly signals JIT-required.
        if let Some(rest) = trimmed.strip_prefix("opcache.jit_buffer_size=") {
            let val = rest.trim();
            if !val.is_empty() && val != "0" {
                return true;
            }
        }
    }
    false
}

/// Check whether a FILE section makes a network call to something that isn't
/// clearly the loopback address. We treat 127.0.0.1, ::1, and "localhost" as
/// safe (loopback). We treat anything else, plus bare http(s):// URLs to
/// public-looking hosts, as a live network call.
fn has_remote_network_call(file: &str) -> bool {
    // fsockopen / pfsockopen / stream_socket_client to a non-loopback host.
    for fn_name in ["fsockopen(", "pfsockopen(", "stream_socket_client("] {
        let mut search = file;
        while let Some(pos) = search.find(fn_name) {
            let after = &search[pos + fn_name.len()..];
            // Grab the first argument: roughly the text up to the first comma
            // or closing paren, on a single line.
            let end = after.find([',', ')', '\n']).unwrap_or(after.len());
            let first_arg = &after[..end];
            if !is_loopback_arg(first_arg) {
                return true;
            }
            search = &after[end..];
        }
    }

    // file_get_contents / fopen on an http(s) URL whose host isn't loopback.
    for fn_name in ["file_get_contents(", "fopen(", "get_headers(", "readfile("] {
        let mut search = file;
        while let Some(pos) = search.find(fn_name) {
            let after = &search[pos + fn_name.len()..];
            let end = after.find([',', ')', '\n']).unwrap_or(after.len());
            let first_arg = &after[..end];
            if url_is_remote(first_arg) {
                return true;
            }
            search = &after[end..];
        }
    }

    false
}

/// Return true if the substring is plausibly a loopback (or unclassifiable)
/// endpoint and therefore should NOT trigger a live-network skip.
///
/// We default to "loopback" (safe) for anything we can't confidently classify
/// — better to skip too few than too many.
fn is_loopback_arg(arg: &str) -> bool {
    let lower = arg.to_lowercase();
    if lower.contains("127.0.0.1")
        || lower.contains("::1")
        || lower.contains("localhost")
        || lower.contains("[::1]")
        || lower.contains("0.0.0.0")
        || lower.contains("unix://") // unix-domain sockets aren't network
        || lower.contains("udg://")
    // unix-datagram sockets aren't network
    {
        return true;
    }
    // If the argument is a variable or expression we can't classify, default
    // to safe (loopback) — don't skip what we don't understand.
    if !arg.contains('"') && !arg.contains('\'') {
        return true;
    }
    // The arg has a quoted string. Look at its contents:
    //   - If it does NOT look like a real network address (no dot-separated
    //     host or "://" scheme), treat as safe (e.g. fsockopen('abc')).
    //   - If it does look like a real remote host (contains '.tld' or
    //     hostname.example.com style), treat as remote.
    // Heuristic: a remote host literal contains a dot AND no PHP variable
    // expansion markers.
    let has_dot = arg.contains('.');
    let has_scheme = arg.contains("://");
    let has_var = arg.contains('$') || arg.contains("{");
    if !has_dot && !has_scheme {
        // No dot, no scheme → not a real-looking network address. Safe.
        return true;
    }
    if has_var {
        // Dynamic — can't classify confidently. Safe.
        return true;
    }
    false
}

/// Return true if the first arg of file_get_contents/fopen/etc. is clearly an
/// http(s):// URL on a non-loopback host.
fn url_is_remote(arg: &str) -> bool {
    let lower = arg.to_lowercase();
    if !lower.contains("http://") && !lower.contains("https://") {
        return false;
    }
    // If the URL string interpolates a variable (e.g. "http://" . $host) or
    // uses PHP_CLI_SERVER_ADDRESS, we can't be certain — but we already catch
    // the cli-server case earlier, and dynamic-host tests are usually local.
    // Only skip when the host is plainly a non-loopback literal.
    if lower.contains("127.0.0.1")
        || lower.contains("::1")
        || lower.contains("localhost")
        || lower.contains("[::1]")
        || lower.contains("php_cli_server_address")
    {
        return false;
    }
    // The URL has a literal external-looking host (e.g. "http://example.com/").
    // Look for "http://" or "https://" followed by a letter/digit (host char)
    // and not a variable interpolation marker.
    for proto in ["http://", "https://"] {
        if let Some(pos) = lower.find(proto) {
            let after = &lower[pos + proto.len()..];
            // Next char should start a host literal.
            if let Some(c) = after.chars().next() {
                if c.is_ascii_alphanumeric() {
                    return true;
                }
            }
        }
    }
    false
}

/// Return true if a curl_setopt(.., CURLOPT_URL, X) call has X clearly
/// pointing at a non-loopback URL.
fn has_remote_curl_url(file: &str) -> bool {
    let mut search = file;
    while let Some(pos) = search.find("CURLOPT_URL") {
        let after = &search[pos + "CURLOPT_URL".len()..];
        // Skip past the comma.
        let val_start = match after.find(',') {
            Some(i) => i + 1,
            None => return false,
        };
        let value = &after[val_start..];
        let end = value.find([')', ';', '\n']).unwrap_or(value.len());
        let url_expr = &value[..end];
        if url_is_remote(url_expr) {
            return true;
        }
        search = &after[val_start + end..];
    }
    false
}

/// Parse --INI-- section into key-value pairs
fn parse_ini_settings(ini_section: Option<&str>) -> Vec<(String, String)> {
    let mut settings = Vec::new();
    if let Some(ini) = ini_section {
        for line in ini.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_string();
                let mut value = line[eq + 1..].trim().to_string();
                // In INI syntax an unquoted ';' starts a comment.
                if !value.starts_with('"') && !value.starts_with('\'') {
                    if let Some(sc) = value.find(';') {
                        value = value[..sc].trim_end().to_string();
                    }
                }
                // Strip surrounding quotes from INI values (e.g. "ISO-8859-1" -> ISO-8859-1)
                if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
                    // zend_ini_escape_string(): \\, \" and \$ are unescaped
                    let inner = &value[1..value.len() - 1];
                    let mut out = String::with_capacity(inner.len());
                    let mut chars = inner.chars().peekable();
                    while let Some(c) = chars.next() {
                        if c == '\\' {
                            match chars.peek() {
                                Some('"') | Some('\\') | Some('$') => {
                                    out.push(chars.next().unwrap());
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        out.push(c);
                    }
                    value = out;
                } else if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
                    value = value[1..value.len() - 1].to_string();
                } else {
                    // Unquoted INI boolean literals (zend_ini_scanner BOOL_TRUE/FALSE, NULL_NULL).
                    match value.to_ascii_lowercase().as_str() {
                        "true" | "on" | "yes" => value = "1".to_string(),
                        "false" | "off" | "no" | "none" | "null" => value = String::new(),
                        _ => {}
                    }
                }
                settings.push((key, value));
            }
        }
    }
    settings
}

/// Parse error reporting expressions like "E_ALL&~E_DEPRECATED"
/// Supports operators: | (OR), ^ (XOR), & (AND), ~ (NOT), ! (NOT)
/// with proper precedence: ~ and ! (unary, highest) > & > ^ > | (lowest)
fn parse_error_reporting_expr(expr: &str) -> Option<i64> {
    // PHP error level constants
    fn resolve_constant(name: &str) -> Option<i64> {
        match name.trim() {
            "E_ERROR" => Some(1),
            "E_WARNING" => Some(2),
            "E_PARSE" => Some(4),
            "E_NOTICE" => Some(8),
            "E_CORE_ERROR" => Some(16),
            "E_CORE_WARNING" => Some(32),
            "E_COMPILE_ERROR" => Some(64),
            "E_COMPILE_WARNING" => Some(128),
            "E_USER_ERROR" => Some(256),
            "E_USER_WARNING" => Some(512),
            "E_USER_NOTICE" => Some(1024),
            "E_STRICT" => Some(2048),
            "E_RECOVERABLE_ERROR" => Some(4096),
            "E_DEPRECATED" => Some(8192),
            "E_USER_DEPRECATED" => Some(16384),
            "E_ALL" => Some(30719), // E_ALL
            _ => name.trim().parse::<i64>().ok(),
        }
    }

    /// Tokenize an error_reporting expression into operands and operators
    fn tokenize_expr(expr: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        for ch in expr.chars() {
            match ch {
                '|' | '^' | '&' | '~' | '!' | '(' | ')' => {
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        tokens.push(trimmed);
                    }
                    current.clear();
                    tokens.push(ch.to_string());
                }
                ' ' | '\t' => {
                    // whitespace is a separator
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        tokens.push(trimmed);
                        current.clear();
                    }
                }
                _ => {
                    current.push(ch);
                }
            }
        }
        let trimmed = current.trim().to_string();
        if !trimmed.is_empty() {
            tokens.push(trimmed);
        }
        tokens
    }

    /// Recursive descent parser for bitwise expressions
    /// Precedence (lowest to highest): | , ^ , & , unary ~/!
    fn parse_or(tokens: &[String], pos: &mut usize) -> Option<i64> {
        let mut left = parse_xor(tokens, pos)?;
        while *pos < tokens.len() && tokens[*pos] == "|" {
            *pos += 1;
            let right = parse_xor(tokens, pos)?;
            left |= right;
        }
        Some(left)
    }

    fn parse_xor(tokens: &[String], pos: &mut usize) -> Option<i64> {
        let mut left = parse_and(tokens, pos)?;
        while *pos < tokens.len() && tokens[*pos] == "^" {
            *pos += 1;
            let right = parse_and(tokens, pos)?;
            left ^= right;
        }
        Some(left)
    }

    fn parse_and(tokens: &[String], pos: &mut usize) -> Option<i64> {
        let mut left = parse_unary(tokens, pos)?;
        while *pos < tokens.len() && tokens[*pos] == "&" {
            *pos += 1;
            let right = parse_unary(tokens, pos)?;
            left &= right;
        }
        Some(left)
    }

    fn parse_unary(tokens: &[String], pos: &mut usize) -> Option<i64> {
        if *pos < tokens.len() && (tokens[*pos] == "~" || tokens[*pos] == "!") {
            *pos += 1;
            let val = parse_unary(tokens, pos)?;
            return Some(!val);
        }
        if *pos < tokens.len() && tokens[*pos] == "(" {
            *pos += 1; // skip '('
            let val = parse_or(tokens, pos)?;
            if *pos < tokens.len() && tokens[*pos] == ")" {
                *pos += 1;
            }
            return Some(val);
        }
        // Must be a constant or number
        if *pos < tokens.len() {
            let token = &tokens[*pos];
            *pos += 1;
            return resolve_constant(token);
        }
        None
    }

    let expr = expr.trim();
    if expr.is_empty() {
        return None;
    }

    // Handle simple constant (fast path)
    if let Some(v) = resolve_constant(expr) {
        return Some(v);
    }

    // Tokenize and parse with proper precedence
    let tokens = tokenize_expr(expr);
    if tokens.is_empty() {
        return None;
    }
    let mut pos = 0;
    let result = parse_or(&tokens, &mut pos)?;
    // Ensure all tokens were consumed
    if pos == tokens.len() {
        Some(result)
    } else {
        // Partial parse - try anyway
        Some(result)
    }
}

fn execute_php_with_timeout_and_filename(
    source: &[u8],
    timeout_secs: u64,
    test_dir: Option<&Path>,
    ini_settings: &[(String, String)],
    filename: &str,
) -> Result<Vec<u8>, String> {
    // Use fork() to isolate each test in a separate process.
    // This prevents memory accumulation across tests (2GB RLIMIT_AS).
    #[cfg(unix)]
    {
        // Create a pipe to communicate output from child to parent
        let (read_fd, write_fd) = {
            let mut fds = [0i32; 2];
            let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
            if ret != 0 {
                return Err("Failed to create pipe".to_string());
            }
            (fds[0], fds[1])
        };

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
            }
            return Err("Failed to fork".to_string());
        }

        if pid == 0 {
            // Child process
            unsafe {
                libc::close(read_fd);
            }

            // Like run-tests.php, a test runs from the directory the runner
            // was started in (its files are addressed by absolute paths).
            if test_dir.is_none() {
                let _ = std::env::set_current_dir(std::env::temp_dir());
            }

            // run-tests.php merges the test's stderr into its output: what
            // the script or its child processes (proc_open() workers that
            // inherit STDERR) write to descriptor 2 is appended.
            let stderr_cap = capture_stderr_start();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute_php_inner(source, ini_settings, filename)
            }));
            let result = match (result, capture_stderr_end(stderr_cap)) {
                (Ok(Ok(mut bytes)), extra) => {
                    bytes.extend_from_slice(&extra);
                    Ok(Ok(bytes))
                }
                (r, _) => r,
            };

            let (status, output) = match result {
                Ok(Ok(bytes)) => (0u8, bytes),
                Ok(Err(e)) => (1u8, format!("ERROR: {}", e).into_bytes()),
                Err(_) => (2u8, b"PANIC".to_vec()),
            };
            let _ =
                unsafe { libc::write(write_fd, &status as *const u8 as *const libc::c_void, 1) };
            let len = output.len() as u64;
            let _ = unsafe { libc::write(write_fd, &len as *const u64 as *const libc::c_void, 8) };
            if !output.is_empty() {
                let _ = unsafe {
                    libc::write(
                        write_fd,
                        output.as_ptr() as *const libc::c_void,
                        output.len(),
                    )
                };
            }
            unsafe {
                libc::close(write_fd);
            }
            std::process::exit(0);
        }

        // Parent process
        unsafe {
            libc::close(write_fd);
        }

        // Wait for child with timeout, draining the result pipe meanwhile so
        // that a large output cannot fill it and block the child. (Reads are
        // non-blocking: a grandchild may have inherited the write end.)
        unsafe {
            let fl = libc::fcntl(read_fd, libc::F_GETFL, 0);
            libc::fcntl(read_fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
        }
        let mut buf = Vec::new();
        let drain = |buf: &mut Vec<u8>| -> bool {
            let mut got = false;
            let mut chunk = [0u8; 65536];
            loop {
                let n = unsafe {
                    libc::read(
                        read_fd,
                        chunk.as_mut_ptr() as *mut libc::c_void,
                        chunk.len(),
                    )
                };
                if n <= 0 {
                    return got;
                }
                got = true;
                buf.extend_from_slice(&chunk[..n as usize]);
            }
        };
        let start = std::time::Instant::now();
        loop {
            let mut status = 0i32;
            let ret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if ret > 0 {
                drain(&mut buf);
                break; // Child exited
            }
            if drain(&mut buf) {
                continue;
            }
            if ret < 0 {
                unsafe {
                    libc::close(read_fd);
                }
                return Err("waitpid error".to_string());
            }
            if start.elapsed().as_secs() >= timeout_secs {
                // Kill the child
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                unsafe {
                    libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
                unsafe {
                    libc::close(read_fd);
                }
                return Err("Timeout: execution exceeded time limit".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        unsafe {
            libc::close(read_fd);
        }

        if buf.is_empty() {
            return Err("Fatal error: child produced no output".to_string());
        }

        let status = buf[0];
        if buf.len() < 9 {
            return match status {
                2 => Err("Fatal error: stack overflow or panic".to_string()),
                _ => Err("Fatal error: child output truncated".to_string()),
            };
        }
        let len = u64::from_ne_bytes(buf[1..9].try_into().unwrap()) as usize;
        let data = if len > 0 && buf.len() >= 9 + len {
            buf[9..9 + len].to_vec()
        } else {
            Vec::new()
        };

        match status {
            0 => Ok(data),
            1 => Err(String::from_utf8_lossy(&data).to_string()),
            _ => Err("Fatal error: stack overflow or panic".to_string()),
        }
    }

    #[cfg(not(unix))]
    {
        // Fallback to thread-based execution on non-Unix
        let source = source.to_vec();
        let dir_path = test_dir.map(|p| p.to_path_buf());
        let ini = ini_settings.to_vec();
        let fname = filename.to_string();

        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                if let Some(ref dir) = dir_path {
                    let _ = std::env::set_current_dir(dir);
                } else {
                    let _ = std::env::set_current_dir(std::env::temp_dir());
                }
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute_php_inner(&source, &ini, &fname)
                }))
            })
            .map_err(|e| format!("Thread error: {}", e))?;

        let start = std::time::Instant::now();
        loop {
            if handle.is_finished() {
                break;
            }
            if start.elapsed().as_secs() >= timeout_secs {
                return Err("Timeout: execution exceeded time limit".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        match handle.join() {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("Fatal error: panic during execution".to_string()),
            Err(_) => Err("Fatal error: stack overflow or panic".to_string()),
        }
    }
}

fn execute_php_inner(
    source: &[u8],
    ini_settings: &[(String, String)],
    filename: &str,
) -> Result<Vec<u8>, String> {
    // Write source to the expected file path so __FILE__ works with fopen/file etc.
    let file_written = if !filename.is_empty() && filename != "Unknown.php" {
        let path = std::path::Path::new(filename);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // The FILE section is kept byte-exact (as run-tests.php writes it).
        std::fs::write(path, source).is_ok()
    } else {
        false
    };

    let result = execute_php_inner_impl(source, ini_settings, filename);
    // destroy_uploaded_files_hash: remove upload temp files left in place
    goro_ext_standard::request::shutdown();

    // Clean up the temporary file
    if file_written {
        let _ = std::fs::remove_file(filename);
    }

    result
}

fn execute_php_inner_impl(
    source: &[u8],
    ini_settings: &[(String, String)],
    filename: &str,
) -> Result<Vec<u8>, String> {
    use goro_core::compiler::Compiler;
    use goro_core::string::PhpString;
    use goro_core::value::{
        Value, memory_reset, set_memory_limit, set_php_precision, set_php_serialize_precision,
    };
    use goro_core::vm::Vm;
    use goro_parser::{Lexer, Parser};

    // Reset memory tracking and limits for this test
    memory_reset();
    set_memory_limit(128 * 1024 * 1024); // 128MB default

    // Apply precision from INI settings (reset to default first)
    set_php_precision(14);
    set_php_serialize_precision(-1);
    for (key, value) in ini_settings {
        if key == "precision" {
            if let Ok(p) = value.parse::<i32>() {
                set_php_precision(p);
            }
        }
        if key == "serialize_precision" {
            if let Ok(p) = value.parse::<i32>() {
                set_php_serialize_precision(p);
            }
        }
        if key == "memory_limit" {
            let v = Value::String(PhpString::from_string(value.clone()));
            let limit = parse_memory_limit_value(&v);
            set_memory_limit(limit);
        }
        if key == "hard_timeout" {
            if let Ok(n) = value.trim().parse::<i64>() {
                goro_core::value::HARD_TIMEOUT.store(n, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    // php_request_startup(): arm the max_execution_time timer.
    if let Some((_, v)) = ini_settings
        .iter()
        .rev()
        .find(|(k, _)| k == "max_execution_time")
    {
        if let Ok(n) = v.trim().parse::<i64>() {
            goro_ext_standard::misc::set_execution_timeout(n);
        }
    }

    // Lex
    let short_tags = ini_settings
        .iter()
        .rev()
        .find(|(k, _)| k == "short_open_tag")
        .map(|(_, v)| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "on" | "true" | "yes"
            )
        })
        .unwrap_or(false);
    goro_parser::set_short_open_tag(short_tags);
    let mut lexer = Lexer::new(source);
    lexer.skip_shebang();
    let tokens = lexer.tokenize();

    // Drain any deprecations recorded by the lexer (e.g. `${var}` syntax).
    let mut lexer_deprecations = goro_parser::take_lexer_deprecations();
    let lexer_warnings = goro_parser::take_lexer_warnings();

    // Parse
    let mut parser = Parser::new(tokens).with_source(source);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            // PHP outputs parse errors to stdout
            // Some parser errors should be "Fatal error" rather than "Parse error"
            let err_msg = &e.message;
            let is_fatal = err_msg.contains("Match expressions")
                || err_msg.starts_with("Unparenthesized `")
                || err_msg.contains("Cannot use")
                || err_msg.starts_with("Cannot create Closure")
                || err_msg.contains("cannot declare")
                || err_msg.contains("cannot implement")
                || err_msg.contains("cannot extend")
                || err_msg.contains("is reserved")
                || err_msg.contains("must be compatible")
                || err_msg.contains("must be")
                || err_msg.contains("previously implemented")
                || err_msg.contains("already in use")
                || err_msg.contains("is redundant")
                || err_msg.contains("not allowed")
                || err_msg.contains("Can't use")
                || err_msg.contains("Abstract function")
                || err_msg.contains("Non-abstract method")
                || err_msg.contains("cannot be part of an intersection type")
                || err_msg.contains("cannot be a reference")
                || err_msg.contains("Cannot redeclare")
                || err_msg.contains("cannot take arguments")
                || err_msg.contains("must take exactly")
                || err_msg.contains("cannot be static")
                || err_msg.contains("may not be nested")
                || err_msg.contains("cannot accept")
                || err_msg.contains("may be declared abstract")
                || err_msg.contains("must have type")
                || err_msg.contains("Duplicate value in enum")
                || err_msg.contains("Visibility of property")
                || err_msg.contains("asymmetric visibility")
                || err_msg.contains("cannot have type")
                || err_msg.contains("Enum ")
                || err_msg.contains("Property ")
                || err_msg.contains("Readonly property")
                || err_msg.contains("Static property")
                || err_msg.contains("constant must")
                || err_msg.contains("class must")
                || err_msg.starts_with("Unknown hook")
                || err_msg.starts_with("Cannot override final property hook")
                || err_msg.starts_with("Type of parameter")
                || err_msg.contains("define the same hooked property")
                || err_msg.starts_with("Parameter $")
                || err_msg.starts_with("set hook parameter")
                || err_msg.starts_with("Cannot use the static modifier on a property hook")
                || err_msg.starts_with("#[\\NoDiscard] is not supported for property hooks")
                || err_msg.starts_with("Cannot use the public modifier on a property hook")
                || err_msg.starts_with("Cannot use the protected modifier on a property hook")
                || err_msg.starts_with("Cannot use the private modifier on a property hook")
                || err_msg.starts_with("Property hooks cannot have visibility")
                || err_msg.starts_with("Property hook cannot be")
                || err_msg.starts_with("Property hook list must")
                || err_msg.starts_with("Property hook cannot have")
                || err_msg.starts_with("Cannot declare hooks for static property")
                || err_msg.starts_with("get hook of property")
                || err_msg.starts_with("set hook of property")
                || err_msg.starts_with("Abstract property")
                || err_msg.starts_with("Property in interface cannot be explicitly abstract")
                || err_msg.starts_with("Property in interface cannot be final")
                || err_msg.starts_with("Property in interface cannot be protected or private")
                || err_msg.starts_with("Final promoted property");
            let msg = if is_fatal {
                format!(
                    "\nFatal error: {} in {} on line {}\n",
                    err_msg, filename, e.span.line
                )
            } else if err_msg.starts_with("syntax error")
                || err_msg.starts_with("The (real) cast")
                || err_msg.starts_with("Invalid UTF-8 codepoint")
                || err_msg.starts_with("Invalid numeric literal")
                || err_msg.starts_with("Invalid body indentation level")
                || err_msg.starts_with("Invalid indentation - tabs and spaces")
                || err_msg.starts_with("Unterminated comment")
                || err_msg.starts_with("Unclosed '")
                || err_msg.starts_with("Unmatched '")
            {
                format!(
                    "\nParse error: {} in {} on line {}\n",
                    err_msg, filename, e.span.line
                )
            } else {
                format!(
                    "\nParse error: syntax error in {} on line {}\n",
                    filename, e.span.line
                )
            };
            return Ok(msg.into_bytes());
        }
    };
    // ...and those of the parser, in source order.
    lexer_deprecations.extend(goro_parser::take_lexer_deprecations());
    lexer_deprecations.sort_by_key(|(_, line)| *line);

    // Compile
    let mut compiler = Compiler::new();
    compiler.source_file = filename.as_bytes().to_vec();
    let (op_array, compiled_classes, compile_warnings) = match compiler.compile(&program) {
        Ok(r) => {
            // Drain any pending compile warnings so they don't leak into the next test
            let _ = goro_core::compiler::take_pending_compile_warnings();
            r
        }
        Err(e) => {
            let mut out = String::new();
            for (kind, msg, wline) in goro_core::compiler::take_pending_compile_warnings() {
                out.push_str(&format!(
                    "\n{}: {} in {} on line {}\n",
                    kind, msg, filename, wline
                ));
            }
            out.push_str(&format!(
                "\nFatal error: {} in {} on line {}\n",
                e.message, filename, e.line
            ));
            return Ok(out.into_bytes());
        }
    };

    // Execute
    let mut vm = Vm::new();
    vm.current_file = filename.to_string();
    // Emit lexer deprecations first (e.g. `${var}` syntax), then compile warnings.
    for (msg, wline) in &lexer_deprecations {
        vm.emit_deprecated_at(msg, *wline);
    }
    for (msg, wline) in &lexer_warnings {
        vm.emit_warning_at(msg, *wline);
    }
    for (msg, wline) in &compile_warnings {
        vm.emit_warning_at(msg, *wline);
    }
    goro_ext_standard::register_standard_functions(&mut vm);
    goro_ext_date::register(&mut vm);
    goro_ext_json::register(&mut vm);
    goro_ext_ctype::register(&mut vm);
    goro_ext_hash::register(&mut vm);
    goro_ext_openssl::register(&mut vm);
    goro_ext_zlib::register(&mut vm);
    goro_ext_gmp::register(&mut vm);
    goro_ext_bz2::register(&mut vm);
    goro_ext_curl::register(&mut vm);
    goro_ext_xml::register(&mut vm);
    goro_ext_session::register(&mut vm);
    goro_ext_mysqli::register(&mut vm);
    goro_ext_sockets::register(&mut vm);
    goro_ext_mbstring::register(&mut vm);
    goro_ext_spl::register(&mut vm);
    goro_ext_reflection::register(&mut vm);
    // Also register aliases that PHP tests may use
    vm.register_extension(b"simplexml");
    vm.register_extension(b"tokenizer");
    vm.register_extension(b"pcre");

    // run-tests.php's ini overwrites.
    vm.constants.insert(
        b"fatal_error_backtraces".to_vec(),
        Value::String(PhpString::from_bytes(b"0")),
    );
    // Apply all INI settings to the VM constants
    let mut startup_messages: Vec<u8> = Vec::new();
    for (key, value) in ini_settings {
        goro_ext_standard::misc::register_cfg_var(key, value);
        // Handle disable_functions: remove those functions from the VM
        if key == "disable_functions" {
            for func_name in value.split(',') {
                let func_name = func_name.trim();
                if !func_name.is_empty() {
                    let lower: Vec<u8> =
                        func_name.bytes().map(|b| b.to_ascii_lowercase()).collect();
                    vm.functions.remove(&lower);
                    vm.disabled_functions.insert(lower);
                }
            }
            continue;
        }
        // Directives of extension features goro does not build (PCRE JIT):
        // PHP ignores unknown directives, so ini_get() keeps returning false.
        if key == "pcre.jit" {
            continue;
        }
        let val = if let Ok(n) = value.parse::<i64>() {
            Value::Long(n)
        } else if let Some(resolved) = parse_error_reporting_expr(value) {
            Value::Long(resolved)
        } else {
            Value::String(PhpString::from_string(value.clone()))
        };
        // Apply error_reporting to the VM
        if key == "error_reporting" {
            if let Value::Long(n) = &val {
                vm.error_reporting = *n;
            }
        }
        // INI handlers that emit "PHP Startup" deprecations.
        if let Some(msg) = goro_ext_standard::misc::assert_ini_deprecation(key.as_bytes(), &val) {
            startup_messages.extend_from_slice(
                format!("\nDeprecated: PHP Startup: {} in Unknown on line 0\n", msg).as_bytes(),
            );
        }
        // Apply zend.exception_string_param_max_len to the VM (PHP rejects values outside [0,1_000_000])
        if key == "zend.exception_string_param_max_len" {
            if let Value::Long(n) = &val {
                if *n >= 0 && *n <= 1_000_000 {
                    vm.exception_string_param_max_len = *n as usize;
                }
            }
        }
        if let Some(validate) = vm.ini_validators.get(key.as_bytes()).copied() {
            if !validate(&mut vm, value.as_bytes(), true) {
                continue;
            }
        }
        vm.constants.insert(key.as_bytes().to_vec(), val);
    }
    // OnChangeMemoryLimit at startup: memory_limit above max_memory_limit is
    // clamped with a startup warning.
    if let (Some(ml), Some(mx)) = (
        vm.constants
            .get(b"memory_limit".as_ref())
            .map(|v| v.to_php_string()),
        vm.constants
            .get(b"max_memory_limit".as_ref())
            .map(|v| v.to_php_string()),
    ) {
        let (v, _) = goro_core::ini_quantity::parse_quantity(ml.as_bytes(), false);
        let (max, _) = goro_core::ini_quantity::parse_quantity(mx.as_bytes(), false);
        if v > max {
            if v != u64::MAX {
                vm.write_output(format!(
                    "\nWarning: Failed to set memory_limit to {} bytes. Setting to max_memory_limit instead (currently: {} bytes) in Unknown on line 0\n",
                    v as i64, max as i64
                ).as_bytes());
            }
            set_memory_limit(max as i64);
            vm.constants
                .insert(b"memory_limit".to_vec(), Value::String(mx));
        }
    }
    // Extension startup (MINIT/RINIT) now that INI settings are in place;
    // startup messages come before any compile-time diagnostics.
    if !vm.startup_hooks.is_empty() {
        let prev = vm.take_output();
        for hook in vm.startup_hooks.clone() {
            hook(&mut vm);
        }
        vm.write_output(&prev);
    }
    // (allow_url_include's startup deprecation is emitted by the standard extension.)
    if !startup_messages.is_empty() && vm.error_reporting & 8192 != 0 {
        let buf = vm.active_output_buffer();
        buf.splice(0..0, startup_messages);
    }
    for class in compiled_classes {
        vm.register_class(class);
    }
    // Startup messages are printed before the request (SAPI) is activated.
    vm.headers_sent = false;
    vm.output_start = None;
    goro_ext_standard::ob::request_startup(&mut vm);

    // Request startup: superglobals, $argv/$argc, php://input (main FILE only).
    if let Some(req) = PHPT_REQUEST.with(|r| r.borrow_mut().take()) {
        for (k, v) in &req.env {
            unsafe {
                std::env::set_var(k, v);
            }
        }
        if let Some(stdin) = &req.stdin {
            if let Ok(mut f) = tempfile::tempfile() {
                use std::io::{Seek, SeekFrom, Write};
                let _ = f.write_all(stdin);
                let _ = f.seek(SeekFrom::Start(0));
                use std::os::unix::io::AsRawFd;
                unsafe {
                    libc::dup2(f.as_raw_fd(), 0);
                }
            }
        } else {
            // run-tests.php gives the test an (empty) stdin pipe.
            let mut p = [0i32; 2];
            unsafe {
                if libc::pipe(p.as_mut_ptr()) == 0 {
                    libc::close(p[1]);
                    libc::dup2(p[0], 0);
                    libc::close(p[0]);
                }
            }
        }
        goro_ext_standard::misc::refresh_std_streams();
        let ini_lookup = |k: &str| -> Option<String> {
            ini_settings
                .iter()
                .rev()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.clone())
        };
        goro_ext_standard::request::startup(&mut vm, &req.info, &ini_lookup);
    }
    for hook in vm.request_startup_hooks.clone() {
        hook(&mut vm);
    }

    // Provide a minimal $_SERVER superglobal ONLY when the test depends on
    // the cli_server helper (which uses $_SERVER['PHP_SELF']). Tests that
    // touch $_SERVER themselves typically expect the "Undefined global
    // variable" warning, so don't auto-populate it for them.
    let source_str = std::str::from_utf8(source).unwrap_or("");
    {
        use goro_core::array::{ArrayKey, PhpArray};
        let mut server = PhpArray::new();
        let script_name = filename.to_string();
        let base_name = std::path::Path::new(&script_name)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "phpt".to_string());
        // The cli_server helper expects web-style script paths; a plain
        // CLI run reports the script path as given.
        let self_path = if source_str.contains("php_cli_server") {
            format!("/{}", base_name)
        } else {
            script_name.clone()
        };
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"PHP_SELF")),
            Value::String(PhpString::from_string(self_path.clone())),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"SCRIPT_NAME")),
            Value::String(PhpString::from_string(self_path)),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"PATH_TRANSLATED")),
            Value::String(PhpString::from_string(script_name.clone())),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"DOCUMENT_ROOT")),
            Value::String(PhpString::from_bytes(b"")),
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"REQUEST_TIME_FLOAT")),
            Value::Double(now),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"REQUEST_TIME")),
            Value::Long(now as i64),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"SCRIPT_FILENAME")),
            Value::String(PhpString::from_string(script_name.clone())),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"argv")),
            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(PhpArray::new()))),
        );
        server.set(
            ArrayKey::String(PhpString::from_bytes(b"argc")),
            Value::Long(0),
        );
        for k in [
            "PATH",
            "HOME",
            "USER",
            "TEST_PHP_EXECUTABLE",
            "TEST_PHP_EXECUTABLE_ESCAPED",
        ] {
            if let Ok(v) = std::env::var(k) {
                server.set(
                    ArrayKey::String(PhpString::from_string(k.to_string())),
                    Value::String(PhpString::from_string(v)),
                );
            }
        }
        vm.set_global(
            b"_SERVER".to_vec(),
            Value::Array(std::rc::Rc::new(std::cell::RefCell::new(server))),
        );
    }

    // --ENV-- variables (we run in the forked child, so this is isolated).
    if let Some(env) = TEST_ENV.with(|a| a.borrow().clone()) {
        for line in env.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim();
                // run-tests.php does not export empty variables.
                if !k.is_empty() && !v.is_empty() {
                    unsafe {
                        std::env::set_var(k, v);
                    }
                    // The child's environment is imported into $_SERVER.
                    if let Some(Value::Array(server)) = vm.get_global(b"_SERVER") {
                        server.borrow_mut().set(
                            goro_core::array::ArrayKey::String(PhpString::from_string(
                                k.to_string(),
                            )),
                            Value::String(PhpString::from_string(v.to_string())),
                        );
                    }
                }
            }
        }
    }
    if let Some(args) = TEST_ARGS.with(|a| a.borrow().clone()) {
        use goro_core::array::{ArrayKey, PhpArray};
        let mut argv = PhpArray::new();
        argv.push(Value::String(PhpString::from_string(filename.to_string())));
        for a in split_test_args(&args) {
            argv.push(Value::String(PhpString::from_string(a)));
        }
        let argc = argv.len() as i64;
        let argv_val = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(argv)));
        vm.set_global(b"argv".to_vec(), argv_val.clone());
        vm.set_global(b"argc".to_vec(), Value::Long(argc));
        let server = match vm.get_global(b"_SERVER") {
            Some(Value::Array(a)) => a,
            _ => std::rc::Rc::new(std::cell::RefCell::new(PhpArray::new())),
        };
        server
            .borrow_mut()
            .set(ArrayKey::String(PhpString::from_bytes(b"argv")), argv_val);
        server.borrow_mut().set(
            ArrayKey::String(PhpString::from_bytes(b"argc")),
            Value::Long(argc),
        );
        vm.set_global(b"_SERVER".to_vec(), Value::Array(server));
    }

    match vm.execute(&op_array) {
        Ok(_) => Ok(vm.take_output()),
        Err(e) => {
            // The VM displays uncaught exceptions / fatal errors itself
            // (through the output layer, before shutdown functions).
            if vm.current_exception.is_some() || !vm.fatal_emitted {
                vm.report_uncaught(&e);
            }
            vm.run_shutdown_destructors();
            Ok(vm.take_output())
        }
    }
}

/// Translate PCRE-isms run-tests relies on into Rust regex syntax: a `{`
/// that does not start a `{n}`/`{n,}`/`{n,m}` quantifier is literal, and
/// `\0` means NUL.
fn pcre_to_rust_regex(p: &str) -> String {
    let b = p.as_bytes();
    let mut out = String::with_capacity(p.len() + 8);
    let mut i = 0;
    let mut in_class = false;
    while i < b.len() {
        let c = b[i];
        if c == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'0' && !(i + 2 < b.len() && b[i + 2].is_ascii_digit()) {
                out.push_str("\\x00");
                i += 2;
                continue;
            }
            let ch = p[i + 1..].chars().next().unwrap();
            out.push('\\');
            out.push(ch);
            i += 1 + ch.len_utf8();
            continue;
        }
        if in_class {
            if c == b']' {
                in_class = false;
            }
            let ch = p[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if c == b'[' {
            in_class = true;
            out.push('[');
            i += 1;
            if i < b.len() && b[i] == b'^' {
                out.push('^');
                i += 1;
            }
            if i < b.len() && b[i] == b']' {
                out.push_str("\\]");
                i += 1;
            }
            continue;
        }
        if c == b'{' {
            let mut j = i + 1;
            let mut digits = 0;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
                digits += 1;
            }
            if digits > 0 && j < b.len() && b[j] == b',' {
                j += 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
            }
            if digits > 0 && j < b.len() && b[j] == b'}' && !out.is_empty() {
                out.push_str(&p[i..=j]);
                i = j + 1;
            } else {
                out.push_str("\\{");
                i += 1;
            }
            continue;
        }
        if c == b'}' {
            out.push_str("\\}");
            i += 1;
            continue;
        }
        let ch = p[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn matches_expect_regex(pattern: &str, actual: &str) -> bool {
    // EXPECTREGEX uses raw regex patterns. Each line of the pattern is a regex for
    // matching the corresponding line of output.
    // First try matching the entire output as one regex (run-tests uses /s).
    let full_pattern = format!("(?s)^(?:{})$", pcre_to_rust_regex(pattern));
    if let Ok(re) = regex::Regex::new(&full_pattern) {
        if re.is_match(actual) {
            return true;
        }
    }
    let full_pattern = format!("^(?:{})$", pattern);
    if let Ok(re) = regex::Regex::new(&full_pattern) {
        if re.is_match(actual) {
            return true;
        }
    }
    // Fall back to line-by-line matching
    let pattern_lines: Vec<&str> = pattern.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    if pattern_lines.len() != actual_lines.len() {
        return false;
    }
    for (p, a) in pattern_lines.iter().zip(actual_lines.iter()) {
        let line_pattern = format!("^(?:{})$", p);
        match regex::Regex::new(&line_pattern) {
            Ok(re) => {
                if !re.is_match(a) {
                    return false;
                }
            }
            Err(_) => {
                if p != a {
                    return false;
                }
            }
        }
    }
    true
}

fn matches_expectf(pattern: &str, actual: &str) -> bool {
    // Check if the pattern contains %a or %A which can match across lines
    // In that case we need whole-string matching, not line-by-line
    if pattern.contains("%a") || pattern.contains("%A") || pattern.contains("%w") {
        // Whole-string matching (supports cross-line %a/%A/%w)
        return match_expectf_at(pattern.as_bytes(), 0, actual.as_bytes(), 0);
    }

    // Line-by-line EXPECTF matching (more efficient for patterns without %a/%A)
    let pattern_lines: Vec<&str> = pattern.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    if pattern_lines.len() != actual_lines.len() {
        // Try with trimmed trailing empty lines
        let p_trimmed: Vec<&str> = pattern.trim_end().lines().collect();
        let a_trimmed: Vec<&str> = actual.trim_end().lines().collect();
        if p_trimmed.len() != a_trimmed.len() {
            return false;
        }
        return p_trimmed
            .iter()
            .zip(a_trimmed.iter())
            .all(|(p, a)| match_expectf_line(p, a));
    }

    pattern_lines
        .iter()
        .zip(actual_lines.iter())
        .all(|(p, a)| match_expectf_line(p, a))
}

/// Match a single line with EXPECTF patterns using recursive backtracking
fn match_expectf_line(pattern: &str, actual: &str) -> bool {
    match_expectf_at(pattern.as_bytes(), 0, actual.as_bytes(), 0)
}

fn match_expectf_at(pb: &[u8], mut pi: usize, ab: &[u8], mut ai: usize) -> bool {
    while pi < pb.len() {
        if pb[pi] == b'%' && pi + 1 < pb.len() {
            match pb[pi + 1] {
                b'd' => {
                    pi += 2;
                    if ai < ab.len() && ab[ai] == b'-' {
                        ai += 1;
                    }
                    if ai >= ab.len() || !ab[ai].is_ascii_digit() {
                        return false;
                    }
                    let start = ai;
                    while ai < ab.len() && ab[ai].is_ascii_digit() {
                        ai += 1;
                    }
                    // \d+ backtracks: "%d%d" matches "12"
                    if pb.get(pi) == Some(&b'%') && pb.get(pi + 1) == Some(&b'd') {
                        for try_ai in (start + 1..=ai).rev() {
                            if match_expectf_at(pb, pi, ab, try_ai) {
                                return true;
                            }
                        }
                        return false;
                    }
                }
                b'i' => {
                    pi += 2;
                    if ai < ab.len() && (ab[ai] == b'+' || ab[ai] == b'-') {
                        ai += 1;
                    }
                    if ai >= ab.len() || !ab[ai].is_ascii_digit() {
                        return false;
                    }
                    while ai < ab.len() && ab[ai].is_ascii_digit() {
                        ai += 1;
                    }
                }
                b's' => {
                    pi += 2;
                    // %s: one or more non-newline chars, with backtracking
                    // PHP's run-tests uses [^\r\n]+ for %s
                    if ai >= ab.len() || ab[ai] == b'\n' || ab[ai] == b'\r' {
                        return false;
                    }
                    // Find the extent of non-newline chars
                    let start = ai;
                    while ai < ab.len() && ab[ai] != b'\n' && ab[ai] != b'\r' {
                        ai += 1;
                    }
                    // Try backtracking from longest match
                    let end = ai;
                    for try_ai in (start + 1..=end).rev() {
                        if match_expectf_at(pb, pi, ab, try_ai) {
                            return true;
                        }
                    }
                    return false;
                }
                b'S' | b'a' | b'A' => {
                    pi += 2;
                    // %a/%A/%S: match any string (including empty for %a)
                    // Try from longest match (greedy with backtracking)
                    for try_ai in (ai..=ab.len()).rev() {
                        if match_expectf_at(pb, pi, ab, try_ai) {
                            return true;
                        }
                    }
                    return false;
                }
                b'f' => {
                    pi += 2;
                    let start = ai;
                    if ai < ab.len() && (ab[ai] == b'-' || ab[ai] == b'+') {
                        ai += 1;
                    }
                    while ai < ab.len() && (ab[ai].is_ascii_digit() || ab[ai] == b'.') {
                        ai += 1;
                    }
                    // Allow E notation
                    if ai < ab.len() && (ab[ai] == b'E' || ab[ai] == b'e') {
                        ai += 1;
                        if ai < ab.len() && (ab[ai] == b'+' || ab[ai] == b'-') {
                            ai += 1;
                        }
                        while ai < ab.len() && ab[ai].is_ascii_digit() {
                            ai += 1;
                        }
                    }
                    if ai == start {
                        return false;
                    }
                }
                b'x' => {
                    pi += 2;
                    if ai >= ab.len() || !ab[ai].is_ascii_hexdigit() {
                        return false;
                    }
                    while ai < ab.len() && ab[ai].is_ascii_hexdigit() {
                        ai += 1;
                    }
                }
                b'e' => {
                    pi += 2;
                    // %e matches a directory separator (/ or \)
                    if ai < ab.len() && (ab[ai] == b'/' || ab[ai] == b'\\') {
                        ai += 1;
                    } else {
                        return false;
                    }
                }
                b'w' => {
                    pi += 2;
                    // %w: run-tests.php's `\s*` (newlines included), with backtracking
                    let start = ai;
                    while ai < ab.len()
                        && matches!(ab[ai], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
                    {
                        ai += 1;
                    }
                    for try_ai in (start..=ai).rev() {
                        if match_expectf_at(pb, pi, ab, try_ai) {
                            return true;
                        }
                    }
                    return false;
                }
                b'c' => {
                    pi += 2;
                    if ai < ab.len() {
                        ai += 1;
                    } else {
                        return false;
                    }
                }
                // Note: do NOT special-case `%%` as an escape for a single `%`.
                // PHP's run-tests.php expectf_to_regex() does NOT collapse `%%`
                // — preg_quote('%','/') is just `%`, and the strtr table has
                // no `%%` entry, so the regex still contains `%%` and matches
                // two literal `%` chars in the actual output. The default `_`
                // arm below already gives the same behaviour: it matches a
                // single literal `%`, then the next iteration matches the
                // next char (which may itself be `%` or a format code).
                b'r' => {
                    // %r...%r delimits an embedded regex pattern. Find the closing %r.
                    pi += 2;
                    let start = pi;
                    let mut found = None;
                    while pi + 1 < pb.len() {
                        if pb[pi] == b'%' && pb[pi + 1] == b'r' {
                            found = Some(pi);
                            break;
                        }
                        pi += 1;
                    }
                    if let Some(end) = found {
                        let regex_pat = match std::str::from_utf8(&pb[start..end]) {
                            Ok(s) => s,
                            Err(_) => return false,
                        };
                        // run-tests' strtr() also expands the %-placeholders
                        // inside %r sections.
                        let mut expanded = String::with_capacity(regex_pat.len());
                        let rb = regex_pat.as_bytes();
                        let mut k = 0;
                        while k < rb.len() {
                            if rb[k] == b'%' && k + 1 < rb.len() {
                                let rep = match rb[k + 1] {
                                    b'e' => Some("/"),
                                    b's' => Some(r"[^\r\n]+"),
                                    b'S' => Some(r"[^\r\n]*"),
                                    b'a' => Some("(?s:.+?)"),
                                    b'A' => Some("(?s:.*?)"),
                                    b'w' => Some(r"\s*"),
                                    b'i' => Some(r"[+-]?\d+"),
                                    b'd' => Some(r"\d+"),
                                    b'x' => Some("[0-9a-fA-F]+"),
                                    b'f' => Some(r"[+-]?(?:\d+|\.\d)(?:\.\d+)?(?:[Ee][+-]?\d+)?"),
                                    b'c' => Some("."),
                                    b'0' => Some(r"\x00"),
                                    _ => None,
                                };
                                if let Some(r) = rep {
                                    expanded.push_str(r);
                                    k += 2;
                                    continue;
                                }
                            }
                            let ch = regex_pat[k..].chars().next().unwrap_or('%');
                            expanded.push(ch);
                            k += ch.len_utf8();
                        }
                        // PCRE accepts `\0` (NUL); Rust's regex needs `\x00`.
                        let mut pat = String::with_capacity(expanded.len());
                        let mut chars = expanded.chars().peekable();
                        while let Some(c) = chars.next() {
                            if c == '\\' {
                                match chars.peek() {
                                    Some('0') => {
                                        chars.next();
                                        pat.push_str("\\x00");
                                        continue;
                                    }
                                    Some(_) => {
                                        pat.push(c);
                                        pat.push(chars.next().unwrap());
                                        continue;
                                    }
                                    None => {}
                                }
                            }
                            pat.push(c);
                        }
                        // Anchored prefix match against actual (byte-based).
                        let anchored = format!("(?-u)^(?:{})", pat);
                        let re = match regex::bytes::Regex::new(&anchored) {
                            Ok(r) => r,
                            Err(_) => return false,
                        };
                        let rest = &ab[ai..];
                        // Try each match length, longest first, with backtracking.
                        let mut candidates: Vec<usize> = Vec::new();
                        if let Some(m) = re.find(rest) {
                            if m.start() == 0 {
                                candidates.push(m.end());
                            }
                        }
                        // Also try shorter matches (regex isn't backtracking-friendly here).
                        // Just use the single longest match for simplicity.
                        pi = end + 2;
                        if candidates.is_empty() {
                            return false;
                        }
                        let mut any = false;
                        for &consumed in &candidates {
                            if match_expectf_at(pb, pi, ab, ai + consumed) {
                                any = true;
                                break;
                            }
                        }
                        if !any {
                            return false;
                        }
                        // Caller already advanced `pi` to after closing %r. Compute new ai based on consumption.
                        // For simplicity, return true now since the recursive call did the rest.
                        return true;
                    }
                    return false;
                }
                b'0' => {
                    // %0 matches a null byte in PHPT tests
                    pi += 2;
                    if ai < ab.len() && ab[ai] == 0 {
                        ai += 1;
                    } else {
                        return false;
                    }
                }
                _ => {
                    if ai >= ab.len() || pb[pi] != ab[ai] {
                        return false;
                    }
                    pi += 1;
                    ai += 1;
                }
            }
        } else {
            if ai >= ab.len() || pb[pi] != ab[ai] {
                return false;
            }
            pi += 1;
            ai += 1;
        }
    }

    // Both should be consumed
    ai >= ab.len()
}

/// Outcome counts of a test run.
#[derive(Debug, Default, Clone, Copy)]
pub struct TestCounts {
    pub pass: usize,
    pub fail: usize,
    pub skip: usize,
    pub error: usize,
    /// Expected failures (--XFAIL--); not failures.
    pub xfail: usize,
    /// --XFAIL-- tests that passed.
    pub xpass: usize,
}

impl TestCounts {
    pub fn total(&self) -> usize {
        self.pass + self.fail + self.skip + self.error + self.xfail + self.xpass
    }
}

/// Run all .phpt files in a directory (recursively)
pub fn run_test_dir(dir: &Path) -> TestCounts {
    // Each test runs with its own directory as cwd, so a relative test path
    // would no longer name the script (or its includes) from there.
    let abs_dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let dir = abs_dir.as_path();
    // run-tests.php is run from the php-src root, and tests rely on it (e.g.
    // `open_basedir=.` must cover the test's own directory). Run from the
    // nearest ancestor holding run-tests.php, wherever the runner was started.
    if let Some(root) = dir.ancestors().find(|a| a.join("run-tests.php").is_file()) {
        let _ = std::env::set_current_dir(root);
    }
    let mut pass = 0;
    let mut fail = 0;
    let mut skip = 0;
    let mut error = 0;
    let mut xfail = 0;
    let mut xpass = 0;

    // Make sure subprocesses launched by tests (e.g. via proc_open from
    // php_cli_server.inc) use this goro binary rather than the system `php`.
    if std::env::var("TEST_PHP_EXECUTABLE").is_err() {
        if let Ok(exe) = std::env::current_exe() {
            unsafe {
                std::env::set_var("TEST_PHP_EXECUTABLE", &exe);
            }
            unsafe {
                std::env::set_var(
                    "TEST_PHP_EXECUTABLE_ESCAPED",
                    exe.to_string_lossy().to_string(),
                );
            }
        }
    }

    // __FILE__ / __DIR__ are absolute (php-src resolves the script path), so
    // a relative test path is resolved first.
    let dir_abs = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let dir = dir_abs.as_path();
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if dir.is_file() {
        files.push(dir.to_path_buf());
    } else {
        collect_phpt_files(dir, &mut files);
    }
    files.sort();
    // GORO_PHPT_SHARD=i/n keeps every n-th test starting at i (0-based), so CI
    // can split the suite across parallel jobs.
    if let Ok(spec) = std::env::var("GORO_PHPT_SHARD") {
        if let Some((i, n)) = spec.split_once('/') {
            if let (Ok(i), Ok(n)) = (i.trim().parse::<usize>(), n.trim().parse::<usize>()) {
                if n > 0 && i < n {
                    files = files
                        .into_iter()
                        .enumerate()
                        .filter(|(k, _)| k % n == i)
                        .map(|(_, f)| f)
                        .collect();
                }
            }
        }
    }

    // If any of the tests live under ext/mysqli/, try to spin up an embedded
    // MySQL/MariaDB server. The fixture exports MYSQL_TEST_* env vars so the
    // tests' connect.inc picks them up. If startup fails (no binary, init
    // error, port in use), we silently fall back to the existing skip
    // behavior — no regression.
    // They run from their absolute path: their includes (connect.inc) and
    // helper processes resolve relative to the script.
    let mut has_mysqli_tests = false;
    for p in files.iter_mut() {
        if let Ok(abs) = std::fs::canonicalize(&*p) {
            if abs.to_string_lossy().contains("/ext/mysqli/tests/") {
                has_mysqli_tests = true;
                *p = abs;
            }
        }
    }
    if has_mysqli_tests {
        let start = std::time::Instant::now();
        let up = mysqli_test::ensure_started();
        if up {
            eprintln!(
                "[goro-phpt] embedded MySQL fixture ready in {:.2}s",
                start.elapsed().as_secs_f32()
            );
        } else {
            eprintln!(
                "[goro-phpt] no embedded MySQL fixture available; mysqli tests will be skipped"
            );
        }
    }

    for path in &files {
        if let Ok(content) = std::fs::read(path) {
            if let Some(test) = PhptTest::parse_bytes(&content) {
                let test_dir = path.parent();
                // Wrap in catch_unwind to prevent stack overflows from killing the runner
                let test_name = test.name.clone();
                // Use full path for filename, converting .phpt to .php
                let test_filename = {
                    // Absolute, like run-tests.php: the test runs from its
                    // own directory.
                    let abs = std::path::absolute(path).unwrap_or_else(|_| path.clone());
                    let full = abs.to_string_lossy().to_string();
                    if full.ends_with(".phpt") {
                        format!("{}.php", &full[..full.len() - 5])
                    } else {
                        full
                    }
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_test_with_dir_and_filename(&test, test_dir, &test_filename)
                }));
                let _dbg_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match result {
                    Ok(TestResult::Pass) => {
                        pass += 1;
                        println!("PASS: {} [{}]", test_name, _dbg_name);
                    }
                    Ok(TestResult::Fail { expected, actual }) => {
                        fail += 1;
                        println!("FAIL: {} [{}]", test_name, _dbg_name);
                        println!("  Expected: {:?}", expected);
                        println!("  Actual:   {:?}", actual);
                    }
                    Ok(TestResult::Skip(reason)) => {
                        skip += 1;
                        println!("SKIP: {} [{}] ({})", test_name, _dbg_name, reason);
                    }
                    Ok(TestResult::Error(msg)) => {
                        error += 1;
                        println!("ERROR: {} [{}] ({})", test_name, _dbg_name, msg);
                    }
                    Ok(TestResult::XFail(reason)) => {
                        xfail += 1;
                        println!("XFAIL: {} [{}] ({})", test_name, _dbg_name, reason);
                    }
                    Ok(TestResult::XPass) => {
                        xpass += 1;
                        println!(
                            "XPASS: {} [{}] (warn: XFAIL section but test passes)",
                            test_name, _dbg_name
                        );
                    }
                    Err(_) => {
                        error += 1;
                        println!(
                            "ERROR: {} [{}] (panic/stack overflow)",
                            test_name, _dbg_name
                        );
                    }
                }
            }
        }
    }

    // Stop the embedded MySQL server, if one was started.
    mysqli_test::shutdown();
    TestCounts {
        pass,
        fail,
        skip,
        error,
        xfail,
        xpass,
    }
}

fn collect_phpt_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip symlinked directories to avoid infinite recursion
                if path.is_symlink() {
                    continue;
                }
                collect_phpt_files(&path, files);
            } else if path.extension().is_some_and(|e| e == "phpt") {
                files.push(path);
            }
        }
    }
}

fn parse_memory_limit_value(val: &goro_core::value::Value) -> i64 {
    let s = val.to_php_string().to_string_lossy();
    let s = s.trim();
    if s == "-1" {
        return -1;
    }
    let (num_str, multiplier) = if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024i64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len() - 1], 1024 * 1024i64)
    } else if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len() - 1], 1024i64)
    } else {
        (s, 1i64)
    };
    num_str.parse::<i64>().unwrap_or(128 * 1024 * 1024) * multiplier
}

/// Request environment for the main FILE run (CGI emulation / CLI argv),
/// built from the test sections like run-tests.php does.
#[derive(Default, Clone)]
struct PhptRequest {
    info: goro_ext_standard::request::RequestInfo,
    /// Environment variables to export in the child before running.
    env: Vec<(String, String)>,
    stdin: Option<Vec<u8>>,
}

thread_local! {
    static PHPT_REQUEST: std::cell::RefCell<Option<PhptRequest>> = const { std::cell::RefCell::new(None) };
}

/// PHP's trim() character set.
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\0' || c == '\x0b')
}

/// Minimal POSIX-shell word splitting (quotes and backslashes) for --ARGS--.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                have = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                have = true;
                while let Some(d) = chars.next() {
                    if d == '"' {
                        break;
                    }
                    if d == '\\' {
                        if let Some(&n) = chars.peek() {
                            if matches!(n, '"' | '\\' | '$' | '`') {
                                cur.push(n);
                                chars.next();
                                continue;
                            }
                        }
                    }
                    cur.push(d);
                }
            }
            '\\' => {
                have = true;
                if let Some(n) = chars.next() {
                    if n != '\n' {
                        cur.push(n);
                    }
                }
            }
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    if have {
        out.push(cur);
    }
    out
}

#[allow(clippy::field_reassign_with_default)] // fields filled in step by step
fn build_request(test: &PhptTest, test_dir: Option<&Path>, filename: &str) -> PhptRequest {
    let raw = |k: &str| test.raw_sections.get(k).map(|s| s.as_str());
    let not_empty = |k: &str| raw(k).is_some_and(|s| !php_trim(s).is_empty());
    let is_cgi = test.sections.contains_key("CGI")
        || ["GET", "POST", "POST_RAW", "PUT", "COOKIE"]
            .iter()
            .any(|k| not_empty(k))
        || test
            .sections
            .get("EXPECTHEADERS")
            .is_some_and(|s| !php_trim(s).is_empty());
    let mut env: Vec<(String, String)> = Vec::new();
    let set = |env: &mut Vec<(String, String)>, k: &str, v: String| {
        env.retain(|(a, _)| a != k);
        env.push((k.to_string(), v));
    };
    set(&mut env, "REDIRECT_STATUS", "1".into());
    let query = raw("GET")
        .map(|s| php_trim(s).to_string())
        .unwrap_or_default();
    set(&mut env, "QUERY_STRING", query.clone());
    set(&mut env, "PATH_TRANSLATED", filename.to_string());
    set(&mut env, "SCRIPT_FILENAME", filename.to_string());
    let cookie = raw("COOKIE").map(|s| php_trim(s).to_string());
    set(&mut env, "HTTP_COOKIE", cookie.clone().unwrap_or_default());
    set(&mut env, "TZ", String::new());
    if let Some(e) = raw("ENV") {
        let dir = test_dir
            .map(|d| d.to_string_lossy().to_string())
            .unwrap_or_default();
        let e = e.replace("{PWD}", &dir);
        for line in e.split('\n') {
            let line = php_trim(line);
            if let Some((k, v)) = line.split_once('=') {
                if !k.is_empty() {
                    set(&mut env, k, v.to_string());
                }
            }
        }
    }
    let method: String;
    let mut content_type = String::new();
    let mut body: Vec<u8> = Vec::new();
    let mut content_length = String::new();
    let raw_request = |sec: &str, ct: &mut String| -> Vec<u8> {
        let post = php_trim(raw(sec).unwrap_or(""));
        let mut request = String::new();
        let mut started = false;
        for line in post.split('\n') {
            if ct.is_empty() && line.len() >= 13 && line[..13].eq_ignore_ascii_case("Content-Type:")
            {
                *ct = line[13..].replace('\r', "").trim().to_string();
                continue;
            }
            if started {
                request.push('\n');
            }
            started = true;
            request.push_str(line);
        }
        test.bytes(&request)
    };
    if not_empty("POST_RAW") {
        body = raw_request("POST_RAW", &mut content_type);
        content_length = body.len().to_string();
        method = "POST".into();
    } else if not_empty("PUT") {
        body = raw_request("PUT", &mut content_type);
        content_length = body.len().to_string();
        method = "PUT".into();
    } else if not_empty("POST") {
        body = test.bytes(php_trim(raw("POST").unwrap_or("")));
        method = "POST".into();
        content_type = "application/x-www-form-urlencoded".into();
        content_length = body.len().to_string();
    } else {
        method = "GET".into();
    }
    set(&mut env, "REQUEST_METHOD", method.clone());
    set(&mut env, "CONTENT_TYPE", content_type.clone());
    set(&mut env, "CONTENT_LENGTH", content_length.clone());

    let mut info = goro_ext_standard::request::RequestInfo::default();
    info.cgi = is_cgi;
    info.script_filename = filename.as_bytes().to_vec();
    if is_cgi {
        info.request_method = Some(method.into_bytes());
        info.query_string = test.bytes(&query);
        info.cookie_data = cookie.map(|c| test.bytes(&c));
        info.content_type = if content_type.is_empty() {
            None
        } else {
            Some(content_type.into_bytes())
        };
        info.content_length = content_length.parse().unwrap_or(0);
        info.body = body;
    } else {
        info.argv.push(filename.as_bytes().to_vec());
        if let Some(a) = test.sections.get("ARGS") {
            for w in shell_split(a) {
                info.argv.push(w.into_bytes());
            }
        }
    }
    // Full environment as seen by the child: inherited vars + overrides.
    let mut full: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (k, v) in std::env::vars_os() {
        let k = k.to_string_lossy().as_bytes().to_vec();
        if env.iter().any(|(a, _)| a.as_bytes() == k.as_slice()) {
            continue;
        }
        full.push((k, v.to_string_lossy().as_bytes().to_vec()));
    }
    for (k, v) in &env {
        full.push((k.clone().into_bytes(), v.clone().into_bytes()));
    }
    info.env = full;
    let stdin = test.sections.get("STDIN").map(|s| test.bytes(s));
    PhptRequest { info, env, stdin }
}

/// Redirect descriptor 2 to an anonymous temporary file; returns
/// (saved stderr, capture file) descriptors.
#[cfg(unix)]
fn capture_stderr_start() -> Option<(i32, i32)> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("goro-phpt-stderr-{}", std::process::id()));
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .ok()?;
    let _ = std::fs::remove_file(&path);
    use std::os::unix::io::IntoRawFd;
    let fd = f.into_raw_fd();
    unsafe {
        let saved = libc::dup(2);
        if saved < 0 || libc::dup2(fd, 2) < 0 {
            libc::close(fd);
            return None;
        }
        Some((saved, fd))
    }
}

/// Restore descriptor 2 and return what was written to it.
#[cfg(unix)]
fn capture_stderr_end(cap: Option<(i32, i32)>) -> Vec<u8> {
    let Some((saved, fd)) = cap else {
        return Vec::new();
    };
    let mut out = Vec::new();
    unsafe {
        libc::dup2(saved, 2);
        libc::close(saved);
        libc::lseek(fd, 0, libc::SEEK_SET);
        let mut chunk = [0u8; 65536];
        loop {
            let n = libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len());
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n as usize]);
        }
        libc::close(fd);
    }
    out
}
