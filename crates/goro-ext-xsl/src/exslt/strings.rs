//! EXSLT strings (libexslt/strings.c)

use crate::transform::Transform;
use crate::util::tw;
use crate::xpath::{self, PCtx, XObj};
use goro_ext_xml::tree::NodeId;

pub fn exists(name: &str) -> bool {
    matches!(
        name,
        "tokenize"
            | "split"
            | "encode-uri"
            | "decode-uri"
            | "padding"
            | "align"
            | "concat"
            | "replace"
    )
}

pub fn call(t: &mut Transform, p: &mut PCtx, name: &str, nargs: usize) -> bool {
    match name {
        "tokenize" => tokenize(t, p, nargs),
        "split" => split(t, p, nargs),
        "encode-uri" => encode_uri(t, p, nargs),
        "decode-uri" => decode_uri(t, p, nargs),
        "padding" => padding(t, p, nargs),
        "align" => align(t, p, nargs),
        "concat" => concat(t, p, nargs),
        "replace" => replace(t, p, nargs),
        _ => return false,
    }
    true
}

fn s_of(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// xmlNewDocRawNode(container, NULL, "token", content) + xmlAddChild
fn add_token(container: NodeId, content: &[u8]) -> NodeId {
    tw(|tr| {
        let e = tr.new_element(Some(container), "token");
        let txt = tr.new_text(Some(container), &s_of(content));
        tr.add_child(e, txt);
        tr.add_child(container, e);
        e
    })
}

/// xmlUTF8Strsize(s, 1)
fn utf8_size1(s: &[u8]) -> usize {
    match s.first() {
        None => 0,
        Some(&c) if c < 0x80 => 1,
        Some(&c) => {
            let n = if c >= 0xF0 {
                4
            } else if c >= 0xE0 {
                3
            } else {
                2
            };
            n.min(s.len())
        }
    }
}

fn tokenize(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(1..=2).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    let delims = if nargs == 2 {
        match xpath::x_pop_string(t, p) {
            Some(s) if p.error == 0 => s,
            _ => return,
        }
    } else {
        "\t\r\n ".to_string()
    };
    let Some(s) = xpath::x_pop_string(t, p) else {
        return;
    };
    if p.error != 0 {
        return;
    }
    let container = t.create_rvt();
    let mut ret = Vec::new();
    let str_b = s.as_bytes();
    let delims: Vec<char> = delims.chars().collect();
    let mut token = 0usize;
    let mut cur = 0usize;
    while cur < str_b.len() {
        let clen = utf8_size1(&str_b[cur..]);
        if delims.is_empty() {
            ret.push(add_token(container, &str_b[cur..cur + clen]));
            token = cur + clen;
        } else {
            let ch = &str_b[cur..cur + clen];
            for d in &delims {
                let mut buf = [0u8; 4];
                if d.encode_utf8(&mut buf).as_bytes() == ch {
                    if cur == token {
                        token = cur + clen;
                        break;
                    }
                    ret.push(add_token(container, &str_b[token..cur]));
                    token = cur + clen;
                    break;
                }
            }
        }
        cur += clen;
    }
    if token != cur {
        ret.push(add_token(container, &str_b[token..cur]));
    }
    p.push(XObj::NodeSet(ret));
}

fn split(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(1..=2).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    let delim = if nargs == 2 {
        match xpath::x_pop_string(t, p) {
            Some(s) if p.error == 0 => s,
            _ => return,
        }
    } else {
        " ".to_string()
    };
    let Some(s) = xpath::x_pop_string(t, p) else {
        return;
    };
    if p.error != 0 {
        return;
    }
    let container = t.create_rvt();
    let mut ret = Vec::new();
    let b = s.as_bytes();
    let d = delim.as_bytes();
    let dl = d.len();
    let mut token = 0usize;
    let mut cur = 0usize;
    while cur < b.len() {
        if dl == 0 {
            if cur != token {
                ret.push(add_token(container, &b[token..cur]));
                token += 1;
            }
        } else if b.len() - cur >= dl && b[cur..cur + dl].eq_ignore_ascii_case(d) {
            if cur == token {
                cur = cur + dl - 1;
                token = cur + 1;
                cur += 1;
                continue;
            }
            ret.push(add_token(container, &b[token..cur]));
            cur = cur + dl - 1;
            token = cur + 1;
        }
        cur += 1;
    }
    if token != cur && token < b.len() {
        ret.push(add_token(container, &b[token..]));
    }
    p.push(XObj::NodeSet(ret));
}

/// xmlURIEscapeStr
pub fn uri_escape(s: &str, list: &[u8]) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        if c == b'@' || c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c) || list.contains(&c)
        {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{:02X}", c));
        }
    }
    out
}

