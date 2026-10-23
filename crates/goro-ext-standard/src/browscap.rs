//! get_browser() and the browscap.ini database (ext/standard/browscap.c).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

const NUM_CONTAINS: usize = 5;
const DEFAULT_SECTION_NAME: &[u8] = b"Default Browser Capability Settings";

struct Entry {
    pattern: Vec<u8>,
    parent: Option<Vec<u8>>,
    kv: Vec<(Vec<u8>, Vec<u8>)>,
    contains_start: [usize; NUM_CONTAINS],
    contains_len: [usize; NUM_CONTAINS],
    prefix_len: usize,
}

#[derive(Default)]
struct BrowserData {
    /// Entries in section order
    entries: Vec<Entry>,
    /// Pattern -> index into entries
    index: HashMap<Vec<u8>, usize>,
}

thread_local! {
    /// (browscap ini value, its loaded data; None when it could not be read)
    static BDATA: RefCell<Option<(Vec<u8>, Option<Rc<BrowserData>>)>> = const { RefCell::new(None) };
}

fn is_placeholder(c: u8) -> bool {
    c == b'?' || c == b'*'
}

/// browscap_compute_prefix_len()
fn compute_prefix_len(p: &[u8]) -> usize {
    p.iter()
        .position(|&c| is_placeholder(c))
        .unwrap_or(p.len())
        .min(u8::MAX as usize)
}

/// browscap_compute_contains(): (start, len, next position)
fn compute_contains(p: &[u8], start_pos: usize) -> (usize, usize, usize) {
    let mut i = start_pos;
    while i < p.len() {
        if !is_placeholder(p[i]) && i + 1 < p.len() && !is_placeholder(p[i + 1]) {
            break;
        }
        i += 1;
    }
    let start = i;
    while i < p.len() && !is_placeholder(p[i]) {
        i += 1;
    }
    (start, (i - start).min(u8::MAX as usize), i)
}

/// browscap_convert_pattern()
fn convert_pattern(p: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(p.len() + 8);
    t.extend_from_slice(b"~^");
    for &c in p {
        match c {
            b'?' => t.push(b'.'),
            b'*' => t.extend_from_slice(b".*"),
            b'.' | b'\\' | b'(' | b')' | b'~' | b'+' => {
                t.push(b'\\');
                t.push(c);
            }
            _ => t.push(c.to_ascii_lowercase()),
        }
    }
    t.extend_from_slice(b"$~");
    t
}

fn load(vm: &mut Vm, filename: &[u8]) -> Option<BrowserData> {
    let path = String::from_utf8_lossy(filename).into_owned();
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => {
            vm.emit_warning(&format!("Cannot open \"{}\" for reading", path));
            return None;
        }
    };
    let parsed =
        crate::ini_parser::parse_ini(vm, &data, true, crate::ini_parser::SCANNER_RAW, &path);
    let mut bd = BrowserData::default();
    let Value::Array(sections) = parsed else {
        return Some(bd);
    };
    for (k, v) in sections.borrow().iter() {
        let Value::Array(sec) = v else { continue };
        let pattern = match k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(n) => n.to_string().into_bytes(),
        };
        if pattern.len() > u16::MAX as usize {
            vm.emit_warning(&format!(
                "get_browser(): Skipping excessively long pattern of length {}",
                pattern.len()
            ));
            continue;
        }
        let prefix_len = compute_prefix_len(&pattern);
        let mut contains_start = [0usize; NUM_CONTAINS];
        let mut contains_len = [0usize; NUM_CONTAINS];
        let mut pos = prefix_len;
        for i in 0..NUM_CONTAINS {
            let (s, l, next) = compute_contains(&pattern, pos);
            contains_start[i] = s;
            contains_len[i] = l;
            pos = next;
        }
        let mut entry = Entry {
            pattern: pattern.clone(),
            parent: None,
            kv: Vec::new(),
            contains_start,
            contains_len,
            prefix_len,
        };
        for (key, val) in sec.borrow().iter() {
            let key = match key {
                ArrayKey::String(s) => s.as_bytes().to_vec(),
                ArrayKey::Int(n) => n.to_string().into_bytes(),
            };
            let raw = val.to_php_string().as_bytes().to_vec();
            let value: Vec<u8> = if raw.eq_ignore_ascii_case(b"on")
                || raw.eq_ignore_ascii_case(b"yes")
                || raw.eq_ignore_ascii_case(b"true")
            {
                b"1".to_vec()
            } else if raw.eq_ignore_ascii_case(b"no")
                || raw.eq_ignore_ascii_case(b"off")
                || raw.eq_ignore_ascii_case(b"none")
                || raw.eq_ignore_ascii_case(b"false")
            {
                Vec::new()
            } else {
                raw
            };
            if key.eq_ignore_ascii_case(b"parent") {
                if value.eq_ignore_ascii_case(&pattern) {
                    vm.emit_warning(&format!(
                        "Invalid browscap ini file: 'Parent' value cannot be same as the section name: {} (in file {})",
                        goro_core::string::msg_str(&pattern),
                        path
                    ));
                    continue;
                }
                entry.parent = Some(value);
            } else {
                entry.kv.push((key.to_ascii_lowercase(), value));
            }
        }
        match bd.index.get(&pattern) {
            Some(&i) => bd.entries[i] = entry,
            None => {
                bd.index.insert(pattern, bd.entries.len());
                bd.entries.push(entry);
            }
        }
    }
    Some(bd)
}

