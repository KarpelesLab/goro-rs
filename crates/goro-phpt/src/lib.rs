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

        for line in content.lines() {
            if line.starts_with("--") && line.ends_with("--") && line.len() > 4 {
                let section_name = &line[2..line.len() - 2];
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
    // Handle SKIPIF section
    if let Some(skipif) = test.skipif_section() {
        let skipif_trimmed = skipif.trim();
        if !skipif_trimmed.is_empty() {
            match execute_php_with_timeout(skipif_trimmed.as_bytes(), 5) {
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

    // Execute the source with a 10-second timeout
    let output = match execute_php_with_timeout(source.as_bytes(), 10) {
        Ok(output) => output,
        Err(e) => return TestResult::Error(e),
    };

    let actual = String::from_utf8_lossy(&output).to_string();

    // Compare with expected output
    if let Some(expected) = test.expect_section() {
        let expected_trimmed = expected.trim_end_matches('\n');
        let actual_trimmed = actual.trim_end_matches('\n');
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
        if matches_expectf(pattern, &actual) {
            TestResult::Pass
        } else {
            TestResult::Fail {
                expected: pattern.to_string(),
                actual,
            }
        }
    } else if test.expect_regex_section().is_some() {
        // We don't support EXPECTREGEX yet
        TestResult::Skip("EXPECTREGEX not supported".into())
    } else {
        TestResult::Error("missing --EXPECT-- or --EXPECTF-- section".into())
    }
}

fn execute_php_with_timeout(source: &[u8], timeout_secs: u64) -> Result<Vec<u8>, String> {
    let source = source.to_vec();
    let timed_out = Arc::new(AtomicBool::new(false));
    let timed_out2 = timed_out.clone();

    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024) // 64MB stack
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute_php_inner(&source)))
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

fn execute_php_inner(source: &[u8]) -> Result<Vec<u8>, String> {
    use goro_core::compiler::Compiler;
    use goro_core::vm::Vm;
    use goro_parser::{Lexer, Parser};

    // Lex
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    // Parse
    let mut parser = Parser::new(tokens);
    let program = parser.parse().map_err(|e| format!("Parse error: {}", e))?;

    // Compile
    let compiler = Compiler::new();
    let (op_array, compiled_classes) = compiler
        .compile(&program)
        .map_err(|e| format!("Compile error: {}", e))?;

    // Execute
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    for class in compiled_classes {
        vm.register_class(class);
    }
    vm.execute(&op_array)
        .map_err(|e| format!("Runtime error: {}", e))?;

    Ok(vm.take_output())
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

/// Match a single line with EXPECTF patterns
fn match_expectf_line(pattern: &str, actual: &str) -> bool {
    let pb = pattern.as_bytes();
    let ab = actual.as_bytes();
    let mut pi = 0;
    let mut ai = 0;

    while pi < pb.len() && ai < ab.len() {
        if pb[pi] == b'%' && pi + 1 < pb.len() {
            match pb[pi + 1] {
                b'd' => {
                    pi += 2;
                    // Match optional - and digits
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
                    // Match non-whitespace
                    if ai >= ab.len() || ab[ai] == b' ' || ab[ai] == b'\t' {
                        return false;
                    }
                    while ai < ab.len() && ab[ai] != b' ' && ab[ai] != b'\t' && ab[ai] != b'\n' {
                        ai += 1;
                    }
                }
                b'S' | b'a' | b'A' => {
                    pi += 2;
                    // Match anything - greedy
                    // Look ahead for next literal char in pattern
                    if pi >= pb.len() {
                        ai = ab.len(); // consume rest
                    } else {
                        // Find next literal char
                        let next_literal = if pb[pi] == b'%' { None } else { Some(pb[pi]) };
                        if let Some(nc) = next_literal {
                            // Advance ai until we find nc
                            while ai < ab.len() && ab[ai] != nc {
                                ai += 1;
                            }
                        } else {
                            ai = ab.len();
                        }
                    }
                }
                b'f' => {
                    pi += 2;
                    if ai < ab.len() && ab[ai] == b'-' {
                        ai += 1;
                    }
                    while ai < ab.len() && (ab[ai].is_ascii_digit() || ab[ai] == b'.') {
                        ai += 1;
                    }
                }
                b'x' => {
                    pi += 2;
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
                    // Unknown pattern, treat as literal
                    if pb[pi] != ab[ai] {
                        return false;
                    }
                    pi += 1;
                    ai += 1;
                }
            }
        } else {
            if pb[pi] != ab[ai] {
                return false;
            }
            pi += 1;
            ai += 1;
        }
    }

    // Both should be consumed
    pi >= pb.len() && ai >= ab.len()
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
                match run_test(&test) {
                    TestResult::Pass => {
                        pass += 1;
                        println!("PASS: {}", test.name);
                    }
                    TestResult::Fail { expected, actual } => {
                        fail += 1;
                        println!("FAIL: {}", test.name);
                        println!("  Expected: {:?}", expected);
                        println!("  Actual:   {:?}", actual);
                    }
                    TestResult::Skip(reason) => {
                        skip += 1;
                        println!("SKIP: {} ({})", test.name, reason);
                    }
                    TestResult::Error(msg) => {
                        error += 1;
                        println!("ERROR: {} ({})", test.name, msg);
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