/// xmlURIUnescapeString
fn uri_unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn encode_uri(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(2..=3).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    if nargs >= 3 {
        let enc = xpath::x_pop_string(t, p).unwrap_or_default();
        if enc != "UTF-8" {
            p.push(XObj::Str(String::new()));
            return;
        }
    }
    let escape_all = xpath::x_pop_boolean(t, p).unwrap_or(false);
    let s = xpath::x_pop_string(t, p).unwrap_or_default();
    if s.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let list: &[u8] = if escape_all {
        b"-_.!~*'()"
    } else {
        b"-_.!~*'();/?:@&=+$,[]"
    };
    p.push(XObj::Str(uri_escape(&s, list)));
}

fn decode_uri(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(1..=2).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    if nargs >= 2 {
        let enc = xpath::x_pop_string(t, p).unwrap_or_default();
        if enc != "UTF-8" {
            p.push(XObj::Str(String::new()));
            return;
        }
    }
    let s = xpath::x_pop_string(t, p).unwrap_or_default();
    if s.is_empty() {
        p.push(XObj::Str(String::new()));
        return;
    }
    let r = uri_unescape(&s);
    let r = r.split(|&c| c == 0).next().unwrap_or(&[]).to_vec();
    match String::from_utf8(r) {
        Ok(s) => p.push(XObj::Str(s)),
        Err(_) => p.push(XObj::Str(String::new())),
    }
}

fn padding(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(1..=2).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    let mut s = if nargs == 2 {
        xpath::x_pop_string(t, p).unwrap_or_default()
    } else {
        String::new()
    };
    let f = xpath::x_pop_number(t, p).unwrap_or(f64::NAN);
    if s.is_empty() {
        s = " ".to_string();
    }
    let mut number: i64 = if f.is_nan() || f < 0.0 {
        0
    } else if f >= 100000.0 {
        100000
    } else {
        f as i64
    };
    if number <= 0 {
        p.push(XObj::Str(String::new()));
        return;
    }
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    let mut out = String::new();
    while number >= len {
        out.push_str(&s);
        number -= len;
    }
    if number > 0 {
        out.extend(chars.iter().take(number as usize));
    }
    p.push(XObj::Str(out));
}

fn align(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if !(2..=3).contains(&nargs) {
        xpath::x_arity_error(t, p);
        return;
    }
    let alignment = if nargs == 3 {
        xpath::x_pop_string(t, p)
    } else {
        None
    };
    let padding = xpath::x_pop_string(t, p).unwrap_or_default();
    let s = xpath::x_pop_string(t, p).unwrap_or_default();
    let sc: Vec<char> = s.chars().collect();
    let pc: Vec<char> = padding.chars().collect();
    let (sl, pl) = (sc.len(), pc.len());
    if sl == pl {
        p.push(XObj::Str(s));
        return;
    }
    let ret: String = if sl > pl {
        sc[..pl].iter().collect()
    } else {
        match alignment.as_deref() {
            Some("right") => pc[..pl - sl].iter().chain(sc.iter()).collect(),
            Some("center") => {
                let left = (pl - sl) / 2;
                pc[..left]
                    .iter()
                    .chain(sc.iter())
                    .chain(pc[left + sl..].iter())
                    .collect()
            }
            _ => sc.iter().chain(pc[sl..].iter()).collect(),
        }
    };
    p.push(XObj::Str(ret));
}

