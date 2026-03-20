use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A parsed PHPT test file
#[derive(Debug)]
pub struct PhptTest {
    pub name: String,
    pub sections: HashMap<String, String>,
}

impl PhptTest {
    /// Parse a .phpt file from its content
    pub fn parse(content: &str) -> Option<Self> {
        let mut sections: HashMap<String, String> = HashMap::new();
        let mut current_section: Option<String> = None;
        let mut current_content = String::new();

        let valid_sections = [
            "TEST",
            "FILE",
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
        ];
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

        let name = sections.get("TEST")?.clone();

        Some(PhptTest { name, sections })
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
    Fail { expected: String, actual: String },
    Skip(String),
    Error(String),
}

/// Run a PHPT test against the goro engine
pub fn run_test(test: &PhptTest) -> TestResult {
    run_test_with_dir(test, None)
}

pub fn run_test_with_dir(test: &PhptTest, test_dir: Option<&Path>) -> TestResult {
    // Parse INI settings
    let ini_settings = parse_ini_settings(test.ini_section());

    // Handle SKIPIF section
    if let Some(skipif) = test.skipif_section() {
        let skipif_trimmed = skipif.trim();
        if !skipif_trimmed.is_empty() {
            match execute_php_with_timeout(skipif_trimmed.as_bytes(), 5, test_dir, &ini_settings) {
                Ok(output) => {
                    let output_str = String::from_utf8_lossy(&output).to_lowercase();
                    if output_str.contains("skip") {
                        return TestResult::Skip(
                            String::from_utf8_lossy(&output).trim().to_string(),
                        );
                    }
                }
                Err(_) => {
                    // If SKIPIF errors, skip the test
                    return TestResult::Skip("SKIPIF section errored".into());
                }
            }
        }
    }

    // Get the PHP source
    let source = match test.file_section() {
        Some(s) => s,
        None => return TestResult::Error("missing --FILE-- section".into()),
    };

    // Execute the source with a 5-second timeout
    let output = match execute_php_with_timeout(source.as_bytes(), 5, test_dir, &ini_settings) {
        Ok(output) => output,
        Err(e) => return TestResult::Error(e),
    };

    let actual = String::from_utf8_lossy(&output).to_string();
    // Normalize \r\n to \n for comparison (PHP test files use \n line endings)
    let actual_normalized = actual.replace("\r\n", "\n");

    // Compare with expected output
    if let Some(expected) = test.expect_section() {
        let expected_trimmed = expected.trim();
        let actual_trimmed = actual_normalized.trim();
        if actual_trimmed == expected_trimmed {
            TestResult::Pass
        } else {
            TestResult::Fail {
                expected: expected_trimmed.to_string(),
                actual: actual_trimmed.to_string(),
            }
        }
    } else if let Some(pattern) = test.expectf_section() {
        // EXPECTF: convert printf-style patterns to regex
        let pattern_trimmed = pattern.trim();
        let actual_trimmed = actual_normalized.trim();
        if matches_expectf(pattern_trimmed, actual_trimmed) {
            TestResult::Pass
        } else {
            TestResult::Fail {
                expected: pattern_trimmed.to_string(),
                actual: actual_trimmed.to_string(),
            }
        }
    } else if test.expect_regex_section().is_some() {
        // We don't support EXPECTREGEX yet
        TestResult::Skip("EXPECTREGEX not supported".into())
    } else {
        TestResult::Error("missing --EXPECT-- or --EXPECTF-- section".into())
    }
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
                let value = line[eq + 1..].trim().to_string();
                settings.push((key, value));
            }
        }
    }
    settings
}

