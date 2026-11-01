//! ext/dba (PHP manual: Database (dbm-style) Abstraction Layer) with three
//! file formats: cdb (D. J. Bernstein's constant database format, read by
//! `cdb` and written by `cdb_make` or `cdb` in mode "n"), flatfile and
//! inifile (the behaviour the ext/dba tests show).
//!
//! Handlers needing libraries (db4, gdbm, qdbm, lmdb, tcadb) are not built;
//! their tests skip as on a PHP built without them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::rc::Rc;

use goro_runtime::{Key, NativeData, ObjRef, PhpArray, PhpStr, Value, WeakObj};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::Module;
use crate::vm::{R, Vm};

const HANDLERS: &[(&str, &str)] = &[
    ("cdb", "0.75"),
    ("cdb_make", "0.75"),
    ("flatfile", "1.0"),
    ("inifile", "1.0"),
];

pub static MODULE: Module = Module {
    name: "dba",
    functions: &[
        (
            "function dba_popen(string $path, string $mode, ?string $handler = null, int $permission = 0o644, int $map_size = 0, ?int $flags = null): Dba\\Connection|false",
            dba_popen,
        ),
        (
            "function dba_open(string $path, string $mode, ?string $handler = null, int $permission = 0o644, int $map_size = 0, ?int $flags = null): Dba\\Connection|false",
            dba_open,
        ),
        ("function dba_close(Dba\\Connection $dba): void", dba_close),
        (
            "function dba_exists(string|array $key, Dba\\Connection $dba): bool",
            dba_exists,
        ),
        (
            "function dba_fetch(string|array $key, $dba, $skip = 0): string|false",
            dba_fetch,
        ),
        (
            "function dba_key_split(string|false|null $key): array|false",
            dba_key_split,
        ),
        (
            "function dba_firstkey(Dba\\Connection $dba): string|false",
            dba_firstkey,
        ),
        (
            "function dba_nextkey(Dba\\Connection $dba): string|false",
            dba_nextkey,
        ),
        (
            "function dba_delete(string|array $key, Dba\\Connection $dba): bool",
            dba_delete,
        ),
        (
            "function dba_insert(string|array $key, string $value, Dba\\Connection $dba): bool",
            dba_insert,
        ),
        (
            "function dba_replace(string|array $key, string $value, Dba\\Connection $dba): bool",
            dba_replace,
        ),
        (
            "function dba_optimize(Dba\\Connection $dba): bool",
            dba_optimize,
        ),
        ("function dba_sync(Dba\\Connection $dba): bool", dba_sync),
        (
            "function dba_handlers(bool $full_info = false): array",
            dba_handlers,
        ),
        ("function dba_list(): array", dba_list),
    ],
    classes: &[(
        "dba\\connection",
        "<?php namespace Dba; #[\\NotSerializable] final class Connection {}",
    )],
    hooks: &[("dba\\connection", hooks)],
    ini: &[("dba.default_handler", "flatfile")],
    on_ini_set: Some(|vm, name, value| {
        // Test default_handler_ini.phpt.
        if name == b"dba.default_handler"
            && !value.is_empty()
            && !HANDLERS.iter().any(|(h, _)| h.as_bytes() == value)
        {
            let v = String::from_utf8_lossy(value).into_owned();
            vm.warn(&format!("ini_set(): No such handler: {v}"))?;
            return Ok(false);
        }
        Ok(true)
    }),
    shutdown: Some(|vm| {
        let st = vm.ext::<DbaState>();
        st.persistent.clear();
        st.list.clear();
    }),
    info: Some(|_| {
        "DBA support => enabled\nSupported handlers => cdb cdb_make flatfile inifile\n".to_string()
    }),
    ..Module::EMPTY
};

#[derive(Default)]
struct DbaState {
    persistent: HashMap<String, Shared>,
    /// Open connections for `dba_list()`: object id, path.
    list: Vec<(i64, String, WeakObj)>,
}

type Shared = Rc<RefCell<Db>>;

/// One open database.
struct Db {
    store: Store,
    /// Held for the lock (the lock file or a second descriptor of the
    /// database); released when dropped.
    _lock: Option<File>,
}

enum Store {
    CdbRead(CdbRead),
    CdbMake(CdbMake),
    Flat(Flat),
    Ini(Ini),
}

/// The native data of a `Dba\Connection`.
#[derive(Default)]
struct Conn {
    db: Option<Shared>,
    readonly: bool,
    handler: String,
}

