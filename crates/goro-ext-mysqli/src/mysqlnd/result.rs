//! Result sets (mysqlnd_result.c, mysqlnd_result_meta.c), row decoding
//! (php_mysqlnd_rowp_read_*_protocol, mysqlnd_ps_codec.c's fetch functions)
//! and LOAD DATA LOCAL INFILE (mysqlnd_loaddata.c).

use goro_core::string::PhpString;
use goro_core::value::Value;

use super::conn::{Conn, ConnRef};
use super::stats::*;
use super::wire::RowPacket;
use super::*;

/// MYSQLND_FIELD
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub name: Vec<u8>,
    pub org_name: Vec<u8>,
    pub table: Vec<u8>,
    pub org_table: Vec<u8>,
    pub db: Vec<u8>,
    pub catalog: Vec<u8>,
    pub def: Vec<u8>,
    pub length: u32,
    pub max_length: u64,
    pub charsetnr: u32,
    pub flags: u32,
    pub ftype: u8,
    pub decimals: u8,
    pub is_numeric: bool,
    pub num_key: i64,
}

/// MYSQLND_RES_METADATA
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub fields: Vec<Field>,
    pub current_field: u32,
}

impl Meta {
    pub fn field_count(&self) -> u32 {
        self.fields.len() as u32
    }
    /// mysqlnd_res_meta::fetch_field
    pub fn fetch_field(&mut self) -> Option<&Field> {
        if self.current_field as usize >= self.fields.len() {
            return None;
        }
        let i = self.current_field as usize;
        self.current_field += 1;
        self.fields.get(i)
    }
}

/// enum_mysqlnd_res_type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultType {
    Normal,
    PsBuf,
    PsUnbuf,
}

/// MYSQLND_RES_BUFFERED
#[derive(Debug, Default)]
pub struct Buffered {
    pub row_buffers: Vec<Vec<u8>>,
    pub row_count: u64,
    pub current_row: u64,
    pub lengths: Option<Vec<u64>>,
    pub error_info: ErrorInfo,
    pub is_ps: bool,
}

/// MYSQLND_RES_UNBUFFERED
#[derive(Debug, Default)]
pub struct Unbuffered {
    pub eof_reached: bool,
    pub row_count: u64,
    pub has_last_row: bool,
    pub lengths: Option<Vec<u64>>,
    pub is_ps: bool,
    pub row_packet: Option<RowPacket>,
}

/// MYSQLND_RES
pub struct ResultSet {
    pub conn: Option<ConnRef>,
    pub field_count: u32,
    pub meta: Option<Meta>,
    pub rtype: ResultType,
    pub stored: Option<Buffered>,
    pub unbuf: Option<Unbuffered>,
    pub freed: bool,
}

impl ResultSet {
    /// mysqlnd_result_init
    pub fn new(field_count: u32) -> ResultSet {
        ResultSet {
            conn: None,
            field_count,
            meta: None,
            rtype: ResultType::Normal,
            stored: None,
            unbuf: None,
            freed: false,
        }
    }

    pub fn fields(&self) -> &[Field] {
        self.meta.as_ref().map(|m| &m.fields[..]).unwrap_or(&[])
    }

    /// mysqlnd_res::read_result_metadata + mysqlnd_res_meta::read_metadata
    pub fn read_result_metadata(&mut self, conn: &mut Conn) -> bool {
        self.meta = None;
        let mut meta = Meta::default();
        for _ in 0..self.field_count {
            let mut ei = ErrorInfo::default();
            match conn.read_field(false, &mut ei) {
                Err(()) => return false,
                Ok(None) => {
                    conn.error_info.copy_from(&ei);
                    return false;
                }
                Ok(Some(mut f)) => {
                    if !known_type(f.ftype) {
                        php_error_docref(
                            E_WARNING,
                            format!(
                                "Unknown type {} sent by the server. Please send a report to the developers",
                                f.ftype
                            ),
                        );
                        return false;
                    }
                    if let Some(n) = handle_numeric(&f.name) {
                        f.is_numeric = true;
                        f.num_key = n;
                    }
                    meta.fields.push(f);
                }
            }
        }
        self.meta = Some(meta);
        true
    }

    /// mysqlnd_res::use_result (row packet set up)
    pub fn init_use_result(&mut self, conn: &mut Conn, ps: bool) {
        conn.error_info.reset();
        self.rtype = if ps {
            ResultType::PsUnbuf
        } else {
            ResultType::Normal
        };
        self.unbuf = Some(Unbuffered {
            eof_reached: false,
            row_count: 0,
            has_last_row: false,
            lengths: if ps {
                None
            } else {
                Some(vec![0; self.field_count as usize])
            },
            is_ps: ps,
            row_packet: Some(RowPacket {
                binary_protocol: ps,
                ..Default::default()
            }),
        });
    }

    /// mysqlnd_res::store_result (without taking the connection reference)
    pub fn store_result(&mut self, conn: &mut Conn, ps: bool) -> bool {
        conn.state = ConnState::FetchingData;
        self.stored = Some(Buffered {
            lengths: if ps {
                None
            } else {
                Some(vec![0; self.field_count as usize])
            },
            is_ps: ps,
            ..Default::default()
        });
        if !self.store_result_fetch_data(conn, ps) {
            if let Some(s) = &self.stored {
                let e = s.error_info.clone();
                conn.error_info.copy_from(&e);
            } else {
                conn.error_info.set_oom();
            }
            return false;
        }
        let rc = self.stored.as_ref().map(|s| s.row_count).unwrap_or(0);
        if let Some(s) = self.stored.as_mut() {
            s.current_row = 0;
        }
        conn.upsert.affected_rows = rc;
        true
    }

