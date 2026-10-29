//! The URL rewriter (PHP manual: output_add_rewrite_var(),
//! session.use_trans_sid): an output handler named "URL-Rewriter" that
//! appends variables to the URLs of the configured tags
//! (url_rewriter.tags, or session.trans_sid_tags for the session ID) and
//! adds hidden inputs to forms, for relative URLs and the allowed hosts
//! (url_rewriter.hosts / session.trans_sid_hosts, else the request's
//! HTTP_HOST). output_add_rewrite_var() and the session each have their
//! own handler; the session's one runs first when it was started later
//! (tests ext/standard/tests/general_functions/url_rewriting_basic*.phpt,
//! output_add_rewrite_var_basic*.phpt, ext/session/tests/session_basic*.phpt).

use std::rc::Rc;

use goro_runtime::{ObjRef, PhpStr, Value};

use crate::func::{Args, Func};
use crate::registry::Module;
use crate::vm::output::{
    ObBuffer, PHP_OUTPUT_HANDLER_CLEAN, PHP_OUTPUT_HANDLER_FINAL, PHP_OUTPUT_HANDLER_FLUSH,
    PHP_OUTPUT_HANDLER_STDFLAGS,
};
use crate::vm::{R, Vm};

/// output_add_rewrite_var() and output_reset_rewrite_vars() belong to
/// ext/standard.
pub static MODULE: Module = Module {
    name: "standard",
    functions: &[
        (
            "function output_add_rewrite_var(string $name, string $value): bool",
            output_add_rewrite_var,
        ),
        (
            "function output_reset_rewrite_vars(): bool",
            output_reset_rewrite_vars,
        ),
    ],
    listed: false,
    ..Module::EMPTY
};

/// One rewriter: its variables, its handler and what it holds back (an
/// unfinished tag at the end of a chunk).
#[derive(Default)]
struct Rewriter {
    vars: Vec<(Vec<u8>, Vec<u8>)>,
    handler: Option<ObjRef>,
    carry: Vec<u8>,
}

#[derive(Default)]
struct Rw {
    /// output_add_rewrite_var()'s.
    out: Rewriter,
    /// The session ID's.
    sess: Rewriter,
}

#[derive(Clone, Copy)]
enum Which {
    Out,
    Sess,
}

fn rw(vm: &mut Vm, w: Which) -> &mut Rewriter {
    let r = vm.ext::<Rw>();
    match w {
        Which::Out => &mut r.out,
        Which::Sess => &mut r.sess,
    }
}

/// Whether the rewriter's handler is on the output buffer stack.
fn handler_active(vm: &mut Vm, w: Which) -> bool {
    let Some(h) = rw(vm, w).handler.clone() else {
        return false;
    };
    vm.out.buffers.iter().any(|b| {
        matches!(&b.callback, Some(Value::Object(o)) if o.handle() == h.handle())
    })
}

/// Pushes the rewriter's output handler if it is not running.
fn ensure_handler(vm: &mut Vm, w: Which) -> R<()> {
    if handler_active(vm, w) || vm.out.deactivated {
        return Ok(());
    }
    let f: crate::func::NativeFn = match w {
        Which::Out => out_handler,
        Which::Sess => sess_handler,
    };
    let func = Rc::new(Func::native(
        "URL-Rewriter",
        "function url_rewriter(string $buffer, int $phase): string",
        f,
        0,
    ));
    let o = vm.closure_from_callee(crate::vm::calls::Callee::func(func))?;
    rw(vm, w).handler = Some(o.clone());
    rw(vm, w).carry.clear();
    vm.out.buffers.push(ObBuffer::new(
        Some(Value::Object(o)),
        0,
        PHP_OUTPUT_HANDLER_STDFLAGS,
        PhpStr::from("URL-Rewriter"),
    ));
    Ok(())
}