impl NativeData for Conn {}

fn with_conn<T>(o: &ObjRef, f: impl FnOnce(&mut Conn) -> T) -> Option<T> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<Conn>().map(f)
}

fn hooks() -> NativeHooks {
    NativeHooks {
        new_check: Some(|vm| {
            vm.throw(
                "Error",
                "Cannot directly construct Dba\\Connection, use dba_open() or dba_popen() instead",
            )
        }),
        ..Default::default()
    }
}

// ----- cdb: http://cr.yp.to/cdb/cdb.txt ------------------------------------------

fn cdb_hash(key: &[u8]) -> u32 {
    let mut h: u32 = 5381;
    for &c in key {
        h = (h.wrapping_shl(5).wrapping_add(h)) ^ c as u32;
    }
    h
}

struct CdbRead {
    data: Vec<u8>,
    cursor: usize,
}

impl CdbRead {
    fn u32_at(&self, p: usize) -> Option<u32> {
        let b = self.data.get(p..p + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Where the records end: the first hash table.
    fn end_of_records(&self) -> usize {
        self.u32_at(0).map_or(0, |p| p as usize)
    }

    fn fetch(&self, key: &[u8], skip: usize) -> Option<Vec<u8>> {
        let h = cdb_hash(key);
        let t = (h & 255) as usize * 8;
        let pos = self.u32_at(t)? as usize;
        let slots = self.u32_at(t + 4)? as usize;
        if slots == 0 {
            return None;
        }
        let mut slot = ((h >> 8) as usize) % slots;
        let mut left = skip;
        for _ in 0..slots {
            let sp = pos + slot * 8;
            let sh = self.u32_at(sp)?;
            let rec = self.u32_at(sp + 4)? as usize;
            if rec == 0 {
                return None;
            }
            if sh == h {
                let klen = self.u32_at(rec)? as usize;
                let dlen = self.u32_at(rec + 4)? as usize;
                if self.data.get(rec + 8..rec + 8 + klen)? == key {
                    if left == 0 {
                        let d = rec + 8 + klen;
                        return Some(self.data.get(d..d + dlen)?.to_vec());
                    }
                    left -= 1;
                }
            }
            slot = (slot + 1) % slots;
        }
        None
    }

    fn key_at(&mut self, p: usize) -> Option<Vec<u8>> {
        if p < 2048 || p + 8 > self.end_of_records() {
            return None;
        }
        let klen = self.u32_at(p)? as usize;
        let dlen = self.u32_at(p + 4)? as usize;
        let k = self.data.get(p + 8..p + 8 + klen)?.to_vec();
        self.cursor = p + 8 + klen + dlen;
        Some(k)
    }
}

struct CdbMake {
    file: File,
    /// The records so far.
    buf: Vec<u8>,
    entries: Vec<(u32, u32)>,
}

impl CdbMake {
    fn add(&mut self, key: &[u8], value: &[u8]) {
        let pos = 2048 + self.buf.len() as u32;
        self.buf
            .extend_from_slice(&(key.len() as u32).to_le_bytes());
        self.buf
            .extend_from_slice(&(value.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(key);
        self.buf.extend_from_slice(value);
        self.entries.push((cdb_hash(key), pos));
    }
}

impl Drop for CdbMake {
    fn drop(&mut self) {
        // The header, the records, then 256 tables of twice as many slots as
        // they have entries, each entry at its hash's slot or the next free.
        let mut header = vec![0u8; 2048];
        let mut out = std::mem::take(&mut self.buf);
        for b in 0..256usize {
            let list: Vec<(u32, u32)> = self
                .entries
                .iter()
                .copied()
                .filter(|(h, _)| (h & 255) as usize == b)
                .collect();
            let slots = list.len() * 2;
            let table_pos = (2048 + out.len()) as u32;
            header[b * 8..b * 8 + 4].copy_from_slice(&table_pos.to_le_bytes());
            header[b * 8 + 4..b * 8 + 8].copy_from_slice(&(slots as u32).to_le_bytes());
            let mut table = vec![(0u32, 0u32); slots];
            for (h, p) in list {
                let mut s = ((h >> 8) as usize) % slots;
                while table[s].1 != 0 {
                    s = (s + 1) % slots;
                }
                table[s] = (h, p);
            }
            for (h, p) in table {
                out.extend_from_slice(&h.to_le_bytes());
                out.extend_from_slice(&p.to_le_bytes());
            }
        }
        let _ = self.file.set_len(0);
        let _ = self.file.write_all_at(&header, 0);
        let _ = self.file.write_all_at(&out, 2048);
    }
}

// ----- flatfile ------------------------------------------------------------------

/// Records `<key length>\n<key><value length>\n<value>`, as long as the
/// matrix test dba_flatfile_creation_matrix.phpt needs: a length line that
/// is not a number reads as 0, so garbage in the file swallows what follows
/// it. A deleted record's key is overwritten with NUL bytes.
struct Flat {
    file: File,
    data: Vec<u8>,
    cursor: usize,
}

struct FlatRec {
    key_at: usize,
    key_len: usize,
    val_at: usize,
    val_len: usize,
    next: usize,
}

fn atoi(line: &[u8]) -> usize {
    let mut n: usize = 0;
    for &c in line.iter().skip_while(|c| c.is_ascii_whitespace()) {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((c - b'0') as usize);
    }
    n
}

impl Flat {
    fn number_line(&self, off: usize) -> (usize, usize) {
        let rest = &self.data[off.min(self.data.len())..];
        match rest.iter().position(|&c| c == b'\n') {
            Some(i) => (atoi(&rest[..i]), off + i + 1),
            None => (atoi(rest), self.data.len()),
        }
    }

    fn record(&self, off: usize) -> Option<FlatRec> {
        if off >= self.data.len() {
            return None;
        }
        let (klen, p) = self.number_line(off);
        let klen = klen.min(self.data.len() - p);
        let after_key = p + klen;
        let (vlen, v) = self.number_line(after_key);
        let vlen = vlen.min(self.data.len().saturating_sub(v));
        Some(FlatRec {
            key_at: p,
            key_len: klen,
            val_at: v,
            val_len: vlen,
            next: v + vlen,
        })
    }

    fn deleted(&self, r: &FlatRec) -> bool {
        r.key_len > 0 && self.data[r.key_at] == 0
    }

    fn find(&self, key: &[u8]) -> Option<FlatRec> {
        let mut off = 0;
        while let Some(r) = self.record(off) {
            off = r.next;
            if !self.deleted(&r) && &self.data[r.key_at..r.key_at + r.key_len] == key {
                return Some(r);
            }
        }
        None
    }

    fn append(&mut self, key: &[u8], value: &[u8]) {
        let mut rec = format!("{}\n", key.len()).into_bytes();
        rec.extend_from_slice(key);
        rec.extend_from_slice(format!("{}\n", value.len()).as_bytes());
        rec.extend_from_slice(value);
        let at = self.data.len() as u64;
        let _ = self.file.write_all_at(&rec, at);
        self.data.extend_from_slice(&rec);
    }

    fn delete(&mut self, key: &[u8]) -> bool {
        let Some(r) = self.find(key) else {
            return false;
        };
        if r.key_len == 0 {
            // An empty key has nothing to overwrite: the record goes.
            let start = r.key_at - 2;
            self.data.drain(start..r.next);
            let _ = self.file.set_len(0);
            let _ = self.file.write_all_at(&self.data, 0);
            return true;
        }
        self.data[r.key_at..r.key_at + r.key_len].fill(0);
        let _ = self
            .file
            .write_all_at(&self.data[r.key_at..r.key_at + r.key_len], r.key_at as u64);
        true
    }

    fn next_key(&mut self) -> Option<Vec<u8>> {
        while let Some(r) = self.record(self.cursor) {
            self.cursor = r.next;
            if !self.deleted(&r) {
                return Some(self.data[r.key_at..r.key_at + r.key_len].to_vec());
            }
        }
        None
    }
}

// ----- inifile -------------------------------------------------------------------

enum IniLine {
    Section(Vec<u8>),
    Entry(Vec<u8>, Vec<u8>),
    /// Anything else, kept as it is.
    Other(Vec<u8>),
}

/// `[group]` lines and `name=value` lines (tests dba_inifile.phpt,
/// bug70825.phpt, dba_array_keys_inifile.phpt).
struct Ini {
    file: File,
    lines: Vec<IniLine>,
    cursor: usize,
    /// `dba_fetch($key, $db, -1)`: the next occurrence, by key.
    fetch_pos: HashMap<Vec<u8>, usize>,
}

/// `[group]name` to its parts (test dba_split.phpt).
fn split_key(key: &[u8]) -> (&[u8], &[u8]) {
    if key.first() == Some(&b'[')
        && let Some(i) = key.iter().position(|&c| c == b']')
    {
        return (&key[1..i], &key[i + 1..]);
    }
    (&b""[..], key)
}

fn trim(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map_or(start, |e| e + 1);
    &s[start..end]
}

impl Ini {
    fn parse(data: &[u8]) -> Vec<IniLine> {
        let mut out = Vec::new();
        for raw in data.split(|&c| c == b'\n') {
            let t = trim(raw);
            if t.is_empty() {
                continue;
            }
            if t[0] == b'[' && t[t.len() - 1] == b']' {
                out.push(IniLine::Section(t[1..t.len() - 1].to_vec()));
            } else if t[0] != b';'
                && t[0] != b'#'
                && let Some(i) = t.iter().position(|&c| c == b'=')
            {
                out.push(IniLine::Entry(
                    trim(&t[..i]).to_vec(),
                    trim(&t[i + 1..]).to_vec(),
                ));
            } else {
                out.push(IniLine::Other(raw.to_vec()));
            }
        }
        out
    }

    fn save(&self) {
        let mut out = Vec::new();
        for l in &self.lines {
            match l {
                IniLine::Section(g) => {
                    out.push(b'[');
                    out.extend_from_slice(g);
                    out.extend_from_slice(b"]\n");
                }
                IniLine::Entry(n, v) => {
                    out.extend_from_slice(n);
                    out.push(b'=');
                    out.extend_from_slice(v);
                    out.push(b'\n');
                }
                IniLine::Other(x) => {
                    out.extend_from_slice(x);
                    out.push(b'\n');
                }
            }
        }
        let _ = self.file.set_len(0);
        let _ = self.file.write_all_at(&out, 0);
    }

    /// The values of `key`, in file order; a group alone reads as its
    /// header, whose value is empty.
    fn values(&self, key: &[u8]) -> Vec<Vec<u8>> {
        let (group, name) = split_key(key);
        let mut out = Vec::new();
        let mut cur: &[u8] = b"";
        for l in &self.lines {
            match l {
                IniLine::Section(g) => {
                    cur = g;
                    if name.is_empty() && !group.is_empty() && g == group {
                        out.push(Vec::new());
                    }
                }
                IniLine::Entry(n, v)
                    if cur == group && n == name && (!name.is_empty() || !group.is_empty()) =>
                {
                    out.push(v.clone());
                }
                _ => {}
            }
        }
        out
    }

    fn insert(&mut self, key: &[u8], value: &[u8]) {
        let (group, name) = split_key(key);
        if group.is_empty() && name.is_empty() {
            return;
        }
        let entry = IniLine::Entry(name.to_vec(), value.to_vec());
        // After the last line of its section; without group, before the
        // first section.
        let mut at = None;
        let mut in_group = group.is_empty();
        for (i, l) in self.lines.iter().enumerate() {
            if let IniLine::Section(g) = l {
                if in_group {
                    at = Some(i);
                    break;
                }
                in_group = g == group;
            }
        }
        match (at, in_group) {
            (Some(i), _) => self.lines.insert(i, entry),
            (None, true) => self.lines.push(entry),
            (None, false) => {
                self.lines.push(IniLine::Section(group.to_vec()));
                self.lines.push(entry);
            }
        }
        self.save();
    }

    fn delete(&mut self, key: &[u8]) -> bool {
        let (group, name) = split_key(key);
        let mut cur: Vec<u8> = Vec::new();
        let before = self.lines.len();
        self.lines.retain(|l| match l {
            IniLine::Section(g) => {
                cur = g.clone();
                true
            }
            IniLine::Entry(n, _) => !(cur == group && n == name),
            IniLine::Other(_) => true,
        });
        let found = self.lines.len() != before;
        if found {
            self.save();
        }
        found
    }

    fn next_key(&mut self) -> Option<Vec<u8>> {
        let mut group: Vec<u8> = Vec::new();
        for l in &self.lines[..self.cursor.min(self.lines.len())] {
            if let IniLine::Section(g) = l {
                group = g.clone();
            }
        }
        while let Some(l) = self.lines.get(self.cursor) {
            self.cursor += 1;
            match l {
                IniLine::Section(g) => {
                    return Some([b"[", g.as_slice(), b"]"].concat());
                }
                IniLine::Entry(n, _) => {
                    if group.is_empty() {
                        return Some(n.clone());
                    }
                    return Some([b"[", group.as_slice(), b"]", n.as_slice()].concat());
                }
                IniLine::Other(_) => {}
            }
        }
        None
    }
}

// ----- opening -------------------------------------------------------------------

fn io_message(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

fn open_common(vm: &mut Vm, a: &mut Args, persistent: bool) -> R<Value> {
    let fname = a.fname();
    let path = a.str(0);
    let mode = a.str(1);
    // Test value_errors_open.phpt, in its order.
    if path.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($path) must not be empty");
    }
    if mode.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #2 ($mode) must not be empty");
    }
    let handler_arg = a.opt_str(2);
    if handler_arg.as_ref().is_some_and(|h| h.is_empty()) {
        return vm.fthrow(a, "ValueError", "Argument #3 ($handler) must not be empty");
    }
    let m = mode.as_bytes();
    if !matches!(m[0], b'r' | b'w' | b'c' | b'n') {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) first character must be one of \"r\", \"w\", \"c\", or \"n\"",
        );
    }
    let mut lock = b'd';
    let mut test = false;
    if let Some(&c) = m.get(1) {
        match c {
            b'd' | b'l' | b'-' => lock = c,
            b't' => test = true,
            _ => {
                return vm.fthrow(
                    a,
                    "ValueError",
                    "Argument #2 ($mode) second character must be one of \"d\", \"l\", \"-\", or \"t\"",
                );
            }
        }
    }
    if let Some(&c) = m.get(2) {
        if c != b't' {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($mode) third character must be \"t\"",
            );
        }
        if lock == b'-' {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($mode) cannot combine mode \"-\" (no lock) and \"t\" (test lock)",
            );
        }
        test = true;
    }
    if m.len() > 3 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($mode) third character must be \"t\"",
        );
    }
    if a.int_or(4, 0) < 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #5 ($map_size) must be greater than or equal to 0",
        );
    }
    if a.opt_int(5).is_some_and(|f| f < 0) {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #6 ($flags) must be greater than or equal to 0",
        );
    }
    let handler = match handler_arg {
        Some(h) => h.to_string_lossy(),
        None => {
            let d = vm
                .get_ini(b"dba.default_handler")
                .map_or(String::new(), |v| v.to_string_lossy());
            if d.is_empty() {
                vm.fwarn(a, "No default handler selected")?;
                return Ok(Value::Bool(false));
            }
            d
        }
    };
    if !HANDLERS.iter().any(|(h, _)| *h == handler) {
        vm.fwarn(a, &format!("Handler \"{handler}\" is not available"))?;
        return Ok(Value::Bool(false));
    }
    let path_s = path.to_string_lossy();
    let cls = vm
        .find_class(b"Dba\\Connection", false)?
        .expect("Dba\\Connection is declared");
    let readonly = m[0] == b'r';
    let key = format!("{handler}\0{}\0{path_s}", m[0] as char);
    if persistent && let Some(db) = vm.ext::<DbaState>().persistent.get(&key).cloned() {
        return connection(vm, &cls, db, readonly, &handler, &path_s);
    }
    // Test gh16390.phpt: resources without a file have nothing to lock.
    if path_s.starts_with("data:") || path_s.starts_with("php://") {
        vm.fwarn(
            a,
            &format!(
                "Driver initialization failed for handler: {handler}: Unable to determine path for locking"
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let local = path_s.strip_prefix("file://").unwrap_or(&path_s);
    let fs = vm.fs_path(local.as_bytes());
    let perm = a.int_or(3, 0o644) as u32;
    let mut oo = std::fs::OpenOptions::new();
    oo.read(true).mode(perm);
    match m[0] {
        b'r' => {}
        b'w' => {
            oo.write(true);
        }
        _ => {
            oo.write(true).create(true);
        }
    }
    let file = match oo.open(&fs) {
        Ok(f) => f,
        Err(e) => {
            vm.warn(&format!(
                "{fname}({path_s}): Failed to open stream: {}",
                io_message(&e)
            ))?;
            return Ok(Value::Bool(false));
        }
    };
    // The lock comes before truncating (PHP manual: dba_open, mode
    // characters "d", "l", "-" and "t").
    let lock_file = match lock {
        b'-' => None,
        b'l' => {
            let lp = {
                let mut s = fs.clone().into_os_string();
                s.push(".lck");
                s
            };
            let lf = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o644)
                .open(&lp);
            match lf {
                Ok(f) => Some(f),
                Err(e) => {
                    vm.warn(&format!(
                        "{fname}({path_s}): Failed to open stream: {}",
                        io_message(&e)
                    ))?;
                    return Ok(Value::Bool(false));
                }
            }
        }
        // A second open file description: flock() conflicts with other
        // connections of this process too.
        _ => std::fs::File::open(&fs).ok(),
    };
    if let Some(lf) = &lock_file {
        let mut op = if readonly {
            libc::LOCK_SH
        } else {
            libc::LOCK_EX
        };
        if test {
            op |= libc::LOCK_NB;
        }
        // SAFETY: flock on a descriptor this function owns.
        let r = unsafe { libc::flock(lf.as_raw_fd(), op) };
        if r != 0 {
            vm.fwarn(a, "Could not establish lock")?;
            return Ok(Value::Bool(false));
        }
    }
    if m[0] == b'n' {
        let _ = file.set_len(0);
    }
    let store = match handler.as_str() {
        "cdb" | "cdb_make" => {
            if m[0] == b'n' {
                Store::CdbMake(CdbMake {
                    file,
                    buf: Vec::new(),
                    entries: Vec::new(),
                })
            } else if m[0] == b'r' && handler == "cdb" {
                Store::CdbRead(CdbRead {
                    data: read_all(&file),
                    cursor: 2048,
                })
            } else {
                vm.fwarn(
                    a,
                    &format!(
                        "Driver initialization failed for handler: {handler}: Update operations are not supported"
                    ),
                )?;
                return Ok(Value::Bool(false));
            }
        }
        "flatfile" => Store::Flat(Flat {
            data: read_all(&file),
            file,
            cursor: 0,
        }),
        _ => {
            let data = read_all(&file);
            Store::Ini(Ini {
                file,
                lines: Ini::parse(&data),
                cursor: 0,
                fetch_pos: HashMap::new(),
            })
        }
    };
    let db = Rc::new(RefCell::new(Db {
        store,
        _lock: lock_file,
    }));
    if persistent {
        vm.ext::<DbaState>().persistent.insert(key, db.clone());
    }
    connection(vm, &cls, db, readonly, &handler, &path_s)
}

fn read_all(f: &File) -> Vec<u8> {
    let len = f.metadata().map_or(0, |m| m.len()) as usize;
    let mut buf = vec![0u8; len];
    match f.read_exact_at(&mut buf, 0) {
        Ok(()) => buf,
        Err(_) => Vec::new(),
    }
}

fn connection(
    vm: &mut Vm,
    cls: &Rc<crate::class::Class>,
    db: Shared,
    readonly: bool,
    handler: &str,
    path: &str,
) -> R<Value> {
    let o = vm.instantiate_raw(cls)?;
    *o.obj().native.borrow_mut() = Some(Box::new(Conn {
        db: Some(db),
        readonly,
        handler: handler.to_string(),
    }));
    let id = o.handle() as i64;
    let weak = o.downgrade();
    vm.ext::<DbaState>().list.push((id, path.to_string(), weak));
    Ok(Value::Object(o))
}

fn dba_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    open_common(vm, a, false)
}