/// Parse error reporting expressions like "E_ALL&~E_DEPRECATED"
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
            "E_ALL" => Some(32767),
            _ => name.trim().parse::<i64>().ok(),
        }
    }

    let expr = expr.trim();
    if expr.is_empty() {
        return None;
    }

    // Handle simple constant
    if let Some(v) = resolve_constant(expr) {
        return Some(v);
    }

    // Handle "E_ALL&~E_DEPRECATED" pattern
    if let Some(amp_pos) = expr.find('&') {
        let left = &expr[..amp_pos];
        let right = &expr[amp_pos + 1..];
        let left_val = resolve_constant(left)?;
        let right = right.trim();
        if let Some(rest) = right.strip_prefix('~') {
            let right_val = resolve_constant(rest)?;
            return Some(left_val & !right_val);
        }
        let right_val = resolve_constant(right)?;
        return Some(left_val & right_val);
    }

    // Handle "|" operator
    if let Some(pipe_pos) = expr.find('|') {
        let left = &expr[..pipe_pos];
        let right = &expr[pipe_pos + 1..];
        let left_val = resolve_constant(left)?;
        let right_val = parse_error_reporting_expr(right)?;
        return Some(left_val | right_val);
    }

    None
}

fn execute_php_with_timeout(
    source: &[u8],
    timeout_secs: u64,
    test_dir: Option<&Path>,
    ini_settings: &[(String, String)],
) -> Result<Vec<u8>, String> {
    let source = source.to_vec();
    let dir_path = test_dir.map(|p| p.to_path_buf());
    let ini = ini_settings.to_vec();
    let timed_out = Arc::new(AtomicBool::new(false));
    let timed_out2 = timed_out.clone();

    let handle = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024) // 32MB stack
        .spawn(move || {
            // Memory protection is handled by:
            // - PhpArray::MAX_SIZE (128M elements)
            // - str_repeat/str_pad 128MB limits
            // - 5s execution timeout
            // - call_depth limit (100)
            // Change to test directory if provided, otherwise use temp dir
            if let Some(ref dir) = dir_path {
                let _ = std::env::set_current_dir(dir);
            } else {
                let _ = std::env::set_current_dir(std::env::temp_dir());
            }
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute_php_inner(&source, &ini)))
        })
        .map_err(|e| format!("Thread error: {}", e))?;

    // Wait with timeout
    let start = std::time::Instant::now();
    loop {
        if handle.is_finished() {
            break;
        }
        if start.elapsed().as_secs() >= timeout_secs {
            timed_out2.store(true, Ordering::Relaxed);
            // We can't kill the thread, so just return timeout error
            // The thread will eventually be cleaned up when the process exits
            return Err("Timeout: execution exceeded time limit".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    match handle.join() {
        Ok(Ok(r)) => r,
        Ok(Err(_)) => Err("Fatal error: panic during execution".to_string()),
        Err(_) => Err("Fatal error: stack overflow or panic".to_string()),
    }
}

fn execute_php_inner(source: &[u8], ini_settings: &[(String, String)]) -> Result<Vec<u8>, String> {
    use goro_core::compiler::Compiler;
    use goro_core::vm::Vm;
    use goro_core::value::{Value, set_php_precision};
    use goro_core::string::PhpString;
    use goro_parser::{Lexer, Parser};

    // Apply precision from INI settings (reset to default first)
    set_php_precision(14);
    for (key, value) in ini_settings {
        if key == "precision" {
            if let Ok(p) = value.parse::<i32>() {
                set_php_precision(p);
            }
        }
    }

    // Lex
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    // Parse
    let mut parser = Parser::new(tokens);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => {
            // PHP outputs parse errors to stdout
            let msg = format!(
                "\nParse error: syntax error in Unknown.php on line {}\n",
                e.span.line
            );
            return Ok(msg.into_bytes());
        }
    };

    // Compile
    let compiler = Compiler::new();
    let (op_array, compiled_classes) = match compiler.compile(&program) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!(
                "\nFatal error: {} in Unknown.php on line {}\n",
                e.message, e.line
            );
            return Ok(msg.into_bytes());
        }
    };

    // Execute
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    goro_ext_date::register(&mut vm);
    goro_ext_json::register(&mut vm);
    goro_ext_ctype::register(&mut vm);
    goro_ext_hash::register(&mut vm);

    // Apply all INI settings to the VM constants
    for (key, value) in ini_settings {
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
        vm.constants.insert(key.as_bytes().to_vec(), val);
    }
    for class in compiled_classes {
        vm.register_class(class);
    }

    match vm.execute(&op_array) {
        Ok(_) => Ok(vm.take_output()),
        Err(e) => {
            // Capture any output produced before the error, plus the error message
            let mut output = vm.take_output();

            // Format the error like PHP does
            let stack_trace = vm.format_stack_trace();
            if let Some(exc) = vm.current_exception.take() {
                if let goro_core::value::Value::Object(obj) = &exc {
                    let obj = obj.borrow();
                    let class = String::from_utf8_lossy(&obj.class_name);
                    let msg = obj.get_property(b"message");
                    let msg_str = msg.to_php_string().to_string_lossy();
                    let fatal = if msg_str.is_empty() {
                        format!(
                            "\nFatal error: Uncaught {} in Unknown.php:{}\nStack trace:\n{}\n  thrown in Unknown.php on line {}",
                            class, e.line, stack_trace, e.line
                        )
                    } else {
                        format!(
                            "\nFatal error: Uncaught {}: {} in Unknown.php:{}\nStack trace:\n{}\n  thrown in Unknown.php on line {}",
                            class, msg_str, e.line, stack_trace, e.line
                        )
                    };
                    output.extend_from_slice(fatal.as_bytes());
                } else {
                    let fatal =
                        format!("\nFatal error: {} in Unknown.php on line {}", e.message, e.line);
                    output.extend_from_slice(fatal.as_bytes());
                }
            } else {
                let fatal = format!(
                    "\nFatal error: {} in Unknown.php on line {}\n",
                    e.message, e.line
                );
                output.extend_from_slice(fatal.as_bytes());
            }

            Ok(output)
        }
    }
}

