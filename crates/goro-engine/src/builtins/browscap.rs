//! get_browser() over the browscap ini file (PHP manual: get_browser(),
//! the browscap setting). The best matching section is the one with the
//! most literal (non-wildcard) characters, the first on a tie (observed on
//! PHP 8.5); properties are inherited through `Parent`.

use std::rc::Rc;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

pub static MODULE: Module = Module {
    name: "standard",
    functions: &[(
        "function get_browser(?string $user_agent = null, bool $return_array = false): object|array|false",
        get_browser,
    )],
    ini: &[("browscap", "")],
    listed: false,
    ..Module::EMPTY
};

struct Section {
    pattern: Vec<u8>,
    lower: Vec<u8>,
    literal: usize,
    props: Vec<(Vec<u8>, Vec<u8>)>,
}

struct Browscap {
    path: Vec<u8>,
    sections: Vec<Section>,
}

thread_local! {
    static CACHE: std::cell::RefCell<Option<Rc<Browscap>>> = const { std::cell::RefCell::new(None) };
}

fn value(v: &[u8]) -> Vec<u8> {
    let v = v.trim_ascii();
    if v.len() >= 2 && v[0] == b'"' && v[v.len() - 1] == b'"' {
        return v[1..v.len() - 1].to_vec();
    }
    match v.to_ascii_lowercase().as_slice() {
        b"true" | b"on" | b"yes" => b"1".to_vec(),
        b"false" | b"off" | b"no" | b"none" | b"null" => Vec::new(),
        _ => v.to_vec(),
    }
}

fn parse(data: &[u8]) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in data.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line).trim_ascii();
        if line.is_empty() || line[0] == b';' {
            continue;
        }
        if line[0] == b'[' && line.ends_with(b"]") {
            let p = line[1..line.len() - 1].to_vec();
            let literal = p.iter().filter(|&&c| c != b'*' && c != b'?').count();
            out.push(Section {
                lower: p.to_ascii_lowercase(),
                pattern: p,
                literal,
                props: Vec::new(),
            });
            continue;
        }
        let Some(eq) = line.iter().position(|&c| c == b'=') else {
            continue;
        };
        if let Some(s) = out.last_mut() {
            let k = line[..eq].trim_ascii().to_vec();
            s.props.push((k, value(&line[eq + 1..])));
        }
    }
    out
}

/// Glob matching with `*` and `?` (both lower case already).
fn glob(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

fn regex_of(pattern: &[u8]) -> Vec<u8> {
    let mut r = b"~^".to_vec();
    for &c in &pattern.to_ascii_lowercase() {
        match c {
            b'*' => r.extend_from_slice(b".*"),
            b'?' => r.push(b'.'),
            b'.' | b'\\' | b'+' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'^' | b'$' | b'|'
            | b'~' => {
                r.push(b'\\');
                r.push(c);
            }
            _ => r.push(c),
        }
    }
    r.extend_from_slice(b"$~");
    r
}

fn load(vm: &mut Vm, a: &Args) -> R<Option<Rc<Browscap>>> {
    let path = vm
        .get_ini(b"browscap")
        .map(|v| v.to_vec())
        .unwrap_or_default();
    if path.is_empty() {
        vm.fwarn(a, "browscap ini directive not set")?;
        return Ok(None);
    }
    if let Some(c) = CACHE.with(|c| c.borrow().clone())
        && c.path == path
    {
        return Ok(Some(c));
    }
    let Ok(data) = std::fs::read(vm.fs_path(&path)) else {
        vm.fwarn(a, "browscap ini directive not set")?;
        return Ok(None);
    };
    let b = Rc::new(Browscap {
        path,
        sections: parse(&data),
    });
    CACHE.with(|c| *c.borrow_mut() = Some(b.clone()));
    Ok(Some(b))
}

fn get_browser(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(bc) = load(vm, a)? else {
        return Ok(Value::Bool(false));
    };
    let ua = match a.opt_str(0) {
        Some(s) => s,
        None => {
            let server = vm.globals_array().get_bytes(b"_SERVER").map(|v| v.deref());
            let ua = match server {
                Some(Value::Array(s)) => s.get_bytes(b"HTTP_USER_AGENT").map(|v| v.deref()),
                _ => None,
            };
            match ua {
                Some(Value::Str(s)) => s,
                _ => {
                    vm.fwarn(
                        a,
                        "HTTP_USER_AGENT variable is not set, cannot determine user agent name",
                    )?;
                    return Ok(Value::Bool(false));
                }
            }
        }
    };
    let lua = ua.to_ascii_lowercase();
    let mut best: Option<&Section> = None;
    for s in &bc.sections {
        if best.is_some_and(|b| b.literal >= s.literal) {
            continue;
        }
        if glob(&s.lower, &lua) {
            best = Some(s);
        }
    }
    let Some(sec) = best else {
        return Ok(Value::Bool(false));
    };
    let mut out = PhpArray::new();
    out.insert(
        Key::from_bytes(b"browser_name_regex"),
        Value::Str(PhpStr::from(regex_of(&sec.pattern))),
    );
    out.insert(
        Key::from_bytes(b"browser_name_pattern"),
        Value::Str(PhpStr::new(&sec.pattern)),
    );
    let mut cur = Some(sec);
    let mut depth = 0;
    while let Some(s) = cur {
        for (k, v) in &s.props {
            let key = Key::from_bytes(&k.to_ascii_lowercase());
            if out.get(key.as_ref()).is_none() {
                out.insert(key, Value::Str(PhpStr::new(v)));
            }
        }
        let parent = s
            .props
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(b"parent"))
            .map(|(_, v)| v.clone());
        cur = parent.and_then(|p| bc.sections.iter().find(|x| x.pattern == p));
        depth += 1;
        if depth > 64 {
            break;
        }
    }
    if a.bool_or(1, false) {
        return Ok(Value::Array(out));
    }
    let o = vm.new_std_object()?;
    for (k, v) in out.iter() {
        if let goro_runtime::KeyRef::Str(k) = k {
            vm.set_prop_raw(&o, k, v.clone());
        }
    }
    Ok(Value::Object(o))
}