fn concat(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 1 {
        xpath::x_arity_error(t, p);
        return;
    }
    if !matches!(p.top(), Some(XObj::NodeSet(_)) | Some(XObj::Tree(_))) {
        xpath::xp_err(t, p, xpath::XPATH_INVALID_TYPE);
        return;
    }
    let ns = xpath::x_pop_node_set(t, p).unwrap_or_default();
    let mut out = String::new();
    for n in ns {
        out.push_str(&xpath::node_string_id(n));
    }
    p.push(XObj::Str(out));
}

/// exsltStrReturnString
fn return_string(t: &mut Transform, p: &mut PCtx, s: &[u8]) {
    let container = t.create_rvt();
    let txt = tw(|tr| {
        let txt = tr.new_text(Some(container), &s_of(s));
        tr.add_child(container, txt);
        txt
    });
    p.push(XObj::NodeSet(vec![txt]));
}

fn replace(t: &mut Transform, p: &mut PCtx, nargs: usize) {
    if nargs != 3 {
        xpath::x_arity_error(t, p);
        return;
    }
    let is_ns = |p: &PCtx| matches!(p.top(), Some(XObj::NodeSet(_)) | Some(XObj::Tree(_)));
    let (replace_str, replace_set) = if !is_ns(p) {
        (xpath::x_pop_string(t, p), None)
    } else {
        (None, xpath::x_pop_node_set(t, p))
    };
    if p.error != 0 {
        return;
    }
    let (search_str, search_set) = if !is_ns(p) {
        (xpath::x_pop_string(t, p), None)
    } else {
        (None, xpath::x_pop_node_set(t, p))
    };
    if p.error != 0 {
        return;
    }
    let n = match &search_set {
        Some(s) => s.len(),
        None => 1,
    };
    let Some(string) = xpath::x_pop_string(t, p) else {
        return;
    };
    if p.error != 0 {
        return;
    }
    if n == 0 {
        return_string(t, p, string.as_bytes());
        return;
    }
    let mut search: Vec<Vec<u8>> = Vec::with_capacity(n);
    let mut repl: Vec<Option<Vec<u8>>> = Vec::with_capacity(n);
    let mut i_empty: Option<usize> = None;
    for i in 0..n {
        let s = match &search_set {
            Some(set) => xpath::node_string_id(set[i]).into_bytes(),
            None => search_str.clone().unwrap_or_default().into_bytes(),
        };
        if i_empty.is_none() && s.is_empty() {
            i_empty = Some(i);
        }
        search.push(s);
        let r = match &replace_set {
            Some(set) => set.get(i).map(|&nd| xpath::node_string_id(nd).into_bytes()),
            None => {
                if i == 0 {
                    replace_str.clone().map(|s| s.into_bytes())
                } else {
                    None
                }
            }
        };
        repl.push(r);
    }
    let rlen = |i: usize| repl[i].as_ref().map(|r| r.len()).unwrap_or(0);
    if let Some(ie) = i_empty {
        if rlen(ie) == 0 {
            i_empty = None;
        }
    }
    let src = string.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    let mut start = 0usize;
    while pos < src.len() {
        let mut max_len = 0usize;
        let mut i_match = 0usize;
        for (i, s) in search.iter().enumerate() {
            if !s.is_empty() && src[pos] == s[0] && s.len() > max_len && src[pos..].starts_with(s) {
                i_match = i;
                max_len = s.len();
            }
        }
        if max_len == 0 {
            if let Some(ie) = i_empty {
                if start < pos {
                    out.extend_from_slice(&src[start..pos]);
                    if let Some(r) = &repl[ie] {
                        out.extend_from_slice(r);
                    }
                    start = pos;
                }
            }
            pos += utf8_size1(&src[pos..]).max(1);
        } else {
            if start < pos {
                out.extend_from_slice(&src[start..pos]);
            }
            if let Some(r) = &repl[i_match] {
                out.extend_from_slice(r);
            }
            pos += max_len;
            start = pos;
        }
    }
    if start < pos {
        out.extend_from_slice(&src[start..pos]);
    }
    return_string(t, p, &out);
}
