use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::os::unix::io::FromRawFd;

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
    run_test_with_dir_and_filename(test, test_dir, "Unknown.php")
}

pub fn run_test_with_dir_and_filename(test: &PhptTest, test_dir: Option<&Path>, filename: &str) -> TestResult {
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
    let output = match execute_php_with_timeout_and_filename(source.as_bytes(), 5, test_dir, &ini_settings, filename) {
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
    execute_php_with_timeout_and_filename(source, timeout_secs, test_dir, ini_settings, "Unknown.php")
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
        use std::io::Read as IoRead;

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
            unsafe { libc::close(read_fd); libc::close(write_fd); }
            return Err("Failed to fork".to_string());
        }

        if pid == 0 {
            // Child process
            unsafe { libc::close(read_fd); }

            // Change to test directory
            if let Some(dir) = test_dir {
                let _ = std::env::set_current_dir(dir);
            } else {
                let _ = std::env::set_current_dir(std::env::temp_dir());
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute_php_inner(source, ini_settings, filename)
            }));

            let (status, output) = match result {
                Ok(Ok(bytes)) => (0u8, bytes),
                Ok(Err(e)) => (1u8, format!("ERROR: {}", e).into_bytes()),
                Err(_) => (2u8, b"PANIC".to_vec()),
            };
            let _ = unsafe { libc::write(write_fd, &status as *const u8 as *const libc::c_void, 1) };
            let len = output.len() as u64;
            let _ = unsafe { libc::write(write_fd, &len as *const u64 as *const libc::c_void, 8) };
            if !output.is_empty() {
                let _ = unsafe { libc::write(write_fd, output.as_ptr() as *const libc::c_void, output.len()) };
            }
            unsafe { libc::close(write_fd); }
            std::process::exit(0);
        }

        // Parent process
        unsafe { libc::close(write_fd); }

        // Wait for child with timeout
        let start = std::time::Instant::now();
        loop {
            let mut status = 0i32;
            let ret = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
            if ret > 0 {
                break; // Child exited
            }
            if ret < 0 {
                unsafe { libc::close(read_fd); }
                return Err("waitpid error".to_string());
            }
            if start.elapsed().as_secs() >= timeout_secs {
                // Kill the child
                unsafe { libc::kill(pid, libc::SIGKILL); }
                unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0); }
                unsafe { libc::close(read_fd); }
                return Err("Timeout: execution exceeded time limit".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        // Read result from pipe
        let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let mut buf = Vec::new();
        let _ = file.read_to_end(&mut buf);

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
            buf[9..9+len].to_vec()
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
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute_php_inner(&source, &ini, &fname)))
            })
            .map_err(|e| format!("Thread error: {}", e))?;

        let start = std::time::Instant::now();
        loop {
            if handle.is_finished() { break; }
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

fn execute_php_inner(source: &[u8], ini_settings: &[(String, String)], filename: &str) -> Result<Vec<u8>, String> {
    use goro_core::compiler::Compiler;
    use goro_core::vm::Vm;
    use goro_core::value::{Value, set_php_precision, set_php_serialize_precision, memory_reset, set_memory_limit};
    use goro_core::string::PhpString;
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
            // Some parser errors should be "Fatal error" rather than "Parse error"
            let err_msg = &e.message;
            let is_fatal = err_msg.contains("Match expressions")
                || err_msg.contains("Cannot use")
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
                || err_msg.contains("cannot accept");
            let msg = if is_fatal {
                format!(
                    "\nFatal error: {} in {} on line {}\n",
                    err_msg, filename, e.span.line
                )
            } else if err_msg.starts_with("syntax error") {
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

    // Compile
    let mut compiler = Compiler::new();
    compiler.source_file = filename.as_bytes().to_vec();
    let (op_array, compiled_classes) = match compiler.compile(&program) {
        Ok(r) => r,
        Err(e) => {
            let msg = format!(
                "\nFatal error: {} in {} on line {}\n",
                e.message, filename, e.line
            );
            return Ok(msg.into_bytes());
        }
    };

    // Execute
    let mut vm = Vm::new();
    vm.current_file = filename.to_string();
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
                    // Collect the exception chain (innermost first, then previous)
                    let mut chain = Vec::new();
                    {
                        let obj_ref = obj.borrow();
                        let class = String::from_utf8_lossy(&obj_ref.class_name).to_string();
                        let msg_str = obj_ref.get_property(b"message").to_php_string().to_string_lossy();
                        let exc_file = obj_ref.get_property(b"file").to_php_string().to_string_lossy();
                        let exc_line = obj_ref.get_property(b"line").to_long();
                        let file_str = if exc_file.is_empty() || exc_file == "Unknown" { vm.current_file.clone() } else { exc_file };
                        let line_num = if exc_line > 0 { exc_line as u32 } else { e.line };
                        // Get trace from exception object
                        let exc_trace = obj_ref.get_property(b"trace");
                        let trace_str = format_exception_trace(&exc_trace, &stack_trace);
                        chain.push((class, msg_str, file_str, line_num, trace_str));
                        // Walk previous chain
                        let mut prev = obj_ref.get_property(b"previous").clone();
                        for _ in 0..100 {
                            let next_prev;
                            if let goro_core::value::Value::Object(prev_obj) = &prev {
                                let prev_ref = prev_obj.borrow();
                                let pc = String::from_utf8_lossy(&prev_ref.class_name).to_string();
                                let pm = prev_ref.get_property(b"message").to_php_string().to_string_lossy();
                                let pf = prev_ref.get_property(b"file").to_php_string().to_string_lossy();
                                let pl = prev_ref.get_property(b"line").to_long();
                                let pf_str = if pf.is_empty() || pf == "Unknown" { vm.current_file.clone() } else { pf };
                                let pl_num = if pl > 0 { pl as u32 } else { 0 };
                                let pt = prev_ref.get_property(b"trace");
                                let pt_str = format_exception_trace(&pt, &stack_trace);
                                chain.push((pc, pm, pf_str, pl_num, pt_str));
                                next_prev = prev_ref.get_property(b"previous").clone();
                            } else {
                                break;
                            }
                            prev = next_prev;
                        }
                    }

                    // Format: first exception is "Uncaught", rest are "Next"
                    // But PHP displays them in reverse order (innermost first, then previous)
                    // Actually PHP shows the innermost (last thrown) first, with "Uncaught",
                    // then shows each previous with "\n\nNext"
                    // The "thrown in" line comes at the very end with the last exception in chain
                    let first = &chain[0];
                    let thrown_file = &first.2;
                    let thrown_line = first.3;

                    let mut fatal = String::new();
                    for (i, (class, msg_str, file_str, line_num, trace_str)) in chain.iter().rev().enumerate() {
                        if i == 0 {
                            if msg_str.is_empty() {
                                fatal.push_str(&format!(
                                    "\nFatal error: Uncaught {} in {}:{}\nStack trace:\n{}",
                                    class, file_str, line_num, trace_str
                                ));
                            } else {
                                fatal.push_str(&format!(
                                    "\nFatal error: Uncaught {}: {} in {}:{}\nStack trace:\n{}",
                                    class, msg_str, file_str, line_num, trace_str
                                ));
                            }
                        } else {
                            if msg_str.is_empty() {
                                fatal.push_str(&format!(
                                    "\n\nNext {} in {}:{}\nStack trace:\n{}",
                                    class, file_str, line_num, trace_str
                                ));
                            } else {
                                fatal.push_str(&format!(
                                    "\n\nNext {}: {} in {}:{}\nStack trace:\n{}",
                                    class, msg_str, file_str, line_num, trace_str
                                ));
                            }
                        }
                    }
                    fatal.push_str(&format!("\n  thrown in {} on line {}", thrown_file, thrown_line));

                    output.extend_from_slice(fatal.as_bytes());
                } else {
                    let fatal =
                        format!("\nFatal error: {} in {} on line {}", e.message, vm.current_file, e.line);
                    output.extend_from_slice(fatal.as_bytes());
                }
            } else {
                let fatal = format!(
                    "\nFatal error: {} in {} on line {}\n",
                    e.message, vm.current_file, e.line
                );
                output.extend_from_slice(fatal.as_bytes());
            }

            Ok(output)
        }
    }
}