    /// mysqlnd_res::store_result_fetch_data
    pub fn store_result_fetch_data(&mut self, conn: &mut Conn, binary_protocol: bool) -> bool {
        let mut rp = RowPacket {
            binary_protocol,
            ..Default::default()
        };
        let mut rows: Vec<Vec<u8>> = Vec::new();
        let ret = loop {
            match conn.read_row(&mut rp) {
                Ok(Some(row)) => rows.push(row),
                Ok(None) => break true,
                Err(()) => break false,
            }
        };
        let row_count = rows.len() as u64;
        conn.stats.inc_v(
            if binary_protocol {
                STAT_ROWS_BUFFERED_FROM_CLIENT_PS
            } else {
                STAT_ROWS_BUFFERED_FROM_CLIENT_NORMAL
            },
            row_count,
        );
        if rp.eof {
            conn.upsert.reset();
            conn.upsert.warning_count = rp.warning_count;
            conn.upsert.server_status = rp.server_status;
        }
        if !ret {
            conn.upsert.server_status &= !SERVER_MORE_RESULTS_EXISTS;
        }
        conn.state = if conn.upsert.server_status & SERVER_MORE_RESULTS_EXISTS != 0 {
            ConnState::NextResultPending
        } else {
            ConnState::Ready
        };
        let set = self.stored.as_mut().expect("stored");
        set.row_buffers = rows;
        set.row_count = row_count;
        if !ret {
            set.error_info.copy_from(&rp.error_info);
        } else {
            conn.upsert.affected_rows = row_count;
        }
        ret
    }

    /// mysqlnd_res::fetch_row: Ok(Some(row)) / Ok(None) (no more rows) /
    /// Err (failure, errors set on the connection).
    pub fn fetch_row(&mut self, want: bool) -> Result<Option<Vec<Value>>, ()> {
        if self.stored.is_some() {
            return self.buffered_fetch_row(want);
        }
        if self.unbuf.is_some() {
            let conn = match &self.conn {
                Some(c) => c.clone(),
                None => return Err(()),
            };
            let mut c = conn.borrow_mut();
            return self.unbuffered_fetch_row(&mut c, want);
        }
        Ok(None)
    }

    /// mysqlnd_result_buffered::fetch_row
    fn buffered_fetch_row(&mut self, want: bool) -> Result<Option<Vec<Value>>, ()> {
        let (native, conn) = match &self.conn {
            Some(c) => (c.borrow().options.int_and_float_native, Some(c.clone())),
            None => (false, None),
        };
        let fields: Vec<Field> = self.fields().to_vec();
        let set = self.stored.as_mut().unwrap();
        if set.current_row < set.row_count {
            let mut out = None;
            if want {
                let buf = &set.row_buffers[set.current_row as usize];
                let mut dummy = Stats::default();
                let row = {
                    let mut cguard = conn.as_ref().map(|c| c.borrow_mut());
                    let stats = match cguard.as_mut() {
                        Some(c) => &mut c.stats,
                        None => &mut dummy,
                    };
                    if set.is_ps {
                        decode_binary_row(buf, &fields, native, stats)
                    } else {
                        decode_text_row(buf, &fields, native, stats)
                    }
                };
                let row = match row {
                    Some(r) => r,
                    None => return Err(()),
                };
                if let Some(l) = set.lengths.as_mut() {
                    for (i, v) in row.iter().enumerate() {
                        l[i] = match v {
                            Value::String(s) => s.len() as u64,
                            _ => 0,
                        };
                    }
                }
                out = Some(row);
            }
            set.current_row += 1;
            global_inc(if set.is_ps {
                STAT_ROWS_FETCHED_FROM_CLIENT_PS_BUF
            } else {
                STAT_ROWS_FETCHED_FROM_CLIENT_NORMAL_BUF
            });
            Ok(Some(out.unwrap_or_default()))
        } else {
            if set.current_row == set.row_count {
                set.current_row = set.row_count + 1;
            }
            Ok(None)
        }
    }

    /// mysqlnd_result_unbuffered::fetch_row
    pub fn unbuffered_fetch_row(
        &mut self,
        conn: &mut Conn,
        want: bool,
    ) -> Result<Option<Vec<Value>>, ()> {
        let fields: Vec<Field> = self.fields().to_vec();
        let set = self.unbuf.as_mut().unwrap();
        if set.eof_reached {
            return Ok(None);
        }
        if conn.state != ConnState::FetchingData {
            conn.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return Err(());
        }
        let mut rp = match set.row_packet.take() {
            Some(p) => p,
            None => return Err(()),
        };
        let r = conn.read_row(&mut rp);
        let res = match r {
            Ok(Some(buf)) => {
                set.has_last_row = true;
                conn.stats.inc(if set.is_ps {
                    STAT_ROWS_FETCHED_FROM_CLIENT_PS_UNBUF
                } else {
                    STAT_ROWS_FETCHED_FROM_CLIENT_NORMAL_UNBUF
                });
                let mut out = Vec::new();
                if want {
                    let native = conn.options.int_and_float_native;
                    let row = if set.is_ps {
                        decode_binary_row(&buf, &fields, native, &mut conn.stats)
                    } else {
                        decode_text_row(&buf, &fields, native, &mut conn.stats)
                    };
                    let row = match row {
                        Some(r) => r,
                        None => {
                            set.row_packet = Some(rp);
                            return Err(());
                        }
                    };
                    if let Some(l) = set.lengths.as_mut() {
                        for (i, v) in row.iter().enumerate() {
                            l[i] = match v {
                                Value::String(s) => s.len() as u64,
                                _ => 0,
                            };
                        }
                    }
                    out = row;
                }
                set.row_count += 1;
                Ok(Some(out))
            }
            Err(()) => {
                if rp.error_info.error_no != 0 {
                    let e = rp.error_info.clone();
                    conn.error_info.copy_from(&e);
                }
                if conn.state != ConnState::QuitSent {
                    conn.state = ConnState::Ready;
                }
                set.eof_reached = true;
                Err(())
            }
            Ok(None) => {
                set.eof_reached = true;
                conn.upsert.reset();
                conn.upsert.warning_count = rp.warning_count;
                conn.upsert.server_status = rp.server_status;
                conn.state = if conn.upsert.server_status & SERVER_MORE_RESULTS_EXISTS != 0 {
                    ConnState::NextResultPending
                } else {
                    ConnState::Ready
                };
                Ok(None)
            }
        };
        set.row_packet = Some(rp);
        res
    }

