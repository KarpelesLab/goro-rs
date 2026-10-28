//! Corpus check: parses the `--FILE--` section of every `.phpt` test under
//! a directory and compares with the test's expected output.
//!
//! - A test whose expected output has no "Parse error:" must parse.
//! - A test that expects "Parse error: MSG in FILE on line N" must fail
//!   with that message (and line, when the test gives a number).
//!
//! Usage: corpus [DIR] [--report FILE] [--bench]
//! (see tools/syntax-corpus.sh).

use std::path::{Path, PathBuf};
use std::time::Instant;

use goro_syntax::{ErrorKind, ParseOptions, parse_with};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "phpt") {
            out.push(p);
        }
    }
}

/// The string literals passed directly to `eval()` in a source, decoded.
fn eval_literals(src: &[u8]) -> Vec<Vec<u8>> {
    use goro_syntax::T;
    let toks: Vec<_> = goro_syntax::tokenize(src, true)
        .into_iter()
        .filter(|t| !t.kind.is_trivia())
        .collect();
    let mut out = Vec::new();
    for w in toks.windows(4) {
        if w[0].kind == T::Eval
            && w[1].kind == T::LParen
            && w[2].kind == T::ConstantEncapsedString
            && w[3].kind == T::RParen
        {
            // Decode the literal by parsing it.
            let mut code = b"<?php ".to_vec();
            code.extend_from_slice(w[2].text(src));
            code.push(b';');
            let arena = goro_syntax::Arena::new();
            if let Ok(p) = goro_syntax::parse(&arena, &code)
                && let Some(st) = p.stmts.first()
                && let goro_syntax::ast::StmtKind::Expr(e) = st.kind
                && let goro_syntax::ast::ExprKind::String(s, _) = e.kind
            {
                out.push(s.to_vec());
            }
        }
    }
    out
}

/// Splits a .phpt file into (section name, content) pairs.
fn sections(data: &[u8]) -> Vec<(String, &[u8])> {
    let mut out: Vec<(String, &[u8])> = Vec::new();
    let mut name: Option<String> = None;
    let mut start = 0;
    let mut i = 0;
    while i <= data.len() {
        let line_end = data[i..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| i + p)
            .unwrap_or(data.len());
        let line = &data[i..line_end];
        let line_trim = line.strip_suffix(b"\r").unwrap_or(line);
        let is_header = line_trim.len() > 4
            && line_trim.starts_with(b"--")
            && line_trim.ends_with(b"--")
            && line_trim[2..line_trim.len() - 2]
                .iter()
                .all(|&c| c.is_ascii_uppercase() || c == b'_');
        if is_header {
            if let Some(n) = name.take() {
                out.push((n, &data[start..i]));
            }
            name = Some(String::from_utf8_lossy(&line_trim[2..line_trim.len() - 2]).into_owned());
            start = (line_end + 1).min(data.len());
        }
        if line_end >= data.len() {
            break;
        }
        i = line_end + 1;
    }
    if let Some(n) = name {
        out.push((n, &data[start..]));
    }
    out
}

/// Matches `text` against a run-tests EXPECTF pattern (%s, %d, %i, %a...).
fn wild_match(pat: &str, text: &str) -> bool {
    let p = pat.as_bytes();
    let t = text.as_bytes();
    fn go(p: &[u8], t: &[u8]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        if p[0] == b'%' && p.len() > 1 {
            let kind = p[1];
            let rest = &p[2..];
            match kind {
                b's' | b'a' | b'A' | b'S' => {
                    for k in 0..=t.len() {
                        if go(rest, &t[k..]) {
                            return true;
                        }
                    }
                    false
                }
                b'd' | b'i' => {
                    let mut k = 0;
                    if kind == b'i' && k < t.len() && (t[k] == b'-' || t[k] == b'+') {
                        k += 1;
                    }
                    let s = k;
                    while k < t.len() && t[k].is_ascii_digit() {
                        k += 1;
                    }
                    if k == s {
                        return false;
                    }
                    (s + 1..=k).rev().any(|e| go(rest, &t[e..]))
                }
                b'%' => !t.is_empty() && t[0] == b'%' && go(rest, &t[1..]),
                _ => !t.is_empty() && t[0] == p[0] && go(&p[1..], &t[1..]),
            }
        } else {
            !t.is_empty() && t[0] == p[0] && go(&p[1..], &t[1..])
        }
    }
    go(p, t)
}

