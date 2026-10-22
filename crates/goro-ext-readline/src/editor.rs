//! A small emulation of GNU readline 8 as it behaves on a dumb terminal
//! (VT100 cursor motion): line editing, completion, the history list and
//! history files, and the callback interface. Input is read from
//! descriptor 0 a byte at a time, the display goes to standard output.

use std::cell::RefCell;

use goro_core::value::Value;
use goro_core::vm::Vm;

/// rl_library_version
pub const LIBRARY_VERSION: &str = "8.3";

/// rl_basic_word_break_characters
const WORD_BREAK: &[u8] = b" \t\n\"\\'`@$><=;|&{(";

/// The completion function of the CLI shell: matches for (text, start,
/// end), or None to fall back to filename completion. It may set the
/// append character through `set_completion_append_character`.
pub type Completer = fn(&mut Vm, &[u8], usize, usize) -> Option<Vec<Vec<u8>>>;

pub struct Rl {
    pub history: Vec<Vec<u8>>,
    /// rl_line_buffer / rl_point / rl_end
    pub line: Vec<u8>,
    pub point: usize,
    pub mark: i64,
    pub done: i64,
    pub pending_input: i64,
    pub prompt: Vec<u8>,
    pub readline_name: Vec<u8>,
    /// rl_completion_append_character (0: none)
    pub append_char: u8,
    pub suppress_append: bool,
    pub attempted_completion_over: i64,
    /// readline_completion_function()'s callback
    pub completion_cb: Option<Value>,
    /// the CLI shell's completion (rl_attempted_completion_function)
    pub completer: Option<Completer>,
    /// rl_special_prefixes
    pub special_prefixes: Vec<u8>,
    /// readline_callback_handler_install()'s callback
    pub callback: Option<Value>,
    /// a byte read ahead (rl_execute_next)
    pushed: Option<Option<u8>>,
    /// history position while browsing (None: the line being edited)
    hist_pos: Option<usize>,
    saved_line: Vec<u8>,
    /// the previous key was a completion that left the line unchanged (a
    /// TAB then lists the matches)
    last_was_complete: bool,
    completion_changed_buffer: bool,
    /// the terminal prepared by the callback interface
    callback_term: Option<TermGuard>,
}

impl Default for Rl {
    fn default() -> Self {
        Rl {
            history: Vec::new(),
            line: Vec::new(),
            point: 0,
            mark: 0,
            done: 0,
            pending_input: 0,
            prompt: Vec::new(),
            readline_name: b"other".to_vec(),
            append_char: b' ',
            suppress_append: false,
            attempted_completion_over: 0,
            completion_cb: None,
            completer: None,
            special_prefixes: Vec::new(),
            callback: None,
            pushed: None,
            hist_pos: None,
            saved_line: Vec::new(),
            last_was_complete: false,
            completion_changed_buffer: false,
            callback_term: None,
        }
    }
}

thread_local! {
    pub static RL: RefCell<Rl> = RefCell::new(Rl::default());
}

pub fn with<T>(f: impl FnOnce(&mut Rl) -> T) -> T {
    RL.with(|r| f(&mut r.borrow_mut()))
}

pub fn set_completion_append_character(c: u8) {
    with(|r| r.append_char = c);
}

/// Display output (rl_outstream): standard output, after PHP's pending
/// unbuffered output.
fn out(vm: &mut Vm, b: &[u8]) {
    if !b.is_empty() {
        vm.write_stdout_stream(b);
    }
}

/// rl_getc(): a byte of input, None at the end of the input.
fn getc() -> Option<u8> {
    let mut b = 0u8;
    loop {
        let r = unsafe { libc::read(0, &mut b as *mut u8 as *mut libc::c_void, 1) };
        if r == 1 {
            return Some(b);
        }
        if r < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return None;
    }
}

/// _rl_input_queued(0): input is available right now.
fn input_queued() -> bool {
    let mut p = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    let r = unsafe { libc::poll(&mut p, 1, 0) };
    r > 0 && p.revents & libc::POLLIN != 0
}

