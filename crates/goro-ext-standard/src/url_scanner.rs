//! URL rewriter (ext/standard/url_scanner_ex.re): output_add_rewrite_var()
//! and the session trans-sid rewriter share this scanner.

use std::cell::RefCell;

use goro_core::ob::{ObKind, OBH_STDFLAGS, OB_OP_FINAL, OB_OP_FLUSH};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

#[derive(Clone, Copy, PartialEq, Eq)]
enum St {
    Plain,
    Tag,
    NextArg,
    Arg,
    BeforeVal,
    Val,
}

struct Ctx {
    url_app: Vec<u8>,
    form_app: Vec<u8>,
    buf: Vec<u8>,
    result: Vec<u8>,
    tag: Vec<u8>,
    arg: Vec<u8>,
    val: Vec<u8>,
    attr_val: Vec<u8>,
    lookup_data: Option<Vec<u8>>,
    state: St,
    tag_form: bool,
    attr_action: bool,
    active: bool,
    is_session: bool,
    /// tag name (lowercase) -> attribute
    tags: Vec<(Vec<u8>, Vec<u8>)>,
    hosts: Vec<Vec<u8>>,
}

impl Ctx {
    fn new(is_session: bool, tags: &[u8]) -> Ctx {
        let mut c = Ctx {
            url_app: Vec::new(),
            form_app: Vec::new(),
            buf: Vec::new(),
            result: Vec::new(),
            tag: Vec::new(),
            arg: Vec::new(),
            val: Vec::new(),
            attr_val: Vec::new(),
            lookup_data: None,
            state: St::Plain,
            tag_form: false,
            attr_action: false,
            active: false,
            is_session,
            tags: Vec::new(),
            hosts: Vec::new(),
        };
        c.set_tags(tags);
        c
    }

    fn set_tags(&mut self, v: &[u8]) {
        self.tags.clear();
        for key in v.split(|&c| c == b',').filter(|k| !k.is_empty()) {
            if let Some(eq) = key.iter().position(|&c| c == b'=') {
                let name = key[..eq].to_ascii_lowercase();
                if !self.tags.iter().any(|(n, _)| *n == name) {
                    self.tags.push((name, key[eq + 1..].to_vec()));
                }
            }
        }
    }

    fn set_hosts(&mut self, v: &[u8]) {
        self.hosts.clear();
        for key in v.split(|&c| c == b',').filter(|k| !k.is_empty()) {
            self.hosts.push(key.to_ascii_lowercase());
        }
    }
}

thread_local! {
    static SESSION: RefCell<Ctx> = RefCell::new(Ctx::new(true, b"a=href,area=href,frame=src,form="));
    static OUTPUT: RefCell<Ctx> = RefCell::new(Ctx::new(false, b"form="));
}

fn with<T>(is_session: bool, f: impl FnOnce(&mut Ctx) -> T) -> T {
    if is_session {
        SESSION.with(|c| f(&mut c.borrow_mut()))
    } else {
        OUTPUT.with(|c| f(&mut c.borrow_mut()))
    }
}

/// OnUpdate*Tags
pub fn set_tags(is_session: bool, value: &[u8]) {
    with(is_session, |c| c.set_tags(value));
}

/// OnUpdate*Hosts
pub fn set_hosts(is_session: bool, value: &[u8]) {
    with(is_session, |c| c.set_hosts(value));
}

#[derive(Default)]
struct Url {
    scheme: Option<Vec<u8>>,
    user: Option<Vec<u8>>,
    pass: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
    port: Option<u16>,
    path: Option<Vec<u8>>,
    query: Option<Vec<u8>>,
    fragment: Option<Vec<u8>>,
}

fn ctrl(v: &[u8]) -> Vec<u8> {
    v.iter().map(|&c| if c < 0x20 || c == 0x7f { b'_' } else { c }).collect()
}

fn strcspn(s: &[u8], from: usize, to: usize, chars: &[u8]) -> usize {
    let mut e = to;
    for &ch in chars {
        if let Some(p) = s[from..e].iter().position(|&c| c == ch) {
            e = from + p;
        }
    }
    e
}

