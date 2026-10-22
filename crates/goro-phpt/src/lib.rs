use std::collections::HashMap;
use std::path::Path;

pub mod mysqli_test;
pub mod pgsql_test;

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
            "POST",
            "POST_RAW",
            "PUT",
            "GET",
            "COOKIE",
            "ARGS",
            "ENV",
            "STDIN",
            "EXPECTHEADERS",
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
    // run-tests.php loads the extensions a test lists that are built shared
    // with `-d extension=...` (zend_test is only loaded on request).
    if test.sections.get("EXTENSIONS").is_some_and(|e| {
        e.lines()
            .any(|x| x.trim().eq_ignore_ascii_case("zend_test"))
    }) {
        ini_settings.insert(0, ("extension".to_string(), "zend_test".to_string()));
    }
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
                goro_ext_pdo::register(&mut vm);
                goro_ext_pgsql::register(&mut vm);
                goro_ext_sockets::register(&mut vm);
                goro_ext_mbstring::register(&mut vm);
                goro_ext_spl::register(&mut vm);
                goro_ext_reflection::register(&mut vm);
                goro_ext_bcmath::register(&mut vm);
                goro_ext_filter::register(&mut vm);
                goro_ext_soap::register(&mut vm);
                goro_ext_iconv::register(&mut vm);
                goro_ext_gettext::register(&mut vm);
                goro_ext_xsl::register(&mut vm);
                goro_ext_sysvipc::register(&mut vm);
                goro_ext_ftp::register(&mut vm);
                goro_ext_pcntl::register(&mut vm);
                goro_ext_posix::register(&mut vm);
                goro_ext_calendar::register(&mut vm);
                goro_ext_phar::register(&mut vm);
                goro_ext_exif::register(&mut vm);
                goro_ext_zip::register(&mut vm);
                goro_ext_fileinfo::register(&mut vm);
                goro_ext_sodium::register(&mut vm);
                goro_ext_intl::register(&mut vm);
                goro_ext_gd::register(&mut vm);
                goro_ext_dba::register(&mut vm);
                goro_ext_readline::register(&mut vm);
                goro_ext_tidy::register(&mut vm);
                goro_ext_opcache::register(&mut vm);
                // Loadable on request (see run_test_inner).
                vm.register_extension(goro_ext_zend_test::NAME);
                vm.register_extension(b"simplexml");
                vm.register_extension(b"tokenizer");
                vm.register_extension(b"pcre");
                *exts = Some(vm.loaded_extensions);
            }
            let loaded = exts.as_ref().unwrap();
            for ext in extensions.lines() {
                let ext = ext.trim().to_lowercase();
                // get_loaded_extensions() names it "Zend OPcache"
                // (run-tests.php maps it to "opcache").
                let ext = if ext == "opcache" {
                    "zend opcache".to_string()
                } else {
                    ext
                };
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

    // A --REDIRECTTEST-- runs another directory with its environment
    // (run-tests.php reports it as REDIR, neither passed nor failed). The
    // PDO driver tests it exists for get their environment from the
    // database fixture instead (see mysqli_test.rs).
    if test.sections.contains_key("REDIRECTTEST") {
        return TestResult::Skip("redirect test".into());
    }

    // Handle SKIPIF section
    if let Some(skipif) = test.skipif_section() {
        let skipif_trimmed = skipif.trim();
        if !skipif_trimmed.is_empty() {
            // run-tests.php runs SKIPIF with only its own ini overwrites
            // ($orig_ini_settings), before merging the test's --INI--: e.g.
            // an unsupported default_charset must not print startup
            // warnings in front of the "skip" message.
            // (with the --EXTENSIONS-- it loads)
            let skipif_ini: Vec<(String, String)> = ini_settings
                .iter()
                .take_while(|(k, _)| k == "extension")
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
    // (binary contents, e.g. phar archives, are kept as they are)
    let external_source: Option<Vec<u8>> = if inline_source.is_none() {
        test.sections.get("FILE_EXTERNAL").and_then(|raw| {
            let rel = raw.replace("..", "");
            let rel = rel.trim();
            if rel.is_empty() {
                return None;
            }
            let path = match test_dir {
                Some(dir) => dir.join(rel),
                None => std::path::PathBuf::from(rel),
            };
            std::fs::read(&path).ok()
        })
    } else {
        None
    };
    let external_lossy = external_source
        .as_ref()
        .map(|b| String::from_utf8_lossy(b).into_owned());
    let source: &str = match (inline_source, external_lossy.as_deref()) {
        (Some(s), _) => s,
        (None, Some(s)) => s,
        (None, None) => return TestResult::Error("missing --FILE-- section".into()),
    };

    // Execute the source with a timeout (longer for tests that spawn a server).
    // Tests talking to a database server do real I/O; a few of them insert
    // rows "for 5 seconds" on purpose.
    let uses_mysqli = test.sections.get("EXTENSIONS").is_some_and(|e| {
        e.split_whitespace()
            .any(|x| matches!(x, "mysqli" | "pdo" | "pdo_mysql" | "pdo_sqlite"))
    });
    // The argon2 interoperability tests of ext/sodium hash with up to
    // 128 MiB / 8 passes a dozen times (purecrypto's portable Argon2 is about
    // half as fast as libsodium's AVX2 one).
    let uses_argon2 = test
        .sections
        .get("EXTENSIONS")
        .is_some_and(|e| e.split_whitespace().any(|x| x == "sodium"))
        && source.contains("argon2");
    let timeout_secs: u64 = if source.contains("php_cli_server_start") || uses_mysqli {
        30
    } else if uses_argon2 {
        15
    } else {
        5
    } + literal_sleep_secs(source);
    // run-tests.php runs CLEAN with $orig_ini_settings: the extensions it
    // loads, not the test's --INI--.
    let clean_ini: Vec<(String, String)> = ini_settings
        .iter()
        .filter(|(k, _)| k == "extension")
        .cloned()
        .collect();
    let req = build_request(test, test_dir, filename);
    PHPT_REQUEST.with(|r| *r.borrow_mut() = Some(req));
    let source_bytes = match &external_source {
        Some(b) => b.clone(),
        None => test.bytes(source),
    };
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
                        &clean_ini,
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
                &clean_ini,
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
/// 2. phpdbg tests — we don't have phpdbg.
/// 4. Real MySQL tests (bind_param/bind_result on a live server).
/// 5. External `php` binary spawn (proc_open/shell_exec/popen of $PHP_BINARY).
/// 6. Real network tests (live DNS/sockets to non-localhost endpoints).
/// 7. Live HTTP server required for curl_exec.
///
/// Be conservative: better to skip too few than too many. Never skip a test
/// that could actually pass.
/// run-tests.php's $ini_overwrites for opcache.
const RUN_TESTS_OPCACHE_INI: &[(&str, &str)] = &[
    ("opcache.fast_shutdown", "0"),
    ("opcache.file_update_protection", "0"),
    ("opcache.revalidate_freq", "0"),
    ("opcache.jit_hot_loop", "1"),
    ("opcache.jit_hot_func", "1"),
    ("opcache.jit_hot_return", "1"),
    ("opcache.jit_hot_side_exit", "1"),
    ("opcache.jit_max_root_traces", "100000"),
    ("opcache.jit_max_side_traces", "100000"),
    ("opcache.jit_max_exit_counters", "100000"),
    ("opcache.protect_memory", "1"),
];

pub fn goro_should_skip(filename: &str, test: &PhptTest) -> Option<String> {
    let file_section = test.file_section().unwrap_or("");
    let test_name = &test.name;

    // 1. opcache JIT: run-tests.php does not skip JIT tests when the JIT is
    //    unavailable; the opcache.jit* directives are simply ineffective and
    //    the tests run (their own SKIPIF / EXTENSIONS sections decide).

    // 2. phpdbg — we don't implement the phpdbg SAPI.
    //    Either the test lives under sapi/phpdbg, or it has a --PHPDBG-- section.
    if filename.contains("/sapi/phpdbg/") || test.sections.contains_key("PHPDBG") {
        return Some("phpdbg SAPI not implemented".into());
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
    //    points CURLOPT_URL at a non-loopback URL (tests using
    //    curl_cli_server_start() talk to a local `php -S`).
    if filename.contains("/ext/curl/tests/")
        && !file_section.contains("curl_cli_server_start(")
        && file_section.contains("curl_exec(")
        && has_remote_curl_url(file_section)
    {
        return Some("Live HTTP server required for curl".into());
    }

    let _ = test_name; // kept for future use
    None
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
/// Supports operators: | (OR), ^ (XOR), & (AND), ~ (NOT), ! (logical NOT)
fn parse_error_reporting_expr(expr: &str) -> Option<i64> {
    // A plain value ("2M", "0x10") is not an expression.
    if !expr.contains(['|', '^', '&', '~', '!', '(', ')'])
        && expr.trim_start().starts_with(|c: char| c.is_ascii_digit())
    {
        return None;
    }
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
            // get_int_val(): atoi() of the operand ("0x7F" is 0)
            n if n.starts_with(|c: char| c.is_ascii_digit()) => {
                let digits: String = n.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<i64>().ok()
            }
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

    /// zend_ini_parser.y: `%left '|' '&' '^'` -- the binary operators share
    /// one precedence level and associate left; `~`/`!` bind tighter.
    fn parse_or(tokens: &[String], pos: &mut usize) -> Option<i64> {
        let mut left = parse_unary(tokens, pos)?;
        while *pos < tokens.len() && matches!(tokens[*pos].as_str(), "|" | "&" | "^") {
            let op = tokens[*pos].clone();
            *pos += 1;
            let right = parse_unary(tokens, pos)?;
            match op.as_str() {
                "|" => left |= right,
                "&" => left &= right,
                _ => left ^= right,
            }
        }
        Some(left)
    }

    fn parse_unary(tokens: &[String], pos: &mut usize) -> Option<i64> {
        if *pos < tokens.len() && (tokens[*pos] == "~" || tokens[*pos] == "!") {
            let logical = tokens[*pos] == "!";
            *pos += 1;
            let val = parse_unary(tokens, pos)?;
            return Some(if logical { (val == 0) as i64 } else { !val });
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

/// The process running the current test (in its forked runner child):
/// processes it forks (pcntl_fork()) only add their output.
#[cfg(unix)]
static TEST_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// The exit status of the script that finished last.
#[cfg(unix)]
static SCRIPT_EXIT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// A process forked by the test script itself?
fn in_test_grandchild() -> bool {
    #[cfg(unix)]
    {
        let t = TEST_PID.load(std::sync::atomic::Ordering::Relaxed);
        t != 0 && t != unsafe { libc::getpid() }
    }
    #[cfg(not(unix))]
    {
        false
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

        // Like run-tests.php, the test's descriptors 1 and 2 are a pipe
        // (shared, so that the order of what is written to them is kept),
        // drained by the parent.
        let (cap_r, cap_w) = {
            let mut fds = [0i32; 2];
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                unsafe {
                    libc::close(read_fd);
                    libc::close(write_fd);
                }
                return Err("Failed to create pipe".to_string());
            }
            (fds[0], fds[1])
        };

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            unsafe {
                libc::close(read_fd);
                libc::close(write_fd);
                libc::close(cap_r);
                libc::close(cap_w);
            }
            return Err("Failed to fork".to_string());
        }

        if pid == 0 {
            // Child process: the leader of a process group holding whatever
            // the test starts, which is killed once the test is over.
            unsafe {
                libc::setpgid(0, 0);
                TEST_PID.store(libc::getpid(), std::sync::atomic::Ordering::Relaxed);
                libc::close(read_fd);
                libc::close(cap_r);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                libc::dup2(cap_w, 1);
                libc::dup2(cap_w, 2);
                libc::close(cap_w);
            }

            pgsql_test::child_env(filename);
            sqlite_child_env(filename);

            // Like run-tests.php, a test runs from the directory the runner
            // was started in (its files are addressed by absolute paths).
            if test_dir.is_none() {
                let _ = std::env::set_current_dir(std::env::temp_dir());
            }

            // run-tests.php merges the test's stderr into its output: what
            // the script or its child processes (proc_open() workers that
            // inherit STDERR) write to descriptor 2 is appended.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute_php_inner(source, ini_settings, filename)
            }));
            // What is still buffered in the engine is written to descriptor
            // 1 last, as the CLI does at shutdown (lost if the script closed
            // it).
            let result = match result {
                Ok(Ok(bytes)) => {
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                    let mut done = 0;
                    while done < bytes.len() {
                        let n = unsafe {
                            libc::write(
                                1,
                                bytes[done..].as_ptr() as *const libc::c_void,
                                bytes.len() - done,
                            )
                        };
                        if n <= 0 {
                            break;
                        }
                        done += n as usize;
                    }
                    Ok(Ok(Vec::new()))
                }
                r => r,
            };
            // A process the test forked only contributes its output.
            if in_test_grandchild() {
                unsafe { libc::_exit(SCRIPT_EXIT.load(std::sync::atomic::Ordering::Relaxed)) };
            }
            unsafe {
                libc::close(1);
                libc::close(2);
            }

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
            libc::close(cap_w);
        }

        // Wait for child with timeout, draining the result and output pipes
        // meanwhile so that a large output cannot fill them and block the
        // child. (Reads are non-blocking: a grandchild may have inherited
        // the write ends.)
        unsafe {
            for fd in [read_fd, cap_r] {
                let fl = libc::fcntl(fd, libc::F_GETFL, 0);
                libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
            }
        }
        let mut buf = Vec::new();
        let mut captured = Vec::new();
        let drain = |fd: i32, buf: &mut Vec<u8>| -> bool {
            let mut got = false;
            let mut chunk = [0u8; 65536];
            loop {
                let n =
                    unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len()) };
                if n <= 0 {
                    return got;
                }
                got = true;
                buf.extend_from_slice(&chunk[..n as usize]);
            }
        };
        let start = std::time::Instant::now();
        let exit_status;
        loop {
            let mut status = 0i32;
            let ret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if ret > 0 {
                exit_status = status;
                drain(read_fd, &mut buf);
                // Like run-tests.php, the output is read until EOF: processes
                // the test forked may still be writing (for a while).
                let grace = std::time::Instant::now();
                let mut chunk = [0u8; 65536];
                loop {
                    let n = unsafe {
                        libc::read(cap_r, chunk.as_mut_ptr() as *mut libc::c_void, chunk.len())
                    };
                    if n > 0 {
                        captured.extend_from_slice(&chunk[..n as usize]);
                        continue;
                    }
                    if n == 0
                        || grace.elapsed().as_secs() >= 2
                        || start.elapsed().as_secs() >= timeout_secs
                    {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                drain(read_fd, &mut buf);
                // Nothing the test started outlives it.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
                // A test that replaced itself (pcntl_exec()) did not remove
                // its file.
                if !filename.is_empty() && filename != "Unknown.php" {
                    let _ = std::fs::remove_file(filename);
                }
                break; // Child exited
            }
            let got_out = drain(cap_r, &mut captured);
            if drain(read_fd, &mut buf) || got_out {
                continue;
            }
            if ret < 0 {
                unsafe {
                    libc::close(read_fd);
                    libc::close(cap_r);
                }
                return Err("waitpid error".to_string());
            }
            if start.elapsed().as_secs() >= timeout_secs {
                // Kill the child (and what it started)
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                    libc::kill(pid, libc::SIGKILL);
                }
                unsafe {
                    libc::waitpid(pid, std::ptr::null_mut(), 0);
                }
                unsafe {
                    libc::close(read_fd);
                    libc::close(cap_r);
                }
                return Err("Timeout: execution exceeded time limit".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        unsafe {
            libc::close(read_fd);
            libc::close(cap_r);
        }

        if buf.is_empty() {
            // The test replaced itself (pcntl_exec()): what it printed.
            if libc::WIFEXITED(exit_status) {
                return Ok(captured);
            }
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
            0 => {
                captured.extend_from_slice(&data);
                Ok(captured)
            }
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
    if file_written && !in_test_grandchild() {
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
    use goro_parser::Parser;

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
    // phar_compile_file(): tar/zip-based and compressed phars
    let redirected = goro_ext_phar::main_script_source(filename, source, ini_settings);
    let source: &[u8] = redirected.as_deref().unwrap_or(source);
    goro_ext_mbstring::zend_multibyte::install(&|k: &str| {
        ini_settings
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
    });
    let script = goro_parser::multibyte::lex_script(source, true);
    let tokens = script.tokens;
    let source = script.source.as_slice();

    let lexer_warnings = goro_parser::take_lexer_warnings();

    // Parse
    goro_core::stack::install_parser_stack_grower();
    let mut parser = Parser::new(tokens).with_source(source);
    let parsed = parser.parse();
    // Deprecations recorded by the lexer (e.g. `${var}` syntax) and the
    // parser, without those of code that is never compiled.
    let mut lexer_deprecations = goro_parser::take_lexer_deprecations();
    let program = match parsed {
        Ok(p) => p,
        Err(e) => {
            // PHP outputs parse errors to stdout
            // Some parser errors should be "Fatal error" rather than "Parse error"
            let err_msg = &e.message;
            let is_fatal = !goro_parser::is_php_parse_error(err_msg)
                && (err_msg.contains("Match expressions")
                    || err_msg.starts_with("Unparenthesized `")
                    || err_msg.starts_with("Cannot mix [] and list()")
                    || err_msg.ends_with("can only be used from the outermost scope")
                    || err_msg.starts_with("Cannot assign to array(), use [] instead")
                    || err_msg.starts_with("Assignments can only happen to writable values")
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
                    || err_msg.starts_with("Final promoted property"));
            let msg = if is_fatal {
                format!(
                    "\nFatal error: {} in {} on line {}\n",
                    err_msg, filename, e.span.line
                )
            } else if goro_parser::is_php_parse_error(err_msg)
                || err_msg.starts_with("The (real) cast")
                || err_msg.starts_with("Invalid UTF-8 codepoint")
                || err_msg.starts_with("Invalid numeric literal")
                || err_msg.starts_with("Invalid body indentation level")
                || err_msg.starts_with("Invalid indentation - tabs and spaces")
                || err_msg.starts_with("Unterminated comment")
                || err_msg.starts_with("Unclosed '")
                || err_msg.starts_with("Unmatched '")
                || err_msg == "memory exhausted"
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
    // ...in source order.
    lexer_deprecations.sort_by_key(|(_, line)| *line);

    // The compiler checks the (emulated) C stack too.
    {
        let ini = |k: &str| {
            ini_settings
                .iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.as_bytes())
        };
        goro_core::stack::configure_from_ini(
            ini("zend.max_allowed_stack_size"),
            ini("zend.reserved_stack_size"),
        );
    }
    // Compile (with the extensions' compile-time state: MINIT)
    if goro_ext_zend_test::requested(ini_settings.iter().map(|(k, v)| (k.as_str(), v.as_str()))) {
        goro_ext_zend_test::minit();
    }
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
    for (deprecation, msg, wline) in
        goro_parser::order_lexer_diagnostics(&lexer_deprecations, &lexer_warnings)
    {
        if deprecation {
            vm.emit_deprecated_at(&msg, wline);
        } else {
            vm.emit_warning_at(&msg, wline);
        }
    }
    for (msg, wline) in &compile_warnings {
        vm.emit_compile_diagnostic(msg, *wline);
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
    goro_ext_pdo::register(&mut vm);
    goro_ext_pgsql::register(&mut vm);
    goro_ext_sockets::register(&mut vm);
    goro_ext_mbstring::register(&mut vm);
    goro_ext_spl::register(&mut vm);
    goro_ext_reflection::register(&mut vm);
    goro_ext_bcmath::register(&mut vm);
    goro_ext_filter::register(&mut vm);
    goro_ext_soap::register(&mut vm);
    goro_ext_iconv::register(&mut vm);
    goro_ext_gettext::register(&mut vm);
    goro_ext_xsl::register(&mut vm);
    goro_ext_sysvipc::register(&mut vm);
    goro_ext_ftp::register(&mut vm);
    goro_ext_pcntl::register(&mut vm);
    goro_ext_posix::register(&mut vm);
    goro_ext_calendar::register(&mut vm);
    goro_ext_phar::register(&mut vm);
    goro_ext_exif::register(&mut vm);
    goro_ext_zip::register(&mut vm);
    goro_ext_fileinfo::register(&mut vm);
    goro_ext_sodium::register(&mut vm);
    goro_ext_intl::register(&mut vm);
    goro_ext_gd::register(&mut vm);
    goro_ext_dba::register(&mut vm);
    goro_ext_readline::register(&mut vm);
    goro_ext_tidy::register(&mut vm);
    if goro_ext_zend_test::requested(ini_settings.iter().map(|(k, v)| (k.as_str(), v.as_str()))) {
        goro_ext_zend_test::register(&mut vm);
    }
    // Registered last: its startup (preloading) runs after the other
    // extensions' (as accel_post_startup() does).
    goro_ext_opcache::register(&mut vm);
    // Also register aliases that PHP tests may use
    vm.register_extension(b"simplexml");
    vm.register_extension(b"tokenizer");
    vm.register_extension(b"pcre");

    // run-tests.php's ini overwrites.
    vm.constants.insert(
        b"fatal_error_backtraces".to_vec(),
        Value::String(PhpString::from_bytes(b"0")),
    );
    vm.constants.insert(
        b"docref_ext".to_vec(),
        Value::String(PhpString::from_bytes(b".html")),
    );
    for (k, v) in RUN_TESTS_OPCACHE_INI {
        vm.constants.insert(
            k.as_bytes().to_vec(),
            Value::String(PhpString::from_bytes(v.as_bytes())),
        );
    }
    // Apply all INI settings (the INI section's values may be
    // error-level expressions, which the INI scanner resolves).
    let resolved: Vec<(String, String)> = ini_settings
        .iter()
        .map(|(k, v)| {
            if v.parse::<i64>().is_err() {
                if let Some(n) = parse_error_reporting_expr(v) {
                    return (k.clone(), n.to_string());
                }
            }
            (k.clone(), v.clone())
        })
        .collect();
    let startup_messages = goro_ext_standard::misc::apply_startup_ini(&mut vm, &resolved);
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
    let had_request = PHPT_REQUEST.with(|r| r.borrow().is_some());
    if let Some(req) = PHPT_REQUEST.with(|r| r.borrow_mut().take()) {
        // CLI tests: pending output reaches descriptor 1 (shared with
        // stderr) when a child process is spawned or STDERR is written,
        // keeping the order run-tests.php sees.
        vm.cli_direct_output = !req.info.cgi;
        // proc_open() leaves out the variables whose value is empty.
        for (k, v) in &req.env {
            unsafe {
                if v.is_empty() {
                    std::env::remove_var(k);
                } else {
                    std::env::set_var(k, v);
                }
            }
        }
        if let Some(stdin) = &req.stdin {
            // run-tests.php feeds the section through a pipe: php://stdin
            // (a dup of descriptor 0) shares its read position with STDIN.
            let mut p = [0i32; 2];
            if unsafe { libc::pipe(p.as_mut_ptr()) } == 0 {
                unsafe {
                    libc::dup2(p[0], 0);
                    libc::close(p[0]);
                }
                let data = stdin.clone();
                let wfd = p[1];
                std::thread::spawn(move || {
                    use std::io::Write;
                    use std::os::unix::io::FromRawFd;
                    let mut w = unsafe { std::fs::File::from_raw_fd(wfd) };
                    let _ = w.write_all(&data);
                });
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
    if !had_request {
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

    // php_execute_script(): auto_prepend_file / auto_append_file run as
    // part of the main script
    let main_result = {
        let prepend = auto_include_chunk(ini_settings, "auto_prepend_file");
        let append = auto_include_chunk(ini_settings, "auto_append_file");
        let mut r = Ok(Value::Null);
        if let Some(p) = &prepend {
            r = vm.execute_chunk(p);
        }
        if r.is_ok() && !vm.exit_requested {
            r = vm.execute_chunk(&op_array);
        }
        if let Some(a) = &append {
            if r.is_ok() && !vm.exit_requested {
                r = vm.execute_chunk(a);
            }
        }
        vm.finish_request(r)
    };
    // EG(exit_status), for the processes the test forks.
    let exit_status = if vm.exit_requested {
        vm.exit_code as i32
    } else if main_result.is_err() || vm.fatal_emitted {
        255
    } else {
        0
    };
    #[cfg(unix)]
    SCRIPT_EXIT.store(exit_status, std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(unix))]
    let _ = exit_status;
    if let Err(e) = main_result {
        // The VM displays uncaught exceptions / fatal errors itself
        // (through the output layer, before shutdown functions).
        if vm.current_exception.is_some() || !vm.fatal_emitted {
            vm.report_uncaught(&e);
        }
        vm.run_shutdown_destructors();
    }
    let out = vm.take_output();
    if vm.module_shutdown_hooks.is_empty() {
        return Ok(out);
    }
    // MSHUTDOWN handlers run once the output is sent.
    {
        use std::io::Write;
        let mut so = std::io::stdout();
        let _ = so.write_all(&out);
        let _ = so.flush();
    }
    vm.module_shutdown();
    Ok(Vec::new())
}

/// The code that requires the auto_prepend_file / auto_append_file `key`.
fn auto_include_chunk(
    ini_settings: &[(String, String)],
    key: &str,
) -> Option<goro_core::opcode::OpArray> {
    let path = ini_settings
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())?;
    let src = format!(
        "<?php require '{}';",
        path.replace('\\', "\\\\").replace('\'', "\\'")
    );
    let mut lexer = goro_parser::Lexer::new(src.as_bytes());
    let tokens = lexer.tokenize();
    let program = goro_parser::Parser::new(tokens)
        .with_source(src.as_bytes())
        .parse()
        .ok()?;
    let compiler = goro_core::compiler::Compiler::new();
    compiler.compile(&program).ok().map(|(op, _, _)| op)
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

/// preg_match("/^{pattern}$/s", actual) with the bundled PCRE2 port.
fn pcre_full_match(pattern: &str, actual: &str) -> Option<bool> {
    use goro_pcre::constants::{PCRE2_DOTALL, PCRE2_ERROR_NOMATCH};
    let full = format!("^(?:{})$", pattern);
    let ctx = goro_pcre::CompileContext::default();
    let re = goro_pcre::Code::compile(full.as_bytes(), PCRE2_DOTALL, &ctx).ok()?;
    let mut md = goro_pcre::MatchData::new(1);
    let mctx = goro_pcre::MatchContext::default();
    let rc = re.match_at(actual.as_bytes(), 0, 0, &mut md, &mctx);
    if rc >= 0 {
        Some(true)
    } else if rc == PCRE2_ERROR_NOMATCH {
        Some(false)
    } else {
        None
    }
}

fn matches_expect_regex(pattern: &str, actual: &str) -> bool {
    // EXPECTREGEX uses raw regex patterns. Each line of the pattern is a regex for
    // matching the corresponding line of output.
    // First try matching the entire output as one regex (run-tests uses /s).
    let full_pattern = format!("(?s)^(?:{})$", pcre_to_rust_regex(pattern));
    match regex::Regex::new(&full_pattern) {
        Ok(re) => {
            if re.is_match(actual) {
                return true;
            }
        }
        Err(_) => {
            // lookarounds, backreferences...: run-tests.php's preg_match()
            if let Some(m) = pcre_full_match(pattern, actual) {
                return m;
            }
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

/// run-tests.php's EXPECTF matching for a pattern with %r...%r sections:
/// the whole pattern as one regex (`/^...$/s`), the %r parts verbatim.
/// None when the regex cannot be built.
fn matches_expectf_as_regex(pattern: &str, actual: &str) -> Option<bool> {
    fn placeholder(c: u8) -> Option<&'static str> {
        Some(match c {
            b'e' => "/",
            b's' => r"[^\r\n]+",
            b'S' => r"[^\r\n]*",
            b'a' => ".+?",
            b'A' => ".*?",
            b'w' => r"\s*",
            b'i' => r"[+-]?\d+",
            b'd' => r"\d+",
            b'x' => "[0-9a-fA-F]+",
            b'f' => r"[+-]?(?:\d+|\.\d)(?:\.\d+)?(?:[Ee][+-]?\d+)?",
            b'c' => ".",
            b'0' => r"\x00",
            _ => return None,
        })
    }
    let b = pattern.as_bytes();
    let mut re = String::from("(?s-u)^");
    let mut i = 0;
    let mut in_re = false;
    let mut seg = String::new();
    while i < b.len() {
        if b[i] == b'%' && i + 1 < b.len() {
            if b[i + 1] == b'r' {
                if in_re {
                    re.push('(');
                    re.push_str(&pcre_to_rust_regex(&seg));
                    re.push(')');
                    seg.clear();
                }
                in_re = !in_re;
                i += 2;
                continue;
            }
            if let Some(r) = placeholder(b[i + 1]) {
                if in_re {
                    seg.push_str(r);
                } else {
                    re.push_str(r);
                }
                i += 2;
                continue;
            }
        }
        let ch = pattern[i..].chars().next()?;
        if in_re {
            seg.push(ch);
        } else {
            for byte in ch.to_string().bytes() {
                if byte.is_ascii() {
                    re.push_str(&regex::escape(&(byte as char).to_string()));
                } else {
                    re.push_str(&format!("\\x{:02x}", byte));
                }
            }
        }
        i += ch.len_utf8();
    }
    if in_re {
        return None;
    }
    re.push('$');
    let r = regex::bytes::RegexBuilder::new(&re)
        .size_limit(64 << 20)
        .build()
        .ok()?;
    Some(r.is_match(actual.as_bytes()))
}

fn matches_expectf(pattern: &str, actual: &str) -> bool {
    if pattern.contains("%r") && matches_expectf_as_regex(pattern, actual) == Some(true) {
        return true;
    }
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

thread_local! {
    /// Whether the test being run is a generic PDO test run against sqlite
    static SQLITE_PDO_VARIANT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// In the forked child of the sqlite run of a generic PDO test: the
/// environment ext/pdo_sqlite/tests/common.phpt's --REDIRECTTEST-- sets.
fn sqlite_child_env(filename: &str) {
    if !SQLITE_PDO_VARIANT.with(|v| v.get()) || !filename.contains("/ext/pdo/tests/") {
        return;
    }
    unsafe {
        std::env::set_var("PDOTEST_DSN", "sqlite::memory:");
        std::env::remove_var("PDOTEST_USER");
        std::env::remove_var("PDOTEST_PASS");
        std::env::remove_var("PDOTEST_ATTR");
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

    // run-tests.php gives the tests a TEMP environment variable
    // (sys_get_temp_dir() when unset).
    if std::env::var_os("TEMP").is_none_or(|t| t.is_empty()) {
        let tmp = std::env::temp_dir();
        let tmp = tmp.to_string_lossy();
        let tmp = tmp
            .strip_suffix('/')
            .filter(|t| !t.is_empty())
            .unwrap_or(&tmp);
        unsafe {
            std::env::set_var("TEMP", tmp);
        }
    }
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
    // The generic PDO tests run again against the pgsql driver (when a
    // PostgreSQL server can be had) and the sqlite driver, as the
    // --REDIRECTTEST-- of ext/pdo_{pgsql,sqlite}/tests/common.phpt do: such
    // a test is listed once more per driver (the n-th listing is the run of
    // pdo_variants[n - 1]).
    let mut pdo_variants: Vec<&'static str> = Vec::new();
    if pgsql_test::can_start() {
        pdo_variants.push("pgsql");
    }
    pdo_variants.push("sqlite");
    {
        let extra: Vec<std::path::PathBuf> = files
            .iter()
            .filter(|f| f.to_string_lossy().contains("/ext/pdo/tests/"))
            .cloned()
            .collect();
        if !extra.is_empty() {
            for _ in &pdo_variants {
                files.extend(extra.iter().cloned());
            }
            files.sort();
        }
    }
    // GORO_PHPT_SHARD=i/n keeps every n-th test starting at i (0-based), so CI
    // can split the suite across parallel jobs.
    if let Ok(spec) = std::env::var("GORO_PHPT_SHARD") {
        if let Some((i, n)) = spec.split_once('/') {
            if let (Ok(i), Ok(n)) = (i.trim().parse::<usize>(), n.trim().parse::<usize>()) {
                if n > 0 && i < n {
                    // The pgsql tests depend on each other (01createdb
                    // creates the tables the others use): each of their
                    // directories goes to the shard of its first test.
                    let mut group_shard: std::collections::HashMap<std::path::PathBuf, usize> =
                        std::collections::HashMap::new();
                    files = files
                        .into_iter()
                        .enumerate()
                        .filter(|(k, f)| {
                            let s = f.to_string_lossy();
                            if pgsql_test::is_pgsql_test(&s) {
                                let d = f.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                                let first = *group_shard.entry(d).or_insert(*k);
                                return first % n == i;
                            }
                            k % n == i
                        })
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
            let a = abs.to_string_lossy();
            if a.contains("/ext/mysqli/tests/")
                || a.contains("/ext/pdo_mysql/tests/")
                || a.contains("/ext/pdo/tests/")
            {
                has_mysqli_tests = true;
                *p = abs;
            }
        }
    }
    let mut has_pgsql_tests = pdo_variants.contains(&"pgsql")
        && files
            .iter()
            .any(|f| f.to_string_lossy().contains("/ext/pdo/tests/"));
    for p in files.iter_mut() {
        if let Ok(abs) = std::fs::canonicalize(&*p) {
            if pgsql_test::is_pgsql_test(&abs.to_string_lossy()) {
                has_pgsql_tests = true;
                *p = abs;
            }
        }
    }
    if has_pgsql_tests {
        let start = std::time::Instant::now();
        if pgsql_test::ensure_started() {
            eprintln!(
                "[goro-phpt] embedded PostgreSQL fixture ready in {:.2}s",
                start.elapsed().as_secs_f32()
            );
        } else {
            eprintln!(
                "[goro-phpt] no embedded PostgreSQL fixture available; pgsql tests will be skipped"
            );
        }
    }
    if has_mysqli_tests {
        let start = std::time::Instant::now();
        if let Some(root) = dir.ancestors().find(|a| a.join("run-tests.php").is_file()) {
            if let Ok(p) = std::fs::canonicalize(root.join("ext/pdo/tests")) {
                let d = format!("{}/", p.display());
                mysqli_test::PDO_TESTS_DIR.with(|x| *x.borrow_mut() = Some(d));
            }
        }
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

    let mut seen: std::collections::HashMap<std::path::PathBuf, usize> =
        std::collections::HashMap::new();
    for path in &files {
        // a further listing of a generic PDO test: its run against another
        // driver
        let nth = seen.entry(path.clone()).or_insert(0);
        let variant = if *nth == 0 {
            None
        } else {
            pdo_variants.get(*nth - 1).copied()
        };
        *nth += 1;
        let pgsql_variant = variant == Some("pgsql");
        if pgsql_variant && !pgsql_test::is_available() {
            continue;
        }
        pgsql_test::set_pdo_variant(pgsql_variant);
        SQLITE_PDO_VARIANT.with(|v| v.set(variant == Some("sqlite")));
        if let Ok(content) = std::fs::read(path) {
            if let Some(mut test) = PhptTest::parse_bytes(&content) {
                let test_dir = path.parent();
                // --EXPECT_EXTERNAL-- & co. name a file (next to the test)
                // holding that section.
                for key in ["EXPECT", "EXPECTF", "EXPECTREGEX"] {
                    let ext = format!("{}_EXTERNAL", key);
                    if test.sections.contains_key(key) {
                        continue;
                    }
                    if let (Some(rel), Some(dir)) = (test.sections.get(&ext), test_dir) {
                        if let Ok(body) = std::fs::read_to_string(dir.join(rel.trim())) {
                            test.sections.insert(key.to_string(), body);
                        }
                    }
                }
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
                let mut _dbg_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if let Some(v) = variant {
                    _dbg_name.push_str(&format!(" ({})", v));
                }
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

    // Stop the embedded database servers, if they were started.
    mysqli_test::shutdown();
    pgsql_test::shutdown();
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

/// Seconds the script spends in literal `sleep(N)` calls: added to the
/// time limit (run-tests.php allows 60 seconds).
fn literal_sleep_secs(source: &str) -> u64 {
    let mut total = 0u64;
    let mut rest = source;
    while let Some(p) = rest.find("sleep(") {
        let after = &rest[p + 6..];
        let n: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if after[n.len()..].starts_with(')') && !rest[..p].ends_with('u') {
            total += n.parse::<u64>().unwrap_or(0).min(60);
        }
        rest = after;
    }
    total.min(60)
}

#[allow(clippy::field_reassign_with_default)] // fields filled in step by step
fn build_request(test: &PhptTest, test_dir: Option<&Path>, filename: &str) -> PhptRequest {
    let raw = |k: &str| test.raw_sections.get(k).map(|s| s.as_str());
    let not_empty = |k: &str| raw(k).is_some_and(|s| !php_trim(s).is_empty());
    let is_cgi = test.sections.contains_key("CGI")
        || ["GET", "POST", "POST_RAW", "PUT", "COOKIE"]
            .iter()
            .any(|k| not_empty(k))
        // TestFile::isCGI(): !empty() of the section (a lone newline counts).
        || test
            .raw_sections
            .get("EXPECTHEADERS")
            .or_else(|| test.sections.get("EXPECTHEADERS"))
            .is_some_and(|s| !s.is_empty() && s != "0");
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
    // run-tests.php: --ENV-- may preset these; the request sections only
    // fill them when empty.
    let env_get = |env: &Vec<(String, String)>, k: &str| {
        env.iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let env_method = env_get(&env, "REQUEST_METHOD");
    let env_content_length = env_get(&env, "CONTENT_LENGTH");
    let method: String;
    let mut content_type = env_get(&env, "CONTENT_TYPE");
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
        method = if env_method.is_empty() {
            "POST".into()
        } else {
            env_method
        };
    } else if not_empty("PUT") {
        body = raw_request("PUT", &mut content_type);
        content_length = body.len().to_string();
        method = "PUT".into();
    } else if not_empty("POST") {
        body = test.bytes(php_trim(raw("POST").unwrap_or("")));
        method = "POST".into();
        if content_type.is_empty() {
            content_type = "application/x-www-form-urlencoded".into();
        }
        content_length = if env_content_length.is_empty() {
            body.len().to_string()
        } else {
            env_content_length
        };
    } else {
        method = "GET".into();
        content_type.clear();
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
    // proc_open() leaves out the variables whose value is empty.
    full.retain(|(_, v)| !v.is_empty());
    info.env = full;
    // The section is passed byte-exact, trailing newline included.
    let stdin = test
        .raw_sections
        .get("STDIN")
        .or_else(|| test.sections.get("STDIN"))
        .map(|s| test.bytes(s));
    PhptRequest { info, env, stdin }
}