pub(crate) fn output_add_rewrite_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let name = a.str(0).to_vec();
    let value = a.str(1).to_vec();
    let r = rw(vm, Which::Out);
    match r.vars.iter_mut().find(|(n, _)| *n == name) {
        Some(e) => e.1 = value,
        None => r.vars.push((name, value)),
    }
    ensure_handler(vm, Which::Out)?;
    Ok(Value::Bool(true))
}

pub(crate) fn output_reset_rewrite_vars(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    rw(vm, Which::Out).vars.clear();
    Ok(Value::Bool(true))
}

/// session_start() with transparent session IDs.
pub(crate) fn session_started(vm: &mut Vm, name: &[u8], id: &[u8]) {
    rw(vm, Which::Sess).vars = vec![(name.to_vec(), id.to_vec())];
    let r = ensure_handler(vm, Which::Sess);
    vm.handle_unwind(r);
}

/// The session ended; output already buffered keeps its rewriting.
pub(crate) fn session_ended(_vm: &mut Vm) {}

fn out_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    handle(vm, a, Which::Out)
}

fn sess_handler(vm: &mut Vm, a: &mut Args) -> R<Value> {
    handle(vm, a, Which::Sess)
}

fn handle(vm: &mut Vm, a: &mut Args, w: Which) -> R<Value> {
    let input = a.str(0);
    let mode = a.int_or(1, 0);
    let mut data = std::mem::take(&mut rw(vm, w).carry);
    if mode & PHP_OUTPUT_HANDLER_CLEAN != 0 {
        data.clear();
    }
    data.extend_from_slice(&input);
    let vars = rw(vm, w).vars.clone();
    let fin = mode & (PHP_OUTPUT_HANDLER_FINAL | PHP_OUTPUT_HANDLER_FLUSH) != 0;
    if vars.is_empty() {
        return Ok(Value::Str(PhpStr::from(data)));
    }
    let cfg = Config::load(vm, w, vars);
    let (out, rest) = rewrite(&cfg, &data, fin);
    rw(vm, w).carry = rest;
    Ok(Value::Str(PhpStr::from(out)))
}

/// What a rewriter applies.
struct Config {
    /// (tag, attribute) pairs; an empty attribute is a form's hidden
    /// inputs.
    tags: Vec<(Vec<u8>, Vec<u8>)>,
    hosts: Vec<Vec<u8>>,
    /// "name=value" pairs joined with arg_separator.output.
    query: Vec<u8>,
    inputs: Vec<u8>,
    sep: Vec<u8>,
}

fn html_escape(s: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'&' => o.extend_from_slice(b"&amp;"),
            b'<' => o.extend_from_slice(b"&lt;"),
            b'>' => o.extend_from_slice(b"&gt;"),
            b'"' => o.extend_from_slice(b"&quot;"),
            b'\'' => o.extend_from_slice(b"&#039;"),
            _ => o.push(c),
        }
    }
    o
}