fn dba_popen(vm: &mut Vm, a: &mut Args) -> R<Value> {
    open_common(vm, a, true)
}

/// The database of connection `o`; closed ones are an error (test dba015.phpt).
fn db_of(vm: &mut Vm, o: &ObjRef) -> R<(Shared, bool, String)> {
    let d = with_conn(o, |c| {
        c.db.clone().map(|d| (d, c.readonly, c.handler.clone()))
    })
    .flatten();
    match d {
        Some(x) => Ok(x),
        None => vm.throw("Error", "DBA connection has already been closed"),
    }
}

fn arg_db(vm: &mut Vm, a: &Args, i: usize) -> R<(Shared, bool, String)> {
    let o = a.obj(i).expect("Dba\\Connection argument");
    db_of(vm, &o)
}

/// A key argument: a string, or `[group, name]` (tests dba_array_keys*.phpt).
fn key_arg(vm: &mut Vm, a: &Args) -> R<Vec<u8>> {
    match a.val(0) {
        Value::Array(arr) => {
            if arr.len() != 2 {
                return vm.throw(
                    "Error",
                    &format!(
                        "{}(): Argument #1 ($key) must have exactly two elements: \"key\" and \"name\"",
                        a.fname()
                    ),
                );
            }
            let mut parts = Vec::with_capacity(2);
            for (_, v) in arr.iter() {
                parts.push(vm.to_php_str(&v.deref())?);
            }
            let (g, n) = (&parts[0], &parts[1]);
            if g.is_empty() {
                return Ok(n.as_bytes().to_vec());
            }
            let mut k = Vec::with_capacity(g.len() + n.len() + 2);
            k.push(b'[');
            k.extend_from_slice(g.as_bytes());
            k.push(b']');
            k.extend_from_slice(n.as_bytes());
            Ok(k)
        }
        v => Ok(vm.to_php_str(&v)?.as_bytes().to_vec()),
    }
}

