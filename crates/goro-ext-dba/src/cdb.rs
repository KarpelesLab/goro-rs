//! The cdb and cdb_make handlers (dba_cdb.c) over php-src's bundled
//! libcdb (D. J. Bernstein's cdb-0.75: cdb.c, cdb_make.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::vm::Vm;

use crate::stream::{PStream, SEEK_SET};
use crate::{Driver, Info, Mode};

pub const CDB_VERSION: &str = "0.75, $Id: e694d6a6eb12f2bbad27f8ec7ebf3788257532d9 $";
pub const CDB_MAKE_VERSION: &str = "0.75, $Id: 570e3625b0cea5c29455cecf40fdfd491c0eb3c6 $";

const CDB_HASHSTART: u32 = 5381;

fn unpack(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// cdb_hash()
fn hash(key: &[u8]) -> u32 {
    let mut h = CDB_HASHSTART;
    for &c in key {
        h = h.wrapping_add(h << 5) ^ c as u32;
    }
    h
}

/// struct cdb
#[derive(Default)]
struct Reader {
    loop_: u32,
    khash: u32,
    kpos: u32,
    hpos: u32,
    hslots: u32,
    dpos: u32,
    dlen: u32,
}

/// struct cdb_make
struct Maker {
    pos: u32,
    entries: Vec<(u32, u32)>,
}

pub struct Cdb {
    fp: Rc<RefCell<PStream>>,
    make: Option<Maker>,
    c: Reader,
    eod: u32,
    pos: u32,
}

/// cdb_read(): Err on a short read.
fn cdb_read(fp: &mut PStream, len: usize, pos: u32) -> Result<Vec<u8>, ()> {
    if fp.seek(pos as i64, SEEK_SET).is_err() {
        return Err(());
    }
    let d = fp.read(len);
    if d.len() < len {
        return Err(());
    }
    Ok(d)
}

impl Cdb {
    /// cdb_findnext(): 1 found, 0 not found, -1 error.
    fn findnext(&mut self, key: &[u8]) -> i32 {
        let mut fp = self.fp.borrow_mut();
        let c = &mut self.c;
        if c.loop_ == 0 {
            let u = hash(key);
            let Ok(buf) = cdb_read(&mut fp, 8, (u << 3) & 2047) else {
                return -1;
            };
            c.hslots = unpack(&buf[4..]);
            if c.hslots == 0 {
                return 0;
            }
            c.hpos = unpack(&buf);
            c.khash = u;
            let u = ((u >> 8) % c.hslots) << 3;
            c.kpos = c.hpos.wrapping_add(u);
        }
        while c.loop_ < c.hslots {
            let Ok(buf) = cdb_read(&mut fp, 8, c.kpos) else {
                return -1;
            };
            let pos = unpack(&buf[4..]);
            if pos == 0 {
                return 0;
            }
            c.loop_ += 1;
            c.kpos = c.kpos.wrapping_add(8);
            if c.kpos == c.hpos.wrapping_add(c.hslots << 3) {
                c.kpos = c.hpos;
            }
            let u = unpack(&buf);
            if u == c.khash {
                let Ok(buf) = cdb_read(&mut fp, 8, pos) else {
                    return -1;
                };
                if unpack(&buf) as usize == key.len() {
                    // cdb_match()
                    match cdb_read(&mut fp, key.len(), pos.wrapping_add(8)) {
                        Err(()) => return -1,
                        Ok(k) if k == key => {
                            c.dlen = unpack(&buf[4..]);
                            c.dpos = pos.wrapping_add(8).wrapping_add(key.len() as u32);
                            return 1;
                        }
                        Ok(_) => {}
                    }
                }
            }
        }
        0
    }

    fn find(&mut self, key: &[u8]) -> i32 {
        self.c.loop_ = 0;
        self.findnext(key)
    }

    /// cdb_make_add()
    fn make_add(&mut self, key: &[u8], data: &[u8]) -> bool {
        let Some(m) = self.make.as_mut() else {
            return false;
        };
        let mut fp = self.fp.borrow_mut();
        let mut hdr = Vec::with_capacity(8);
        hdr.extend_from_slice(&(key.len() as u32).to_le_bytes());
        hdr.extend_from_slice(&(data.len() as u32).to_le_bytes());
        if fp.write(&hdr) != 8 {
            return false;
        }
        if fp.write(key) != key.len() || fp.write(data) != data.len() {
            return false;
        }
        m.entries.push((hash(key), m.pos));
        let Some(p) = m
            .pos
            .checked_add(8)
            .and_then(|p| p.checked_add(key.len() as u32))
            .and_then(|p| p.checked_add(data.len() as u32))
        else {
            return false;
        };
        m.pos = p;
        true
    }

    /// cdb_make_finish()
    fn make_finish(&mut self) {
        let Some(m) = self.make.take() else {
            return;
        };
        let mut fp = self.fp.borrow_mut();
        let mut count = [0u32; 256];
        for &(h, _) in &m.entries {
            count[(h & 255) as usize] += 1;
        }
        // c->head is a list of blocks of up to 1000 entries, newest block
        // first, each walked from its last entry: the split order.
        let mut ordered: Vec<(u32, u32)> = Vec::with_capacity(m.entries.len());
        for block in m.entries.chunks(1000).rev() {
            for e in block.iter().rev() {
                ordered.push(*e);
            }
        }
        let mut start = [0u32; 256];
        let mut u = 0u32;
        for i in 0..256 {
            u += count[i];
            start[i] = u;
        }
        let mut split = vec![(0u32, 0u32); ordered.len()];
        for e in &ordered {
            let b = (e.0 & 255) as usize;
            start[b] -= 1;
            split[start[b] as usize] = *e;
        }
        let mut pos = m.pos;
        let mut fin = vec![0u8; 2048];
        for i in 0..256 {
            let cnt = count[i];
            let len = cnt * 2;
            fin[8 * i..8 * i + 4].copy_from_slice(&pos.to_le_bytes());
            fin[8 * i + 4..8 * i + 8].copy_from_slice(&len.to_le_bytes());
            let mut table = vec![(0u32, 0u32); len as usize];
            let base = start[i] as usize;
            for hp in &split[base..base + cnt as usize] {
                let mut w = (hp.0 >> 8) % len;
                while table[w as usize].1 != 0 {
                    w += 1;
                    if w == len {
                        w = 0;
                    }
                }
                table[w as usize] = *hp;
            }
            let mut out = Vec::with_capacity(table.len() * 8);
            for (h, p) in table {
                out.extend_from_slice(&h.to_le_bytes());
                out.extend_from_slice(&p.to_le_bytes());
            }
            if fp.write(&out) != out.len() {
                return;
            }
            pos = pos.wrapping_add(8 * len);
        }
        fp.flush();
        fp.rewind();
        if fp.tell() != 0 {
            return;
        }
        fp.write(&fin);
        fp.flush();
    }

    /// CSEEK + CREAD
    fn seek_read(&mut self, at: u32, n: usize) -> Option<Vec<u8>> {
        if at >= self.eod {
            return None;
        }
        let mut fp = self.fp.borrow_mut();
        let _ = fp.seek(at as i64, SEEK_SET);
        if fp.tell() != at as u64 {
            return None;
        }
        let d = fp.read(n);
        if d.len() < n { None } else { Some(d) }
    }

    fn read_key_at(&mut self, at: u32) -> Option<Vec<u8>> {
        let buf = self.seek_read(at, 8)?;
        let klen = unpack(&buf);
        let dlen = unpack(&buf[4..]);
        let key = self.fp.borrow_mut().read(klen as usize);
        self.pos = at.wrapping_add(8).wrapping_add(klen).wrapping_add(dlen);
        if key.len() < klen as usize {
            None
        } else {
            Some(key)
        }
    }
}

/// DBA_OPEN_FUNC(cdb)
pub fn open(info: &Info) -> Result<Box<dyn Driver>, &'static str> {
    let make = match info.mode {
        Mode::Reader => false,
        Mode::Trunc => true,
        Mode::Creat | Mode::Writer => return Err("Update operations are not supported"),
    };
    let fp = info.fp.clone().ok_or("Currently not supported")?;
    let mut cdb = Cdb {
        fp,
        make: None,
        c: Reader::default(),
        eod: 0,
        pos: 0,
    };
    if make {
        // cdb_make_start()
        let mut fp = cdb.fp.borrow_mut();
        if fp.seek(2048, SEEK_SET).is_err() {
            drop(fp);
            // php_error_docref(NULL, E_NOTICE, "Fseek failed") is not
            // reachable for plain files.
        } else {
            drop(fp);
        }
        cdb.make = Some(Maker {
            pos: 2048,
            entries: Vec::new(),
        });
    }
    Ok(Box::new(cdb))
}

