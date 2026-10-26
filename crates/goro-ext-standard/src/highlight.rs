//! Syntax highlighting and whitespace stripping: ports of
//! Zend/zend_highlight.c (zend_highlight, zend_strip, zend_html_putc,
//! zend_html_puts) and the highlight_string(), highlight_file()/show_source()
//! and php_strip_whitespace() functions of ext/standard/basic_functions.c.
//!
//! Both are driven by the raw PHP token stream from
//! [`goro_parser::php_scanner`], which reproduces zend_language_scanner's
//! token boundaries outside parser mode.

use goro_core::opcode::ParamType;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_parser::php_scanner::{self as ps, Scanner};

const INI_DEFAULTS: [(&[u8], &[u8]); 5] = [
    (b"highlight.comment", b"#FF8000"),
    (b"highlight.default", b"#0000BB"),
    (b"highlight.html", b"#000000"),
    (b"highlight.keyword", b"#007700"),
    (b"highlight.string", b"#DD0000"),
];

pub fn register(vm: &mut Vm) {
    for (k, v) in INI_DEFAULTS {
        vm.constants
            .entry(k.to_vec())
            .or_insert_with(|| Value::String(PhpString::from_bytes(v)));
    }
    vm.register_function(b"highlight_string", highlight_string_fn);
    vm.register_function(b"highlight_file", highlight_file_fn);
    vm.register_function(b"show_source", show_source_fn);
    vm.register_function(b"php_strip_whitespace", php_strip_whitespace_fn);
    let s = || Some(ParamType::Simple(b"string".to_vec()));
    let b = || Some(ParamType::Simple(b"bool".to_vec()));
    vm.register_builtin_param_types(b"highlight_string", vec![s(), b()]);
    vm.register_builtin_param_types(b"highlight_file", vec![s(), b()]);
    vm.register_builtin_param_types(b"show_source", vec![s(), b()]);
    vm.register_builtin_param_types(b"php_strip_whitespace", vec![s()]);
}

/// zend_syntax_highlighter_ini, filled by php_get_highlight_struct().
struct HighlighterIni {
    comment: Vec<u8>,
    default: Vec<u8>,
    html: Vec<u8>,
    keyword: Vec<u8>,
    string: Vec<u8>,
}

/// zend_highlight() of a whole source (the CLI's -s mode).
pub fn highlight_source(vm: &Vm, src: &[u8]) -> Vec<u8> {
    zend_highlight(src, true, &get_highlight_struct(vm))
}

/// zend_strip() of a whole source (the CLI's -w mode).
pub fn strip_source(src: &[u8]) -> Vec<u8> {
    zend_strip(src, true)
}

fn get_highlight_struct(vm: &Vm) -> HighlighterIni {
    let get = |k: &[u8], d: &[u8]| -> Vec<u8> {
        vm.constants
            .get(k)
            .map(|v| v.to_php_string().as_bytes().to_vec())
            .unwrap_or_else(|| d.to_vec())
    };
    HighlighterIni {
        comment: get(b"highlight.comment", b"#FF8000"),
        default: get(b"highlight.default", b"#0000BB"),
        html: get(b"highlight.html", b"#000000"),
        keyword: get(b"highlight.keyword", b"#007700"),
        string: get(b"highlight.string", b"#DD0000"),
    }
}

/// zend_html_puts (without an output filter).
fn html_puts(out: &mut Vec<u8>, s: &[u8]) {
    for &c in s {
        match c {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'&' => out.extend_from_slice(b"&amp;"),
            b'\t' => out.extend_from_slice(b"    "),
            _ => out.push(c),
        }
    }
}

/// The highlighter compares color *pointers*; which INI entry a token uses
/// is what matters, not the color strings themselves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Color {
    Html,
    Comment,
    Default,
    Keyword,
    String,
}

fn color_str(ini: &HighlighterIni, c: Color) -> &[u8] {
    match c {
        Color::Html => &ini.html,
        Color::Comment => &ini.comment,
        Color::Default => &ini.default,
        Color::Keyword => &ini.keyword,
        Color::String => &ini.string,
    }
}