fn format_exception_trace(trace_val: &goro_core::value::Value, fallback_trace: &str) -> String {
    use goro_core::value::Value;
    use goro_core::array::ArrayKey;
    if let Value::Array(arr) = trace_val {
        let arr = arr.borrow();
        let mut lines = Vec::new();
        let mut idx = 0;
        for (_key, frame_val) in arr.iter() {
            if let Value::Array(frame) = frame_val {
                let frame = frame.borrow();
                let file = frame.get(&ArrayKey::String(goro_core::string::PhpString::from_bytes(b"file")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let line = frame.get(&ArrayKey::String(goro_core::string::PhpString::from_bytes(b"line")))
                    .map(|v| v.to_long())
                    .unwrap_or(0);
                let function = frame.get(&ArrayKey::String(goro_core::string::PhpString::from_bytes(b"function")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let class = frame.get(&ArrayKey::String(goro_core::string::PhpString::from_bytes(b"class")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                let type_str = frame.get(&ArrayKey::String(goro_core::string::PhpString::from_bytes(b"type")))
                    .map(|v| v.to_php_string().to_string_lossy())
                    .unwrap_or_default();
                // Format args
                let args_str = if let Some(args_val) = frame.get(&ArrayKey::String(goro_core::string::PhpString::from_bytes(b"args"))) {
                    if let Value::Array(args_arr) = args_val {
                        let args_arr = args_arr.borrow();
                        let formatted: Vec<String> = args_arr.iter().map(|(_k, v)| {
                            goro_core::vm::Vm::format_trace_arg(v)
                        }).collect();
                        formatted.join(", ")
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                let loc = if file.is_empty() {
                    "[internal function]".to_string()
                } else {
                    format!("{}({})", file, line)
                };
                lines.push(format!("#{} {}: {}{}{}({})", idx, loc, class, type_str, function, args_str));
            }
            idx += 1;
        }
        lines.push(format!("#{} {{main}}", idx));
        return lines.join("\n");
    }
    fallback_trace.to_string()
}

fn matches_expectf(pattern: &str, actual: &str) -> bool {
    // Check if the pattern contains %a or %A which can match across lines
    // In that case we need whole-string matching, not line-by-line
    if pattern.contains("%a") || pattern.contains("%A") {
        // Whole-string matching (supports cross-line %a/%A)
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
                // Use full path for filename, converting .phpt to .php
                let test_filename = {
                    let full = path.to_string_lossy().to_string();
                    if full.ends_with(".phpt") {
                        format!("{}.php", &full[..full.len() - 5])
                    } else {
                        full
                    }
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_test_with_dir_and_filename(&test, test_dir, &test_filename)
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
    if s == "-1" { return -1; }
    let (num_str, multiplier) = if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len()-1], 1024 * 1024 * 1024i64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len()-1], 1024 * 1024i64)
    } else if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len()-1], 1024i64)
    } else {
        (s, 1i64)
    };
    num_str.parse::<i64>().unwrap_or(128 * 1024 * 1024) * multiplier
}