fn read_key() -> Option<u8> {
    let pushed = with(|r| r.pushed.take());
    match pushed {
        Some(k) => k,
        None => getc(),
    }
}

fn is_self_insert(c: u8) -> bool {
    c >= 0x20 && c != 0x7f
}

/// What a key did to the line.
enum Step {
    Continue,
    /// accept-line (or EOF on a non-empty line)
    Accept,
    /// EOF on an empty line
    Eof,
}

fn back(n: usize) -> Vec<u8> {
    vec![8u8; n]
}

/// Redraw from the point to the end of the line, clearing what follows,
/// and come back to the point.
fn redraw_tail(vm: &mut Vm, clear: bool) {
    let tail = with(|r| r.line[r.point..].to_vec());
    let mut o = tail.clone();
    if clear {
        o.extend_from_slice(b"\x1b[K");
    }
    o.extend_from_slice(&back(tail.len()));
    out(vm, &o);
}

fn insert(vm: &mut Vm, text: &[u8]) {
    if text.is_empty() {
        return;
    }
    let at_end = with(|r| {
        let p = r.point;
        r.line.splice(p..p, text.iter().copied());
        r.point += text.len();
        r.point == r.line.len()
    });
    out(vm, text);
    if !at_end {
        redraw_tail(vm, false);
    }
}

fn move_to(vm: &mut Vm, np: usize) {
    let p = with(|r| r.point);
    if np < p {
        out(vm, &back(p - np));
    } else if np > p {
        out(vm, &b"\x1b[C".repeat(np - p));
    }
    with(|r| r.point = np);
}

/// Replace the whole line (history browsing).
fn replace_line(vm: &mut Vm, new: Vec<u8>) {
    let p = with(|r| r.point);
    let mut o = back(p);
    o.extend_from_slice(&new);
    o.extend_from_slice(b"\x1b[K");
    out(vm, &o);
    with(|r| {
        r.point = new.len();
        r.line = new;
    });
}

fn delete_range(vm: &mut Vm, from: usize, to: usize) {
    if from >= to {
        return;
    }
    let p = with(|r| r.point);
    if p > from {
        out(vm, &back(p - from));
    }
    with(|r| {
        r.line.drain(from..to);
        r.point = from;
    });
    redraw_tail(vm, true);
}

/// Process one key (rl_dispatch).
fn dispatch(vm: &mut Vm, c: Option<u8>) -> Step {
    let Some(c) = c else {
        // EOF: an empty line ends the input, otherwise the line is taken
        return if with(|r| r.line.is_empty()) {
            Step::Eof
        } else {
            Step::Accept
        };
    };
    let was_complete = with(|r| {
        // (readline_info() may have shortened the line under the point)
        r.point = r.point.min(r.line.len());
        std::mem::replace(&mut r.last_was_complete, false)
    });
    match c {
        b'\n' | b'\r' => return Step::Accept,
        b'\t' => {
            let list = was_complete && !with(|r| r.completion_changed_buffer);
            let changed = complete(vm, list);
            with(|r| {
                r.last_was_complete = true;
                r.completion_changed_buffer = changed;
            });
        }
        0x7f | 0x08 => {
            let p = with(|r| r.point);
            if p > 0 {
                delete_range(vm, p - 1, p);
            }
        }
        0x04 => {
            // delete-char, or EOF on an empty line
            let (empty, p, len) = with(|r| (r.line.is_empty(), r.point, r.line.len()));
            if empty {
                return Step::Eof;
            }
            if p < len {
                delete_range(vm, p, p + 1);
            } else {
                ding(vm);
            }
        }
        0x01 => move_to(vm, 0),
        0x05 => {
            let len = with(|r| r.line.len());
            move_to(vm, len);
        }
        0x02 => {
            let p = with(|r| r.point);
            if p > 0 {
                move_to(vm, p - 1);
            }
        }
        0x06 => {
            let (p, len) = with(|r| (r.point, r.line.len()));
            if p < len {
                move_to(vm, p + 1);
            }
        }
        0x0b => {
            let (p, len) = with(|r| (r.point, r.line.len()));
            delete_range(vm, p, len);
        }
        0x15 => {
            let p = with(|r| r.point);
            delete_range(vm, 0, p);
        }
        0x10 => history_prev(vm),
        0x0e => history_next(vm),
        0x1b => escape(vm),
        c if is_self_insert(c) => {
            insert(vm, &[c]);
            // insert the typed-ahead self-inserting input in one go
            loop {
                if with(|r| r.pushed.is_some()) || !input_queued() {
                    break;
                }
                let n = getc();
                match n {
                    Some(n) if is_self_insert(n) => insert(vm, &[n]),
                    other => {
                        with(|r| r.pushed = Some(other));
                        break;
                    }
                }
            }
        }
        _ => {}
    }
    Step::Continue
}