fn parse_port_digits(d: &[u8]) -> Option<u16> {
    let s = std::str::from_utf8(d).ok()?;
    let n: i64 = s.parse().ok()?;
    if (0..=65535).contains(&n) { Some(n as u16) } else { None }
}

/// php_url_parse_ex
fn parse_url(str: &[u8]) -> Option<Url> {
    let mut ret = Url::default();
    let ue = str.len();
    let mut s = 0usize;
    let isdigit = |c: u8| c.is_ascii_digit();

    enum Next {
        Port(usize),
        Host,
        Path,
    }
    let next: Next;
    let colon = str.iter().position(|&c| c == b':');
    match colon {
        Some(e) if e != s => {
            let mut p = s;
            let mut bad = false;
            while p < e {
                let c = str[p];
                if !c.is_ascii_alphabetic() && !isdigit(c) && c != b'+' && c != b'.' && c != b'-' {
                    bad = true;
                    break;
                }
                p += 1;
            }
            if bad {
                if e + 1 < ue && e < strcspn(str, s, ue, b"?#") {
                    next = Next::Port(e);
                } else if s + 1 < ue && str[s] == b'/' && str[s + 1] == b'/' {
                    s += 2;
                    next = Next::Host;
                } else {
                    next = Next::Path;
                }
            } else if e + 1 == ue {
                ret.scheme = Some(ctrl(&str[s..e]));
                return Some(ret);
            } else if str[e + 1] != b'/' {
                let mut p = e + 1;
                while p < ue && isdigit(str[p]) {
                    p += 1;
                }
                if (p == ue || str[p] == b'/') && (p - e) < 7 {
                    next = Next::Port(e);
                } else {
                    ret.scheme = Some(ctrl(&str[s..e]));
                    s = e + 1;
                    next = Next::Path;
                }
            } else {
                ret.scheme = Some(ctrl(&str[s..e]));
                if e + 2 < ue && str[e + 2] == b'/' {
                    s = e + 3;
                    let is_file = ret.scheme.as_deref().map(|x| x.eq_ignore_ascii_case(b"file")).unwrap_or(false);
                    if is_file && e + 3 < ue && str[e + 3] == b'/' {
                        if e + 5 < ue && str[e + 5] == b':' {
                            s = e + 4;
                        }
                        next = Next::Path;
                    } else {
                        next = Next::Host;
                    }
                } else {
                    s = e + 1;
                    next = Next::Path;
                }
            }
        }
        Some(e) => next = Next::Port(e),
        None => {
            if s + 1 < ue && str[s] == b'/' && str[s + 1] == b'/' {
                s += 2;
                next = Next::Host;
            } else {
                next = Next::Path;
            }
        }
    }

    let mut go_host = false;
    match next {
        Next::Port(e) => {
            let p = e + 1;
            let mut pp = p;
            while pp < ue && pp - p < 6 && isdigit(str[pp]) {
                pp += 1;
            }
            if pp - p > 0 && pp - p < 6 && (pp == ue || str[pp] == b'/') {
                ret.port = Some(parse_port_digits(&str[p..pp])?);
                if s + 1 < ue && str[s] == b'/' && str[s + 1] == b'/' {
                    s += 2;
                }
                go_host = true;
            } else if p == pp && pp == ue {
                return None;
            } else if s + 1 < ue && str[s] == b'/' && str[s + 1] == b'/' {
                s += 2;
                go_host = true;
            }
        }
        Next::Host => go_host = true,
        Next::Path => {}
    }

    if go_host {
        let e = strcspn(str, s, ue, b"/?#");
        if let Some(at) = str[s..e].iter().rposition(|&c| c == b'@') {
            let at = s + at;
            match str[s..at].iter().position(|&c| c == b':') {
                Some(pc) => {
                    ret.user = Some(ctrl(&str[s..s + pc]));
                    ret.pass = Some(ctrl(&str[s + pc + 1..at]));
                }
                None => ret.user = Some(ctrl(&str[s..at])),
            }
            s = at + 1;
        }
        let p = if s < ue && str[s] == b'[' && e > 0 && str[e - 1] == b']' {
            None
        } else {
            str[s..e].iter().rposition(|&c| c == b':').map(|x| s + x)
        };
        let host_end = match p {
            Some(pc) => {
                if ret.port.is_none() {
                    let pd = pc + 1;
                    if e - pd > 5 {
                        return None;
                    } else if e - pd > 0 {
                        ret.port = Some(parse_port_digits(&str[pd..e])?);
                    }
                }
                pc
            }
            None => e,
        };
        if host_end < s + 1 {
            return None;
        }
        ret.host = Some(ctrl(&str[s..host_end]));
        if e == ue {
            return Some(ret);
        }
        s = e;
    }

    // just_path
    let mut e = ue;
    if let Some(h) = str[s..e].iter().position(|&c| c == b'#') {
        let p = s + h + 1;
        ret.fragment = Some(if p < e { ctrl(&str[p..e]) } else { Vec::new() });
        e = p - 1;
    }
    if let Some(q) = str[s..e].iter().position(|&c| c == b'?') {
        let p = s + q + 1;
        ret.query = Some(if p < e { ctrl(&str[p..e]) } else { Vec::new() });
        e = p - 1;
    }
    if s < e || s == ue {
        ret.path = Some(ctrl(&str[s..e]));
    }
    Some(ret)
}