    /// mysqlnd_res::skip_result
    pub fn skip_result(&mut self) {
        if let Some(u) = &self.unbuf {
            if !u.eof_reached {
                let conn = match &self.conn {
                    Some(c) => c.clone(),
                    None => return,
                };
                let mut c = conn.borrow_mut();
                self.skip_result_with(&mut c);
            }
        }
    }

    pub fn skip_result_with(&mut self, c: &mut Conn) {
        if let Some(u) = &self.unbuf {
            if u.eof_reached {
                return;
            }
            let normal = self.rtype == ResultType::Normal;
            c.stats.inc(if normal {
                STAT_FLUSHED_NORMAL_SETS
            } else {
                STAT_FLUSHED_PS_SETS
            });
            while let Ok(Some(_)) = self.unbuffered_fetch_row(c, false) {
                c.stats.inc(if normal {
                    STAT_ROWS_SKIPPED_NORMAL
                } else {
                    STAT_ROWS_SKIPPED_PS
                });
            }
        }
    }

    /// mysqlnd_res::free_result
    pub fn free_result(&mut self, implicit: bool) {
        if self.freed {
            return;
        }
        if let Some(c) = &self.conn {
            c.borrow_mut().stats.inc(if implicit {
                STAT_FREE_RESULT_IMPLICIT
            } else {
                STAT_FREE_RESULT_EXPLICIT
            });
        } else {
            // MYSQLND_INC_CONN_STATISTIC(NULL, ...) still counts globally
            global_inc(if implicit {
                STAT_FREE_RESULT_IMPLICIT
            } else {
                STAT_FREE_RESULT_EXPLICIT
            });
        }
        self.skip_result();
        self.free_result_contents();
    }

    /// free_result with the connection already borrowed
    pub fn free_result_with(&mut self, c: &mut Conn, implicit: bool) {
        if self.freed {
            return;
        }
        c.stats.inc(if implicit {
            STAT_FREE_RESULT_IMPLICIT
        } else {
            STAT_FREE_RESULT_EXPLICIT
        });
        self.skip_result_with(c);
        self.meta = None;
        self.unbuf = None;
        self.stored = None;
        self.freed = true;
    }

    /// mysqlnd_res::free_result_buffers
    pub fn free_result_buffers(&mut self) {
        self.meta = None;
        self.unbuf = None;
        self.stored = None;
    }

    /// mysqlnd_res::free_result_contents_internal
    pub fn free_result_contents(&mut self) {
        self.free_result_buffers();
        self.conn = None;
        self.freed = true;
    }

    /// mysqlnd_res::data_seek
    pub fn data_seek(&mut self, row: u64) -> bool {
        match self.stored.as_mut() {
            Some(s) => {
                s.current_row = if row >= s.row_count { s.row_count } else { row };
                true
            }
            None => false,
        }
    }

    /// mysqlnd_res::num_rows
    pub fn num_rows(&self) -> u64 {
        if let Some(s) = &self.stored {
            s.row_count
        } else if let Some(u) = &self.unbuf {
            if u.eof_reached { u.row_count } else { 0 }
        } else {
            0
        }
    }

    /// mysqlnd_res::fetch_lengths
    pub fn fetch_lengths(&self) -> Option<Vec<u64>> {
        if let Some(s) = &self.stored {
            if s.current_row > s.row_count || s.current_row == 0 {
                return None;
            }
            return s.lengths.clone();
        }
        if let Some(u) = &self.unbuf {
            if u.is_ps {
                return None;
            }
            if u.has_last_row || u.eof_reached {
                return u.lengths.clone();
            }
        }
        None
    }

    pub fn is_unbuffered(&self) -> bool {
        self.unbuf.is_some()
    }

    pub fn unbuffered_and_not_eof(&self) -> bool {
        self.unbuf.as_ref().map(|u| !u.eof_reached).unwrap_or(false)
    }

    pub fn field_tell(&self) -> u32 {
        self.meta.as_ref().map(|m| m.current_field).unwrap_or(0)
    }

    pub fn field_seek(&mut self, ofs: u32) -> u32 {
        match self.meta.as_mut() {
            Some(m) => {
                let r = m.current_field;
                m.current_field = ofs;
                r
            }
            None => 0,
        }
    }
}

impl Drop for ResultSet {
    fn drop(&mut self) {
        if !self.freed && (self.unbuf.is_some() || self.stored.is_some() || self.meta.is_some()) {
            // free_obj of mysqli_result: mysql_free_result(result) (explicit)
            if let Some(conn) = self.conn.clone() {
                if let Ok(mut c) = conn.try_borrow_mut() {
                    c.stats.inc(STAT_FREE_RESULT_EXPLICIT);
                    self.skip_result_with(&mut c);
                }
            }
        }
    }
}

