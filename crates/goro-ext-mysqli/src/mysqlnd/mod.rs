//! mysqlnd — the MySQL native driver, ported from php-src's ext/mysqlnd.
//!
//! The structure follows the C sources: `vio` (network I/O and the packet
//! frame codec), `wire` (packet decoding, mysqlnd_wireprotocol.c), `conn`
//! (mysqlnd_connection.c / mysqlnd_commands.c), `result` / `ps` (result sets
//! and prepared statements), `auth`, `charset` and `stats`.
//!
//! This layer knows nothing about the VM except for PHP values: warnings that
//! mysqlnd raises with php_error_docref() are queued (see `push_warning`) and
//! emitted by the mysqli layer once the operation is finished.
#![allow(dead_code)]

pub mod auth;
pub mod charset;
pub mod conn;
pub mod ps;
pub mod result;
pub mod stats;
pub mod vio;
pub mod wire;

use std::cell::RefCell;

// ── mysqlnd_enum_n_def.h ──────────────────────────────────────────────

pub const MYSQLND_MAX_PACKET_SIZE: usize = 256 * 256 * 256 - 1;
pub const MYSQLND_ASSEMBLED_PACKET_MAX_SIZE: u32 = 3 * 1024 * 1024 * 1024;
pub const MYSQLND_DEFAULT_AUTH_PROTOCOL: &str = "mysql_native_password";
pub const MYSQLND_ERRMSG_SIZE: usize = 512;
pub const MYSQLND_SQLSTATE_NULL: &str = "00000";
pub const MYSQLND_MAX_ALLOWED_USER_LEN: usize = 252;
pub const MYSQLND_MAX_ALLOWED_DB_LEN: usize = 1024;
pub const MYSQLND_NET_CMD_BUFFER_MIN_SIZE: usize = 4096;
pub const MYSQLND_HEADER_SIZE: usize = 4;
pub const COMPRESSED_HEADER_SIZE: usize = 3;
pub const MYSQLND_NULL_LENGTH: u64 = u64::MAX;

pub const SERVER_STATUS_IN_TRANS: u32 = 1;
pub const SERVER_STATUS_AUTOCOMMIT: u32 = 2;
pub const SERVER_MORE_RESULTS_EXISTS: u32 = 8;
pub const SERVER_QUERY_NO_GOOD_INDEX_USED: u32 = 16;
pub const SERVER_QUERY_NO_INDEX_USED: u32 = 32;
pub const SERVER_STATUS_CURSOR_EXISTS: u32 = 64;
pub const SERVER_STATUS_LAST_ROW_SENT: u32 = 128;
pub const SERVER_STATUS_DB_DROPPED: u32 = 256;
pub const SERVER_STATUS_NO_BACKSLASH_ESCAPES: u32 = 512;
pub const SERVER_QUERY_WAS_SLOW: u32 = 2048;
pub const SERVER_PS_OUT_PARAMS: u32 = 4096;

pub const SCRAMBLE_LENGTH: usize = 20;
pub const SCRAMBLE_LENGTH_323: usize = 8;

pub const CLIENT_LONG_PASSWORD: u32 = 1;
pub const CLIENT_FOUND_ROWS: u32 = 2;
pub const CLIENT_LONG_FLAG: u32 = 4;
pub const CLIENT_CONNECT_WITH_DB: u32 = 8;
pub const CLIENT_NO_SCHEMA: u32 = 16;
pub const CLIENT_COMPRESS: u32 = 32;
pub const CLIENT_ODBC: u32 = 64;
pub const CLIENT_LOCAL_FILES: u32 = 128;
pub const CLIENT_IGNORE_SPACE: u32 = 256;
pub const CLIENT_PROTOCOL_41: u32 = 512;
pub const CLIENT_INTERACTIVE: u32 = 1024;
pub const CLIENT_SSL: u32 = 2048;
pub const CLIENT_IGNORE_SIGPIPE: u32 = 4096;
pub const CLIENT_TRANSACTIONS: u32 = 8192;
pub const CLIENT_RESERVED: u32 = 16384;
pub const CLIENT_SECURE_CONNECTION: u32 = 32768;
pub const CLIENT_MULTI_STATEMENTS: u32 = 1 << 16;
pub const CLIENT_MULTI_RESULTS: u32 = 1 << 17;
pub const CLIENT_PS_MULTI_RESULTS: u32 = 1 << 18;
pub const CLIENT_PLUGIN_AUTH: u32 = 1 << 19;
pub const CLIENT_CONNECT_ATTRS: u32 = 1 << 20;
pub const CLIENT_PLUGIN_AUTH_LENENC_CLIENT_DATA: u32 = 1 << 21;
pub const CLIENT_CAN_HANDLE_EXPIRED_PASSWORDS: u32 = 1 << 22;
pub const CLIENT_SESSION_TRACK: u32 = 1 << 23;
pub const CLIENT_SSL_DONT_VERIFY_SERVER_CERT: u32 = CLIENT_ODBC;
pub const CLIENT_SSL_VERIFY_SERVER_CERT: u32 = 1 << 30;
pub const CLIENT_REMEMBER_OPTIONS: u32 = 1 << 31;

