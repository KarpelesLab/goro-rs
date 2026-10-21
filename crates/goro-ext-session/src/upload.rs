//! Session upload progress (php_session_rfc1867_callback in session.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::rfc1867::Event;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::{ps, Phase, PHASE};

struct FileEntry {
    field_name: Vec<u8>,
    name: Vec<u8>,
    tmp_name: Option<Vec<u8>>,
    error: i64,
    done: bool,
    start_time: i64,
    bytes_processed: i64,
}

#[derive(Default)]
struct Progress {
    content_length: i64,
    query_string: Vec<u8>,
    cookie: Vec<u8>,
    sid: Option<Vec<u8>>,
    key: Option<Vec<u8>>,
    apply_trans_sid: bool,
    has_data: bool,
    start_time: i64,
    bytes_processed: i64,
    done: bool,
    files: Vec<FileEntry>,
    update_step: i64,
    next_update: i64,
    next_update_time: f64,
    cancel_upload: bool,
}

thread_local! {
    static PROGRESS: RefCell<Option<Progress>> = const { RefCell::new(None) };
}

fn now_f() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn k(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_string(s.to_string()))
}

fn sv(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn b(v: bool) -> Value {
    if v { Value::True } else { Value::False }
}

fn build_data(p: &Progress) -> Value {
    let mut files = PhpArray::new();
    for f in &p.files {
        let mut a = PhpArray::new();
        a.set(k("field_name"), sv(&f.field_name));
        a.set(k("name"), sv(&f.name));
        a.set(k("tmp_name"), f.tmp_name.as_ref().map(|t| sv(t)).unwrap_or(Value::Null));
        a.set(k("error"), Value::Long(f.error));
        a.set(k("done"), b(f.done));
        a.set(k("start_time"), Value::Long(f.start_time));
        a.set(k("bytes_processed"), Value::Long(f.bytes_processed));
        files.push(Value::Array(Rc::new(RefCell::new(a))));
    }
    let mut d = PhpArray::new();
    d.set(k("start_time"), Value::Long(p.start_time));
    d.set(k("content_length"), Value::Long(p.content_length));
    d.set(k("bytes_processed"), Value::Long(p.bytes_processed));
    d.set(k("done"), b(p.done));
    d.set(k("files"), Value::Array(Rc::new(RefCell::new(files))));
    Value::Array(Rc::new(RefCell::new(d)))
}

fn url_decode(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < s.len() => {
                let h = std::str::from_utf8(&s[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(c) => {
                        out.push(c);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Look up `name` in a raw query string / cookie header.
fn find_var(data: &[u8], seps: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    for part in data.split(|c| seps.contains(c)) {
        let mut part = part;
        while part.first() == Some(&b' ') {
            part = &part[1..];
        }
        if part.is_empty() {
            continue;
        }
        let (n, v) = match part.iter().position(|&c| c == b'=') {
            Some(p) => (&part[..p], &part[p + 1..]),
            None => (part, &b""[..]),
        };
        if url_decode(n) == name {
            return Some(url_decode(v));
        }
    }
    None
}

/// php_session_rfc1867_early_find_sid
fn early_find_sid(vm: &Vm, p: &mut Progress) {
    let (use_cookies, use_only_cookies, name) = ps(|s| (s.use_cookies, s.use_only_cookies, s.session_name.clone()));
    if use_cookies {
        if let Some(v) = find_var(&p.cookie, b";", &name) {
            p.sid = Some(v);
            p.apply_trans_sid = false;
            return;
        }
    }
    if use_only_cookies {
        return;
    }
    let sep = vm
        .constants
        .get(b"arg_separator.input".as_slice())
        .map(|v| v.to_php_string().as_bytes().to_vec())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| b"&".to_vec());
    if let Some(v) = find_var(&p.query_string, &sep, &name) {
        p.sid = Some(v);
    }
}

fn check_cancel_upload(key: &[u8]) -> bool {
    let arr = match crate::session_vars_arr() {
        Some(a) => a,
        None => return false,
    };
    let a = arr.borrow();
    let ent = match a.get(&goro_core::vm::Vm::value_to_array_key(sv(key))) {
        Some(Value::Array(e)) => e.clone(),
        _ => return false,
    };
    let e = ent.borrow();
    matches!(e.get(&k("cancel_upload")), Some(Value::True))
}

/// php_session_rfc1867_update
fn update(vm: &mut Vm, p: &mut Progress, force: bool) {
    if !force {
        if p.bytes_processed < p.next_update {
            return;
        }
        let min_freq = ps(|s| s.rfc1867_min_freq);
        if min_freq > 0.0 {
            let t = now_f();
            if t < p.next_update_time {
                return;
            }
            p.next_update_time = t + min_freq;
        }
        p.next_update = p.bytes_processed + p.update_step;
    }
    let _ = crate::session_initialize(vm);
    ps(|s| s.status = crate::Status::Active);
    let key = p.key.clone().unwrap_or_default();
    if let Some(arr) = crate::session_vars_arr_mut() {
        p.cancel_upload |= check_cancel_upload(&key);
        let d = build_data(p);
        arr.borrow_mut().set(goro_core::vm::Vm::value_to_array_key(sv(&key)), d);
    }
    let _ = crate::session_flush(vm, true);
}

/// php_session_rfc1867_cleanup
fn cleanup(vm: &mut Vm, p: &Progress) {
    let _ = crate::session_initialize(vm);
    ps(|s| s.status = crate::Status::Active);
    let key = p.key.clone().unwrap_or_default();
    if let Some(arr) = crate::session_vars_arr_mut() {
        arr.borrow_mut().remove(&goro_core::vm::Vm::value_to_array_key(sv(&key)));
    }
    let _ = crate::session_flush(vm, true);
}

pub(crate) fn callback(vm: &mut Vm, ev: &Event) -> bool {
    if !ps(|s| s.rfc1867_enabled) {
        return true;
    }
    let saved_phase = PHASE.with(|ph| ph.get());
    PHASE.with(|ph| ph.set(Phase::RequestStartup));
    let r = callback_inner(vm, ev);
    PHASE.with(|ph| ph.set(saved_phase));
    r
}

fn callback_inner(vm: &mut Vm, ev: &Event) -> bool {
    if let Event::Start { content_length, query_string, cookie } = ev {
        PROGRESS.with(|p| {
            *p.borrow_mut() = Some(Progress {
                content_length: *content_length,
                query_string: query_string.to_vec(),
                cookie: cookie.to_vec(),
                ..Default::default()
            })
        });
        return true;
    }
    let mut p = match PROGRESS.with(|p| p.borrow_mut().take()) {
        Some(p) => p,
        None => return true,
    };
    let tracking = |p: &Progress| p.sid.is_some() && p.key.is_some();
    match ev {
        Event::Start { .. } => {}
        Event::FormData { name, value, .. } => {
            if !tracking(&p) && !name.is_empty() && !value.is_empty() {
                let (sname, rname, prefix, trans) = ps(|s| {
                    (s.session_name.clone(), s.rfc1867_name.clone(), s.rfc1867_prefix.clone(), s.use_trans_sid && !s.use_only_cookies)
                });
                if *name == sname.as_slice() {
                    p.sid = Some(value.to_vec());
                } else if *name == rname.as_slice() {
                    let mut key = prefix;
                    key.extend_from_slice(value);
                    p.key = Some(key);
                    p.apply_trans_sid = trans;
                    early_find_sid(vm, &mut p);
                }
            }
        }
        Event::FileStart { post_bytes_processed, name, filename } => {
            if tracking(&p) {
                if !p.has_data {
                    let freq = ps(|s| s.rfc1867_freq);
                    p.update_step = if freq >= 0 { freq } else { p.content_length * -freq / 100 };
                    p.next_update = 0;
                    p.next_update_time = 0.0;
                    p.has_data = true;
                    p.start_time = now_f() as i64;
                    p.bytes_processed = *post_bytes_processed;
                    p.done = false;
                    let _ = crate::rinit(vm, false);
                    let sid = p.sid.clone();
                    let apply = p.apply_trans_sid;
                    ps(|s| {
                        s.id = sid;
                        if apply {
                            s.use_trans_sid = true;
                            s.use_only_cookies = false;
                        }
                        s.send_cookie = false;
                    });
                }
                p.files.push(FileEntry {
                    field_name: name.to_vec(),
                    name: filename.to_vec(),
                    tmp_name: None,
                    error: 0,
                    done: false,
                    start_time: now_f() as i64,
                    bytes_processed: *post_bytes_processed,
                });
                update(vm, &mut p, false);
            }
        }
        Event::FileData { post_bytes_processed, offset, length } => {
            if tracking(&p) {
                if let Some(f) = p.files.last_mut() {
                    f.bytes_processed = offset + length;
                }
                p.bytes_processed = *post_bytes_processed;
                update(vm, &mut p, false);
            }
        }
        Event::FileEnd { post_bytes_processed, temp_filename, cancel_upload } => {
            if tracking(&p) {
                if let Some(f) = p.files.last_mut() {
                    if let Some(t) = temp_filename {
                        f.tmp_name = Some(t.to_vec());
                    }
                    f.error = *cancel_upload;
                    f.done = true;
                }
                p.bytes_processed = *post_bytes_processed;
                update(vm, &mut p, false);
            }
        }
        Event::End { post_bytes_processed } => {
            if tracking(&p) {
                if ps(|s| s.rfc1867_cleanup) {
                    cleanup(vm, &p);
                } else if p.has_data {
                    p.done = true;
                    p.bytes_processed = *post_bytes_processed;
                    update(vm, &mut p, true);
                }
                let _ = crate::rshutdown_globals(vm);
            }
            return !p.cancel_upload;
        }
    }
    let cancel = p.cancel_upload;
    PROGRESS.with(|slot| *slot.borrow_mut() = Some(p));
    !cancel
}