fn browser_data(vm: &mut Vm) -> Option<Rc<BrowserData>> {
    let ini = vm
        .constants
        .get(b"browscap".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .unwrap_or_default();
    if ini.is_empty() {
        return None;
    }
    if let Some(d) = BDATA.with(|b| {
        b.borrow()
            .as_ref()
            .filter(|(f, _)| *f == ini)
            .map(|(_, d)| d.clone())
    }) {
        return d;
    }
    let d = load(vm, &ini).map(Rc::new);
    BDATA.with(|b| *b.borrow_mut() = Some((ini, d.clone())));
    d
}

fn minimum_length(e: &Entry) -> usize {
    e.prefix_len + e.contains_len.iter().sum::<usize>()
}

/// browscap_match_string_wildcard()
fn match_wildcard(s: &[u8], pattern: &[u8]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    let mut restore: Option<(usize, usize)> = None;
    while si < s.len() {
        let pc = pattern.get(pi).copied().unwrap_or(0);
        if pc == b'*' {
            pi += 1;
            while pi < pattern.len() && pattern[pi] == b'*' {
                pi += 1;
            }
            if pi == pattern.len() {
                return true;
            }
            if pattern[pi] != b'?' {
                while si < s.len() && s[si] != pattern[pi] {
                    si += 1;
                }
            }
            restore = Some((pi, si));
            continue;
        } else if pi < pattern.len() && (pc == s[si] || pc == b'?') {
            pi += 1;
            si += 1;
            if pi == pattern.len() {
                if si == s.len() {
                    return true;
                }
            } else {
                continue;
            }
        }
        match restore.as_mut() {
            Some((rp, rs)) => {
                pi = *rp;
                *rs += 1;
                si = *rs;
            }
            None => return false,
        }
    }
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }
    pi == pattern.len()
}