pub const MYSQLND_CAPABILITIES: u32 = CLIENT_LONG_PASSWORD
    | CLIENT_LONG_FLAG
    | CLIENT_TRANSACTIONS
    | CLIENT_PROTOCOL_41
    | CLIENT_SECURE_CONNECTION
    | CLIENT_MULTI_RESULTS
    | CLIENT_LOCAL_FILES
    | CLIENT_PLUGIN_AUTH;

pub const CR_UNKNOWN_ERROR: u32 = 2000;
pub const CR_CONNECTION_ERROR: u32 = 2002;
pub const CR_SERVER_GONE_ERROR: u32 = 2006;
pub const CR_OUT_OF_MEMORY: u32 = 2008;
pub const CR_SERVER_LOST: u32 = 2013;
pub const CR_COMMANDS_OUT_OF_SYNC: u32 = 2014;
pub const CR_CANT_FIND_CHARSET: u32 = 2019;
pub const CR_MALFORMED_PACKET: u32 = 2027;
pub const CR_NOT_IMPLEMENTED: u32 = 2054;
pub const CR_NO_PREPARE_STMT: u32 = 2030;
pub const CR_PARAMS_NOT_BOUND: u32 = 2031;
pub const CR_INVALID_PARAMETER_NO: u32 = 2034;
pub const CR_INVALID_BUFFER_USE: u32 = 2035;
pub const CR_LOAD_DATA_LOCAL_INFILE_REJECTED: u32 = 2068;
pub const CR_CLIENT_INTERACTION_TIMEOUT: u32 = 4031;
pub const MYSQLND_EE_FILENOTFOUND: u32 = 7890;
pub const UNKNOWN_SQLSTATE: &str = "HY000";

pub const TRANS_START_NO_OPT: u32 = 0;
pub const TRANS_START_WITH_CONSISTENT_SNAPSHOT: u32 = 1;
pub const TRANS_START_READ_WRITE: u32 = 2;
pub const TRANS_START_READ_ONLY: u32 = 4;
pub const TRANS_COR_NO_OPT: u32 = 0;
pub const TRANS_COR_AND_CHAIN: u32 = 1;
pub const TRANS_COR_AND_NO_CHAIN: u32 = 2;
pub const TRANS_COR_RELEASE: u32 = 4;
pub const TRANS_COR_NO_RELEASE: u32 = 8;

pub const MYSQLND_FETCH_ASSOC: u32 = 1;
pub const MYSQLND_FETCH_NUM: u32 = 2;