/// ESC sequences: the VT100 arrow keys, Home/End and Delete.
fn escape(vm: &mut Vm) {
    let Some(c) = read_key() else { return };
    if c != b'[' && c != b'O' {
        return;
    }
    let mut seq = Vec::new();
    while let Some(k) = read_key() {
        seq.push(k);
        if k.is_ascii_alphabetic() || k == b'~' {
            break;
        }
    }
    match seq.as_slice() {
        b"A" => history_prev(vm),
        b"B" => history_next(vm),
        b"C" => {
            let (p, len) = with(|r| (r.point, r.line.len()));
            if p < len {
                move_to(vm, p + 1);
            }
        }
        b"D" => {
            let p = with(|r| r.point);
            if p > 0 {
                move_to(vm, p - 1);
            }
        }
        b"H" | b"1~" => move_to(vm, 0),
        b"F" | b"4~" => {
            let len = with(|r| r.line.len());
            move_to(vm, len);
        }
        b"3~" => {
            let (p, len) = with(|r| (r.point, r.line.len()));
            if p < len {
                delete_range(vm, p, p + 1);
            }
        }
        _ => {}
    }
}

fn history_prev(vm: &mut Vm) {
    let new = with(|r| {
        if r.history.is_empty() {
            return None;
        }
        let pos = match r.hist_pos {
            None => {
                r.saved_line = r.line.clone();
                r.history.len() - 1
            }
            Some(0) => return None,
            Some(p) => p - 1,
        };
        r.hist_pos = Some(pos);
        Some(r.history[pos].clone())
    });
    if let Some(l) = new {
        replace_line(vm, l);
    }
}

fn history_next(vm: &mut Vm) {
    let new = with(|r| {
        let p = r.hist_pos?;
        if p + 1 < r.history.len() {
            r.hist_pos = Some(p + 1);
            Some(r.history[p + 1].clone())
        } else {
            r.hist_pos = None;
            Some(std::mem::take(&mut r.saved_line))
        }
    });
    if let Some(l) = new {
        replace_line(vm, l);
    }
}

/// rl_completion_matches(): (common prefix, matches)
fn common_prefix(matches: &[Vec<u8>]) -> Vec<u8> {
    let Some(first) = matches.first() else {
        return Vec::new();
    };
    let mut n = first.len();
    for m in &matches[1..] {
        n = n.min(m.len());
        let mut i = 0;
        while i < n && m[i] == first[i] {
            i += 1;
        }
        n = i;
    }
    first[..n].to_vec()
}

/// rl_filename_completion_function()
fn filename_matches(text: &[u8]) -> Vec<(Vec<u8>, bool)> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let (dir, base) = match text.iter().rposition(|&c| c == b'/') {
        Some(p) => (&text[..p + 1], &text[p + 1..]),
        None => (&b""[..], text),
    };
    let dpath: &[u8] = if dir.is_empty() { b"." } else { dir };
    let Ok(rd) = std::fs::read_dir(std::ffi::OsStr::from_bytes(dpath)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().into_vec();
        if !name.starts_with(base) {
            continue;
        }
        if base.is_empty() && name.first() == Some(&b'.') {
            continue;
        }
        let is_dir = e.path().is_dir();
        let mut full = dir.to_vec();
        full.extend_from_slice(&name);
        out.push((full, is_dir));
    }
    out.sort();
    out
}

