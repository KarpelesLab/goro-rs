//! `SQLite3::openBlob()` (PHP manual): a stream over one BLOB value.
//!
//! graphitesql has no incremental blob I/O (`sqlite3_blob_open`): the
//! value is read with a SELECT by rowid, and each write stores the whole
//! value back with an UPDATE. As with SQLite's blob handles, writes can't
//! change the size of the value (test sqlite3_30_blobopen.phpt).

use std::rc::Rc;

use goro_ext_sqlite::graphitesql::Value as SqlValue;
use goro_ext_sqlite::{SqlError, scan};
use goro_runtime::Value;

use super::{Conn, complain, fail, prepare, run, sql_str};
use crate::builtins::file::{Backing, Stream, new_stream_res};
use crate::builtins::streams::NativeStream;
use crate::func::Args;
use crate::vm::{R, Vm};

struct BlobStream {
    conn: Rc<Conn>,
    /// `UPDATE … SET col = ?1 WHERE rowid = ?2`.
    update: String,
    rowid: i64,
    data: Vec<u8>,
    pos: usize,
    writable: bool,
}

impl NativeStream for BlobStream {
    fn read(&mut self, _vm: &mut Vm, buf: &mut [u8]) -> std::io::Result<usize> {
        let rest = &self.data[self.pos.min(self.data.len())..];
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.pos += n;
        Ok(n)
    }

    fn write(&mut self, vm: &mut Vm, data: &[u8]) -> std::io::Result<usize> {
        if !self.writable {
            let _ = vm.warn("fwrite(): Can't write to blob stream: is open as read only");
            return Err(std::io::Error::other("read only"));
        }
        if self.pos + data.len() > self.data.len() {
            let _ = vm.warn("fwrite(): It is not possible to increase the size of a BLOB");
            return Err(std::io::Error::other("blob size"));
        }
        let mut next = self.data.clone();
        next[self.pos..self.pos + data.len()].copy_from_slice(data);
        let r = match prepare(vm, &self.conn, &self.update) {
            Ok(Some((st, _, _))) => run(
                vm,
                &self.conn,
                &st,
                &[SqlValue::Blob(next.clone()), SqlValue::Integer(self.rowid)],
            )
            .map(|_| ()),
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(e) = r {
            let _ = vm.warn(&format!("fwrite(): Unable to write to blob: {}", e.msg));
            return Err(std::io::Error::other("write failed"));
        }
        self.data = next;
        self.pos += data.len();
        Ok(data.len())
    }

    fn seek(&mut self, _vm: &mut Vm, pos: std::io::SeekFrom) -> Option<std::io::Result<u64>> {
        let len = self.data.len() as i64;
        let to = match pos {
            std::io::SeekFrom::Start(n) => n as i64,
            std::io::SeekFrom::Current(d) => self.pos as i64 + d,
            std::io::SeekFrom::End(d) => len + d,
        };
        if to < 0 || to > len {
            return Some(Err(std::io::Error::other("out of range")));
        }
        self.pos = to as usize;
        Some(Ok(to as u64))
    }

    fn tell(&mut self) -> Option<u64> {
        Some(self.pos as u64)
    }

    fn eof(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// `SQLite3::openBlob(table, column, rowid, database = "main", flags)`.
pub(super) fn m_open_blob(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let conn = super::open_conn_of(vm, &a.this_obj().clone())?;
    let table = sql_str(&a.str(0));
    let column = sql_str(&a.str(1));
    let rowid = a.int(2);
    let dbname = if a.len() > 3 {
        sql_str(&a.str(3))
    } else {
        "main".to_string()
    };
    let flags = a.int_or(4, 1);
    let target = format!(
        "{}.{}",
        scan::quote_ident(&dbname),
        scan::quote_ident(&table)
    );
    let col = scan::quote_ident(&column);
    let select = format!("SELECT {col} FROM {target} WHERE rowid = ?1");
    let r = prepare(vm, &conn, &select).and_then(|st| match st {
        Some((st, _, _)) => run(vm, &conn, &st, &[SqlValue::Integer(rowid)]),
        None => Ok(Default::default()),
    });
    let rows = match r {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Unable to open blob: {}", e.msg);
            conn.set_err(e);
            fail(vm, a, &conn, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    let data = match rows.rows.first().and_then(|r| r.first()) {
        Some(SqlValue::Blob(b)) => b.clone(),
        Some(SqlValue::Text(t)) => t.as_bytes().to_vec(),
        Some(other) => {
            let ty = match other {
                SqlValue::Null => "null",
                SqlValue::Integer(_) => "integer",
                _ => "real",
            };
            let e = SqlError::new(1, format!("cannot open value of type {ty}"));
            let msg = format!("Unable to open blob: {}", e.msg);
            conn.set_err(e);
            complain(vm, a, &conn, &msg)?;
            return Ok(Value::Bool(false));
        }
        None => {
            let e = SqlError::new(1, format!("no such rowid: {rowid}"));
            let msg = format!("Unable to open blob: {}", e.msg);
            conn.set_err(e);
            complain(vm, a, &conn, &msg)?;
            return Ok(Value::Bool(false));
        }
    };
    let bs = BlobStream {
        conn,
        update: format!("UPDATE {target} SET {col} = ?1 WHERE rowid = ?2"),
        rowid,
        data,
        pos: 0,
        writable: flags & 2 != 0,
    };
    let s = Stream::new(Backing::Native(Box::new(bs)), b"r+b", b"", "", "SQLite3");
    Ok(Value::Resource(new_stream_res(s)))
}