/// zend_highlight()
fn zend_highlight(src: &[u8], skip_shebang: bool, ini: &HighlighterIni) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() * 2 + 64);
    let mut last_color = Color::Html;
    out.extend_from_slice(b"<pre><code style=\"color: ");
    out.extend_from_slice(&ini.html);
    out.extend_from_slice(b"\">");
    let mut sc = Scanner::new(src, goro_parser::short_open_tag(), skip_shebang);
    while let Some(t) = sc.next_token() {
        let text = &src[t.start..t.end];
        let next_color = match t.id {
            ps::T_INLINE_HTML => Color::Html,
            ps::T_COMMENT | ps::T_DOC_COMMENT => Color::Comment,
            ps::T_OPEN_TAG
            | ps::T_OPEN_TAG_WITH_ECHO
            | ps::T_CLOSE_TAG
            | ps::T_LINE
            | ps::T_FILE
            | ps::T_DIR
            | ps::T_TRAIT_C
            | ps::T_METHOD_C
            | ps::T_FUNC_C
            | ps::T_PROPERTY_C
            | ps::T_NS_C
            | ps::T_CLASS_C => Color::Default,
            34 /* '"' */ | ps::T_ENCAPSED_AND_WHITESPACE | ps::T_CONSTANT_ENCAPSED_STRING => Color::String,
            ps::T_WHITESPACE => {
                html_puts(&mut out, text); // no color needed
                continue;
            }
            id => {
                if ps::token_has_value(id) {
                    Color::Default
                } else {
                    Color::Keyword
                }
            }
        };
        if last_color != next_color {
            if last_color != Color::Html {
                out.extend_from_slice(b"</span>");
            }
            last_color = next_color;
            if last_color != Color::Html {
                out.extend_from_slice(b"<span style=\"color: ");
                out.extend_from_slice(color_str(ini, last_color));
                out.extend_from_slice(b"\">");
            }
        }
        html_puts(&mut out, text);
    }
    if last_color != Color::Html {
        out.extend_from_slice(b"</span>");
    }
    out.extend_from_slice(b"</code></pre>");
    out
}

/// zend_strip()
fn zend_strip(src: &[u8], skip_shebang: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    let mut prev_space = false;
    let mut sc = Scanner::new(src, goro_parser::short_open_tag(), skip_shebang);
    while let Some(t) = sc.next_token() {
        match t.id {
            ps::T_WHITESPACE => {
                if !prev_space {
                    out.push(b' ');
                    prev_space = true;
                }
                continue;
            }
            ps::T_COMMENT | ps::T_DOC_COMMENT => continue,
            ps::T_END_HEREDOC => {
                out.extend_from_slice(&src[t.start..t.end]);
                // read the following character, either newline or ;
                if let Some(n) = sc.next_token() {
                    if n.id != ps::T_WHITESPACE {
                        out.extend_from_slice(&src[n.start..n.end]);
                    }
                }
                out.push(b'\n');
                prev_space = true;
                continue;
            }
            _ => out.extend_from_slice(&src[t.start..t.end]),
        }
        prev_space = false;
    }
    out
}

fn value_error(vm: &mut Vm, msg: String) -> VmError {
    let exc = vm.create_exception(b"ValueError", &msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError {
        message: msg,
        line: vm.current_line,
    }
}

/// php_strip_url_passwd()
fn strip_url_passwd(url: &str) -> String {
    let b = url.as_bytes();
    if let Some(proto) = url.find("://") {
        let start = proto + 3;
        if let Some(at) = b[start..].iter().position(|&c| c == b'@') {
            let at = start + at;
            let dots = (at - start).min(3);
            let mut out = b[..start].to_vec();
            out.extend(std::iter::repeat_n(b'.', dots));
            out.extend_from_slice(&b[at..]);
            return String::from_utf8_lossy(&out).into_owned();
        }
    }
    url.to_string()
}

fn ini_flag(vm: &Vm, key: &[u8], default: bool) -> bool {
    match vm.constants.get(key) {
        None => default,
        Some(v) => {
            let s = v.to_php_string().to_string_lossy();
            let t = s.trim();
            t.eq_ignore_ascii_case("on")
                || t.eq_ignore_ascii_case("yes")
                || t.eq_ignore_ascii_case("true")
                || t.parse::<i64>().map(|n| n != 0).unwrap_or(false)
        }
    }
}

/// Open a file the way zend_stream_open()/php_stream_open_for_zend_ex() does
/// (USE_PATH | REPORT_ERRORS | STREAM_OPEN_FOR_INCLUDE) and read it whole.
/// Emits the stream-layer warnings under `fname`; returns None on failure.
fn open_for_scanning(vm: &mut Vm, fname: &str, path: &str) -> Option<Vec<u8>> {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("data:") {
        let reason = if !ini_flag(vm, b"allow_url_fopen", true) {
            Some("allow_url_fopen=0")
        } else if !ini_flag(vm, b"allow_url_include", false) {
            Some("allow_url_include=0")
        } else {
            None
        };
        if let Some(r) = reason {
            vm.emit_warning(&format!(
                "{}(): data:// wrapper is disabled in the server configuration by {}",
                fname, r
            ));
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: no suitable wrapper could be found",
                fname, path
            ));
            return None;
        }
    }
    if path.contains("://") && !lower.starts_with("file://") || lower.starts_with("data:") {
        let args = [
            Value::String(PhpString::from_string(path.to_string())),
            Value::String(PhpString::from_bytes(b"rb")),
        ];
        let fp = crate::misc::fopen_fn(vm, &args).ok()?;
        let Value::Resource(id) = fp else { return None };
        let data = crate::misc::stream_get_contents_fn(vm, &[Value::resource(&id)]).ok();
        let _ = crate::misc::fclose_fn(vm, &[Value::resource(&id)]);
        return match data {
            Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
            _ => None,
        };
    }
    let plain = path.strip_prefix("file://").unwrap_or(path);
    if !goro_core::basedir::check_open_basedir_ex(vm, fname, plain, true) {
        vm.emit_warning(&format!(
            "{}({}): Failed to open stream: Operation not permitted",
            fname, path
        ));
        return None;
    }
    let resolved = crate::misc::resolve_with_include_path(vm, plain, true);
    let result = if std::path::Path::new(&resolved).is_dir() {
        Err(std::io::Error::from_raw_os_error(21))
    } else {
        std::fs::read(&resolved)
    };
    match result {
        Ok(data) => Some(data),
        Err(e) => {
            vm.emit_warning(&format!(
                "{}({}): Failed to open stream: {}",
                fname,
                path,
                crate::misc::strip_os_error(&e.to_string())
            ));
            None
        }
    }
}