/// mysqlnd_ps_fetch_functions[type].func != NULL
pub fn known_type(t: u8) -> bool {
    matches!(
        t,
        MYSQL_TYPE_NULL
            | MYSQL_TYPE_TINY
            | MYSQL_TYPE_SHORT
            | MYSQL_TYPE_YEAR
            | MYSQL_TYPE_INT24
            | MYSQL_TYPE_LONG
            | MYSQL_TYPE_LONGLONG
            | MYSQL_TYPE_FLOAT
            | MYSQL_TYPE_DOUBLE
            | MYSQL_TYPE_TIME
            | MYSQL_TYPE_DATE
            | MYSQL_TYPE_NEWDATE
            | MYSQL_TYPE_DATETIME
            | MYSQL_TYPE_TIMESTAMP
            | MYSQL_TYPE_VECTOR
            | MYSQL_TYPE_JSON
            | MYSQL_TYPE_TINY_BLOB
            | MYSQL_TYPE_BLOB
            | MYSQL_TYPE_MEDIUM_BLOB
            | MYSQL_TYPE_LONG_BLOB
            | MYSQL_TYPE_BIT
            | MYSQL_TYPE_VAR_STRING
            | MYSQL_TYPE_VARCHAR
            | MYSQL_TYPE_STRING
            | MYSQL_TYPE_DECIMAL
            | MYSQL_TYPE_NEWDECIMAL
            | MYSQL_TYPE_ENUM
            | MYSQL_TYPE_SET
            | MYSQL_TYPE_GEOMETRY
    )
}

/// The PHP type of mysqlnd_ps_fetch_functions[type] (php_type)
#[derive(PartialEq, Eq)]
enum PhpType {
    Null,
    Long,
    Double,
    Str,
}

fn perm_bind(t: u8) -> (PhpType, i32) {
    // (php_type, pack_len)
    match t {
        MYSQL_TYPE_NULL => (PhpType::Null, 0),
        MYSQL_TYPE_TINY => (PhpType::Long, 1),
        MYSQL_TYPE_SHORT | MYSQL_TYPE_YEAR => (PhpType::Long, 2),
        MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG => (PhpType::Long, 4),
        MYSQL_TYPE_LONGLONG => (PhpType::Long, 8),
        MYSQL_TYPE_FLOAT => (PhpType::Double, 4),
        MYSQL_TYPE_DOUBLE => (PhpType::Double, 8),
        MYSQL_TYPE_BIT => (PhpType::Long, 8),
        MYSQL_TYPE_TIME | MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE | MYSQL_TYPE_DATETIME
        | MYSQL_TYPE_TIMESTAMP => (PhpType::Str, -1),
        _ => (PhpType::Str, -2),
    }
}

/// ZEND_HANDLE_NUMERIC on a column name
pub fn handle_numeric(name: &[u8]) -> Option<i64> {
    if name.is_empty() || name.len() > 20 {
        return None;
    }
    let (neg, digits) = if name[0] == b'-' {
        (true, &name[1..])
    } else {
        (false, name)
    };
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 1 && digits[0] == b'0' {
        return None;
    }
    if neg && digits == b"0" {
        return None;
    }
    let s = std::str::from_utf8(name).ok()?;
    s.parse::<i64>().ok()
}

fn s(v: Vec<u8>) -> Value {
    Value::String(PhpString::from_vec(v))
}

// ── Text protocol ─────────────────────────────────────────────────────

fn text_stat(t: u8) -> usize {
    match t {
        MYSQL_TYPE_DECIMAL => STAT_TEXT_TYPE_FETCHED_DECIMAL,
        MYSQL_TYPE_TINY => STAT_TEXT_TYPE_FETCHED_INT8,
        MYSQL_TYPE_SHORT => STAT_TEXT_TYPE_FETCHED_INT16,
        MYSQL_TYPE_LONG => STAT_TEXT_TYPE_FETCHED_INT32,
        MYSQL_TYPE_FLOAT => STAT_TEXT_TYPE_FETCHED_FLOAT,
        MYSQL_TYPE_DOUBLE => STAT_TEXT_TYPE_FETCHED_DOUBLE,
        MYSQL_TYPE_NULL => STAT_TEXT_TYPE_FETCHED_NULL,
        MYSQL_TYPE_TIMESTAMP => STAT_TEXT_TYPE_FETCHED_TIMESTAMP,
        MYSQL_TYPE_LONGLONG => STAT_TEXT_TYPE_FETCHED_INT64,
        MYSQL_TYPE_INT24 => STAT_TEXT_TYPE_FETCHED_INT24,
        MYSQL_TYPE_DATE => STAT_TEXT_TYPE_FETCHED_DATE,
        MYSQL_TYPE_TIME => STAT_TEXT_TYPE_FETCHED_TIME,
        MYSQL_TYPE_DATETIME => STAT_TEXT_TYPE_FETCHED_DATETIME,
        MYSQL_TYPE_YEAR => STAT_TEXT_TYPE_FETCHED_YEAR,
        MYSQL_TYPE_NEWDATE => STAT_TEXT_TYPE_FETCHED_DATE,
        MYSQL_TYPE_VARCHAR => STAT_TEXT_TYPE_FETCHED_STRING,
        MYSQL_TYPE_BIT => STAT_TEXT_TYPE_FETCHED_BIT,
        MYSQL_TYPE_NEWDECIMAL => STAT_TEXT_TYPE_FETCHED_DECIMAL,
        MYSQL_TYPE_ENUM => STAT_TEXT_TYPE_FETCHED_ENUM,
        MYSQL_TYPE_SET => STAT_TEXT_TYPE_FETCHED_SET,
        MYSQL_TYPE_JSON => STAT_TEXT_TYPE_FETCHED_JSON,
        MYSQL_TYPE_TINY_BLOB
        | MYSQL_TYPE_MEDIUM_BLOB
        | MYSQL_TYPE_LONG_BLOB
        | MYSQL_TYPE_BLOB
        | MYSQL_TYPE_VECTOR => STAT_TEXT_TYPE_FETCHED_BLOB,
        MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_STRING => STAT_TEXT_TYPE_FETCHED_STRING,
        MYSQL_TYPE_GEOMETRY => STAT_TEXT_TYPE_FETCHED_GEOMETRY,
        _ => STAT_TEXT_TYPE_FETCHED_OTHER,
    }
}