impl Driver for Cdb {
    fn close(&mut self) {
        self.make_finish();
    }

    fn fetch(&mut self, _vm: &mut Vm, key: &[u8], skip: i64) -> Option<Vec<u8>> {
        if self.make.is_some() {
            return None;
        }
        if self.find(key) != 1 {
            return None;
        }
        let mut skip = skip;
        while skip > 0 {
            skip -= 1;
            if self.findnext(key) != 1 {
                return None;
            }
        }
        let (len, pos) = (self.c.dlen, self.c.dpos);
        cdb_read(&mut self.fp.borrow_mut(), len as usize, pos).ok()
    }

    fn update(&mut self, _vm: &mut Vm, key: &[u8], val: &[u8], mode: i32) -> bool {
        if self.make.is_none() || mode == 0 {
            return false;
        }
        self.make_add(key, val)
    }

    fn exists(&mut self, _vm: &mut Vm, key: &[u8]) -> bool {
        if self.make.is_some() {
            return false;
        }
        self.find(key) == 1
    }

    fn delete(&mut self, _vm: &mut Vm, _key: &[u8]) -> bool {
        false
    }

    fn firstkey(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        if self.make.is_some() {
            return None;
        }
        self.eod = u32::MAX;
        let buf = self.seek_read(0, 4)?;
        self.eod = unpack(&buf);
        self.read_key_at(2048)
    }

    fn nextkey(&mut self, _vm: &mut Vm) -> Option<Vec<u8>> {
        if self.make.is_some() {
            return None;
        }
        let at = self.pos;
        self.read_key_at(at)
    }
}