/// Minimal zpp check: arrays are never accepted for string/bool params.
fn check_args(
    vm: &mut Vm,
    args: &[Value],
    fname: &str,
    names: &[(&str, &str)],
) -> Result<(), VmError> {
    for (i, (name, ty)) in names.iter().enumerate() {
        if let Some(Value::Array(_)) = args.get(i).map(|v| v.deref()) {
            let msg = format!(
                "{}(): Argument #{} (${}) must be of type {}, array given",
                fname,
                i + 1,
                name,
                ty
            );
            let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
            vm.current_exception = Some(exc);
            return Err(VmError {
                message: msg,
                line: vm.current_line,
            });
        }
    }
    Ok(())
}

fn arg_bool(args: &[Value], i: usize) -> bool {
    args.get(i).map(|v| v.is_truthy()).unwrap_or(false)
}

fn finish(vm: &mut Vm, out: Vec<u8>, ret: bool) -> Result<Value, VmError> {
    if ret {
        Ok(Value::String(PhpString::from_vec(out)))
    } else {
        vm.write_output(&out);
        Ok(Value::True)
    }
}

/// PHP_FUNCTION(highlight_string)
fn highlight_string_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_args(
        vm,
        args,
        "highlight_string",
        &[("string", "string"), ("return", "bool")],
    )?;
    let code = args.first().unwrap_or(&Value::Null).to_php_string();
    let ret = arg_bool(args, 1);
    let ini = get_highlight_struct(vm);
    let out = zend_highlight(code.as_bytes(), false, &ini);
    finish(vm, out, ret)
}

/// PHP_FUNCTION(highlight_file) (also show_source)
fn highlight_file_impl(vm: &mut Vm, args: &[Value], fname: &str) -> Result<Value, VmError> {
    check_args(
        vm,
        args,
        fname,
        &[("filename", "string"), ("return", "bool")],
    )?;
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if path.as_bytes().contains(&0) {
        return Err(value_error(
            vm,
            format!(
                "{}(): Argument #1 ($filename) must not contain any null bytes",
                fname
            ),
        ));
    }
    let ret = arg_bool(args, 1);
    let path = path.to_string_lossy().to_string();
    if !path.contains("://") || path.starts_with("file://") {
        let plain = path.strip_prefix("file://").unwrap_or(&path).to_string();
        if !goro_core::basedir::check_open_basedir_ex(vm, fname, &plain, true) {
            return Ok(Value::False);
        }
    }
    let ini = get_highlight_struct(vm);
    if path.is_empty() {
        // (php_stream_open_wrapper() throws; highlight_file() then warns)
        vm.emit_warning(&format!("{}(): Failed opening '' for highlighting", fname));
        return Err(value_error(vm, "Path must not be empty".to_string()));
    }
    match open_for_scanning(vm, fname, &path) {
        Some(src) => {
            let out = zend_highlight(&src, true, &ini);
            finish(vm, out, ret)
        }
        None => {
            vm.emit_warning(&format!(
                "{}(): Failed opening '{}' for highlighting",
                fname,
                strip_url_passwd(&path)
            ));
            Ok(Value::False)
        }
    }
}

fn highlight_file_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    highlight_file_impl(vm, args, "highlight_file")
}

fn show_source_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    highlight_file_impl(vm, args, "show_source")
}

/// PHP_FUNCTION(php_strip_whitespace)
fn php_strip_whitespace_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    check_args(vm, args, "php_strip_whitespace", &[("filename", "string")])?;
    let path = args.first().unwrap_or(&Value::Null).to_php_string();
    if path.as_bytes().contains(&0) {
        return Err(value_error(
            vm,
            "php_strip_whitespace(): Argument #1 ($filename) must not contain any null bytes"
                .to_string(),
        ));
    }
    let path = path.to_string_lossy().to_string();
    if path.is_empty() {
        // (php_stream_open_wrapper())
        return Err(value_error(vm, "Path must not be empty".to_string()));
    }
    match open_for_scanning(vm, "php_strip_whitespace", &path) {
        Some(src) => Ok(Value::String(PhpString::from_vec(zend_strip(&src, true)))),
        None => Ok(Value::String(PhpString::from_bytes(b""))),
    }
}