impl Config {
    fn load(vm: &mut Vm, w: Which, vars: Vec<(Vec<u8>, Vec<u8>)>) -> Config {
        let (tk, hk) = match w {
            Which::Out => ("url_rewriter.tags", "url_rewriter.hosts"),
            Which::Sess => ("session.trans_sid_tags", "session.trans_sid_hosts"),
        };
        let tags_s = super::ini_str(vm, tk);
        let mut tags = Vec::new();
        for e in tags_s.split(|&c| c == b',') {
            let e = e.trim_ascii();
            if e.is_empty() {
                continue;
            }
            let (t, at) = match e.iter().position(|&c| c == b'=') {
                Some(p) => (&e[..p], &e[p + 1..]),
                None => (e, &b""[..]),
            };
            tags.push((t.trim_ascii().to_ascii_lowercase(), at.trim_ascii().to_ascii_lowercase()));
        }
        let hosts_s = super::ini_str(vm, hk);
        let mut hosts: Vec<Vec<u8>> = hosts_s
            .split(|&c| c == b',')
            .map(|h| h.trim_ascii().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        if hosts.is_empty() {
            // The request's host (test session_basic5.phpt).
            if let Some(p) = vm.global_var_ptr(&PhpStr::from("_SERVER"), false) {
                // SAFETY: read of the global's slot.
                if let Value::Array(s) = unsafe { (*p).deref() }
                    && let Some(Value::Str(h)) = s.get_str(&PhpStr::from("HTTP_HOST")).map(|v| v.deref())
                {
                    hosts.push(h.to_ascii_lowercase());
                }
            }
        }
        let sep = super::ini_str(vm, "arg_separator.output").to_vec();
        let sep = if sep.is_empty() { b"&".to_vec() } else { sep };
        let mut query = Vec::new();
        let mut inputs = Vec::new();
        for (i, (n, v)) in vars.iter().enumerate() {
            if i > 0 {
                query.extend_from_slice(&sep);
            }
            query.extend_from_slice(&goro_ext_standard::url::urlencode(n));
            query.push(b'=');
            query.extend_from_slice(&goro_ext_standard::url::urlencode(v));
            inputs.extend_from_slice(b"<input type=\"hidden\" name=\"");
            inputs.extend_from_slice(&html_escape(n));
            inputs.extend_from_slice(b"\" value=\"");
            inputs.extend_from_slice(&html_escape(v));
            inputs.extend_from_slice(b"\" />");
        }
        Config {
            tags,
            hosts,
            query,
            inputs,
            sep,
        }
    }

    fn attr_of(&self, tag: &[u8]) -> Option<&[u8]> {
        self.tags
            .iter()
            .find(|(t, a)| t == tag && !a.is_empty())
            .map(|(_, a)| a.as_slice())
    }

    fn has_form(&self) -> bool {
        self.tags.iter().any(|(t, a)| t == b"form" && a.is_empty())
    }

    /// Whether a URL gets the variables: relative ones, and http(s) or
    /// protocol-relative ones to an allowed host; not fragments alone.
    fn allowed(&self, url: &[u8]) -> bool {
        let u = url.trim_ascii();
        if u.first() == Some(&b'#') {
            return false;
        }
        let host_part = if let Some(rest) = u.strip_prefix(b"//") {
            Some(rest)
        } else {
            let colon = u.iter().position(|&c| c == b':');
            let first_sep = u.iter().position(|&c| matches!(c, b'/' | b'?' | b'#'));
            match colon {
                Some(c) if first_sep.is_none_or(|s| c < s) => {
                    let scheme = u[..c].to_ascii_lowercase();
                    if scheme != b"http" && scheme != b"https" {
                        return false;
                    }
                    match u[c + 1..].strip_prefix(b"//") {
                        Some(rest) => Some(rest),
                        None => return false,
                    }
                }
                _ => None,
            }
        };
        let Some(rest) = host_part else {
            return true;
        };
        let end = rest
            .iter()
            .position(|&c| matches!(c, b'/' | b'?' | b'#'))
            .unwrap_or(rest.len());
        let mut host = &rest[..end];
        if let Some(at) = host.iter().rposition(|&c| c == b'@') {
            host = &host[at + 1..];
        }
        if let Some(p) = host.iter().rposition(|&c| c == b':') {
            host = &host[..p];
        }
        let host = host.to_ascii_lowercase();
        self.hosts.iter().any(|h| *h == host)
    }

    /// The URL with the variables added.
    fn append(&self, url: &[u8]) -> Vec<u8> {
        let (base, frag) = match url.iter().position(|&c| c == b'#') {
            Some(p) => (&url[..p], &url[p..]),
            None => (url, &b""[..]),
        };
        let mut out = base.to_vec();
        if frag.is_empty() && no_path(base) {
            out.push(b'/');
        }
        if base.contains(&b'?') {
            out.extend_from_slice(&self.sep);
        } else {
            out.push(b'?');
        }
        out.extend_from_slice(&self.query);
        out.extend_from_slice(frag);
        out
    }
}

/// An absolute URL without a path ("http://host", "//host:8443").
fn no_path(u: &[u8]) -> bool {
    let rest = if let Some(r) = u.strip_prefix(b"//") {
        r
    } else if let Some(p) = u.windows(3).position(|w| w == b"://") {
        &u[p + 3..]
    } else {
        return false;
    };
    !rest.iter().any(|&c| matches!(c, b'/' | b'?'))
}

fn is_name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.')
}

