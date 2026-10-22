//! SQLite3::openBlob() and its stream (php_stream_sqlite3_ops).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_standard::ext_io::{self, NativeStream, StatBuf};
use goro_sqlite::consts::*;
use goro_sqlite::vdbeblob::*;

use crate::conn::DbData;
use crate::db::{check_initialised, this_db};
use crate::zpp::{Func, Obj, cstr, parse};
use crate::{errmsg, php_sqlite3_error};

/// php_stream_sqlite3_data
struct BlobStream {
    db: Rc<DbData>,
    blob: Option<Box<Blob>>,
    position: usize,
    size: usize,
    flags: i64,
}

/// php_sqlite3_error(NULL, 0, ...) from a stream operation
fn stream_warning(vm: &mut Vm, msg: &str) {
    let name = vm.current_builtin_name().unwrap_or_default();
    let line = vm.current_line;
    if name.is_empty() {
        vm.emit_warning_at(msg, line);
    } else {
        vm.emit_warning_at(&format!("{}(): {}", name, msg), line);
    }
}

impl NativeStream for BlobStream {
    /// php_sqlite3_stream_read()
    fn read(&mut self, _vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let mut count = count;
        if self.position + count >= self.size {
            count = self.size - self.position;
            *eof = true;
        }
        let mut buf = vec![0u8; count];
        if count > 0 {
            let pos = self.position as i32;
            let blob = self.blob.as_deref_mut()?;
            let rc = self
                .db
                .with_conn_raw(|db| sqlite3_blob_read(db, blob, &mut buf, count as i32, pos))?;
            if rc != SQLITE_OK {
                return None;
            }
            self.position += count;
        }
        Some(buf)
    }

    /// php_sqlite3_stream_write()
    fn write(&mut self, vm: &mut Vm, buf: &[u8]) -> i64 {
        if self.flags & SQLITE_OPEN_READONLY as i64 != 0 {
            stream_warning(vm, "Can't write to blob stream: is open as read only");
            return -1;
        }
        let count = buf.len();
        if self.position + count > self.size {
            stream_warning(vm, "It is not possible to increase the size of a BLOB");
            return -1;
        }
        let pos = self.position as i32;
        let Some(blob) = self.blob.as_deref_mut() else {
            return -1;
        };
        let rc = self
            .db
            .with_conn_raw(|db| sqlite3_blob_write(db, blob, buf, count as i32, pos));
        if rc != Some(SQLITE_OK) {
            return -1;
        }
        if self.position + count >= self.size {
            self.position = self.size;
        } else {
            self.position += count;
        }
        count as i64
    }

    /// php_sqlite3_stream_close()
    fn close(&mut self, _vm: &mut Vm) {
        if let Some(mut blob) = self.blob.take() {
            // An error may occur, but it still closed
            self.db
                .with_conn_raw(|db| sqlite3_blob_close(db, &mut blob));
        }
    }

    /// php_sqlite3_stream_flush(): do nothing
    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        0
    }

    /// php_sqlite3_stream_seek()
    fn seek(&mut self, _vm: &mut Vm, offset: i64, whence: i64) -> Option<i64> {
        match whence {
            // SEEK_CUR
            1 => {
                if offset < 0 {
                    if (self.position as i64) < -offset {
                        self.position = 0;
                        None
                    } else {
                        self.position = (self.position as i64 + offset) as usize;
                        Some(self.position as i64)
                    }
                } else if self.position as i64 + offset > self.size as i64 {
                    self.position = self.size;
                    None
                } else {
                    self.position = (self.position as i64 + offset) as usize;
                    Some(self.position as i64)
                }
            }
            // SEEK_SET
            0 => {
                if (self.size as u64) < offset as u64 {
                    self.position = self.size;
                    None
                } else {
                    self.position = offset as usize;
                    Some(self.position as i64)
                }
            }
            // SEEK_END
            2 => {
                if offset > 0 {
                    self.position = self.size;
                    None
                } else if (self.size as u64) < (-offset) as u64 {
                    self.position = 0;
                    None
                } else {
                    self.position = (self.size as i64 + offset) as usize;
                    Some(self.position as i64)
                }
            }
            _ => None,
        }
    }

    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }

    fn label(&self) -> &'static str {
        "SQLite3"
    }

    /// php_sqlite3_stream_stat()
    fn stat(&mut self, _vm: &mut Vm) -> Option<StatBuf> {
        Some(StatBuf {
            size: self.size as i64,
            ..StatBuf::default()
        })
    }
}

/// SQLite3::openBlob()
pub fn open_blob(vm: &mut Vm, f: &Func, this: Option<&Obj>, args: &[Value]) -> Value {
    let d = this_db(this);
    let Ok(a) = parse(vm, f, "ssl|pl", None, args) else {
        return Value::Null;
    };
    if check_initialised(vm, &d).is_err() {
        return Value::Null;
    }
    let table = a[0].str_or(b"").to_vec();
    let column = a[1].str_or(b"").to_vec();
    let rowid = a[2].long_or(0);
    let dbname = a[3].str_or(b"main").to_vec();
    let flags = a[4].long_or(SQLITE_OPEN_READONLY as i64);

    let sqlite_flags = if flags & SQLITE_OPEN_READWRITE as i64 != 0 {
        1
    } else {
        0
    };

    let Ok((rc, blob, code, msg)) = d.with_conn(vm, |db| {
        let mut blob: Option<Box<Blob>> = None;
        let rc = sqlite3_blob_open(
            db,
            Some(cstr(&dbname)),
            Some(cstr(&table)),
            Some(cstr(&column)),
            rowid,
            sqlite_flags,
            &mut blob,
        );
        let (code, msg) = if rc != SQLITE_OK {
            (goro_sqlite::mainapi::sqlite3_errcode(db), errmsg(db))
        } else {
            (0, String::new())
        };
        (rc, blob, code, msg)
    }) else {
        return Value::Null;
    };
    let Some(mut blob) = blob.filter(|_| rc == SQLITE_OK) else {
        php_sqlite3_error(
            vm,
            Some(&d.shared),
            code,
            &format!("Unable to open blob: {}", msg),
        );
        return Value::False;
    };

    let size = sqlite3_blob_bytes(&mut blob).max(0) as usize;
    let mode = if sqlite_flags != 0 { "r+b" } else { "rb" };
    let s = Rc::new(RefCell::new(BlobStream {
        db: d.clone(),
        blob: Some(blob),
        position: 0,
        size,
        flags,
    }));
    let id = ext_io::ext_alloc_stream(s, mode, None);
    Value::Resource(goro_core::value::ResourceRef::new(id))
}