fn binary_stat(t: u8) -> usize {
    match t {
        MYSQL_TYPE_DECIMAL => STAT_BINARY_TYPE_FETCHED_DECIMAL,
        MYSQL_TYPE_TINY => STAT_BINARY_TYPE_FETCHED_INT8,
        MYSQL_TYPE_SHORT => STAT_BINARY_TYPE_FETCHED_INT16,
        MYSQL_TYPE_LONG => STAT_BINARY_TYPE_FETCHED_INT32,
        MYSQL_TYPE_FLOAT => STAT_BINARY_TYPE_FETCHED_FLOAT,
        MYSQL_TYPE_DOUBLE => STAT_BINARY_TYPE_FETCHED_DOUBLE,
        MYSQL_TYPE_NULL => STAT_BINARY_TYPE_FETCHED_NULL,
        MYSQL_TYPE_TIMESTAMP => STAT_BINARY_TYPE_FETCHED_TIMESTAMP,
        MYSQL_TYPE_LONGLONG => STAT_BINARY_TYPE_FETCHED_INT64,
        MYSQL_TYPE_INT24 => STAT_BINARY_TYPE_FETCHED_INT24,
        MYSQL_TYPE_DATE => STAT_BINARY_TYPE_FETCHED_DATE,
        MYSQL_TYPE_TIME => STAT_BINARY_TYPE_FETCHED_TIME,
        MYSQL_TYPE_DATETIME => STAT_BINARY_TYPE_FETCHED_DATETIME,
        MYSQL_TYPE_YEAR => STAT_BINARY_TYPE_FETCHED_YEAR,
        MYSQL_TYPE_NEWDATE => STAT_BINARY_TYPE_FETCHED_DATE,
        MYSQL_TYPE_VARCHAR => STAT_BINARY_TYPE_FETCHED_STRING,
        MYSQL_TYPE_BIT => STAT_BINARY_TYPE_FETCHED_BIT,
        MYSQL_TYPE_NEWDECIMAL => STAT_BINARY_TYPE_FETCHED_DECIMAL,
        MYSQL_TYPE_ENUM => STAT_BINARY_TYPE_FETCHED_ENUM,
        MYSQL_TYPE_SET => STAT_BINARY_TYPE_FETCHED_SET,
        MYSQL_TYPE_TINY_BLOB
        | MYSQL_TYPE_MEDIUM_BLOB
        | MYSQL_TYPE_LONG_BLOB
        | MYSQL_TYPE_BLOB
        | MYSQL_TYPE_VECTOR => STAT_BINARY_TYPE_FETCHED_BLOB,
        MYSQL_TYPE_VAR_STRING | MYSQL_TYPE_STRING => STAT_BINARY_TYPE_FETCHED_STRING,
        MYSQL_TYPE_GEOMETRY => STAT_BINARY_TYPE_FETCHED_GEOMETRY,
        _ => STAT_BINARY_TYPE_FETCHED_OTHER,
    }
}

/// atoll()
fn atoll(b: &[u8]) -> i64 {
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
    if neg { n.wrapping_neg() } else { n }
}

/// strtoull(p, NULL, 10)
fn strtoull(b: &[u8]) -> u64 {
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut n: u64 = 0;
    let mut overflow = false;
    while i < b.len() && b[i].is_ascii_digit() {
        match n
            .checked_mul(10)
            .and_then(|x| x.checked_add((b[i] - b'0') as u64))
        {
            Some(x) => n = x,
            None => overflow = true,
        }
        i += 1;
    }
    if overflow {
        return u64::MAX;
    }
    if neg { n.wrapping_neg() } else { n }
}