// ----- functions -----------------------------------------------------------------

fn dba_close(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.obj(0).expect("Dba\\Connection argument");
    db_of(vm, &o)?;
    with_conn(&o, |c| c.db = None);
    let id = o.handle() as i64;
    vm.ext::<DbaState>().list.retain(|(i, _, _)| *i != id);
    Ok(Value::Null)
}

fn dba_exists(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (db, _, _) = arg_db(vm, a, 1)?;
    let key = key_arg(vm, a)?;
    let db = db.borrow();
    Ok(Value::Bool(match &db.store {
        Store::CdbRead(c) => c.fetch(&key, 0).is_some(),
        Store::CdbMake(_) => false,
        Store::Flat(f) => f.find(&key).is_some(),
        Store::Ini(i) => !i.values(&key).is_empty(),
    }))
}

fn dba_fetch(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let is_conn = |v: &Value, vm: &mut Vm| -> bool {
        match v {
            Value::Object(o) => {
                let c = vm.class_of(o);
                c.name.as_bytes().eq_ignore_ascii_case(b"Dba\\Connection")
            }
            _ => false,
        }
    };
    let (second, third) = (a.val(1), a.val(2));
    let (conn, skip_v) = if is_conn(&second.deref(), vm) {
        (second.deref(), third.deref())
    } else if is_conn(&third.deref(), vm) {
        // Test dba_fetch_legacy_signature.phpt.
        vm.deprecated("Calling dba_fetch() with $dba at the 3rd parameter is deprecated")?;
        (third.deref(), second.deref())
    } else {
        return vm.fthrow(
            a,
            "TypeError",
            &format!(
                "Argument #2 ($dba) must be of type Dba\\Connection, {} given",
                crate::class::TypeName::tname(&second)
            ),
        );
    };
    let Value::Object(o) = conn else {
        return Ok(Value::Bool(false));
    };
    let skip = match skip_v {
        Value::Null => 0,
        Value::Int(i) => i,
        v => {
            let s = vm.to_php_str(&v)?;
            s.to_string_lossy().trim().parse::<i64>().unwrap_or(0)
        }
    };
    // Test gh19885.phpt.
    if skip < i32::MIN as i64 || skip > i32::MAX as i64 {
        return vm.fthrow(
            a,
            "ValueError",
            &format!(
                "Argument #3 ($skip) must be between {} and {}",
                i32::MIN,
                i32::MAX
            ),
        );
    }
    let (db, _, handler) = db_of(vm, &o)?;
    let key = key_arg(vm, a)?;
    let mut skip = skip;
    if skip < 0 && handler != "inifile" {
        vm.fnotice(
            a,
            &format!(
                "Handler {handler} accepts only skip values greater than or equal to zero, using skip=0"
            ),
        )?;
        skip = 0;
    }
    let mut db = db.borrow_mut();
    let v = match &mut db.store {
        Store::CdbRead(c) => c.fetch(&key, skip as usize),
        Store::CdbMake(_) => None,
        Store::Flat(f) => f
            .find(&key)
            .map(|r| f.data[r.val_at..r.val_at + r.val_len].to_vec()),
        Store::Ini(i) => {
            let vals = i.values(&key);
            if skip < 0 {
                // Test bug70825.phpt: the next occurrence each time.
                let p = i.fetch_pos.entry(key.clone()).or_insert(0);
                let r = vals.get(*p).cloned();
                *p += 1;
                r
            } else {
                vals.get(skip as usize).cloned()
            }
        }
    };
    Ok(match v {
        Some(b) => Value::from(b.as_slice()),
        None => Value::Bool(false),
    })
}