// enum_mysqlnd_client_option
pub const MYSQL_OPT_CONNECT_TIMEOUT: i64 = 0;
pub const MYSQL_OPT_COMPRESS: i64 = 1;
pub const MYSQL_OPT_NAMED_PIPE: i64 = 2;
pub const MYSQL_INIT_COMMAND: i64 = 3;
pub const MYSQL_READ_DEFAULT_FILE: i64 = 4;
pub const MYSQL_READ_DEFAULT_GROUP: i64 = 5;
pub const MYSQL_SET_CHARSET_DIR: i64 = 6;
pub const MYSQL_SET_CHARSET_NAME: i64 = 7;
pub const MYSQL_OPT_LOCAL_INFILE: i64 = 8;
pub const MYSQL_OPT_PROTOCOL: i64 = 9;
pub const MYSQL_SHARED_MEMORY_BASE_NAME: i64 = 10;
pub const MYSQL_OPT_READ_TIMEOUT: i64 = 11;
pub const MYSQL_OPT_WRITE_TIMEOUT: i64 = 12;
pub const MYSQL_OPT_SSL_VERIFY_SERVER_CERT: i64 = 21;
pub const MYSQL_OPT_CONNECT_ATTR_RESET: i64 = 32;
pub const MYSQL_OPT_CONNECT_ATTR_ADD: i64 = 33;
pub const MYSQL_OPT_CONNECT_ATTR_DELETE: i64 = 34;
pub const MYSQL_SERVER_PUBLIC_KEY: i64 = 35;
pub const MYSQL_ENABLE_CLEARTEXT_PLUGIN: i64 = 36;
pub const MYSQL_OPT_CAN_HANDLE_EXPIRED_PASSWORDS: i64 = 37;
pub const MYSQL_OPT_LOAD_DATA_LOCAL_DIR: i64 = 43;
pub const MYSQLND_OPT_INT_AND_FLOAT_NATIVE: i64 = 201;
pub const MYSQLND_OPT_NET_CMD_BUFFER_SIZE: i64 = 202;
pub const MYSQLND_OPT_NET_READ_BUFFER_SIZE: i64 = 203;
pub const MYSQLND_OPT_SSL_KEY: i64 = 204;
pub const MYSQLND_OPT_SSL_CERT: i64 = 205;
pub const MYSQLND_OPT_SSL_CA: i64 = 206;
pub const MYSQLND_OPT_SSL_CAPATH: i64 = 207;
pub const MYSQLND_OPT_SSL_CIPHER: i64 = 208;
pub const MYSQLND_OPT_SSL_PASSPHRASE: i64 = 209;
pub const MYSQLND_OPT_MAX_ALLOWED_PACKET: i64 = 210;
pub const MYSQLND_OPT_AUTH_PROTOCOL: i64 = 211;

pub const MYSQL_PROTOCOL_LAST: u32 = 5;

// enum_mysqlnd_field_types
pub const MYSQL_TYPE_DECIMAL: u8 = 0;
pub const MYSQL_TYPE_TINY: u8 = 1;
pub const MYSQL_TYPE_SHORT: u8 = 2;
pub const MYSQL_TYPE_LONG: u8 = 3;
pub const MYSQL_TYPE_FLOAT: u8 = 4;
pub const MYSQL_TYPE_DOUBLE: u8 = 5;
pub const MYSQL_TYPE_NULL: u8 = 6;
pub const MYSQL_TYPE_TIMESTAMP: u8 = 7;
pub const MYSQL_TYPE_LONGLONG: u8 = 8;
pub const MYSQL_TYPE_INT24: u8 = 9;
pub const MYSQL_TYPE_DATE: u8 = 10;
pub const MYSQL_TYPE_TIME: u8 = 11;
pub const MYSQL_TYPE_DATETIME: u8 = 12;
pub const MYSQL_TYPE_YEAR: u8 = 13;
pub const MYSQL_TYPE_NEWDATE: u8 = 14;
pub const MYSQL_TYPE_VARCHAR: u8 = 15;
pub const MYSQL_TYPE_BIT: u8 = 16;
pub const MYSQL_TYPE_VECTOR: u8 = 242;
pub const MYSQL_TYPE_JSON: u8 = 245;
pub const MYSQL_TYPE_NEWDECIMAL: u8 = 246;
pub const MYSQL_TYPE_ENUM: u8 = 247;
pub const MYSQL_TYPE_SET: u8 = 248;
pub const MYSQL_TYPE_TINY_BLOB: u8 = 249;
pub const MYSQL_TYPE_MEDIUM_BLOB: u8 = 250;
pub const MYSQL_TYPE_LONG_BLOB: u8 = 251;
pub const MYSQL_TYPE_BLOB: u8 = 252;
pub const MYSQL_TYPE_VAR_STRING: u8 = 253;
pub const MYSQL_TYPE_STRING: u8 = 254;
pub const MYSQL_TYPE_GEOMETRY: u8 = 255;

pub const MYSQL_OPTION_MULTI_STATEMENTS_ON: u16 = 0;
pub const MYSQL_OPTION_MULTI_STATEMENTS_OFF: u16 = 1;