fn arg_separator(vm: &Vm) -> Vec<u8> {
    match vm.constants.get(b"arg_separator.output".as_slice()) {
        Some(v) => {
            let s = v.to_php_string().as_bytes().to_vec();
            if s.is_empty() { b"&".to_vec() } else { s }
        }
        None => b"&".to_vec(),
    }
}

/// append_modified_url
fn append_modified_url(url: &[u8], dest: &mut Vec<u8>, url_app: &[u8], sep: &[u8], hosts: &[Vec<u8>]) {
    let parts = match parse_url(url) {
        Some(p) => p,
        None => {
            dest.extend_from_slice(url);
            return;
        }
    };
    if parts.fragment.is_some() && url.first() == Some(&b'#') {
        dest.extend_from_slice(url);
        return;
    }
    if let Some(sch) = &parts.scheme {
        if !sch.eq_ignore_ascii_case(b"http") && !sch.eq_ignore_ascii_case(b"https") {
            dest.extend_from_slice(url);
            return;
        }
    }
    if let Some(h) = &parts.host {
        if !hosts.contains(&h.to_ascii_lowercase()) {
            dest.extend_from_slice(url);
            return;
        }
    }
    if parts.path.is_none() && parts.query.is_none() && parts.fragment.is_none() {
        dest.extend_from_slice(url);
        dest.extend_from_slice(b"/?");
        dest.extend_from_slice(url_app);
        return;
    }
    if let Some(sch) = &parts.scheme {
        dest.extend_from_slice(sch);
        dest.extend_from_slice(b"://");
    } else if url.starts_with(b"//") {
        dest.extend_from_slice(b"//");
    }
    if let Some(u) = &parts.user {
        dest.extend_from_slice(u);
        if let Some(p) = &parts.pass {
            dest.extend_from_slice(p);
            dest.push(b':');
        }
        dest.push(b'@');
    }
    if let Some(h) = &parts.host {
        dest.extend_from_slice(h);
    }
    if let Some(p) = parts.port {
        if p != 0 {
            dest.push(b':');
            dest.extend_from_slice(p.to_string().as_bytes());
        }
    }
    if let Some(p) = &parts.path {
        dest.extend_from_slice(p);
    }
    dest.push(b'?');
    if let Some(q) = &parts.query {
        dest.extend_from_slice(q);
        dest.extend_from_slice(sep);
        dest.extend_from_slice(url_app);
    } else {
        dest.extend_from_slice(url_app);
    }
    if let Some(f) = &parts.fragment {
        dest.push(b'#');
        dest.extend_from_slice(f);
    }
}

fn http_host(vm: &Vm) -> Option<Vec<u8>> {
    let srv = vm.get_global(b"_SERVER")?.deref();
    if let Value::Array(a) = srv {
        let a = a.borrow();
        if let Some(Value::String(h)) = a.get(&goro_core::array::ArrayKey::String(PhpString::from_bytes(b"HTTP_HOST"))) {
            let h = h.as_bytes();
            let h = match h.iter().position(|&c| c == b':') {
                Some(p) => &h[..p],
                None => h,
            };
            return Some(h.to_vec());
        }
    }
    None
}