/// Compile errors the parser reports (the compiler reports the rest).
const PARSER_FATALS: &[&str] = &[
    "Multiple access type modifiers are not allowed",
    "Multiple abstract modifiers are not allowed",
    "Multiple static modifiers are not allowed",
    "Multiple final modifiers are not allowed",
    "Multiple readonly modifiers are not allowed",
    "Cannot use the final modifier on an abstract class",
    "Cannot use the final modifier on an abstract method",
    "Cannot use the final modifier on an abstract property",
    "Cannot use the static modifier on a parameter",
    "Cannot use the abstract modifier on a parameter",
    "Cannot use the readonly modifier on a method",
    "Cannot use the static modifier on a class constant",
    "Cannot use the abstract modifier on a class constant",
    "Cannot use the readonly modifier on a class constant",
    "Cannot use the static modifier on a property hook",
    "Cannot use the public modifier on a property hook",
    "Cannot use the final modifier on an anonymous class",
    "Cannot use the abstract modifier on an anonymous class",
    "Cannot use \"static\" as method modifier",
    "Cannot use \"abstract\" as method modifier",
    "Unparenthesized `",
    "Arrow functions on the right hand side of |> must be parenthesized",
    "Cannot use try without catch or finally",
    "__HALT_COMPILER() can only be used from the outermost scope",
    "Encoding must be a literal",
];

struct Expected {
    message: String,
    /// The whole "MSG in FILE on line N" text.
    full: String,
}

/// The parse error a test expects, from its EXPECT* section.
fn expected_parse_error(expect: &str) -> Option<Expected> {
    for raw in expect.lines() {
        let l = raw
            .trim_start_matches("PHP ")
            .replace("<b>", "")
            .replace("</b>", "");
        let l = l.trim_start();
        let rest = if let Some(r) = l.strip_prefix("Parse error: ") {
            r
        } else {
            continue;
        };
        let rest = rest.trim_start();
        // "MSG in FILE on line N"
        let msg = match rest.rfind(" on line ") {
            Some(p) => {
                let before = &rest[..p];
                match before.rfind(" in ") {
                    Some(q) => &before[..q],
                    None => before,
                }
            }
            None => rest,
        };
        return Some(Expected {
            message: msg.to_string(),
            full: rest.trim_end().to_string(),
        });
    }
    None
}

fn main() {
    // Deeply nested test inputs need a big stack.
    let h = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(run)
        .unwrap();
    h.join().unwrap();
}

fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dir = PathBuf::from("/tmp/claude-1000/php-tests");
    let mut report: Option<PathBuf> = None;
    let mut bench = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--report" => {
                i += 1;
                report = args.get(i).map(PathBuf::from);
            }
            "--bench" => bench = true,
            a => dir = PathBuf::from(a),
        }
        i += 1;
    }
    let mut files = Vec::new();
    collect(&dir, &mut files);

    let mut total = 0;
    let mut ok_accept = 0;
    let mut bad_reject = Vec::new();
    let mut err_match = 0;
    let mut err_msg_mismatch = Vec::new();
    let mut err_line_mismatch = Vec::new();
    let mut err_accepted = Vec::new();
    let mut bytes = 0usize;
    let mut skipped = 0;
    let mut fatal_ok = 0;
    let mut eval_ok = 0;
    let mut eval_bad = Vec::new();
    let mut fatal_bad = Vec::new();
    let mut fatal_missed = Vec::new();
    let mut nested = Vec::new();
    let mut parse_time = std::time::Duration::ZERO;

    for f in &files {
        let Ok(data) = std::fs::read(f) else { continue };
        let secs = sections(&data);
        let mut file: Option<Vec<u8>> = None;
        let mut expect = String::new();
        let mut short_tags = true;
        let mut multibyte = false;
        let mut preload = false;
        for (name, body) in &secs {
            match name.as_str() {
                "FILE" => file = Some(body.to_vec()),
                "FILEEOF" => {
                    // run-tests drops the final newline of FILEEOF.
                    let mut b = body.to_vec();
                    if b.last() == Some(&b'\n') {
                        b.pop();
                        if b.last() == Some(&b'\r') {
                            b.pop();
                        }
                    }
                    file = Some(b);
                }
                "EXPECT" | "EXPECTF" | "EXPECTREGEX" => {
                    expect = String::from_utf8_lossy(body).into_owned()
                }
                "INI" => {
                    for l in String::from_utf8_lossy(body).lines() {
                        let l = l.replace(' ', "");
                        if l.eq_ignore_ascii_case("short_open_tag=0")
                            || l.eq_ignore_ascii_case("short_open_tag=off")
                            || l.eq_ignore_ascii_case("short_open_tag=false")
                        {
                            short_tags = false;
                        }
                        if l.starts_with("opcache.preload=") {
                            preload = true;
                        }
                        if l.eq_ignore_ascii_case("zend.multibyte=1")
                            || l.eq_ignore_ascii_case("zend.multibyte=on")
                        {
                            multibyte = true;
                        }
                    }
                }
                _ => {}
            }
        }
        let Some(src) = file else { continue };
        if multibyte {
            // Source encodings other than UTF-8 / ASCII are out of scope.
            skipped += 1;
            continue;
        }
        total += 1;
        bytes += src.len();
        let opts = ParseOptions {
            short_open_tag: short_tags,
            in_scripting: false,
            ..ParseOptions::default()
        };
        let t0 = Instant::now();
        let arena = goro_syntax::Arena::new();
        let res = parse_with(&arena, &src, opts);
        parse_time += t0.elapsed();
        let rel = f.strip_prefix(&dir).unwrap_or(f).display().to_string();
        // Parse errors of `eval('literal')` calls must appear in the
        // expected output.
        if res.is_ok() {
            for code in eval_literals(&src) {
                let arena = goro_syntax::Arena::new();
                if let Err(e) = goro_syntax::parse_eval(&arena, &code)
                    && e.kind == ErrorKind::Parse
                {
                    let printed = expect.lines().any(|l| l.contains("arse"));
                    let msg = format!("Parse error: {}", e.message);
                    if expect.contains(&e.message)
                        || expect.lines().any(|l| {
                            l.contains('%')
                                && l.rfind(" in ")
                                    .is_some_and(|p| wild_match(l[..p].trim(), &msg))
                        })
                    {
                        eval_ok += 1;
                    } else if !printed {
                        // The test does not print the message.
                    } else {
                        eval_bad.push(format!(
                            "{}: eval({:?}): {}",
                            rel,
                            String::from_utf8_lossy(&code),
                            e.message
                        ));
                    }
                }
            }
        }
        let exp = expected_parse_error(&expect);
        match (exp, res) {
            (None, Ok(prog)) => {
                ok_accept += 1;
                // Compile errors the parser found must be the test's.
                if let Some(ce) = prog.compile_errors.first() {
                    if expect.contains(&format!("Fatal error: {}", ce.message)) {
                        fatal_ok += 1;
                    } else {
                        fatal_bad
                            .push(format!("{}: unexpected compile error: {}", rel, ce.message));
                    }
                } else if let Some(m) = PARSER_FATALS
                    .iter()
                    .find(|m| expect.contains(&format!("Fatal error: {}", m)))
                {
                    fatal_missed.push(format!("{}: missed Fatal error: {}", rel, m));
                }
            }
            (None, Err(e)) => {
                if e.kind == ErrorKind::Compile
                    && expect.contains(&format!("Fatal error: {}", e.message))
                {
                    ok_accept += 1;
                    fatal_ok += 1;
                } else {
                    bad_reject.push(format!("{}: {}", rel, e));
                }
            }
            (Some(x), Ok(_)) => {
                // The error may come from code the test runs: eval(),
                // include, a child process, a generated file...
                let s = String::from_utf8_lossy(&src);
                let markers = [
                    "eval(",
                    "include",
                    "require",
                    "compile_string",
                    "token_get_all",
                    "PHP_BINARY",
                    "php_cli",
                    "preload",
                    "Phar",
                    "phar",
                    "highlight",
                    "php_strip",
                    "-r",
                    "stream_wrapper_register",
                    "proc_open",
                    "ParseError(",
                    "shell_exec",
                ];
                if markers.iter().any(|m| s.contains(m)) || preload {
                    nested.push(format!("{}: expected Parse error: {}", rel, x.message));
                } else {
                    err_accepted.push(format!("{}: expected Parse error: {}", rel, x.message));
                }
            }
            (Some(x), Err(e)) => {
                let script = format!("/tmp/{}", rel.trim_end_matches('t'));
                let actual = format!("{} in {} on line {}", e.message, script, e.line);
                if e.kind == ErrorKind::Parse && wild_match(&x.full, &actual) {
                    err_match += 1;
                } else if e.kind == ErrorKind::Parse && wild_match(&x.message, &e.message) {
                    err_line_mismatch
                        .push(format!("{}: got line {}, expected {}", rel, e.line, x.full));
                } else {
                    err_msg_mismatch.push(format!(
                        "{}:\n    got:      {}\n    expected: {}",
                        rel, e, x.full
                    ));
                }
            }
        }
    }
    let expected_errors = err_match
        + err_msg_mismatch.len()
        + err_line_mismatch.len()
        + err_accepted.len()
        + nested.len();
    println!("files parsed:            {}", total);
    println!("skipped (zend.multibyte): {}", skipped);
    println!("expected to parse:       {}", total - expected_errors);
    println!("  accepted:              {}", ok_accept);
    println!("  rejected (wrong):      {}", bad_reject.len());
    println!(
        "  parser compile errors: {} matched, {} wrong, {} missed",
        fatal_ok,
        fatal_bad.len(),
        fatal_missed.len()
    );
    println!(
        "eval('...') parse errors: {} found in the expected output, {} not",
        eval_ok,
        eval_bad.len()
    );
    println!("expected a parse error:  {}", expected_errors);
    println!("  exact message + line:  {}", err_match);
    println!("  wrong line:            {}", err_line_mismatch.len());
    println!("  wrong message:         {}", err_msg_mismatch.len());
    println!("  accepted (wrong):      {}", err_accepted.len());
    println!(
        "  from nested code:      {} (eval/include/subprocess: the file itself parses)",
        nested.len()
    );
    if bench {
        let secs = parse_time.as_secs_f64();
        println!(
            "parse time: {:.3} s for {:.1} MB ({:.1} MB/s)",
            secs,
            bytes as f64 / 1e6,
            bytes as f64 / 1e6 / secs
        );
    }
    let mut lines = Vec::new();
    lines.push("== rejected but should parse".to_string());
    lines.extend(bad_reject);
    lines.push("== compile errors found by the parser".to_string());
    lines.extend(fatal_bad);
    lines.extend(fatal_missed);
    lines.push("== eval() parse errors not in the expected output".to_string());
    lines.extend(eval_bad);
    lines.push("== wrong error message".to_string());
    lines.extend(err_msg_mismatch);
    lines.push("== wrong error line".to_string());
    lines.extend(err_line_mismatch);
    lines.push("== accepted but expected a parse error".to_string());
    lines.extend(err_accepted);
    lines.push("== parse error expected from nested code (eval, include, ...)".to_string());
    lines.extend(nested);
    let text = lines.join("\n") + "\n";
    match report {
        Some(p) => std::fs::write(p, text).expect("write report"),
        None => print!("{}", text),
    }
}