/// rl_ding()
fn ding(vm: &mut Vm) {
    out(vm, b"\x07");
}

/// rl_complete(): `list` is the second TAB (what_to_do '?'). Whether the
/// line changed.
fn complete(vm: &mut Vm, list: bool) -> bool {
    let (line, point, specials) = with(|r| (r.line.clone(), r.point, r.special_prefixes.clone()));
    // _rl_find_completion_word()
    let mut start = point;
    while start > 0 && !WORD_BREAK.contains(&line[start - 1]) {
        start -= 1;
    }
    if start > 0 && specials.contains(&line[start - 1]) {
        start -= 1;
    }
    let text = line[start..point].to_vec();
    with(|r| {
        r.append_char = b' ';
        r.suppress_append = false;
        r.attempted_completion_over = 0;
    });
    let cb = with(|r| r.completion_cb.clone());
    let completer = with(|r| r.completer);
    let mut matches: Option<Vec<Vec<u8>>> = None;
    let mut dirs: Vec<bool> = Vec::new();
    if let Some(cb) = cb {
        // php_readline_completion_cb()
        let args = [
            Value::String(goro_core::string::PhpString::from_bytes(&text)),
            Value::Long(start as i64),
            Value::Long(point as i64),
        ];
        if let Ok(Value::Array(a)) = vm.call_callback(&cb, &args).map(|v| v.deref()) {
            let entries: Vec<Value> = a.borrow().values().cloned().collect();
            if entries.is_empty() {
                // matches[0] = "": a single empty match
                matches = Some(vec![Vec::new()]);
            } else {
                let mut m = Vec::new();
                for e in entries {
                    let s = e.to_php_string();
                    if s.as_bytes().starts_with(&text) {
                        m.push(s.as_bytes().to_vec());
                    }
                }
                if !m.is_empty() {
                    matches = Some(m);
                }
            }
        }
        if vm.current_exception.is_some() {
            return false;
        }
    } else if let Some(f) = completer {
        matches = f(vm, &text, start, point).filter(|m| !m.is_empty());
    }
    if matches.is_none() && with(|r| r.attempted_completion_over) == 0 {
        let f = filename_matches(&text);
        if !f.is_empty() {
            dirs = f.iter().map(|x| x.1).collect();
            matches = Some(f.into_iter().map(|x| x.0).collect());
        }
    }
    let Some(matches) = matches else {
        ding(vm);
        return false;
    };
    if list {
        display_matches(vm, &matches);
        return false;
    }
    let before = with(|r| r.line.clone());
    if matches.len() == 1 {
        let mut ins = matches[0].clone();
        let (ac, suppress) = with(|r| (r.append_char, r.suppress_append));
        if dirs.first() == Some(&true) {
            ins.push(b'/');
        } else if ac != 0 && !suppress && !ins.is_empty() {
            ins.push(ac);
        }
        replace_word(vm, start, point, &ins);
    } else {
        let prefix = common_prefix(&matches);
        // (the bell rings before the line is redrawn)
        ding(vm);
        if !prefix.is_empty() {
            replace_word(vm, start, point, &prefix);
        }
    }
    with(|r| r.line != before)
}