/// zend_strtod: the longest prefix of the form [+-]digits[.digits][e[+-]digits]
pub fn zend_strtod(b: &[u8]) -> f64 {
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut nd = i - ds;
    if i < b.len() && b[i] == b'.' {
        let j = i + 1;
        let mut k = j;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        nd += k - j;
        i = k;
    }
    if nd == 0 {
        return 0.0;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut k = i + 1;
        if k < b.len() && (b[k] == b'-' || b[k] == b'+') {
            k += 1;
        }
        let es = k;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k > es {
            i = k;
        }
    }
    std::str::from_utf8(&b[..i])
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// php_mysqlnd_rowp_read_text_protocol
pub fn decode_text_row(
    buf: &[u8],
    fields: &[Field],
    as_int_or_float: bool,
    stats: &mut Stats,
) -> Option<Vec<Value>> {
    let mut out = Vec::with_capacity(fields.len());
    let mut p = 0usize;
    let end = buf.len();
    for f in fields {
        let len = net_field_length(buf, &mut p);
        if len == MYSQLND_NULL_LENGTH {
            out.push(Value::Null);
            continue;
        }
        if p > end || len > (end - p) as u64 {
            php_error_docref(
                E_WARNING,
                "Malformed server packet. Field length pointing after end of packet",
            );
            return None;
        }
        let len = len as usize;
        let data = &buf[p..p + len];
        let (php_type, pack_len) = perm_bind(f.ftype);
        stats.inc2(
            text_stat(f.ftype),
            1,
            STAT_BYTES_RECEIVED_PURE_DATA_TEXT,
            len as u64,
        );
        let v = if f.ftype == MYSQL_TYPE_BIT {
            let mut q = 0usize;
            let v = fetch_from_1_to_8_bytes(f, 0, data, &mut q, len as u32);
            match v {
                Some(Value::Long(n)) if !as_int_or_float => {
                    // ZEND_ULONG_FMT
                    s((n as u64).to_string().into_bytes())
                }
                Some(v) => v,
                None => return None,
            }
        } else if as_int_or_float && php_type == PhpType::Long && f.flags & ZEROFILL_FLAG == 0 {
            if pack_len < 8 {
                Value::Long(atoll(data))
            } else {
                let v = strtoull(data);
                let uns = f.flags & UNSIGNED_FLAG != 0;
                if uns && v > 9223372036854775807u64 {
                    s(data.to_vec())
                } else {
                    Value::Long(v as i64)
                }
            }
        } else if as_int_or_float && php_type == PhpType::Double {
            Value::Double(zend_strtod(data))
        } else {
            s(data.to_vec())
        };
        out.push(v);
        p += len;
    }
    Some(out)
}

// ── Binary protocol (mysqlnd_ps_codec.c) ──────────────────────────────

fn over_read_error() {
    php_error_docref(
        E_WARNING,
        "Malformed server packet. Field length pointing after the end of packet",
    );
}

/// ps_fetch_from_1_to_8_bytes: decodes `byte_count` bytes at `*pos`.
/// None on over-read (warning emitted).
fn fetch_from_1_to_8_bytes(
    f: &Field,
    pack_len: usize,
    row: &[u8],
    pos: &mut usize,
    byte_count: u32,
) -> Option<Value> {
    if pack_len > 0 && byte_count as usize > pack_len {
        over_read_error();
        return None;
    }
    let p = row.get(*pos..).unwrap_or(&[]);
    let b = |i: usize| p.get(i).copied().unwrap_or(0) as u64;
    let is_bit = f.ftype == MYSQL_TYPE_BIT;
    let v = if f.flags & UNSIGNED_FLAG != 0 {
        // big endian for BIT (bit_uintNkorr), little endian otherwise
        let be = |n: usize| -> u64 {
            let mut v = 0u64;
            for i in 0..n {
                v = (v << 8) | b(i);
            }
            v
        };
        let le = |n: usize| -> u64 {
            let mut v = 0u64;
            for i in (0..n).rev() {
                v = (v << 8) | b(i);
            }
            v
        };
        let uval: u64 = match byte_count {
            8 => {
                if is_bit {
                    be(8)
                } else {
                    le(8)
                }
            }
            7 => be(7),
            6 => be(6),
            5 => be(5),
            4 => {
                if is_bit {
                    be(4)
                } else {
                    le(4)
                }
            }
            3 => {
                if is_bit {
                    be(3)
                } else {
                    le(3)
                }
            }
            2 => {
                if is_bit {
                    be(2)
                } else {
                    le(2)
                }
            }
            1 => b(0),
            _ => 0,
        };
        if f.flags & ZEROFILL_FLAG != 0 {
            s(format!("{:0width$}", uval, width = f.length as usize).into_bytes())
        } else if byte_count < 8 || uval <= 9223372036854775807u64 {
            Value::Long(uval as i64)
        } else {
            s(uval.to_string().into_bytes())
        }
    } else {
        let lval: i64 = match byte_count {
            8 => i64::from_le_bytes([
                b(0) as u8,
                b(1) as u8,
                b(2) as u8,
                b(3) as u8,
                b(4) as u8,
                b(5) as u8,
                b(6) as u8,
                b(7) as u8,
            ]),
            4 => i32::from_le_bytes([b(0) as u8, b(1) as u8, b(2) as u8, b(3) as u8]) as i64,
            3 => {
                let v = (b(0) | b(1) << 8 | b(2) << 16) as i32;
                ((v << 8) >> 8) as i64
            }
            2 => i16::from_le_bytes([b(0) as u8, b(1) as u8]) as i64,
            1 => b(0) as u8 as i8 as i64,
            _ => 0,
        };
        Value::Long(lval)
    };
    *pos += byte_count as usize;
    Some(v)
}

/// mysql_float_to_double
pub fn mysql_float_to_double(fp4: f32, decimals: i32) -> f64 {
    let d = fp4 as f64;
    let s = if decimals < 0 {
        // zend_gcvt(fp4, FLT_DIG, ...): 6 significant digits
        format!("{:.5e}", d)
    } else {
        format!("{:.*}", decimals as usize, d)
    };
    s.parse::<f64>().unwrap_or(d)
}

/// The fetch function of a binary row field. `pack_len` is the space
/// left in the packet (0: unchecked).
fn ps_fetch(f: &Field, pack_len: usize, row: &[u8], pos: &mut usize) -> Option<Value> {
    let rest_check_var = |pos: &mut usize, start: usize, length: u64| -> bool {
        // ps_fetch_is_packet_over_read_with_variable_length
        if pack_len == 0 {
            return false;
        }
        let length_len = *pos - start;
        if length_len > pack_len || length > (pack_len - length_len) as u64 {
            over_read_error();
            return true;
        }
        false
    };
    match f.ftype {
        MYSQL_TYPE_NULL => Some(Value::Null),
        MYSQL_TYPE_TINY => fetch_from_1_to_8_bytes(f, pack_len, row, pos, 1),
        MYSQL_TYPE_SHORT | MYSQL_TYPE_YEAR => fetch_from_1_to_8_bytes(f, pack_len, row, pos, 2),
        MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG => fetch_from_1_to_8_bytes(f, pack_len, row, pos, 4),
        MYSQL_TYPE_LONGLONG => fetch_from_1_to_8_bytes(f, pack_len, row, pos, 8),
        MYSQL_TYPE_FLOAT => {
            if pack_len > 0 && 4 > pack_len {
                over_read_error();
                return None;
            }
            let p = row.get(*pos..*pos + 4)?;
            let fval = f32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            *pos += 4;
            let dec = if f.decimals >= 31 {
                -1
            } else {
                f.decimals as i32
            };
            Some(Value::Double(mysql_float_to_double(fval, dec)))
        }
        MYSQL_TYPE_DOUBLE => {
            if pack_len > 0 && 8 > pack_len {
                over_read_error();
                return None;
            }
            let p = row.get(*pos..*pos + 8)?;
            let v = f64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]]);
            *pos += 8;
            Some(Value::Double(v))
        }
        MYSQL_TYPE_TIME => {
            let start = *pos;
            let length = net_field_length(row, pos);
            let (mut neg, mut hour, mut minute, mut second, mut second_part) =
                (false, 0u64, 0u32, 0u32, 0u64);
            if length != 0 {
                if rest_check_var(pos, start, length) {
                    return None;
                }
                let to = row.get(*pos..).unwrap_or(&[]);
                let g = |i: usize| to.get(i).copied().unwrap_or(0);
                neg = g(0) != 0;
                let day = i32::from_le_bytes([g(1), g(2), g(3), g(4)]) as u32 as u64;
                hour = g(5) as u64;
                minute = g(6) as u32;
                second = g(7) as u32;
                second_part = if length > 8 {
                    i32::from_le_bytes([g(8), g(9), g(10), g(11)]) as u32 as u64
                } else {
                    0
                };
                if day != 0 {
                    hour = (hour + day * 24) & 0xFFFF_FFFF;
                }
                *pos += length as usize;
            }
            let sign = if neg { "-" } else { "" };
            let txt = if f.decimals > 0 && f.decimals < 7 {
                let frac = (second_part as f64 / 10f64.powi(6 - f.decimals as i32)) as u32;
                format!(
                    "{}{:02}:{:02}:{:02}.{:0w$}",
                    sign,
                    hour,
                    minute,
                    second,
                    frac,
                    w = f.decimals as usize
                )
            } else {
                format!("{}{:02}:{:02}:{:02}", sign, hour, minute, second)
            };
            Some(s(txt.into_bytes()))
        }
        MYSQL_TYPE_DATE => {
            let start = *pos;
            let length = net_field_length(row, pos);
            let (mut y, mut m, mut d) = (0u32, 0u32, 0u32);
            if length != 0 {
                if rest_check_var(pos, start, length) {
                    return None;
                }
                let to = row.get(*pos..).unwrap_or(&[]);
                let g = |i: usize| to.get(i).copied().unwrap_or(0);
                y = i16::from_le_bytes([g(0), g(1)]) as i32 as u32;
                m = g(2) as u32;
                d = g(3) as u32;
                *pos += length as usize;
            }
            Some(s(format!("{:04}-{:02}-{:02}", y, m, d).into_bytes()))
        }
        MYSQL_TYPE_DATETIME | MYSQL_TYPE_TIMESTAMP => {
            let start = *pos;
            let length = net_field_length(row, pos);
            let (mut y, mut mo, mut d, mut h, mut mi, mut se, mut sp) =
                (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u64);
            if length != 0 {
                if rest_check_var(pos, start, length) {
                    return None;
                }
                let to = row.get(*pos..).unwrap_or(&[]);
                let g = |i: usize| to.get(i).copied().unwrap_or(0);
                y = i16::from_le_bytes([g(0), g(1)]) as i32 as u32;
                mo = g(2) as u32;
                d = g(3) as u32;
                if length > 4 {
                    h = g(4) as u32;
                    mi = g(5) as u32;
                    se = g(6) as u32;
                }
                sp = if length > 7 {
                    i32::from_le_bytes([g(7), g(8), g(9), g(10)]) as u32 as u64
                } else {
                    0
                };
                *pos += length as usize;
            }
            let txt = if f.decimals > 0 && f.decimals < 7 {
                let frac = (sp as f64 / 10f64.powi(6 - f.decimals as i32)) as u32;
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:0w$}",
                    y,
                    mo,
                    d,
                    h,
                    mi,
                    se,
                    frac,
                    w = f.decimals as usize
                )
            } else {
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, se)
            };
            Some(s(txt.into_bytes()))
        }
        MYSQL_TYPE_BIT => {
            let start = *pos;
            let length = net_field_length(row, pos);
            if rest_check_var(pos, start, length) {
                return None;
            }
            fetch_from_1_to_8_bytes(f, pack_len, row, pos, length as u32)
        }
        _ => {
            // ps_fetch_string
            let start = *pos;
            let length = net_field_length(row, pos);
            if rest_check_var(pos, start, length) {
                return None;
            }
            let end = (*pos + length as usize).min(row.len());
            let v = row.get(*pos..end).unwrap_or(&[]).to_vec();
            *pos += length as usize;
            Some(s(v))
        }
    }
}

