//! The flatfile handler (dba_flatfile.c over libflatfile/flatfile.c):
//! records of "<key length>\n<key><value length>\n<value>", deleted
//! records have their key's first byte set to NUL.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::vm::Vm;

use crate::stream::{PStream, SEEK_END, SEEK_SET};
use crate::{Driver, Info};

pub const VERSION: &str = "1.0, $Id: bd76ecfdd0aefe633c7f7cbdcce7aaf0e0263575 $";

pub struct Flatfile {
    fp: Rc<RefCell<PStream>>,
    current_pos: u64,
    nextkey: Option<Vec<u8>>,
}

/// atoi()
pub(crate) fn atoi(b: &[u8]) -> i64 {
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut n: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n.wrapping_mul(10).wrapping_add((b[i] - b'0') as i64);
        i += 1;
    }
    let n = if neg { n.wrapping_neg() } else { n };
    n as i32 as i64
}

/// `num = atoi(buf)` as size_t, then php_stream_read(num)
fn read_len(fp: &mut PStream) -> Option<usize> {
    let line = fp.gets(Some(15))?;
    Some(atoi(&line) as usize)
}

fn read_n(fp: &mut PStream, n: usize) -> Vec<u8> {
    // (a size_t conversion of a negative atoi() would not fit in memory)
    if n > isize::MAX as usize {
        return Vec::new();
    }
    fp.read(n)
}

impl Flatfile {
    /// flatfile_findkey()
    fn findkey(&mut self, key: &[u8]) -> bool {
        let mut fp = self.fp.borrow_mut();
        fp.rewind();
        while !fp.eof() {
            let Some(n) = read_len(&mut fp) else { break };
            let buf = read_n(&mut fp, n);
            if buf == key {
                return true;
            }
            let Some(n) = read_len(&mut fp) else { break };
            read_n(&mut fp, n);
        }
        false
    }

    /// flatfile_fetch()
    fn ff_fetch(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        if !self.findkey(key) {
            return None;
        }
        let mut fp = self.fp.borrow_mut();
        let line = fp.gets(Some(16))?;
        let n = atoi(&line);
        Some(read_n(&mut fp, n as usize))
    }

    /// flatfile_delete(): true when found.
    fn ff_delete(&mut self, key: &[u8]) -> bool {
        let mut fp = self.fp.borrow_mut();
        fp.rewind();
        while !fp.eof() {
            let Some(n) = read_len(&mut fp) else { break };
            let pos = fp.tell();
            let buf = read_n(&mut fp, n);
            if buf == key {
                let _ = fp.seek(pos as i64, SEEK_SET);
                fp.putc(0);
                fp.flush();
                let _ = fp.seek(0, SEEK_END);
                return true;
            }
            let Some(n) = read_len(&mut fp) else { break };
            read_n(&mut fp, n);
        }
        false
    }

    fn write_record(&mut self, key: &[u8], val: &[u8]) -> i32 {
        let mut fp = self.fp.borrow_mut();
        fp.write(format!("{}\n", key.len()).as_bytes());
        fp.flush();
        if fp.write(key) < key.len() {
            return -1;
        }
        fp.write(format!("{}\n", val.len()).as_bytes());
        fp.flush();
        if fp.write(val) < val.len() {
            return -1;
        }
        fp.flush();
        0
    }

    /// flatfile_store(): 0 stored, 1 key exists, -1 error.
    fn store(&mut self, key: &[u8], val: &[u8], insert: bool) -> i32 {
        if insert {
            if self.findkey(key) {
                return 1;
            }
            let _ = self.fp.borrow_mut().seek(0, SEEK_END);
        } else {
            self.ff_delete(key);
        }
        self.write_record(key, val)
    }

    /// flatfile_firstkey()
    fn ff_firstkey(&mut self) -> Option<Vec<u8>> {
        let mut fp = self.fp.borrow_mut();
        // (the C code tests the first byte of a buffer reused across reads)
        let mut buf = vec![0u8; 1024];
        fp.rewind();
        while !fp.eof() {
            let Some(n) = read_len(&mut fp) else { break };
            let k = read_into(&mut fp, &mut buf, n);
            if buf[0] != 0 {
                self.current_pos = fp.tell();
                return Some(buf[..k].to_vec());
            }
            let Some(n) = read_len(&mut fp) else { break };
            read_into(&mut fp, &mut buf, n);
        }
        None
    }

    /// flatfile_nextkey()
    fn ff_nextkey(&mut self) -> Option<Vec<u8>> {
        let mut fp = self.fp.borrow_mut();
        let mut buf = vec![0u8; 1024];
        let _ = fp.seek(self.current_pos as i64, SEEK_SET);
        while !fp.eof() {
            let Some(n) = read_len(&mut fp) else { break };
            read_into(&mut fp, &mut buf, n);
            let Some(n) = read_len(&mut fp) else { break };
            let k = read_into(&mut fp, &mut buf, n);
            if buf[0] != 0 {
                self.current_pos = fp.tell();
                return Some(buf[..k].to_vec());
            }
        }
        None
    }
}

/// php_stream_read() into a reused buffer (grown as needed): the count read.
fn read_into(fp: &mut PStream, buf: &mut Vec<u8>, n: usize) -> usize {
    let d = read_n(fp, n);
    if n >= buf.len() && n <= isize::MAX as usize {
        buf.resize(n + 1024, 0);
    }
    buf[..d.len()].copy_from_slice(&d);
    d.len()
}

/// DBA_OPEN_FUNC(flatfile)
pub fn open(info: &Info) -> Result<Box<dyn Driver>, &'static str> {
    let fp = info.fp.clone().ok_or("")?;
    Ok(Box::new(Flatfile {
        fp,
        current_pos: 0,
        nextkey: None,
    }))
}

impl Driver for Flatfile {
    fn close(&mut self) {}

    fn fetch(&mut self, _vm: &mut Vm, key: &[u8], _skip: i64) -> Option<Vec<u8>> {
        self.ff_fetch(key)
    }

    fn update(&mut self, vm: &mut Vm, key: &[u8], val: &[u8], mode: i32) -> bool {
        match self.store(key, val, mode == 1) {
            0 => true,
            1 => false,
            _ => {
                crate::docref_warning(vm, "Operation not possible");
                false
            }
        }
    }

    fn exists(&mut self, _vm: &mut Vm, key: &[u8]) -> bool {
        self.ff_fetch(key).is_some()
    }

    fn delete(&mut self, _vm: &mut Vm, key: &[u8]) -> bool {
        // flatfile_delete() returns SUCCESS (0) or FAILURE (-1)
        self.ff_delete(key)
    }

    fn firstkey(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        self.nextkey = self.ff_firstkey();
        self.nextkey.clone()
    }

    fn nextkey(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        self.nextkey.as_ref()?;
        self.nextkey = self.ff_nextkey();
        self.nextkey.clone()
    }
}