fn memnstr(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from > hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// browser_reg_compare(): true on an exact match (the search ends).
fn reg_compare(
    e: &Entry,
    idx: usize,
    agent: &[u8],
    found: &mut Option<usize>,
    prev_len: &mut usize,
) -> bool {
    let pattern_lc = e.pattern.to_ascii_lowercase();
    let mut cur = e.prefix_len;
    for i in 0..NUM_CONTAINS {
        let l = e.contains_len[i];
        if l != 0 {
            let needle = &pattern_lc[e.contains_start[i]..e.contains_start[i] + l];
            match memnstr(agent, cur, needle) {
                Some(p) => cur = p + l,
                None => return false,
            }
        }
    }
    if agent == pattern_lc.as_slice() {
        *found = Some(idx);
        return true;
    }
    if match_wildcard(&agent[e.prefix_len..], &pattern_lc[e.prefix_len..]) {
        let curr_len = e.prefix_len
            + e.pattern[e.prefix_len..]
                .iter()
                .filter(|&&c| !is_placeholder(c))
                .count();
        if found.is_none() || *prev_len < curr_len {
            *found = Some(idx);
            *prev_len = curr_len;
        }
    }
    false
}

fn add_kv(arr: &mut PhpArray, e: &Entry) {
    for (k, v) in &e.kv {
        let key = Vm::value_to_array_key(Value::String(PhpString::from_vec(k.clone())));
        if !arr.contains_key(&key) {
            arr.set(key, Value::String(PhpString::from_vec(v.clone())));
        }
    }
}

/// PHP_FUNCTION(get_browser)
pub fn get_browser(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let agent = args
        .first()
        .map(|v| v.deref())
        .filter(|v| !matches!(v, Value::Null | Value::Undef));
    let return_array = args.get(1).is_some_and(|v| v.is_truthy());
    let Some(bdata) = browser_data(vm) else {
        vm.emit_warning("get_browser(): browscap ini directive not set");
        return Ok(Value::False);
    };
    let agent: Vec<u8> = match agent {
        Some(a) => a.to_php_string().as_bytes().to_vec(),
        None => {
            let ua = match vm.global_lookup(b"_SERVER").map(|v| v.deref()) {
                Some(Value::Array(a)) => a
                    .borrow()
                    .get(&ArrayKey::String(PhpString::from_bytes(b"HTTP_USER_AGENT")))
                    .map(|v| v.deref()),
                _ => None,
            };
            match ua {
                Some(v) => v.to_php_string().as_bytes().to_vec(),
                None => {
                    vm.emit_warning(
                        "get_browser(): HTTP_USER_AGENT variable is not set, cannot determine user agent name",
                    );
                    return Ok(Value::False);
                }
            }
        }
    };
    let lookup = agent.to_ascii_lowercase();
    let mut found = bdata.index.get(&lookup).copied();
    if found.is_none() {
        let mut prev_len = 0usize;
        for (idx, e) in bdata.entries.iter().enumerate() {
            if lookup.len() < minimum_length(e) {
                continue;
            }
            if !lookup
                .iter()
                .zip(e.pattern.iter())
                .take(e.prefix_len)
                .all(|(a, p)| *a == p.to_ascii_lowercase())
                || lookup.len() < e.prefix_len
            {
                continue;
            }
            if reg_compare(e, idx, &lookup, &mut found, &mut prev_len) {
                break;
            }
        }
        if found.is_none() {
            found = bdata.index.get(DEFAULT_SECTION_NAME).copied();
            if found.is_none() {
                return Ok(Value::False);
            }
        }
    }
    let mut e = &bdata.entries[found.unwrap_or_default()];
    let mut arr = PhpArray::new();
    let s = |b: Vec<u8>| Value::String(PhpString::from_vec(b));
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"browser_name_regex")),
        s(convert_pattern(&e.pattern)),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"browser_name_pattern")),
        s(e.pattern.clone()),
    );
    if let Some(p) = &e.parent {
        arr.set(
            ArrayKey::String(PhpString::from_bytes(b"parent")),
            s(p.clone()),
        );
    }
    add_kv(&mut arr, e);
    let mut depth = 0;
    while let Some(p) = &e.parent {
        let Some(&i) = bdata.index.get(p) else { break };
        e = &bdata.entries[i];
        add_kv(&mut arr, e);
        depth += 1;
        if depth > 1000 {
            break;
        }
    }
    if return_array {
        return Ok(Value::Array(Rc::new(RefCell::new(arr))));
    }
    let mut obj = PhpObject::new(b"stdClass".to_vec(), vm.next_object_id());
    for (k, v) in arr.iter() {
        let name = match k {
            ArrayKey::String(s) => s.as_bytes().to_vec(),
            ArrayKey::Int(n) => n.to_string().into_bytes(),
        };
        obj.set_property(name, v.clone());
    }
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}