/// php_mysqlnd_rowp_read_binary_protocol
pub fn decode_binary_row(
    buf: &[u8],
    fields: &[Field],
    _as_int_or_float: bool,
    stats: &mut Stats,
) -> Option<Vec<Value>> {
    let rbs = buf.len();
    let field_count = fields.len();
    let mut out = Vec::with_capacity(field_count);
    let mut p = 1usize; // skip the status byte
    let mut null_ptr = p;
    p += (field_count + 9) / 8;
    let mut bit: u32 = 4;
    for f in fields {
        let orig_p = p;
        let (v, statistic) = if buf.get(null_ptr).copied().unwrap_or(0) as u32 & bit != 0 {
            (Value::Null, STAT_BINARY_TYPE_FETCHED_NULL)
        } else {
            if rbs <= p {
                php_error_docref(
                    E_WARNING,
                    "Malformed server packet. No packet space left for the field",
                );
                return None;
            }
            (ps_fetch(f, rbs - p, buf, &mut p)?, binary_stat(f.ftype))
        };
        let n = match &v {
            Value::String(s) => s.len() as u64,
            _ => (p - orig_p) as u64,
        };
        stats.inc2(statistic, 1, STAT_BYTES_RECEIVED_PURE_DATA_PS, n);
        out.push(v);
        bit <<= 1;
        if bit & 255 == 0 {
            bit = 1;
            null_ptr += 1;
        }
    }
    Some(out)
}