fn dba_key_split(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    if matches!(v, Value::Null | Value::Bool(false)) {
        vm.fdeprecated(a, "Passing false or null is deprecated since 8.4")?;
        return Ok(Value::Bool(false));
    }
    let s = vm.to_php_str(&v)?;
    let (g, n) = split_key(s.as_bytes());
    let mut arr = PhpArray::new();
    let _ = arr.push(Value::from(g));
    let _ = arr.push(Value::from(n));
    Ok(Value::Array(arr))
}

fn key_value(k: Option<Vec<u8>>) -> Value {
    match k {
        Some(k) => Value::from(k.as_slice()),
        None => Value::Bool(false),
    }
}

fn dba_firstkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (db, _, _) = arg_db(vm, a, 0)?;
    let mut db = db.borrow_mut();
    Ok(key_value(match &mut db.store {
        Store::CdbRead(c) => c.key_at(2048),
        Store::CdbMake(_) => None,
        Store::Flat(f) => {
            f.cursor = 0;
            f.next_key()
        }
        Store::Ini(i) => {
            i.cursor = 0;
            i.next_key()
        }
    }))
}

fn dba_nextkey(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (db, _, _) = arg_db(vm, a, 0)?;
    let mut db = db.borrow_mut();
    Ok(key_value(match &mut db.store {
        Store::CdbRead(c) => {
            let p = c.cursor;
            c.key_at(p)
        }
        Store::CdbMake(_) => None,
        Store::Flat(f) => f.next_key(),
        Store::Ini(i) => i.next_key(),
    }))
}