pub const NOT_NULL_FLAG: u32 = 1;
pub const PRI_KEY_FLAG: u32 = 2;
pub const UNIQUE_KEY_FLAG: u32 = 4;
pub const MULTIPLE_KEY_FLAG: u32 = 8;
pub const BLOB_FLAG: u32 = 16;
pub const UNSIGNED_FLAG: u32 = 32;
pub const ZEROFILL_FLAG: u32 = 64;
pub const BINARY_FLAG: u32 = 128;
pub const ENUM_FLAG: u32 = 256;
pub const AUTO_INCREMENT_FLAG: u32 = 512;
pub const TIMESTAMP_FLAG: u32 = 1024;
pub const SET_FLAG: u32 = 2048;
pub const NO_DEFAULT_VALUE_FLAG: u32 = 4096;
pub const ON_UPDATE_NOW_FLAG: u32 = 8192;
pub const PART_KEY_FLAG: u32 = 16384;
pub const GROUP_FLAG: u32 = 32768;
pub const NUM_FLAG: u32 = 32768;

// enum php_mysqlnd_server_command
pub const COM_SLEEP: u8 = 0;
pub const COM_QUIT: u8 = 1;
pub const COM_INIT_DB: u8 = 2;
pub const COM_QUERY: u8 = 3;
pub const COM_FIELD_LIST: u8 = 4;
pub const COM_CREATE_DB: u8 = 5;
pub const COM_DROP_DB: u8 = 6;
pub const COM_REFRESH: u8 = 7;
pub const COM_SHUTDOWN: u8 = 8;
pub const COM_STATISTICS: u8 = 9;
pub const COM_PROCESS_INFO: u8 = 10;
pub const COM_CONNECT: u8 = 11;
pub const COM_PROCESS_KILL: u8 = 12;
pub const COM_DEBUG: u8 = 13;
pub const COM_PING: u8 = 14;
pub const COM_TIME: u8 = 15;
pub const COM_DELAYED_INSERT: u8 = 16;
pub const COM_CHANGE_USER: u8 = 17;
pub const COM_BINLOG_DUMP: u8 = 18;
pub const COM_TABLE_DUMP: u8 = 19;
pub const COM_CONNECT_OUT: u8 = 20;
pub const COM_REGISTER_SLAVE: u8 = 21;
pub const COM_STMT_PREPARE: u8 = 22;
pub const COM_STMT_EXECUTE: u8 = 23;
pub const COM_STMT_SEND_LONG_DATA: u8 = 24;
pub const COM_STMT_CLOSE: u8 = 25;
pub const COM_STMT_RESET: u8 = 26;
pub const COM_SET_OPTION: u8 = 27;
pub const COM_STMT_FETCH: u8 = 28;
pub const COM_DAEMON: u8 = 29;

/// mysqlnd_command_to_text
pub const COMMAND_TO_TEXT: [&str; 32] = [
    "SLEEP",
    "QUIT",
    "INIT_DB",
    "QUERY",
    "FIELD_LIST",
    "CREATE_DB",
    "DROP_DB",
    "REFRESH",
    "SHUTDOWN",
    "STATISTICS",
    "PROCESS_INFO",
    "CONNECT",
    "PROCESS_KILL",
    "DEBUG",
    "PING",
    "TIME",
    "DELAYED_INSERT",
    "CHANGE_USER",
    "BINLOG_DUMP",
    "TABLE_DUMP",
    "CONNECT_OUT",
    "REGISTER_SLAVE",
    "STMT_PREPARE",
    "STMT_EXECUTE",
    "STMT_SEND_LONG_DATA",
    "STMT_CLOSE",
    "STMT_RESET",
    "SET_OPTION",
    "STMT_FETCH",
    "DAEMON",
    "BINLOG_DUMP_GTID",
    "RESET_CONNECTION",
];

pub fn command_to_text(cmd: u8) -> &'static str {
    COMMAND_TO_TEXT
        .get(cmd as usize)
        .copied()
        .unwrap_or("UNKNOWN")
}

pub const MYSQLND_SERVER_GONE: &str = "MySQL server has gone away";
pub const MYSQLND_OUT_OF_SYNC: &str = "Commands out of sync; you can't run this command now";
pub const MYSQLND_OUT_OF_MEMORY: &str = "Out of memory";

/// enum mysqlnd_connection_state
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnState {
    Alloced = 0,
    Ready = 1,
    QuerySent = 2,
    SendingLoadData = 3,
    FetchingData = 4,
    NextResultPending = 5,
    QuitSent = 6,
}

/// enum_mysqlnd_query_type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryType {
    Upsert,
    Select,
    LoadLocal,
}

// ── Warnings raised by mysqlnd ────────────────────────────────────────