// ── LOAD DATA LOCAL INFILE (mysqlnd_loaddata.c) ───────────────────────

thread_local! {
    /// The open_basedir setting, synced by the mysqli layer.
    pub static OPEN_BASEDIR: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// mysqlnd_handle_local_infile: (result, is_warning)
pub fn handle_local_infile(conn: &mut Conn, filename: &[u8]) -> (bool, bool) {
    let mut result = false;
    let is_warning;
    let fname = String::from_utf8_lossy(filename).into_owned();
    let enabled = conn.options.flags & CLIENT_LOCAL_FILES == CLIENT_LOCAL_FILES;
    let dir = conn.options.local_infile_directory.clone();
    let mut ok = true;
    if !enabled && dir.is_none() {
        conn.error_info.set_str(
            CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
            UNKNOWN_SQLSTATE,
            "LOAD DATA LOCAL INFILE is forbidden, check related settings like mysqli.allow_local_infile|mysqli.local_infile_directory or Pdo\\Mysql::ATTR_LOCAL_INFILE|Pdo\\Mysql::ATTR_LOCAL_INFILE_DIRECTORY",
        );
        ok = false;
    }
    if let Some(d) = &dir {
        if std::fs::read_dir(d).is_err() {
            php_error_docref(
                E_WARNING,
                format!(
                    "opendir({}): Failed to open directory: No such file or directory",
                    d
                ),
            );
            conn.error_info.set_str(
                CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
                UNKNOWN_SQLSTATE,
                "cannot open local_infile_directory",
            );
            ok = false;
        }
    }
    if ok && !enabled {
        if let Some(d) = &dir {
            if !goro_core::basedir::check_specific_open_basedir(d, &fname) {
                conn.error_info.set_str(
                    CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
                    UNKNOWN_SQLSTATE,
                    "LOAD DATA LOCAL INFILE DIRECTORY restriction in effect. Unable to open file",
                );
                ok = false;
            }
        }
    }
    'body: {
        if !ok {
            conn.pfc
                .send(&mut conn.vio, &[], &mut conn.stats, &mut conn.error_info);
            is_warning = true;
            break 'body;
        }
        // mysqlnd_local_infile_init
        let basedir = OPEN_BASEDIR.with(|b| b.borrow().clone());
        let mut init_err: Option<(u32, String)> = None;
        if !basedir.is_empty() {
            let allowed = basedir
                .split(':')
                .filter(|d| !d.is_empty())
                .any(|d| goro_core::basedir::check_specific_open_basedir(d, &fname));
            if !allowed {
                init_err = Some((
                    CR_UNKNOWN_ERROR,
                    "open_basedir restriction in effect. Unable to open file".to_string(),
                ));
            }
        }
        let mut file = None;
        if init_err.is_none() {
            match std::fs::File::open(&fname) {
                Ok(f) => file = Some(f),
                Err(_) => {
                    let short: String = fname.chars().take(64).collect();
                    init_err = Some((
                        MYSQLND_EE_FILENOTFOUND,
                        format!("Can't find file '{}'.", short),
                    ));
                }
            }
        }
        if let Some((no, msg)) = init_err {
            is_warning = true;
            conn.error_info.set_str(no, UNKNOWN_SQLSTATE, &msg);
            conn.pfc
                .send(&mut conn.vio, &[], &mut conn.stats, &mut conn.error_info);
            break 'body;
        }
        is_warning = false;
        let mut file = file.unwrap();
        let mut buf = vec![0u8; 4096 - MYSQLND_HEADER_SIZE];
        use std::io::Read;
        let mut read_failed = false;
        loop {
            match file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if conn.pfc.send(
                        &mut conn.vio,
                        &buf[..n],
                        &mut conn.stats,
                        &mut conn.error_info,
                    ) == 0
                    {
                        conn.error_info.set_str(
                            CR_SERVER_LOST,
                            UNKNOWN_SQLSTATE,
                            "Lost connection to MySQL server during LOAD DATA of a local file",
                        );
                        break 'body;
                    }
                }
                Err(_) => {
                    read_failed = true;
                    break;
                }
            }
        }
        if conn
            .pfc
            .send(&mut conn.vio, &[], &mut conn.stats, &mut conn.error_info)
            == 0
        {
            conn.error_info.set_str(
                CR_SERVER_LOST,
                UNKNOWN_SQLSTATE,
                "Lost connection to MySQL server during LOAD DATA of a local file",
            );
            break 'body;
        }
        if read_failed {
            conn.error_info
                .set_str(CR_UNKNOWN_ERROR, UNKNOWN_SQLSTATE, "Error reading file");
            return finish_infile(conn, false, true);
        }
        result = true;
    }
    finish_infile(conn, result, is_warning)
}

fn finish_infile(conn: &mut Conn, mut result: bool, is_warning: bool) -> (bool, bool) {
    if !conn.send_command_handle_response(true, false, COM_QUERY, false) {
        result = false;
    }
    (result, is_warning)
}