fn check_host_whitelist(c: &Ctx, http_host: &Option<Vec<u8>>) -> bool {
    if c.attr_val.is_empty() {
        return true;
    }
    let parts = match parse_url(&c.attr_val) {
        Some(p) => p,
        None => return false,
    };
    if let Some(s) = &parts.scheme {
        if !s.eq_ignore_ascii_case(b"http") && !s.eq_ignore_ascii_case(b"https") {
            return false;
        }
    }
    let host = match &parts.host {
        None => return true,
        Some(h) => h,
    };
    if c.hosts.is_empty() {
        if let Some(hh) = http_host {
            if hh.eq_ignore_ascii_case(host) {
                return true;
            }
        }
    }
    c.hosts.contains(host)
}

fn tag_arg(c: &mut Ctx, quotes: bool, q: u8, sep: &[u8]) {
    let f = c.lookup_data.as_ref().map(|l| l.eq_ignore_ascii_case(&c.arg)).unwrap_or(false);
    if quotes {
        c.result.push(q);
    }
    if f {
        let mut out = Vec::new();
        append_modified_url(&c.val, &mut out, &c.url_app, sep, &c.hosts);
        c.result.extend_from_slice(&out);
    } else {
        let v = c.val.clone();
        c.result.extend_from_slice(&v);
    }
    if quotes {
        c.result.push(q);
    }
}

