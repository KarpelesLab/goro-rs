//! The inifile handler (dba_inifile.c over libinifile/inifile.c).
//! Keys are "[group]name" or "name"; the library works on C strings.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::vm::Vm;

use crate::stream::{self, PStream, SEEK_END, SEEK_SET};
use crate::{Driver, Info, Mode};

pub const VERSION: &str = "1.0, $Id: c5467396d4bfe62513fab439a836d384a4652b26 $";

/// key_type (None: NULL)
#[derive(Clone, Default)]
struct Key {
    group: Option<Vec<u8>>,
    name: Option<Vec<u8>>,
}

/// line_type
#[derive(Clone, Default)]
struct Line {
    key: Key,
    val: Option<Vec<u8>>,
    pos: u64,
}

impl Line {
    fn free(&mut self) {
        *self = Line::default();
    }
}

pub struct Inifile {
    fp: Rc<RefCell<PStream>>,
    curr: Line,
    next: Line,
}

/// A C string: the bytes up to the first NUL.
fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(p) => &b[..p],
        None => b,
    }
}

/// inifile_key_split()
fn key_split(group_name: &[u8]) -> Key {
    let g = cstr(group_name);
    if g.first() == Some(&b'[') {
        if let Some(p) = g.iter().position(|&c| c == b']') {
            return Key {
                group: Some(g[1..p].to_vec()),
                name: Some(g[p + 1..].to_vec()),
            };
        }
    }
    Key {
        group: Some(Vec::new()),
        name: Some(g.to_vec()),
    }
}

/// inifile_key_string()
fn key_string(key: &Key) -> Option<Vec<u8>> {
    match (&key.group, &key.name) {
        (Some(g), n) if !g.is_empty() => {
            let mut r = b"[".to_vec();
            r.extend_from_slice(g);
            r.push(b']');
            if let Some(n) = n {
                r.extend_from_slice(n);
            }
            Some(r)
        }
        (_, Some(n)) => Some(n.clone()),
        _ => None,
    }
}

/// etrim()
fn etrim(s: &[u8]) -> Vec<u8> {
    let s = cstr(s);
    let ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let start = s.iter().position(|c| !ws(c)).unwrap_or(s.len());
    let mut end = s.len();
    while end > start && ws(&s[end - 1]) {
        end -= 1;
    }
    s[start..end].to_vec()
}

/// inifile_key_cmp(): 0 equal, 1 same group, 2 different.
fn key_cmp(k1: &Key, k2: &Key) -> i32 {
    let e = |a: &Option<Vec<u8>>, b: &Option<Vec<u8>>| {
        a.as_deref()
            .unwrap_or_default()
            .eq_ignore_ascii_case(b.as_deref().unwrap_or_default())
    };
    if e(&k1.group, &k2.group) {
        if e(&k1.name, &k2.name) { 0 } else { 1 }
    } else {
        2
    }
}

/// inifile_read()
fn ini_read(fp: &mut PStream, ln: &mut Line) -> bool {
    ln.val = None;
    while let Some(fline) = fp.gets(None) {
        let fline = cstr(&fline).to_vec();
        if fline.first() == Some(&b'[') {
            // A value name cannot start with '[': either a ']' follows or
            // the line is an error.
            if let Some(p) = fline[1..].iter().position(|&c| c == b']') {
                ln.key.group = Some(etrim(&fline[1..1 + p]));
                ln.key.name = Some(Vec::new());
                ln.pos = fp.tell();
                return true;
            }
            continue;
        }
        if let Some(p) = fline.iter().position(|&c| c == b'=') {
            // keep group or make empty if not existent
            if ln.key.group.is_none() {
                ln.key.group = Some(Vec::new());
            }
            ln.key.name = Some(etrim(&fline[..p]));
            ln.val = Some(etrim(&fline[p + 1..]));
            ln.pos = fp.tell();
            return true;
        }
        // lines without '=' are comments
    }
    ln.free();
    false
}

impl Inifile {
    /// inifile_fetch()
    fn ini_fetch(&mut self, key: &Key, skip: i64) -> Option<Vec<u8>> {
        let mut fp = self.fp.borrow_mut();
        let mut ln = Line::default();
        let mut skip = skip;
        if skip == -1
            && self.next.key.group.is_some()
            && self.next.key.name.is_some()
            && key_cmp(&self.next.key, key) == 0
        {
            // we got the position already from the last fetch
            let _ = fp.seek(self.next.pos as i64, SEEK_SET);
            ln.key.group = self.next.key.group.clone();
        } else {
            // specific instance or another key: search from the start
            fp.rewind();
            self.next.free();
        }
        if skip == -1 {
            skip = 0;
        }
        let mut grp_eq = false;
        while ini_read(&mut fp, &mut ln) {
            let res = key_cmp(&ln.key, key);
            if res == 0 {
                if skip == 0 {
                    let val = ln.val.clone().unwrap_or_default();
                    // allow faster access by keeping the key read
                    self.next = ln;
                    self.next.pos = fp.tell();
                    return Some(val);
                }
                skip -= 1;
            } else if res == 1 {
                grp_eq = true;
            } else if grp_eq {
                // leaving the group: the key cannot be found
                break;
            }
        }
        self.next.pos = fp.tell();
        None
    }