/// Refuses changes in mode "r".
fn writable(vm: &mut Vm, a: &Args, readonly: bool) -> R<bool> {
    if readonly {
        vm.fwarn(a, "Cannot perform a modification on a readonly database")?;
        return Ok(false);
    }
    Ok(true)
}

fn dba_delete(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (db, ro, _) = arg_db(vm, a, 1)?;
    let key = key_arg(vm, a)?;
    if !writable(vm, a, ro)? {
        return Ok(Value::Bool(false));
    }
    let mut db = db.borrow_mut();
    Ok(Value::Bool(match &mut db.store {
        Store::Flat(f) => f.delete(&key),
        Store::Ini(i) => i.delete(&key),
        _ => false,
    }))
}

fn dba_insert(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (db, ro, _) = arg_db(vm, a, 2)?;
    let key = key_arg(vm, a)?;
    let value = a.str(1);
    if !writable(vm, a, ro)? {
        return Ok(Value::Bool(false));
    }
    let mut db = db.borrow_mut();
    Ok(Value::Bool(match &mut db.store {
        Store::CdbMake(m) => {
            m.add(&key, value.as_bytes());
            true
        }
        Store::Flat(f) => {
            // An existing key is refused (test dba_flatfile.phpt).
            if f.find(&key).is_some() {
                false
            } else {
                f.append(&key, value.as_bytes());
                true
            }
        }
        Store::Ini(i) => {
            i.insert(&key, value.as_bytes());
            true
        }
        Store::CdbRead(_) => false,
    }))
}