/// rl_display_match_list(), after asking when there are many.
fn display_matches(vm: &mut Vm, matches: &[Vec<u8>]) {
    let mut m: Vec<Vec<u8>> = matches.to_vec();
    m.sort();
    m.dedup();
    if m.len() >= 100 {
        out(
            vm,
            format!("\nDisplay all {} possibilities? (y or n)", m.len()).as_bytes(),
        );
        loop {
            match read_key() {
                Some(b'y') | Some(b'Y') | Some(b' ') => break,
                Some(b'n') | Some(b'N') | Some(0x7f) => {
                    out(vm, b"\n");
                    redisplay_full(vm);
                    return;
                }
                None => {
                    // rl_abort()
                    ding(vm);
                    return;
                }
                _ => ding(vm),
            }
        }
    }
    let max = m.iter().map(|x| x.len()).max().unwrap_or(0) + 2;
    // (a dumb terminal does not wrap: one column less than COLUMNS)
    let width = 79usize;
    let mut cols = (width / max).max(1);
    if cols != 1 && cols * max == width {
        cols -= 1;
    }
    let rows = m.len().div_ceil(cols);
    let mut o = b"\n".to_vec();
    for r in 0..rows {
        for c in 0..cols {
            let i = c * rows + r;
            let Some(e) = m.get(i) else { break };
            o.extend_from_slice(e);
            if c + 1 < cols {
                o.extend(std::iter::repeat_n(b' ', max - e.len()));
            }
        }
        o.push(b'\n');
    }
    out(vm, &o);
    redisplay_full(vm);
}

/// rl_forced_update_display(): the prompt and the line on a fresh line.
fn redisplay_full(vm: &mut Vm) {
    let (prompt, line, p) = with(|r| (r.prompt.clone(), r.line.clone(), r.point));
    let mut o = prompt;
    o.extend_from_slice(&line);
    o.extend_from_slice(&back(line.len() - p));
    out(vm, &o);
}

/// Replace line[start..end] by `text`, redrawing from the first byte
/// that differs.
fn replace_word(vm: &mut Vm, start: usize, end: usize, text: &[u8]) {
    let line = with(|r| r.line.clone());
    let old = &line[start..end];
    let common = old.iter().zip(text).take_while(|(a, b)| a == b).count();
    if common == old.len() {
        insert(vm, &text[common..]);
        return;
    }
    // move back to the first difference, then rewrite the rest
    let p = with(|r| r.point);
    let target = start + common;
    out(vm, &back(p - target));
    let mut newline = line[..start].to_vec();
    newline.extend_from_slice(text);
    newline.extend_from_slice(&line[end..]);
    let tail = newline[target..].to_vec();
    let shrink = newline.len() < line.len();
    with(|r| {
        r.line = newline;
        r.point = start + text.len();
    });
    let mut o = tail.clone();
    if shrink {
        o.extend_from_slice(b"\x1b[K");
    }
    let after = line.len() - end;
    o.extend_from_slice(&back(after));
    out(vm, &o);
}

/// rl_prep_terminal(): on a terminal, characters come one at a time and
/// are echoed by the editor (restored when dropped).
pub struct TermGuard(Option<libc::termios>);

impl TermGuard {
    fn new() -> TermGuard {
        unsafe {
            if libc::isatty(0) != 1 {
                return TermGuard(None);
            }
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return TermGuard(None);
            }
            let orig = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::IEXTEN);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSADRAIN, &t);
            TermGuard(Some(orig))
        }
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        if let Some(t) = self.0.as_ref() {
            unsafe {
                libc::tcsetattr(0, libc::TCSADRAIN, t);
            }
        }
    }
}

/// readline(): the line read, None at the end of the input.
pub fn readline(vm: &mut Vm, prompt: Option<&[u8]>) -> Option<Vec<u8>> {
    let prompt = prompt.unwrap_or_default().to_vec();
    with(|r| {
        r.prompt = prompt.clone();
        r.line.clear();
        r.point = 0;
        r.done = 0;
        r.hist_pos = None;
        r.last_was_complete = false;
    });
    vm.flush_cli_output();
    out(vm, &prompt);
    let _term = TermGuard::new();
    loop {
        let k = read_key();
        match dispatch(vm, k) {
            Step::Continue => {}
            Step::Accept => {
                out(vm, b"\n");
                return Some(with(|r| {
                    r.done = 1;
                    r.line.clone()
                }));
            }
            Step::Eof => {
                with(|r| r.done = 1);
                return None;
            }
        }
    }
}