    /// inifile_nextkey()
    fn ini_nextkey(&mut self) -> bool {
        let mut fp = self.fp.borrow_mut();
        let mut ln = Line::default();
        let _ = fp.seek(self.curr.pos as i64, SEEK_SET);
        ln.key.group = Some(self.curr.key.group.clone().unwrap_or_default());
        ini_read(&mut fp, &mut ln);
        let r = ln.key.group.is_some() || ln.key.name.is_some();
        self.curr = ln;
        r
    }

    /// inifile_find_group(): (found, pos_grp_start)
    fn find_group(&mut self, key: &Key) -> (bool, u64) {
        let mut fp = self.fp.borrow_mut();
        fp.flush();
        let _ = fp.seek(0, SEEK_SET);
        self.curr.free();
        self.next.free();
        let mut pos_grp_start = 0;
        let mut ret = false;
        if key.group.as_ref().is_some_and(|g| !g.is_empty()) {
            let mut ln = Line::default();
            while ini_read(&mut fp, &mut ln) {
                if key_cmp(&ln.key, key) < 2 {
                    ret = true;
                    break;
                }
                pos_grp_start = fp.tell();
            }
        } else {
            ret = true;
        }
        if !ret {
            pos_grp_start = fp.tell();
        }
        (ret, pos_grp_start)
    }

    /// inifile_next_group(): pos_grp_next
    fn next_group(&mut self, key: &Key) -> u64 {
        let mut fp = self.fp.borrow_mut();
        let mut pos = fp.tell();
        let mut ln = Line {
            key: Key {
                group: key.group.clone(),
                name: None,
            },
            ..Line::default()
        };
        while ini_read(&mut fp, &mut ln) {
            if key_cmp(&ln.key, key) == 2 {
                break;
            }
            pos = fp.tell();
        }
        pos
    }

    /// inifile_delete_replace_append(); found is set for a deleted key.
    fn delete_replace_append(
        &mut self,
        vm: &mut Vm,
        key: &Key,
        value: Option<&[u8]>,
        append: bool,
        found: &mut bool,
    ) -> bool {
        // 1 - 3
        let (_, pos_grp_start) = self.find_group(key);
        let pos_grp_next = self.next_group(key);
        let mut ini_tmp: Option<PStream> = None;
        let mut ret = true;
        if !append && pos_grp_start != pos_grp_next {
            // inifile_copy_to()
            let mut tmp = PStream::memory(Vec::new());
            let mut fp = self.fp.borrow_mut();
            let _ = fp.seek(pos_grp_start as i64, SEEK_SET);
            if !stream::copy(
                &mut fp,
                &mut tmp,
                Some((pos_grp_next - pos_grp_start) as usize),
            ) {
                crate::docref_warning(
                    vm,
                    &format!(
                        "Could not copy group [{} - {}] to temporary stream",
                        pos_grp_start, pos_grp_next
                    ),
                );
                ret = false;
            }
            ini_tmp = Some(tmp);
        }
        // 4
        let mut fp_tmp: Option<PStream> = None;
        if ret {
            let mut tmp = PStream::memory(Vec::new());
            let mut fp = self.fp.borrow_mut();
            let _ = fp.seek(0, SEEK_END);
            if pos_grp_next != fp.tell() {
                let _ = fp.seek(pos_grp_next as i64, SEEK_SET);
                if !stream::copy(&mut fp, &mut tmp, None) {
                    crate::docref_warning(vm, "Could not copy remainder to temporary stream");
                    ret = false;
                }
            }
            fp_tmp = Some(tmp);
        }
        let name_given = key.name.as_ref().is_some_and(|n| !n.is_empty());
        // 5
        if ret && (value.is_none() || name_given) {
            // inifile_truncate()
            let size = if append { pos_grp_next } else { pos_grp_start };
            let mut fp = self.fp.borrow_mut();
            let res = fp.truncate(size);
            if res != 0 {
                drop(fp);
                crate::docref_warning(vm, &format!("Error in ftruncate: {}", res));
                ret = false;
            } else {
                let _ = fp.seek(size as i64, SEEK_SET);
            }
        }
        if ret {
            if name_given {
                // 6
                if let Some(tmp) = ini_tmp.as_mut() {
                    if !self.filter(vm, tmp, key, found) {
                        ret = false;
                    }
                }
                // 7 (done even when the filter failed)
                if let Some(v) = value {
                    let mut fp = self.fp.borrow_mut();
                    if pos_grp_start == pos_grp_next
                        && key.group.as_ref().is_some_and(|g| !g.is_empty())
                    {
                        let mut l = b"[".to_vec();
                        l.extend_from_slice(key.group.as_deref().unwrap_or_default());
                        l.extend_from_slice(b"]\n");
                        fp.write(&l);
                    }
                    let mut l = key.name.clone().unwrap_or_default();
                    l.push(b'=');
                    l.extend_from_slice(cstr(v));
                    l.push(b'\n');
                    fp.write(&l);
                }
            }
            // 8
            if let Some(tmp) = fp_tmp.as_mut() {
                if tmp.tell() != 0 {
                    let _ = tmp.seek(0, SEEK_SET);
                    let mut fp = self.fp.borrow_mut();
                    let _ = fp.seek(0, SEEK_END);
                    if !stream::copy(tmp, &mut fp, None) {
                        drop(fp);
                        let e = vm.create_exception(
                            b"Error",
                            "Could not copy from temporary stream - ini file truncated",
                            vm.current_line,
                        );
                        vm.current_exception = Some(e);
                        ret = false;
                    }
                }
            }
        }
        let mut fp = self.fp.borrow_mut();
        fp.flush();
        let _ = fp.seek(0, SEEK_SET);
        ret
    }

