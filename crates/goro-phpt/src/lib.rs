use std::collections::HashMap;
use std::path::Path;

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
    // Get the PHP source
    let source = match test.file_section() {
        Some(s) => s,
        None => return TestResult::Error("missing --FILE-- section".into()),
    };

    // Execute the source
    let output = match execute_php(source.as_bytes()) {
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
    } else {
        TestResult::Error("missing --EXPECT-- or --EXPECTF-- section".into())
    }
}

fn execute_php(source: &[u8]) -> Result<Vec<u8>, String> {
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
    let op_array = compiler
        .compile(&program)
        .map_err(|e| format!("Compile error: {}", e))?;

    // Execute
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    vm.execute(&op_array)
        .map_err(|e| format!("Runtime error: {}", e))?;

    Ok(vm.take_output())
}

fn matches_expectf(pattern: &str, actual: &str) -> bool {
    // Simple EXPECTF matching: convert %d, %s, %f, etc. to regex patterns
    let mut regex_str = String::from("^");
    let pattern_bytes = pattern.as_bytes();
    let mut i = 0;

    while i < pattern_bytes.len() {
        if pattern_bytes[i] == b'%' && i + 1 < pattern_bytes.len() {
            match pattern_bytes[i + 1] {
                b'd' => {
                    regex_str.push_str("-?[0-9]+");
                    i += 2;
                }
                b's' => {
                    regex_str.push_str("[^\\s]+");
                    i += 2;
                }
                b'f' => {
                    regex_str.push_str("-?[0-9]*\\.?[0-9]+");
                    i += 2;
                }
                b'c' => {
                    regex_str.push('.');
                    i += 2;
                }
                b'x' => {
                    regex_str.push_str("[0-9a-fA-F]+");
                    i += 2;
                }
                b'e' => {
                    regex_str.push_str("[/\\\\]");
                    i += 2;
                }
                b'a' | b'A' => {
                    regex_str.push_str(".*");
                    i += 2;
                }
                b'w' => {
                    regex_str.push_str("\\s*");
                    i += 2;
                }
                b'i' => {
                    regex_str.push_str("[+-]?[0-9]+");
                    i += 2;
                }
                b'S' => {
                    regex_str.push_str(".*");
                    i += 2;
                }
                _ => {
                    regex_str.push(escape_regex_char(pattern_bytes[i] as char));
                    i += 1;
                }
            }
        } else {
            regex_str.push(escape_regex_char(pattern_bytes[i] as char));
            i += 1;
        }
    }
    regex_str.push('$');

    // Simple string matching without regex dependency for now
    // TODO: add proper regex matching
    let expected = pattern.trim_end_matches('\n');
    let actual_trimmed = actual.trim_end_matches('\n');
    expected == actual_trimmed
}

fn escape_regex_char(c: char) -> char {
    c
}

/// Run all .phpt files in a directory
pub fn run_test_dir(dir: &Path) -> (usize, usize, usize, usize) {
    let mut pass = 0;
    let mut fail = 0;
    let mut skip = 0;
    let mut error = 0;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "phpt") {
                if let Ok(content) = std::fs::read_to_string(&path) {
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
        }
    }

    (pass, fail, skip, error)
}