pub const E_WARNING: i64 = 2;
pub const E_NOTICE: i64 = 8;
pub const E_DEPRECATED: i64 = 8192;

/// A diagnostic raised inside mysqlnd: `docref` ones get the active
/// function's name as prefix (php_error_docref), others are emitted as is
/// (php_error).
#[derive(Clone, Debug)]
pub struct Warning {
    pub docref: bool,
    pub level: i64,
    pub msg: String,
}

thread_local! {
    static WARNINGS: RefCell<Vec<Warning>> = const { RefCell::new(Vec::new()) };
}

/// php_error_docref(NULL, level, ...)
pub fn php_error_docref(level: i64, msg: impl Into<String>) {
    WARNINGS.with(|w| {
        w.borrow_mut().push(Warning {
            docref: true,
            level,
            msg: msg.into(),
        })
    });
}

/// php_error(level, ...)
pub fn php_error(level: i64, msg: impl Into<String>) {
    WARNINGS.with(|w| {
        w.borrow_mut().push(Warning {
            docref: false,
            level,
            msg: msg.into(),
        })
    });
}

pub fn take_warnings() -> Vec<Warning> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

// ── MYSQLND_G() ───────────────────────────────────────────────────────

/// The mysqlnd INI settings (php_mysqlnd.c), refreshed by the mysqli layer.
#[derive(Clone, Debug)]
pub struct Globals {
    pub collect_statistics: bool,
    pub net_cmd_buffer_size: usize,
    pub net_read_buffer_size: usize,
    pub net_read_timeout: i64,
    pub sha256_server_public_key: Option<String>,
    pub default_socket_timeout: f64,
}

impl Default for Globals {
    fn default() -> Self {
        Globals {
            collect_statistics: true,
            net_cmd_buffer_size: MYSQLND_NET_CMD_BUFFER_MIN_SIZE,
            net_read_buffer_size: 32768,
            net_read_timeout: 86400,
            sha256_server_public_key: None,
            default_socket_timeout: 60.0,
        }
    }
}

thread_local! {
    static GLOBALS: RefCell<Globals> = RefCell::new(Globals::default());
}

pub fn globals() -> Globals {
    GLOBALS.with(|g| g.borrow().clone())
}

pub fn set_globals(g: Globals) {
    GLOBALS.with(|x| *x.borrow_mut() = g);
}

// ── Error info / upsert status ────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct ErrorListElem {
    pub error_no: u32,
    pub sqlstate: Vec<u8>,
    pub error: Vec<u8>,
}

/// MYSQLND_ERROR_INFO
#[derive(Clone, Debug, Default)]
pub struct ErrorInfo {
    pub error_no: u32,
    /// Empty after a reset (mysqlnd memsets it); at most 5 bytes.
    pub sqlstate: Vec<u8>,
    /// At most MYSQLND_ERRMSG_SIZE - 1 bytes (strlcpy).
    pub error: Vec<u8>,
    pub error_list: Vec<ErrorListElem>,
}

fn strlcpy(src: &[u8], size: usize) -> Vec<u8> {
    let end = src.iter().position(|&c| c == 0).unwrap_or(src.len());
    let n = end.min(size - 1);
    src[..n].to_vec()
}

impl ErrorInfo {
    /// mysqlnd_error_info::reset (SET_EMPTY_ERROR)
    pub fn reset(&mut self) {
        self.error_no = 0;
        self.error.clear();
        self.sqlstate.clear();
        self.error_list.clear();
    }

    /// SET_CLIENT_ERROR
    pub fn set(&mut self, err_no: u32, sqlstate: &[u8], error: &[u8]) {
        if err_no != 0 {
            self.error_no = err_no;
            self.sqlstate = strlcpy(sqlstate, 6);
            self.error = strlcpy(error, MYSQLND_ERRMSG_SIZE);
            self.error_list.push(ErrorListElem {
                error_no: err_no,
                sqlstate: strlcpy(sqlstate, 6),
                error: strlcpy(error, MYSQLND_ERRMSG_SIZE),
            });
        } else {
            self.reset();
        }
    }

    pub fn set_str(&mut self, err_no: u32, sqlstate: &str, error: &str) {
        self.set(err_no, sqlstate.as_bytes(), error.as_bytes());
    }