fn dba_replace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (db, ro, _) = arg_db(vm, a, 2)?;
    let key = key_arg(vm, a)?;
    let value = a.str(1);
    if !writable(vm, a, ro)? {
        return Ok(Value::Bool(false));
    }
    let mut db = db.borrow_mut();
    Ok(Value::Bool(match &mut db.store {
        Store::Flat(f) => {
            f.delete(&key);
            f.append(&key, value.as_bytes());
            true
        }
        Store::Ini(i) => {
            i.delete(&key);
            i.insert(&key, value.as_bytes());
            true
        }
        _ => false,
    }))
}

fn dba_optimize(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, ro, _) = arg_db(vm, a, 0)?;
    if !writable(vm, a, ro)? {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

fn dba_sync(vm: &mut Vm, a: &mut Args) -> R<Value> {
    arg_db(vm, a, 0)?;
    Ok(Value::Bool(true))
}

fn dba_handlers(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let mut arr = PhpArray::new();
    let full = a.bool_or(0, false);
    for (h, info) in HANDLERS {
        if full {
            arr.insert(Key::Str(PhpStr::from(*h)), Value::from(info.as_bytes()));
        } else {
            let _ = arr.push(Value::from(h.as_bytes()));
        }
    }
    Ok(Value::Array(arr))
}

fn dba_list(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let st = vm.ext::<DbaState>();
    st.list.retain(|(_, _, w)| w.upgrade().is_some());
    let mut arr = PhpArray::new();
    for (id, path, _) in &st.list {
        arr.insert(Key::Int(*id), Value::from(path.as_bytes()));
    }
    Ok(Value::Array(arr))
}