/// rl_callback_handler_install(): show the prompt.
pub fn callback_install(vm: &mut Vm, prompt: &[u8]) {
    with(|r| {
        r.prompt = prompt.to_vec();
        r.line.clear();
        r.point = 0;
        r.done = 0;
        r.hist_pos = None;
        if r.callback_term.is_none() {
            r.callback_term = Some(TermGuard::new());
        }
    });
    out(vm, prompt);
}

/// rl_callback_handler_remove(): the terminal settings come back.
pub fn callback_remove() {
    let t = with(|r| r.callback_term.take());
    drop(t);
}

/// rl_callback_read_char(): the lines completed (None: EOF), in order.
pub fn callback_read_char(vm: &mut Vm) -> Vec<Option<Vec<u8>>> {
    let mut lines = Vec::new();
    loop {
        let k = read_key();
        match dispatch(vm, k) {
            Step::Continue => {}
            Step::Accept => {
                out(vm, b"\n");
                let l = with(|r| {
                    r.done = 1;
                    std::mem::take(&mut r.line)
                });
                lines.push(Some(l));
                break;
            }
            Step::Eof => {
                with(|r| r.done = 1);
                lines.push(None);
                break;
            }
        }
        if with(|r| r.pushed.is_none()) {
            break;
        }
    }
    lines
}

/// _rl_callback_newline(): a fresh line and the prompt again.
pub fn callback_newline(vm: &mut Vm) {
    let prompt = with(|r| {
        r.line.clear();
        r.point = 0;
        r.done = 0;
        r.hist_pos = None;
        r.prompt.clone()
    });
    out(vm, &prompt);
}

// ---------------------------------------------------------------------------
// history files (histfile.c)

fn default_history_file() -> Vec<u8> {
    let mut p = std::env::var_os("HOME")
        .map(|h| {
            use std::os::unix::ffi::OsStringExt;
            h.into_vec()
        })
        .unwrap_or_default();
    p.extend_from_slice(b"/.history");
    p
}

fn os(p: &[u8]) -> &std::ffi::OsStr {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(p)
}

/// read_history(): 0 or an errno value.
pub fn read_history(file: Option<&[u8]>) -> i32 {
    let f = file
        .map(|f| f.to_vec())
        .unwrap_or_else(default_history_file);
    match std::fs::read(os(&f)) {
        Ok(data) => {
            if !data.is_empty() {
                let mut lines: Vec<&[u8]> = data.split(|&c| c == b'\n').collect();
                if data.last() == Some(&b'\n') {
                    lines.pop();
                }
                with(|r| {
                    for l in lines {
                        r.history.push(l.to_vec());
                    }
                });
            }
            0
        }
        Err(e) => e.raw_os_error().unwrap_or(libc::EIO),
    }
}

/// write_history(): 0 or an errno value.
pub fn write_history(file: Option<&[u8]>) -> i32 {
    use std::os::unix::fs::OpenOptionsExt;
    let f = file
        .map(|f| f.to_vec())
        .unwrap_or_else(default_history_file);
    let mut data = Vec::new();
    with(|r| {
        for l in &r.history {
            data.extend_from_slice(l);
            data.push(b'\n');
        }
    });
    let r = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(os(&f))
        .and_then(|mut fh| std::io::Write::write_all(&mut fh, &data));
    match r {
        Ok(()) => 0,
        Err(e) => e.raw_os_error().unwrap_or(libc::EIO),
    }
}

/// append_history(): the last `n` entries are added to an existing file.
pub fn append_history(n: usize, file: &[u8]) -> i32 {
    let mut data = Vec::new();
    with(|r| {
        let from = r.history.len().saturating_sub(n);
        for l in &r.history[from..] {
            data.extend_from_slice(l);
            data.push(b'\n');
        }
    });
    let r = std::fs::OpenOptions::new()
        .append(true)
        .open(os(file))
        .and_then(|mut fh| std::io::Write::write_all(&mut fh, &data));
    match r {
        Ok(()) => 0,
        Err(e) => e.raw_os_error().unwrap_or(libc::EIO),
    }
}

pub fn add_history(line: &[u8]) {
    with(|r| r.history.push(line.to_vec()));
}