    /// COPY_CLIENT_ERROR(self, src)
    pub fn copy_from(&mut self, src: &ErrorInfo) {
        let (no, st, er) = (src.error_no, src.sqlstate.clone(), src.error.clone());
        self.set(no, &st, &er);
    }

    /// SET_OOM_ERROR
    pub fn set_oom(&mut self) {
        self.set_str(CR_OUT_OF_MEMORY, UNKNOWN_SQLSTATE, MYSQLND_OUT_OF_MEMORY);
    }

    /// The sqlstate as reported by mysqlnd's sqlstate() accessors.
    pub fn sqlstate_or_null(&self) -> Vec<u8> {
        if self.sqlstate.is_empty() {
            MYSQLND_SQLSTATE_NULL.as_bytes().to_vec()
        } else {
            self.sqlstate.clone()
        }
    }
}

/// MYSQLND_UPSERT_STATUS
#[derive(Clone, Copy, Debug, Default)]
pub struct UpsertStatus {
    pub warning_count: u32,
    pub server_status: u32,
    pub affected_rows: u64,
    pub last_insert_id: u64,
}

impl UpsertStatus {
    pub fn reset(&mut self) {
        *self = UpsertStatus::default();
    }
    pub fn set_affected_rows_to_error(&mut self) {
        self.affected_rows = u64::MAX;
    }
}

// ── Little endian helpers ─────────────────────────────────────────────

pub fn uint2korr(p: &[u8]) -> u32 {
    p.first().copied().unwrap_or(0) as u32 | (p.get(1).copied().unwrap_or(0) as u32) << 8
}

pub fn uint3korr(p: &[u8]) -> u32 {
    uint2korr(p) | (p.get(2).copied().unwrap_or(0) as u32) << 16
}

pub fn uint4korr(p: &[u8]) -> u32 {
    uint3korr(p) | (p.get(3).copied().unwrap_or(0) as u32) << 24
}

pub fn uint8korr(p: &[u8]) -> u64 {
    uint4korr(p) as u64 | (uint4korr(p.get(4..).unwrap_or(&[])) as u64) << 32
}

pub fn int2store(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&(x as u16).to_le_bytes());
}

pub fn int3store(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes()[..3]);
}

pub fn int4store(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

pub fn int8store(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// php_mysqlnd_net_store_length
pub fn net_store_length(v: &mut Vec<u8>, length: u64) {
    if length < 251 {
        v.push(length as u8);
    } else if length < 65536 {
        v.push(252);
        int2store(v, length as u32);
    } else if length < 16777216 {
        v.push(253);
        int3store(v, length as u32);
    } else {
        v.push(254);
        int8store(v, length);
    }
}

/// php_mysqlnd_net_store_length_size
pub fn net_store_length_size(length: u64) -> usize {
    if length < 251 {
        1
    } else if length < 65536 {
        3
    } else if length < 16777216 {
        4
    } else {
        9
    }
}

/// php_mysqlnd_net_field_length: reads a length-encoded integer at `*pos`
/// (32-bit variant: the 8-byte form only keeps the low 4 bytes).
pub fn net_field_length(buf: &[u8], pos: &mut usize) -> u64 {
    let p = *pos;
    let c = buf.get(p).copied().unwrap_or(0);
    if c < 251 {
        *pos += 1;
        return c as u64;
    }
    let rest = buf.get(p + 1..).unwrap_or(&[]);
    match c {
        251 => {
            *pos += 1;
            MYSQLND_NULL_LENGTH
        }
        252 => {
            *pos += 3;
            uint2korr(rest) as u64
        }
        253 => {
            *pos += 4;
            uint3korr(rest) as u64
        }
        _ => {
            *pos += 9;
            uint4korr(rest) as u64
        }
    }
}

/// php_mysqlnd_net_field_length_ll
pub fn net_field_length_ll(buf: &[u8], pos: &mut usize) -> u64 {
    let p = *pos;
    let c = buf.get(p).copied().unwrap_or(0);
    if c < 251 {
        *pos += 1;
        return c as u64;
    }
    let rest = buf.get(p + 1..).unwrap_or(&[]);
    match c {
        251 => {
            *pos += 1;
            MYSQLND_NULL_LENGTH
        }
        252 => {
            *pos += 3;
            uint2korr(rest) as u64
        }
        253 => {
            *pos += 4;
            uint3korr(rest) as u64
        }
        _ => {
            *pos += 9;
            uint8korr(rest)
        }
    }
}