/// Rewrites `data`; an unfinished tag at the end is returned to hold back
/// unless `fin`.
fn rewrite(cfg: &Config, data: &[u8], fin: bool) -> (Vec<u8>, Vec<u8>) {
    let mut out = Vec::with_capacity(data.len() + 64);
    let mut i = 0;
    let n = data.len();
    while i < n {
        let Some(off) = data[i..].iter().position(|&c| c == b'<') else {
            out.extend_from_slice(&data[i..]);
            break;
        };
        out.extend_from_slice(&data[i..i + off]);
        let start = i + off;
        let mut p = start + 1;
        while p < n && is_name(data[p]) {
            p += 1;
        }
        let tag = data[start + 1..p].to_ascii_lowercase();
        if tag.is_empty() {
            out.push(b'<');
            i = start + 1;
            continue;
        }
        // The attributes, up to the tag's end.
        let mut attrs: Vec<(Vec<u8>, usize, usize, u8)> = Vec::new();
        let mut closed = None;
        while p < n {
            let c = data[p];
            if c == b'>' {
                closed = Some(p);
                break;
            }
            if c.is_ascii_whitespace() || c == b'/' {
                p += 1;
                continue;
            }
            let ns = p;
            while p < n && !data[p].is_ascii_whitespace() && !matches!(data[p], b'=' | b'>') {
                p += 1;
            }
            let aname = data[ns..p].to_ascii_lowercase();
            let mut q = p;
            while q < n && data[q].is_ascii_whitespace() {
                q += 1;
            }
            if q < n && data[q] == b'=' {
                q += 1;
                while q < n && data[q].is_ascii_whitespace() {
                    q += 1;
                }
                if q >= n {
                    break;
                }
                let quote = data[q];
                if quote == b'"' || quote == b'\'' {
                    let vs = q + 1;
                    match data[vs..].iter().position(|&c| c == quote) {
                        Some(e) => {
                            attrs.push((aname, vs, vs + e, quote));
                            p = vs + e + 1;
                        }
                        None => break,
                    }
                } else {
                    let vs = q;
                    let mut e = q;
                    while e < n && !data[e].is_ascii_whitespace() && data[e] != b'>' {
                        e += 1;
                    }
                    attrs.push((aname, vs, e, 0));
                    p = e;
                }
            } else {
                attrs.push((aname, p, p, 1));
            }
        }
        let Some(end) = closed else {
            // An unfinished tag.
            if fin {
                out.extend_from_slice(&data[start..]);
                return (out, Vec::new());
            }
            return (out, data[start..].to_vec());
        };
        let attr = cfg.attr_of(&tag);
        let mut last = start;
        if let Some(want) = attr {
            for (an, vs, ve, q) in &attrs {
                if an.as_slice() == want && *q != 1 {
                    let url = &data[*vs..*ve];
                    if cfg.allowed(url) {
                        out.extend_from_slice(&data[last..*vs]);
                        out.extend_from_slice(&cfg.append(url));
                        last = *ve;
                    }
                    break;
                }
            }
        }
        out.extend_from_slice(&data[last..=end]);
        if tag == b"form" && cfg.has_form() {
            let action = attrs
                .iter()
                .find(|(an, _, _, q)| an == b"action" && *q != 1)
                .map(|(_, vs, ve, _)| &data[*vs..*ve]);
            if action.is_none_or(|u| cfg.allowed(u)) {
                out.extend_from_slice(&cfg.inputs);
            }
        }
        i = end + 1;
    }
    (out, Vec::new())
}