    /// inifile_filter(): copy `from` back while leaving out `key`.
    fn filter(&mut self, vm: &mut Vm, from: &mut PStream, key: &Key, found: &mut bool) -> bool {
        let mut pos_start = 0u64;
        let mut pos_next = 0u64;
        let mut ret = true;
        let mut ln = Line::default();
        let _ = from.seek(0, SEEK_SET);
        let _ = self.fp.borrow_mut().seek(0, SEEK_END);
        while ini_read(from, &mut ln) {
            match key_cmp(&ln.key, key) {
                0 => {
                    *found = true;
                    let pos_curr = from.tell();
                    if pos_start != pos_next {
                        let _ = from.seek(pos_start as i64, SEEK_SET);
                        let mut fp = self.fp.borrow_mut();
                        if !stream::copy(from, &mut fp, Some((pos_next - pos_start) as usize)) {
                            drop(fp);
                            crate::docref_warning(
                                vm,
                                &format!(
                                    "Could not copy [{} - {}] from temporary stream",
                                    pos_next, pos_start
                                ),
                            );
                            ret = false;
                        }
                        let _ = from.seek(pos_curr as i64, SEEK_SET);
                    }
                    pos_next = pos_curr;
                    pos_start = pos_curr;
                }
                1 => pos_next = from.tell(),
                _ => {}
            }
        }
        if pos_start != pos_next {
            let _ = from.seek(pos_start as i64, SEEK_SET);
            let mut fp = self.fp.borrow_mut();
            if !stream::copy(from, &mut fp, Some((pos_next - pos_start) as usize)) {
                drop(fp);
                crate::docref_warning(
                    vm,
                    &format!(
                        "Could not copy [{} - {}] from temporary stream",
                        pos_next, pos_start
                    ),
                );
                ret = false;
            }
        }
        ret
    }
}

/// DBA_OPEN_FUNC(inifile) / inifile_alloc()
pub fn open(info: &Info) -> Result<Box<dyn Driver>, &'static str> {
    let fp = info.fp.clone().ok_or("")?;
    let _readonly = info.mode == Mode::Reader;
    Ok(Box::new(Inifile {
        fp,
        curr: Line::default(),
        next: Line::default(),
    }))
}

impl Driver for Inifile {
    fn close(&mut self) {}

    fn fetch(&mut self, _vm: &mut Vm, key: &[u8], skip: i64) -> Option<Vec<u8>> {
        let k = key_split(key);
        self.ini_fetch(&k, skip)
    }

    fn update(&mut self, vm: &mut Vm, key: &[u8], val: &[u8], mode: i32) -> bool {
        let k = key_split(key);
        let mut found = false;
        if self.delete_replace_append(vm, &k, Some(val), mode == 1, &mut found) {
            true
        } else {
            crate::docref_warning(vm, "Operation not possible");
            false
        }
    }

    fn exists(&mut self, _vm: &mut Vm, key: &[u8]) -> bool {
        let k = key_split(key);
        self.ini_fetch(&k, 0).is_some()
    }

    fn delete(&mut self, vm: &mut Vm, key: &[u8]) -> bool {
        let k = key_split(key);
        let mut found = false;
        let r = self.delete_replace_append(vm, &k, None, false, &mut found);
        r && found
    }

    fn firstkey(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        self.curr.free();
        self.curr.pos = 0;
        if self.ini_nextkey() {
            key_string(&self.curr.key)
        } else {
            None
        }
    }

    fn nextkey(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        if self.curr.key.group.is_none() && self.curr.key.name.is_none() {
            return None;
        }
        if self.ini_nextkey() {
            key_string(&self.curr.key)
        } else {
            None
        }
    }
}
