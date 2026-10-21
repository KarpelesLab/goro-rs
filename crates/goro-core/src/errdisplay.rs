//! Error display (php_error_cb / php_verror presentation): display_errors,
//! html_errors, docref links, error_prepend_string / error_append_string.

use crate::vm::Vm;

fn ini_str(vm: &Vm, key: &[u8]) -> Option<String> {
    vm.constants
        .get(key)
        .map(|v| v.to_php_string().to_string_lossy())
}

fn ini_bool(vm: &Vm, key: &[u8]) -> bool {
    match ini_str(vm, key) {
        Some(s) => {
            let l = s.trim().to_ascii_lowercase();
            matches!(l.as_str(), "1" | "on" | "yes" | "true")
                || l.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
        None => false,
    }
}

/// htmlspecialchars(ENT_COMPAT) for error messages.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// Split a php_error_docref style message "fname(args): text" into
/// (function name, origin, text).
fn split_docref(msg: &str) -> Option<(&str, &str, &str)> {
    let b = msg.as_bytes();
    let mut i = 0;
    while i < b.len()
        && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b':' || b[i] == b'\\')
    {
        i += 1;
    }
    if i == 0 || i >= b.len() || b[i] != b'(' {
        return None;
    }
    let name = &msg[..i];
    let rest = &msg[i..];
    let close = rest.find("): ")?;
    let origin = &msg[..i + close + 1];
    let text = &rest[close + 3..];
    Some((name, origin, text))
}

impl Vm {
    /// 0 = don't display, 1 = stdout (through the output layer), 2 = stderr.
    fn display_errors_mode(&self) -> u8 {
        match ini_str(self, b"display_errors") {
            None => 1,
            Some(s) => {
                let l = s.trim().to_ascii_lowercase();
                match l.as_str() {
                    "on" | "yes" | "true" | "stdout" => 1,
                    "stderr" => 2,
                    _ => match l.parse::<i64>() {
                        Ok(0) | Err(_) => 0,
                        Ok(2) => 2,
                        Ok(_) => 1,
                    },
                }
            }
        }
    }

    /// Format and output one error message the way php_error_cb displays it.
    pub fn display_error(&mut self, kind: &str, msg: &str, file: &str, line: u32) {
        // log_errors without an error_log: the CLI SAPI logs to stderr
        // (sapi_cli_log_message()).
        if ini_bool(self, b"log_errors") && ini_str(self, b"error_log").is_none_or(|s| s.is_empty())
        {
            use std::io::Write;
            let _ = std::io::stderr().write_all(
                format!("PHP {}:  {} in {} on line {}\n", kind, msg, file, line).as_bytes(),
            );
        }
        let mode = self.display_errors_mode();
        if mode == 0 {
            return;
        }
        // fatal_error_backtraces: php_error_cb() appends the backtrace taken
        // when the fatal error was raised (not for uncaught exceptions,
        // which print their own trace).
        let trace = if kind == "Fatal error"
            && !msg.starts_with("Uncaught ")
            && ini_bool(self, b"fatal_error_backtraces")
        {
            self.materialize_builtin_frames(false);
            format!("\nStack trace:\n{}", self.format_stack_trace())
        } else {
            String::new()
        };
        let prepend = ini_str(self, b"error_prepend_string").unwrap_or_default();
        let append = ini_str(self, b"error_append_string").unwrap_or_default();
        let text = if ini_bool(self, b"html_errors") {
            let body = if kind == "Fatal error" || kind == "Parse error" {
                escape_html(msg)
            } else if let Some((name, origin, rest)) = split_docref(msg) {
                let root = ini_str(self, b"docref_root").unwrap_or_default();
                if !root.is_empty() {
                    let mut docref =
                        format!("function.{}", name.replace('_', "-").to_ascii_lowercase());
                    let ext = ini_str(self, b"docref_ext").unwrap_or_default();
                    if !ext.is_empty() {
                        docref.push_str(&ext);
                    }
                    format!(
                        "{} [<a href='{}{}'>{}</a>]: {}",
                        escape_html(origin),
                        root,
                        docref,
                        docref,
                        escape_html(rest)
                    )
                } else {
                    format!("{}: {}", escape_html(origin), escape_html(rest))
                }
            } else {
                msg.to_string()
            };
            format!(
                "{}<br />\n<b>{}</b>:  {} in <b>{}</b> on line <b>{}</b><br />{}\n{}",
                prepend, kind, body, file, line, trace, append
            )
        } else {
            format!(
                "{}\n{}: {} in {} on line {}{}\n{}",
                prepend, kind, msg, file, line, trace, append
            )
        };
        if mode == 2 {
            use std::io::Write;
            let _ = std::io::stderr().write_all(text.trim_start_matches('\n').as_bytes());
        } else {
            self.write_output(text.as_bytes());
        }
    }
}