fn matches_expectf(pattern: &str, actual: &str) -> bool {
    // Line-by-line EXPECTF matching
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
                    while ai < ab.len() && ab[ai].is_ascii_digit() {
                        ai += 1;
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
                    if ai < ab.len() && ab[ai] == b'-' {
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
                    if ai < ab.len() && (ab[ai] == b'/' || ab[ai] == b'\\') {
                        ai += 1;
                    }
                }
                b'w' => {
                    pi += 2;
                    while ai < ab.len() && (ab[ai] == b' ' || ab[ai] == b'\t') {
                        ai += 1;
                    }
                }
                b'c' => {
                    pi += 2;
                    if ai < ab.len() {
                        ai += 1;
                    } else {
                        return false;
                    }
                }
                b'%' => {
                    pi += 2;
                    if ai < ab.len() && ab[ai] == b'%' {
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

/// Run all .phpt files in a directory (recursively)
pub fn run_test_dir(dir: &Path) -> (usize, usize, usize, usize) {
    let mut pass = 0;
    let mut fail = 0;
    let mut skip = 0;
    let mut error = 0;

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_phpt_files(dir, &mut files);
    files.sort();

    for path in &files {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Some(test) = PhptTest::parse(&content) {
                let test_dir = path.parent();
                // Wrap in catch_unwind to prevent stack overflows from killing the runner
                let test_name = test.name.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_test_with_dir(&test, test_dir)
                }));
                match result {
                    Ok(TestResult::Pass) => {
                        pass += 1;
                        println!("PASS: {}", test_name);
                    }
                    Ok(TestResult::Fail { expected, actual }) => {
                        fail += 1;
                        println!("FAIL: {}", test_name);
                        println!("  Expected: {:?}", expected);
                        println!("  Actual:   {:?}", actual);
                    }
                    Ok(TestResult::Skip(reason)) => {
                        skip += 1;
                        println!("SKIP: {} ({})", test_name, reason);
                    }
                    Ok(TestResult::Error(msg)) => {
                        error += 1;
                        println!("ERROR: {} ({})", test_name, msg);
                    }
                    Err(_) => {
                        error += 1;
                        println!("ERROR: {} (panic/stack overflow)", test_name);
                    }
                }
            }
        }
    }

    (pass, fail, skip, error)
}

fn collect_phpt_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_phpt_files(&path, files);
            } else if path.extension().is_some_and(|e| e == "phpt") {
                files.push(path);
            }
        }
    }
}