fn is_alpha(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

/// xx_mainloop
fn mainloop(c: &mut Ctx, data: &[u8], sep: &[u8], host: &Option<Vec<u8>>) {
    c.buf.extend_from_slice(data);
    let buf = std::mem::take(&mut c.buf);
    let end = buf.len();
    let mut x = 0usize;
    let mut start;
    // `stop` keeps buf[start..] for the next call.
    macro_rules! passthru {
        ($s:expr, $e:expr) => {
            c.result.extend_from_slice(&buf[$s..$e]);
        };
    }
    loop {
        start = x;
        match c.state {
            St::Plain => {
                if x >= end {
                    break;
                }
                if buf[x] == b'<' {
                    x += 1;
                    passthru!(start, x);
                    c.state = St::Tag;
                } else {
                    while x < end && buf[x] != b'<' {
                        x += 1;
                    }
                    passthru!(start, x);
                }
            }
            St::Tag => {
                if x >= end {
                    break;
                }
                if is_alpha(buf[x]) || buf[x] == b':' {
                    while x < end && (is_alpha(buf[x]) || buf[x] == b':') {
                        x += 1;
                    }
                    if x >= end {
                        // re2c needs one more char to decide the match end.
                        break;
                    }
                    // handle_tag
                    c.tag = buf[start..x].to_ascii_lowercase();
                    c.lookup_data = c.tags.iter().find(|(n, _)| *n == c.tag).map(|(_, a)| a.clone());
                    let ok = c.lookup_data.is_some();
                    if ok {
                        c.tag_form = c.tag == b"form";
                    }
                    c.state = if ok { St::NextArg } else { St::Plain };
                    passthru!(start, x);
                } else {
                    x += 1;
                    passthru!(start, x);
                    c.state = St::Plain;
                }
            }
            St::NextArg => {
                if x >= end {
                    break;
                }
                let ch = buf[x];
                if ch == b'>' || (ch == b'/' && x + 1 < end && buf[x + 1] == b'>') {
                    x += if ch == b'/' { 2 } else { 1 };
                    passthru!(start, x);
                    // handle_form
                    if !c.form_app.is_empty() && c.tag == b"form" && check_host_whitelist(c, host) {
                        let f = c.form_app.clone();
                        c.result.extend_from_slice(&f);
                    }
                    c.state = St::Plain;
                } else if ch == b'/' && x + 1 >= end {
                    break;
                } else if matches!(ch, b' ' | 0x0b | b'\r' | b'\t' | b'\n') {
                    while x < end && matches!(buf[x], b' ' | 0x0b | b'\r' | b'\t' | b'\n') {
                        x += 1;
                    }
                    if x >= end {
                        break;
                    }
                    passthru!(start, x);
                } else if is_alpha(ch) {
                    c.state = St::Arg;
                } else {
                    x += 1;
                    passthru!(start, x);
                    c.state = St::Plain;
                }
            }
            St::Arg => {
                if x >= end {
                    break;
                }
                if is_alpha(buf[x]) {
                    x += 1;
                    while x < end && (is_alpha(buf[x]) || buf[x] == b'-') {
                        x += 1;
                    }
                    if x >= end {
                        break;
                    }
                    passthru!(start, x);
                    // handle_arg
                    c.arg = buf[start..x].to_vec();
                    c.attr_action = c.tag_form && c.arg.eq_ignore_ascii_case(b"action");
                    c.state = St::BeforeVal;
                } else {
                    x += 1;
                    passthru!(start, x);
                    c.state = St::NextArg;
                }
            }
            St::BeforeVal => {
                // [ ]* "=" [ ]*
                let mut y = x;
                while y < end && buf[y] == b' ' {
                    y += 1;
                }
                if y >= end {
                    break;
                }
                if buf[y] == b'=' {
                    y += 1;
                    while y < end && buf[y] == b' ' {
                        y += 1;
                    }
                    if y >= end {
                        break;
                    }
                    x = y;
                    passthru!(start, x);
                    c.state = St::Val;
                } else {
                    c.state = St::NextArg;
                }
            }
            St::Val => {
                if x >= end {
                    break;
                }
                let ch = buf[x];
                if ch == b'"' || ch == b'\'' {
                    let mut y = x + 1;
                    while y < end && buf[y] != ch && buf[y] != b'>' {
                        y += 1;
                    }
                    if y >= end {
                        break;
                    }
                    if buf[y] == ch {
                        x = y + 1;
                        c.val = buf[start + 1..x - 1].to_vec();
                        if c.tag_form && c.attr_action {
                            c.attr_val = c.val.clone();
                        }
                        tag_arg(c, true, ch, sep);
                        c.state = St::NextArg;
                        continue;
                    }
                    // Unterminated quote: fall through to the unquoted rule.
                }
                if !matches!(ch, b' ' | b'\r' | b'\t' | b'\n' | b'>' | b'\'' | b'"') {
                    let mut y = x;
                    while y < end && !matches!(buf[y], b' ' | b'\r' | b'\t' | b'\n' | b'>' | b'\'' | b'"') {
                        y += 1;
                    }
                    if y >= end {
                        break;
                    }
                    x = y;
                    c.val = buf[start..x].to_vec();
                    if c.tag_form && c.attr_action {
                        c.attr_val = c.val.clone();
                    }
                    tag_arg(c, false, b' ', sep);
                    c.state = St::NextArg;
                } else {
                    x += 1;
                    passthru!(start, x);
                    c.state = St::NextArg;
                }
            }
        }
    }
    c.buf = buf[start.min(end)..].to_vec();
}

fn url_adapt_ext(c: &mut Ctx, src: &[u8], do_flush: bool, sep: &[u8], host: &Option<Vec<u8>>) -> Vec<u8> {
    mainloop(c, src, sep, host);
    let mut out = std::mem::take(&mut c.result);
    if do_flush {
        out.extend_from_slice(&c.buf);
        c.buf.clear();
        c.val.clear();
        c.attr_val.clear();
    }
    out
}

fn handler_impl(vm: &mut Vm, data: &[u8], mode: i64, is_session: bool) -> Option<Vec<u8>> {
    let sep = arg_separator(vm);
    let host = http_host(vm);
    with(is_session, |c| {
        if !c.url_app.is_empty() {
            let flush = mode & (OB_OP_FLUSH | OB_OP_FINAL) != 0;
            Some(url_adapt_ext(c, data, flush, &sep, &host))
        } else if !c.buf.is_empty() {
            let mut out = std::mem::take(&mut c.buf);
            out.extend_from_slice(data);
            c.result.clear();
            Some(out)
        } else {
            None
        }
    })
}

fn session_handler(vm: &mut Vm, data: &[u8], mode: i64) -> Option<Vec<u8>> {
    handler_impl(vm, data, mode, true)
}

fn output_handler(vm: &mut Vm, data: &[u8], mode: i64) -> Option<Vec<u8>> {
    handler_impl(vm, data, mode, false)
}

fn raw_url_encode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{:02X}", c).as_bytes());
        }
    }
    out
}

