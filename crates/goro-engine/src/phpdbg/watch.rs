//! Watchpoints (`watch`): a variable, an element or a property path, read
//! at each statement; a change stops the script (messages as the tests
//! sapi/phpdbg/tests/watch_*.phpt show them).

use goro_runtime::{Key, KeyRef, Value};

use super::{msg, sync_out, with, write_raw};
use crate::vm::{Vm, frame_flags};

#[derive(Clone)]
pub(crate) enum Seg {
    Key(Key),
    Prop(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Plain,
    /// `watch r`: the value and everything in it.
    Recursive,
    /// `watch a`: the number of elements of an array.
    Array,
}

#[derive(Clone, PartialEq)]
pub(crate) enum Snap {
    /// (is a reference, shown value)
    Val(bool, String),
    Count(usize),
}

#[derive(Clone)]
pub(crate) struct Watch {
    pub id: u32,
    pub shown: String,
    pub var: Vec<u8>,
    pub path: Vec<Seg>,
    pub kind: Kind,
    /// The frame (index and slot base) whose variable it is.
    pub frame: usize,
    pub base: usize,
    pub snap: Option<Snap>,
}

/// A value as watch messages show it (print_r's words, on one line).
pub(crate) fn render(v: &Value) -> String {
    match v {
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "1".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => goro_runtime::convert::float_to_string(*f, 14).to_string_lossy(),
        Value::Str(s) => s.to_string_lossy(),
        Value::Array(a) => {
            let items: Vec<String> = a
                .iter()
                .map(|(k, v)| {
                    let k = match k {
                        KeyRef::Int(i) => i.to_string(),
                        KeyRef::Str(s) => s.to_string_lossy(),
                    };
                    format!("[{}] => {}", k, render(&v.deref()))
                })
                .collect();
            format!("Array ({})", items.join(","))
        }
        Value::Object(o) => format!("{} Object", o.class_name().to_string_lossy()),
        Value::Resource(r) => format!("Resource id #{}", r.id()),
        Value::Ref(r) => render(&r.get()),
    }
}

enum Read {
    /// The frame is gone.
    Gone,
    Value(Option<(bool, Value)>),
}

fn read(vm: &Vm, w: &Watch) -> Read {
    let Some(f) = vm.ex.frames.get(w.frame) else {
        return Read::Gone;
    };
    if f.base as usize != w.base {
        return Read::Gone;
    }
    let code = f.code();
    let mut cur: Option<Value> = match code.cv_slot(&w.var) {
        // SAFETY: a variable slot of a live frame.
        Some(i) => unsafe { (*f.base.add(i as usize)).clone() },
        None if f.flags & frame_flags::GLOBAL != 0 => {
            vm.globals.dyn_vars.get_bytes(&w.var).cloned()
        }
        None => f
            .dyn_vars
            .as_ref()
            .and_then(|d| d.get_bytes(&w.var).cloned()),
    };
    for s in &w.path {
        let Some(v) = cur.take() else { break };
        cur = match (v.deref(), s) {
            (Value::Array(a), Seg::Key(k)) => a.get(k.as_ref()).cloned(),
            (Value::Object(o), Seg::Prop(p)) => {
                let v = vm.get_prop_raw(&o, p);
                (!v.is_null()).then_some(v)
            }
            _ => None,
        };
    }
    Read::Value(cur.map(|v| match v {
        Value::Ref(r) => (true, r.get()),
        other => (false, other),
    }))
}

fn snap(kind: Kind, v: &Option<(bool, Value)>) -> Option<Snap> {
    let (is_ref, v) = v.as_ref()?;
    Some(match kind {
        Kind::Array => match v {
            Value::Array(a) => Snap::Count(a.len()),
            _ => return None,
        },
        _ => Snap::Val(*is_ref, render(v)),
    })
}

fn removed_msg(w: &Watch) -> String {
    format!(
        "[{} has been removed, removing watchpoint{}]",
        w.shown,
        if w.kind == Kind::Recursive {
            " recursively"
        } else {
            ""
        }
    )
}

/// Reads every watchpoint: removes those whose frame is gone (with a
/// message), and returns the message of the changed ones.
pub(crate) fn check(vm: &mut Vm) -> Option<String> {
    if with(|d| d.watches.is_empty()) {
        return None;
    }
    if vm.header_callback.is_some() && vm.out.written == 0 {
        report_before_headers(vm);
    }
    let mut watches = with(|d| std::mem::take(&mut d.watches));
    let mut out: Vec<String> = Vec::new();
    let mut keep = Vec::new();
    for mut w in watches.drain(..) {
        match read(vm, &w) {
            Read::Gone => msg(vm, &removed_msg(&w)),
            Read::Value(v) => {
                let new = snap(w.kind, &v);
                if new != w.snap {
                    let mut m = format!("[Breaking on watchpoint {}]", w.shown);
                    match (&w.snap, &new) {
                        (Some(Snap::Count(a)), Some(Snap::Count(b))) => {
                            if b > a {
                                m.push_str(&format!(
                                    "\n{} elements were added to the array",
                                    b - a
                                ));
                            } else {
                                m.push_str(&format!(
                                    "\n{} elements were removed from the array",
                                    a - b
                                ));
                            }
                        }
                        (_, Some(Snap::Count(_))) | (Some(Snap::Count(_)), _) => {}
                        (None, Some(Snap::Val(r, t))) => {
                            m.push_str(&format!(
                                "\nOld value: \nNew value{}: {}",
                                if *r { " (reference)" } else { "" },
                                t
                            ));
                        }
                        (Some(Snap::Val(..)), None) => {
                            m.push_str("\nOld value inaccessible or destroyed\nNew value: ");
                        }
                        (Some(Snap::Val(r0, t0)), Some(Snap::Val(r, t))) => {
                            if r0 != r {
                                m.push_str(&format!(
                                    "\nOld value inaccessible or destroyed\nNew value{}: {}",
                                    if *r { " (reference)" } else { "" },
                                    t
                                ));
                            } else {
                                m.push_str(&format!("\nOld value: {}\nNew value: {}", t0, t));
                            }
                        }
                        (None, None) => {}
                    }
                    out.push(m);
                    w.snap = new;
                }
                keep.push(w);
            }
        }
    }
    with(|d| {
        keep.append(&mut d.watches);
        d.watches = keep;
    });
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
}

/// A watch message as the script's first output: the header callback runs
/// in the middle of it (after "Old value: "), under the debugger, and the
/// script does not stop again once the message ends (tests
/// sapi/phpdbg/tests/gh15210_001.phpt, gh15210_002.phpt).
fn report_before_headers(vm: &mut Vm) {
    for w in with(|d| d.watches.clone()) {
        let Read::Value(v) = read(vm, &w) else {
            continue;
        };
        let new = snap(w.kind, &v);
        let (Some(Snap::Val(_, old)), Some(Snap::Val(_, text))) = (&w.snap, &new) else {
            continue;
        };
        if old == text {
            continue;
        }
        msg(vm, &format!("[Breaking on watchpoint {}]", w.shown));
        write_raw(b"Old value: ");
        with(|d| d.line_start = true);
        vm.send_headers();
        // (A command in the callback may have abandoned the script.)
        if with(|d| d.abort.is_some()) {
            return;
        }
        sync_out(vm);
        write_raw(format!("{}\nNew value: {}\n\n", old, text).as_bytes());
        with(|d| {
            if let Some(x) = d.watches.iter_mut().find(|x| x.id == w.id) {
                x.snap = new.clone();
            }
        });
        return;
    }
}

/// Removes every watchpoint, newest first, with its message.
pub(crate) fn remove_all(vm: &mut Vm) {
    let ws = with(|d| std::mem::take(&mut d.watches));
    for w in ws.iter().rev() {
        msg(vm, &removed_msg(w));
    }
}

/// Parses `$name[key]->prop...`; the flag tells a final `[]` (every
/// element).
fn parse(s: &str) -> Option<(Vec<u8>, Vec<Seg>, bool)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'$') {
        return None;
    }
    let ident = |i: usize| -> usize {
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] >= 0x80) {
            j += 1;
        }
        j
    };
    let end = ident(1);
    if end == 1 || b[1].is_ascii_digit() {
        return None;
    }
    let var = b[1..end].to_vec();
    let mut path = Vec::new();
    let mut i = end;
    let mut all = false;
    while i < b.len() {
        if b[i] == b'[' {
            let close = b[i..].iter().position(|&c| c == b']')? + i;
            let k = &s[i + 1..close];
            i = close + 1;
            if k.is_empty() {
                if i != b.len() {
                    return None;
                }
                all = true;
                break;
            }
            let k = k.trim_matches(|c| c == '\'' || c == '"');
            path.push(Seg::Key(Key::from_bytes(k.as_bytes())));
        } else if b[i..].starts_with(b"->") {
            let e = ident(i + 2);
            if e == i + 2 {
                return None;
            }
            path.push(Seg::Prop(b[i + 2..e].to_vec()));
            i = e;
        } else {
            return None;
        }
    }
    Some((var, path, all))
}

