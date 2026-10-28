//! Syntax highlighting and stripping (PHP manual: highlight_string,
//! highlight_file, show_source, php_strip_whitespace). The output layout
//! and the token classes are as observed on PHP 8.5: one `<span>` per run
//! of tokens of a class, whitespace joining the current run, tabs as four
//! spaces, `&`, `<` and `>` escaped, inside `<pre><code>`.

use goro_runtime::{PhpStr, Value};
use goro_syntax::token::T;

use super::string::sv;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function highlight_string(string $string, bool $return = false): string|true",
            highlight_string,
        ),
        (
            "function highlight_file(string $filename, bool $return = false): string|bool",
            highlight_file,
        ),
        (
            "function show_source(string $filename, bool $return = false): string|bool",
            highlight_file,
        ),
        (
            "function php_strip_whitespace(string $filename): string",
            php_strip_whitespace,
        ),
    ],
    listed: false,
    ..Module::EMPTY
};

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Html,
    Comment,
    Default,
    Keyword,
    Str,
}

fn class_of(t: T) -> Class {
    match t {
        T::InlineHtml => Class::Html,
        T::Comment | T::DocComment => Class::Comment,
        T::ConstantEncapsedString | T::EncapsedAndWhitespace | T::DoubleQuote => Class::Str,
        T::OpenTag
        | T::OpenTagWithEcho
        | T::CloseTag
        | T::Variable
        | T::String
        | T::NameFullyQualified
        | T::NameRelative
        | T::NameQualified
        | T::LNumber
        | T::DNumber
        | T::StringVarname
        | T::NumString
        | T::Line
        | T::File
        | T::Dir
        | T::ClassC
        | T::TraitC
        | T::MethodC
        | T::FuncC
        | T::PropertyC
        | T::NsC => Class::Default,
        _ => Class::Keyword,
    }
}

fn escape(out: &mut Vec<u8>, b: &[u8]) {
    for &c in b {
        match c {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'\t' => out.extend_from_slice(b"    "),
            c => out.push(c),
        }
    }
}

fn color(vm: &Vm, c: Class) -> PhpStr {
    let k: &[u8] = match c {
        Class::Html => b"highlight.html",
        Class::Comment => b"highlight.comment",
        Class::Default => b"highlight.default",
        Class::Keyword => b"highlight.keyword",
        Class::Str => b"highlight.string",
    };
    vm.get_ini(k).unwrap_or_default()
}

/// The highlighted HTML of a PHP source.
pub(crate) fn highlight(vm: &Vm, src: &[u8]) -> Vec<u8> {
    let toks = goro_syntax::lexer::tokenize(src, false);
    let mut out = Vec::with_capacity(src.len() * 2);
    out.extend_from_slice(b"<pre><code style=\"color: ");
    out.extend_from_slice(&color(vm, Class::Html));
    out.extend_from_slice(b"\">");
    let mut cur = Class::Html;
    let mut pos = 0usize;
    for t in &toks {
        let (s, e) = (t.start as usize, t.end as usize);
        if s > pos {
            // Bytes the lexer skipped (should not happen) keep the class.
            escape(&mut out, &src[pos..s]);
        }
        let text = &src[s..e.min(src.len())];
        if t.kind != T::Whitespace {
            let c = class_of(t.kind);
            if c != cur {
                if cur != Class::Html {
                    out.extend_from_slice(b"</span>");
                }
                if c != Class::Html {
                    out.extend_from_slice(b"<span style=\"color: ");
                    out.extend_from_slice(&color(vm, c));
                    out.extend_from_slice(b"\">");
                }
                cur = c;
            }
        }
        escape(&mut out, text);
        pos = e;
    }
    if pos < src.len() {
        escape(&mut out, &src[pos..]);
    }
    if cur != Class::Html {
        out.extend_from_slice(b"</span>");
    }
    out.extend_from_slice(b"</code></pre>");
    out
}

fn emit(vm: &mut Vm, a: &Args, html: Vec<u8>) -> R<Value> {
    if a.bool_or(1, false) {
        return Ok(sv(html));
    }
    vm.echo(&html);
    Ok(Value::Bool(true))
}

fn highlight_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let html = highlight(vm, a.bytes(0));
    emit(vm, a, html)
}

/// A whole file through the stream layer (wrappers such as data: work).
fn read_source(vm: &mut Vm, a: &Args, name: &[u8]) -> R<Option<Vec<u8>>> {
    // RFC 2397 data: URLs ("data:,text", "data:...;base64,...").
    if name.len() >= 5 && name[..5].eq_ignore_ascii_case(b"data:") {
        let rest = &name[5..];
        if let Some(c) = rest.iter().position(|&b| b == b',') {
            let meta = &rest[..c];
            let body = &rest[c + 1..];
            if meta.ends_with(b";base64") {
                if let Some(d) = goro_ext_standard::url::base64_decode(body, false) {
                    return Ok(Some(d));
                }
            } else {
                return Ok(Some(goro_ext_standard::url::rawurldecode(body)));
            }
        }
    }
    let Some(mut st) = super::file::open_stream(vm, a, name, b"r", false)? else {
        return Ok(None);
    };
    let mut data = Vec::new();
    let mut buf = vec![0u8; 8192];
    loop {
        match st.read_some(vm, &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
        }
    }
    Ok(Some(data))
}

fn highlight_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let Some(src) = read_source(vm, a, &f)? else {
        vm.fwarn(
            a,
            &format!("Failed opening '{}' for highlighting", f.to_string_lossy()),
        )?;
        return Ok(Value::Bool(false));
    };
    let html = highlight(vm, &src);
    emit(vm, a, html)
}

/// PHP manual: php_strip_whitespace(): comments go, whitespace runs become
/// one space (the open tag keeps its own whitespace).
fn php_strip_whitespace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let Some(src) = read_source(vm, a, &f)? else {
        return Ok(Value::Str(PhpStr::empty()));
    };
    let toks = goro_syntax::lexer::tokenize(&src, false);
    let mut out = Vec::with_capacity(src.len());
    let mut last_ws = false;
    for t in &toks {
        let text = &src[t.start as usize..(t.end as usize).min(src.len())];
        match t.kind {
            T::Comment | T::DocComment => {}
            T::Whitespace => {
                if !last_ws {
                    out.push(b' ');
                    last_ws = true;
                }
            }
            _ => {
                out.extend_from_slice(text);
                last_ws = false;
            }
        }
    }
    Ok(sv(out))
}