/// htmlspecialchars(ENT_QUOTES|ENT_SUBSTITUTE, double_encode=false)
fn html_escape(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        match c {
            b'&' => {
                // Keep existing entities (double_encode = 0).
                let rest = &s[i + 1..];
                let ent_len = rest.iter().position(|&x| x == b';');
                let valid = match ent_len {
                    Some(n) if n > 0 => {
                        let e = &rest[..n];
                        if e[0] == b'#' {
                            e.len() > 1
                                && (e[1..].iter().all(|x| x.is_ascii_digit())
                                    || ((e[1] == b'x' || e[1] == b'X') && e.len() > 2 && e[2..].iter().all(|x| x.is_ascii_hexdigit())))
                        } else {
                            e.iter().all(|x| x.is_ascii_alphanumeric())
                        }
                    }
                    _ => false,
                };
                if valid {
                    out.push(b'&');
                } else {
                    out.extend_from_slice(b"&amp;");
                }
            }
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&#039;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// php_url_scanner_add_var_impl
pub fn add_var(vm: &mut Vm, name: &[u8], value: &[u8], encode: bool, is_session: bool) {
    let sep = arg_separator(vm);
    let should_start = with(is_session, |c| {
        let start = !c.active;
        if start {
            c.buf.clear();
            c.result.clear();
            c.tag.clear();
            c.arg.clear();
            c.val.clear();
            c.attr_val.clear();
            c.state = St::Plain;
            c.tag_form = false;
            c.attr_action = false;
            c.active = true;
            c.is_session = is_session;
        }
        if !c.url_app.is_empty() {
            c.url_app.extend_from_slice(&sep);
        }
        let (sn, sv, hn, hv) = if encode {
            (raw_url_encode(name), raw_url_encode(value), html_escape(name), html_escape(value))
        } else {
            (name.to_vec(), value.to_vec(), name.to_vec(), value.to_vec())
        };
        c.url_app.extend_from_slice(&sn);
        c.url_app.push(b'=');
        c.url_app.extend_from_slice(&sv);
        c.form_app.extend_from_slice(b"<input type=\"hidden\" name=\"");
        c.form_app.extend_from_slice(&hn);
        c.form_app.extend_from_slice(b"\" value=\"");
        c.form_app.extend_from_slice(&hv);
        c.form_app.extend_from_slice(b"\" />");
        start
    });
    if should_start {
        let f: goro_core::ob::ObInternalFn = if is_session { session_handler } else { output_handler };
        let h = Vm::ob_make_handler(b"URL-Rewriter".to_vec(), ObKind::Internal(f), 0, OBH_STDFLAGS);
        vm.ob_handler_start(Some(h));
    }
}

/// php_url_scanner_reset_vars_impl
pub fn reset_vars(is_session: bool) {
    with(is_session, |c| {
        c.form_app.clear();
        c.url_app.clear();
    });
}

/// php_url_scanner_reset_var_impl
pub fn reset_var(vm: &mut Vm, name: &[u8], encode: bool, is_session: bool) -> bool {
    let sep = arg_separator(vm);
    with(is_session, |c| {
        if c.url_app.is_empty() {
            return true;
        }
        let (sname, hname) = if encode { (raw_url_encode(name), html_escape(name)) } else { (name.to_vec(), name.to_vec()) };
        let mut url_app = sname.clone();
        url_app.push(b'=');
        let mut form_app = b"<input type=\"hidden\" name=\"".to_vec();
        form_app.extend_from_slice(&hname);
        form_app.extend_from_slice(b"\" value=\"");
        let find = |h: &[u8], n: &[u8]| h.windows(n.len()).position(|w| w == n);
        let mut start = match find(&c.url_app, &url_app) {
            Some(p) => p,
            None => return false,
        };
        let limit = c.url_app.len();
        let mut end = start + url_app.len();
        let mut sep_removed = false;
        while end < limit {
            if c.url_app[end..].starts_with(&sep) {
                end += sep.len();
                sep_removed = true;
                break;
            }
            end += 1;
        }
        if c.url_app.len() == end - start {
            c.form_app.clear();
            c.url_app.clear();
            return true;
        }
        if !sep_removed && start >= sep.len() && c.url_app[start - sep.len()..start] == sep[..] {
            start -= sep.len();
        }
        c.url_app.drain(start..end.min(limit));
        let fstart = match find(&c.form_app, &form_app) {
            Some(p) => p,
            None => {
                c.form_app.clear();
                c.url_app.clear();
                return false;
            }
        };
        let flimit = c.form_app.len();
        let mut fend = fstart + form_app.len();
        while fend < flimit {
            if c.form_app[fend] == b'>' {
                fend += 1;
                break;
            }
            fend += 1;
        }
        c.form_app.drain(fstart..fend.min(flimit));
        true
    })
}

/// php_url_scanner_adapt_single_url
pub fn adapt_single_url(vm: &Vm, url: &[u8], name: &[u8], value: &[u8], encode: bool) -> Vec<u8> {
    let mut app = if encode { raw_url_encode(name) } else { name.to_vec() };
    app.push(b'=');
    app.extend_from_slice(&if encode { raw_url_encode(value) } else { value.to_vec() });
    let sep = arg_separator(vm);
    let hosts = with(true, |c| c.hosts.clone());
    let mut out = Vec::new();
    append_modified_url(url, &mut out, &app, &sep, &hosts);
    out
}

fn output_add_rewrite_var_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if args.len() != 2 {
        let msg = format!("output_add_rewrite_var() expects exactly 2 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    let mut strs = Vec::new();
    for (i, (a, pname)) in args.iter().zip(["name", "value"]).enumerate() {
        let a = a.deref();
        match &a {
            Value::Array(_) | Value::Object(_) => {
                let msg = format!(
                    "output_add_rewrite_var(): Argument #{} (${}) must be of type string, {} given",
                    i + 1,
                    pname,
                    Vm::value_type_name(&a)
                );
                let exc = vm.create_exception(b"TypeError", &msg, vm.current_line);
                vm.current_exception = Some(exc);
                return Ok(Value::Null);
            }
            _ => strs.push(a.to_php_string().as_bytes().to_vec()),
        }
    }
    add_var(vm, &strs[0], &strs[1], true, false);
    Ok(Value::True)
}

fn output_reset_rewrite_vars_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if !args.is_empty() {
        let msg = format!("output_reset_rewrite_vars() expects exactly 0 arguments, {} given", args.len());
        let exc = vm.create_exception(b"ArgumentCountError", &msg, vm.current_line);
        vm.current_exception = Some(exc);
        return Ok(Value::Null);
    }
    reset_vars(false);
    Ok(Value::True)
}

fn ini_hook(_vm: &mut Vm, key: &[u8], value: &Value) -> bool {
    let v = value.to_php_string().as_bytes().to_vec();
    match key {
        b"url_rewriter.tags" => set_tags(false, &v),
        b"url_rewriter.hosts" => set_hosts(false, &v),
        _ => {}
    }
    true
}

fn startup(vm: &mut Vm) {
    let tags = vm.constants.get(b"url_rewriter.tags".as_slice()).map(|v| v.to_php_string().as_bytes().to_vec());
    if let Some(t) = tags {
        set_tags(false, &t);
    }
    let hosts = vm.constants.get(b"url_rewriter.hosts".as_slice()).map(|v| v.to_php_string().as_bytes().to_vec());
    if let Some(h) = hosts {
        set_hosts(false, &h);
    }
}

pub fn register(vm: &mut Vm) {
    SESSION.with(|c| *c.borrow_mut() = Ctx::new(true, b"a=href,area=href,frame=src,form="));
    OUTPUT.with(|c| *c.borrow_mut() = Ctx::new(false, b"form="));
    vm.register_function(b"output_add_rewrite_var", output_add_rewrite_var_fn);
    vm.register_function(b"output_reset_rewrite_vars", output_reset_rewrite_vars_fn);
    vm.constants.entry(b"url_rewriter.tags".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"form=")));
    vm.constants.entry(b"url_rewriter.hosts".to_vec()).or_insert_with(|| Value::String(PhpString::from_bytes(b"")));
    vm.ini_set_hooks.insert(b"url_rewriter.tags".to_vec(), ini_hook);
    vm.ini_set_hooks.insert(b"url_rewriter.hosts".to_vec(), ini_hook);
    vm.startup_hooks.push(startup);
}