fn shown_of(var: &[u8], path: &[Seg]) -> String {
    let mut s = format!("${}", String::from_utf8_lossy(var));
    for p in path {
        match p {
            Seg::Key(Key::Int(i)) => s.push_str(&format!("[{}]", i)),
            Seg::Key(Key::Str(k)) => s.push_str(&format!("[{}]", k.to_string_lossy())),
            Seg::Prop(n) => s.push_str(&format!("->{}", String::from_utf8_lossy(n))),
        }
    }
    s
}

/// `watch [r|a] EXPR`, `watch d N`, `watch l`.
pub(crate) fn command(vm: &mut Vm, rest: &str, running: bool) {
    let mut words = rest.split_whitespace();
    let mut kind = Kind::Plain;
    let mut expr = words.next().unwrap_or("");
    match expr {
        "r" | "recursive" => {
            kind = Kind::Recursive;
            expr = words.next().unwrap_or("");
        }
        "a" | "array" => {
            kind = Kind::Array;
            expr = words.next().unwrap_or("");
        }
        "l" | "list" => {
            let list = with(|d| {
                d.watches
                    .iter()
                    .map(|w| format!("#{}\t\t{}", w.id, w.shown))
                    .collect::<Vec<_>>()
            });
            if !list.is_empty() {
                msg(vm, &list.join("\n"));
            }
            return;
        }
        "d" | "delete" => {
            let n: Option<u32> = words.next().and_then(|w| w.parse().ok());
            let found = with(|d| {
                let before = d.watches.len();
                d.watches.retain(|w| Some(w.id) != n);
                before != d.watches.len()
            });
            if found {
                msg(vm, &format!("[Removed watchpoint #{}]", n.unwrap_or(0)));
            } else {
                msg(
                    vm,
                    "[Nothing was deleted, no corresponding watchpoint found]",
                );
            }
            return;
        }
        _ => {}
    }
    let Some((var, path, all)) = parse(expr) else {
        msg(vm, "[Malformed input]");
        return;
    };
    if !running || !vm.has_frame() {
        msg(vm, "[No active op array!]");
        return;
    }
    let frame = vm.ex.frames.len() - 1;
    let base = vm.frame().base as usize;
    let mut probe = Watch {
        id: 0,
        shown: String::new(),
        var: var.clone(),
        path: path.clone(),
        kind,
        frame,
        base,
        snap: None,
    };
    let mut targets: Vec<Vec<Seg>> = Vec::new();
    if all || kind == Kind::Array {
        // An array watch needs an array (test sapi/phpdbg/tests/gh13681).
        let cur = match read(vm, &probe) {
            Read::Value(Some((_, v))) => v,
            _ => return,
        };
        let Value::Array(a) = cur else { return };
        if all && kind != Kind::Array {
            for (k, _) in a.iter() {
                let mut p = path.clone();
                p.push(Seg::Key(k.to_owned()));
                targets.push(p);
            }
        } else {
            targets.push(path.clone());
        }
    } else {
        targets.push(path.clone());
    }
    for p in targets {
        probe.path = p;
        let v = match read(vm, &probe) {
            Read::Value(v) => v,
            Read::Gone => None,
        };
        let shown = if kind == Kind::Array {
            format!("{}[]", shown_of(&var, &probe.path))
        } else {
            shown_of(&var, &probe.path)
        };
        let id = with(|d| {
            let id = d.next_watch;
            d.next_watch += 1;
            d.watches.push(Watch {
                id,
                shown: shown.clone(),
                var: var.clone(),
                path: probe.path.clone(),
                kind,
                frame,
                base,
                snap: snap(kind, &v),
            });
            id
        });
        msg(
            vm,
            &format!(
                "[Added {}watchpoint #{} for {}]",
                if kind == Kind::Recursive {
                    "recursive "
                } else {
                    ""
                },
                id,
                shown
            ),
        );
    }
}
