//! Connections (mysqlnd_connection.c, mysqlnd_commands.c and the protocol
//! command helpers of mysqlnd_wireprotocol.c).

use std::cell::RefCell;
use std::rc::Rc;

use super::charset::{self, Charset};
use super::result::{ResultSet, ResultType};
use super::stats::*;
use super::vio::{Pfc, Vio};
use super::*;

pub type ConnRef = Rc<RefCell<Conn>>;

/// MYSQLND_SESSION_OPTIONS
#[derive(Clone, Debug, Default)]
pub struct SessionOptions {
    pub flags: u32,
    pub init_commands: Vec<Vec<u8>>,
    pub cfg_file: Option<String>,
    pub cfg_section: Option<String>,
    pub auth_protocol: Option<String>,
    /// connect attributes, in insertion order
    pub connect_attr: Option<Vec<(Vec<u8>, Vec<u8>)>>,
    pub protocol: u32,
    pub charset_name: Option<String>,
    pub max_allowed_packet: u32,
    pub int_and_float_native: bool,
    pub local_infile_directory: Option<String>,
}

/// MYSQLND_CONN_DATA
pub struct Conn {
    pub vio: Vio,
    pub pfc: Pfc,
    pub state: ConnState,
    pub error_info: ErrorInfo,
    pub upsert: UpsertStatus,
    pub options: SessionOptions,
    pub stats: Stats,
    pub thread_id: u64,
    pub server_version: Option<Vec<u8>>,
    pub host_info: Option<Vec<u8>>,
    pub protocol_version: u32,
    pub server_capabilities: u32,
    pub client_flag: u32,
    pub charset: Option<&'static Charset>,
    pub greet_charset: Option<&'static Charset>,
    pub scheme: Option<String>,
    pub port: u32,
    pub authentication_plugin_data: Vec<u8>,
    pub last_message: Option<Vec<u8>>,
    pub field_count: u32,
    pub last_query_type: QueryType,
    pub current_result: Option<Box<ResultSet>>,
    pub persistent: bool,
    pub client_api_capabilities: usize,
}

impl Conn {
    /// mysqlnd_object_factory::get_connection
    pub fn new(persistent: bool) -> Conn {
        let mut upsert = UpsertStatus::default();
        upsert.set_affected_rows_to_error();
        Conn {
            vio: Vio::new(),
            pfc: Pfc::new(),
            state: ConnState::Alloced,
            error_info: ErrorInfo::default(),
            upsert,
            options: SessionOptions::default(),
            stats: Stats::default(),
            thread_id: 0,
            server_version: None,
            host_info: None,
            protocol_version: 0,
            server_capabilities: 0,
            client_flag: 0,
            charset: None,
            greet_charset: None,
            scheme: None,
            port: 0,
            authentication_plugin_data: Vec::new(),
            last_message: None,
            field_count: 0,
            last_query_type: QueryType::Upsert,
            current_result: None,
            persistent,
            client_api_capabilities: 0,
        }
    }

    /// mysqlnd_conn_data::free_contents
    pub fn free_contents(&mut self) {
        self.current_result = None;
        self.pfc.free_contents();
        self.vio.close_stream();
        self.scheme = None;
        self.server_version = None;
        self.host_info = None;
        self.authentication_plugin_data.clear();
        self.last_message = None;
        self.charset = None;
        self.greet_charset = None;
    }

    // ── accessors ─────────────────────────────────────────────────────

    pub fn get_server_version(&self) -> u64 {
        let v = match &self.server_version {
            Some(v) => v,
            None => return 0,
        };
        // strtol x3 separated by single characters
        let mut p = 0usize;
        let num = |p: &mut usize| -> i64 {
            let start = *p;
            let mut neg = false;
            let mut i = *p;
            while i < v.len() && (v[i] == b' ' || v[i] == b'\t') {
                i += 1;
            }
            if i < v.len() && (v[i] == b'-' || v[i] == b'+') {
                neg = v[i] == b'-';
                i += 1;
            }
            let ds = i;
            let mut n: i64 = 0;
            while i < v.len() && v[i].is_ascii_digit() {
                n = n.saturating_mul(10).saturating_add((v[i] - b'0') as i64);
                i += 1;
            }
            if i == ds {
                *p = start;
                return 0;
            }
            *p = i;
            if neg { -n } else { n }
        };
        let major = num(&mut p);
        p += 1;
        let minor = num(&mut p);
        p += 1;
        let patch = num(&mut p);
        (major * 10000 + minor * 100 + patch) as u64
    }

    pub fn charset_name(&self) -> &'static str {
        self.charset.map(|c| c.name).unwrap_or("")
    }

    pub fn sqlstate(&self) -> Vec<u8> {
        self.error_info.sqlstate_or_null()
    }

    pub fn more_results(&self) -> bool {
        self.upsert.server_status & SERVER_MORE_RESULTS_EXISTS != 0
    }

    // ── mysqlnd_protocol::send_command & co ───────────────────────────

    /// mysqlnd_protocol::send_command
    pub fn send_command(&mut self, command: u8, arg: &[u8], silent: bool) -> bool {
        match self.state {
            ConnState::Ready => {}
            ConnState::QuitSent => {
                self.error_info.set_str(
                    CR_SERVER_GONE_ERROR,
                    UNKNOWN_SQLSTATE,
                    MYSQLND_SERVER_GONE,
                );
                return false;
            }
            _ => {
                self.error_info.set_str(
                    CR_COMMANDS_OUT_OF_SYNC,
                    UNKNOWN_SQLSTATE,
                    MYSQLND_OUT_OF_SYNC,
                );
                return false;
            }
        }
        self.upsert.set_affected_rows_to_error();
        self.error_info.reset();
        if command >= 1 {
            self.stats.inc(STAT_COM_QUIT + command as usize - 1);
        }
        if self.write_cmd(command, arg) == 0 {
            if !silent && self.error_info.error_no != CR_SERVER_GONE_ERROR {
                php_error(
                    E_WARNING,
                    format!(
                        "Error while sending {} packet. PID={}",
                        command_to_text(command),
                        std::process::id()
                    ),
                );
            }
            self.state = ConnState::QuitSent;
            self.send_close();
            return false;
        }
        true
    }

    /// mysqlnd_protocol::send_command_handle_OK
    pub fn send_command_handle_ok(&mut self, ignore_upsert_status: bool) -> bool {
        let ok = match self.read_ok() {
            Ok(p) => p,
            Err(()) => {
                if self.error_info.error_no != CR_SERVER_GONE_ERROR {
                    self.error_info.set_str(
                        CR_MALFORMED_PACKET,
                        UNKNOWN_SQLSTATE,
                        "Malformed packet",
                    );
                }
                return false;
            }
        };
        if ok.field_count == 0xFF {
            self.error_info.set(ok.error_no, &ok.sqlstate, &ok.error);
            self.upsert.server_status &= !SERVER_MORE_RESULTS_EXISTS;
            self.upsert.set_affected_rows_to_error();
            return false;
        }
        self.last_message = ok.message;
        if !ignore_upsert_status {
            self.upsert.reset();
            self.upsert.warning_count = ok.warning_count;
            self.upsert.server_status = ok.server_status;
            self.upsert.affected_rows = ok.affected_rows;
            self.upsert.last_insert_id = ok.last_insert_id;
        }
        true
    }

    /// mysqlnd_protocol::send_command_handle_EOF
    pub fn send_command_handle_eof(&mut self) -> bool {
        match self.read_eof() {
            Err(()) => {
                self.error_info
                    .set_str(CR_MALFORMED_PACKET, UNKNOWN_SQLSTATE, "Malformed packet");
                false
            }
            Ok(r) => {
                if r.field_count == 0xFF {
                    self.error_info.set(r.error_no, &r.sqlstate, &r.error);
                    self.upsert.set_affected_rows_to_error();
                } else if r.field_count != 0xFE {
                    self.error_info.set_str(
                        CR_MALFORMED_PACKET,
                        UNKNOWN_SQLSTATE,
                        "Malformed packet",
                    );
                    php_error_docref(
                        E_WARNING,
                        format!(
                            "EOF packet expected, field count wasn't 0xFE but 0x{:2X}",
                            r.field_count
                        ),
                    );
                }
                // the C returns the PACKET_READ status
                true
            }
        }
    }

    /// mysqlnd_protocol::send_command_handle_response
    pub fn send_command_handle_response(
        &mut self,
        ok_packet: bool,
        silent: bool,
        command: u8,
        ignore_upsert_status: bool,
    ) -> bool {
        let ret = if ok_packet {
            self.send_command_handle_ok(ignore_upsert_status)
        } else {
            self.send_command_handle_eof()
        };
        if !silent && self.error_info.error_no == CR_MALFORMED_PACKET {
            php_error_docref(
                E_WARNING,
                format!(
                    "Error while reading {}'s response packet. PID={}",
                    command_to_text(command),
                    std::process::id()
                ),
            );
        }
        ret
    }

    // ── mysqlnd_commands.c ────────────────────────────────────────────

    pub fn cmd_set_option(&mut self, option: u16) -> bool {
        let mut buf = Vec::new();
        int2store(&mut buf, option as u32);
        self.send_command(COM_SET_OPTION, &buf, false)
            && self.send_command_handle_response(false, false, COM_SET_OPTION, true)
    }

    pub fn cmd_debug(&mut self) -> bool {
        self.send_command(COM_DEBUG, &[], false)
            && self.send_command_handle_response(false, false, COM_DEBUG, true)
    }

    pub fn cmd_init_db(&mut self, db: &[u8]) -> bool {
        let mut ret = self.send_command(COM_INIT_DB, db, false);
        if ret {
            ret = self.send_command_handle_response(true, false, COM_INIT_DB, true);
        }
        self.upsert.set_affected_rows_to_error();
        ret
    }

    pub fn cmd_ping(&mut self) -> bool {
        let mut ret = self.send_command(COM_PING, &[], true);
        if ret {
            ret = self.send_command_handle_response(true, true, COM_PING, true);
        }
        self.upsert.set_affected_rows_to_error();
        ret
    }

    pub fn cmd_statistics(&mut self) -> Option<Vec<u8>> {
        if !self.send_command(COM_STATISTICS, &[], false) {
            return None;
        }
        self.read_stats().ok()
    }

    pub fn cmd_process_kill(&mut self, process_id: u32, read_response: bool) -> bool {
        let mut buf = Vec::new();
        int4store(&mut buf, process_id);
        let mut ret = self.send_command(COM_PROCESS_KILL, &buf, false);
        if ret && read_response {
            ret = self.send_command_handle_response(true, false, COM_PROCESS_KILL, true);
        }
        if read_response {
            self.upsert.set_affected_rows_to_error();
        } else if ret {
            self.state = ConnState::QuitSent;
            self.send_close();
        }
        ret
    }

    pub fn cmd_refresh(&mut self, options: u8) -> bool {
        self.send_command(COM_REFRESH, &[options], false)
            && self.send_command_handle_response(true, false, COM_REFRESH, true)
    }

    pub fn cmd_quit(&mut self) -> bool {
        self.send_command(COM_QUIT, &[], true)
    }

    pub fn cmd_query(&mut self, query: &[u8]) -> bool {
        let ret = self.send_command(COM_QUERY, query, false);
        if ret {
            self.state = ConnState::QuerySent;
        }
        ret
    }

    /// mysqlnd_command::reap_result
    pub fn cmd_reap_result(&mut self) -> bool {
        let state = self.state;
        if state <= ConnState::Ready || state == ConnState::QuitSent {
            php_error_docref(E_WARNING, "Connection not opened, clear or has been closed");
            return false;
        }
        self.query_read_result_set_header(None)
    }

    // ── mysqlnd_conn_data methods ─────────────────────────────────────

    /// mysqlnd_conn_data::query
    pub fn query(&mut self, query: &[u8]) -> bool {
        let ret = self.cmd_query(query) && self.cmd_reap_result();
        if ret && self.last_query_type == QueryType::Upsert && self.upsert.affected_rows != 0 {
            let n = self.upsert.affected_rows;
            self.stats.inc_v(STAT_ROWS_AFFECTED_NORMAL, n);
        }
        ret
    }

    /// mysqlnd_conn_data::send_query
    pub fn send_query(&mut self, query: &[u8]) -> bool {
        self.cmd_query(query)
    }

    /// mysqlnd_conn_data::reap_query
    pub fn reap_query(&mut self) -> bool {
        self.cmd_reap_result()
    }

    /// mysqlnd_query_read_result_set_header; `ps` is the statement's result
    /// slot (stmt->result) for prepared statements.
    pub fn query_read_result_set_header(
        &mut self,
        mut ps: Option<&mut Option<Box<ResultSet>>>,
    ) -> bool {
        let ret;
        self.upsert.set_affected_rows_to_error();
        let rset_header = match self.read_rset_header() {
            Ok(h) => h,
            Err(()) => {
                if self.error_info.error_no != CR_SERVER_GONE_ERROR
                    && self.error_info.error_no != CR_CLIENT_INTERACTION_TIMEOUT
                {
                    php_error_docref(E_WARNING, "Error reading result set's header");
                }
                return false;
            }
        };
        if rset_header.error_info.error_no != 0 {
            self.upsert.server_status &= !SERVER_MORE_RESULTS_EXISTS;
            self.error_info.copy_from(&rset_header.error_info);
            self.state = ConnState::Ready;
            return false;
        }
        self.error_info.error_no = 0;
        match rset_header.field_count {
            MYSQLND_NULL_LENGTH => {
                self.last_query_type = QueryType::LoadLocal;
                self.field_count = 0;
                self.state = ConnState::SendingLoadData;
                let filename = rset_header.info_or_local_file.clone().unwrap_or_default();
                let (r, is_warning) = super::result::handle_local_infile(self, &filename);
                ret = r;
                self.state = if ret || is_warning {
                    ConnState::Ready
                } else {
                    ConnState::QuitSent
                };
                self.stats.inc(STAT_NON_RSET_QUERY);
            }
            0 => {
                self.last_query_type = QueryType::Upsert;
                self.field_count = 0;
                self.upsert.reset();
                self.upsert.warning_count = rset_header.warning_count;
                self.upsert.server_status = rset_header.server_status;
                self.upsert.affected_rows = rset_header.affected_rows;
                self.upsert.last_insert_id = rset_header.last_insert_id;
                self.last_message = rset_header.info_or_local_file.clone();
                self.state = if self.upsert.server_status & SERVER_MORE_RESULTS_EXISTS != 0 {
                    ConnState::NextResultPending
                } else {
                    ConnState::Ready
                };
                ret = true;
                self.stats.inc(STAT_NON_RSET_QUERY);
            }
            n => {
                self.last_message = None;
                self.stats.inc(STAT_RSET_QUERY);
                self.upsert.reset();
                self.upsert.set_affected_rows_to_error();
                self.last_query_type = QueryType::Select;
                self.state = ConnState::FetchingData;
                self.field_count = n as u32;
                let n32 = n as u32;
                let mut result = match ps.as_mut() {
                    None => Box::new(ResultSet::new(n32)),
                    Some(slot) => match slot.take() {
                        Some(r) if r.field_count == n32 => r,
                        Some(mut r) => {
                            r.free_result_with(self, true);
                            Box::new(ResultSet::new(n32))
                        }
                        None => Box::new(ResultSet::new(n32)),
                    },
                };
                if !result.read_result_metadata(self) {
                    match ps {
                        None => self.current_result = None,
                        // For PS, we leave them in Prepared state
                        Some(slot) => *slot = Some(result),
                    }
                    return false;
                }
                match self.read_eof() {
                    Err(()) => {
                        result.free_result_contents();
                        match ps {
                            None => self.current_result = None,
                            Some(slot) => *slot = None,
                        }
                        return false;
                    }
                    Ok(eof) => {
                        self.upsert.warning_count = eof.warning_count;
                        self.upsert.server_status = eof.server_status;
                        let st = if eof.server_status & SERVER_QUERY_NO_GOOD_INDEX_USED != 0 {
                            STAT_BAD_INDEX_USED
                        } else if eof.server_status & SERVER_QUERY_NO_INDEX_USED != 0 {
                            STAT_NO_INDEX_USED
                        } else if eof.server_status & SERVER_QUERY_WAS_SLOW != 0 {
                            STAT_QUERY_WAS_SLOW
                        } else {
                            STAT_LAST
                        };
                        self.stats.inc(st);
                    }
                }
                if let Some(slot) = ps {
                    *slot = Some(result);
                    return true;
                }
                self.current_result = Some(result);
                ret = true;
            }
        }
        ret
    }

    /// mysqlnd_conn_data::next_result
    pub fn next_result(&mut self) -> bool {
        self.error_info.reset();
        if self.state != ConnState::NextResultPending {
            return false;
        }
        self.upsert.set_affected_rows_to_error();
        if !self.query_read_result_set_header(None) {
            if self.error_info.error_no == 0 {
                php_error_docref(
                    E_WARNING,
                    format!("Serious error. PID={}", std::process::id()),
                );
                self.state = ConnState::QuitSent;
                self.send_close();
            }
            return false;
        }
        if self.last_query_type == QueryType::Upsert && self.upsert.affected_rows != 0 {
            let n = self.upsert.affected_rows;
            self.stats.inc_v(STAT_ROWS_AFFECTED_NORMAL, n);
        }
        true
    }

    /// mysqlnd_conn_data::escape_string
    pub fn escape_string(&self, s: &[u8]) -> Vec<u8> {
        let cs = self
            .charset
            .or_else(|| charset::find_charset_name("latin1"))
            .expect("latin1");
        if self.upsert.server_status & SERVER_STATUS_NO_BACKSLASH_ESCAPES != 0 {
            charset::escape_quotes(cs, s)
        } else {
            charset::escape_slashes(cs, s)
        }
    }

    /// mysqlnd_conn_data::set_charset
    pub fn set_charset(&mut self, csname: &str) -> bool {
        let cs = match charset::find_charset_name(csname) {
            Some(c) => c,
            None => {
                self.error_info.set_str(
                    CR_CANT_FIND_CHARSET,
                    UNKNOWN_SQLSTATE,
                    "Invalid character set was provided",
                );
                return false;
            }
        };
        let q = format!("SET NAMES {}", csname);
        if !self.query(q.as_bytes()) || self.error_info.error_no != 0 {
            false
        } else {
            self.charset = Some(cs);
            true
        }
    }

    pub fn select_db(&mut self, db: &[u8]) -> bool {
        self.cmd_init_db(db)
    }

    pub fn ping(&mut self) -> bool {
        self.cmd_ping()
    }

    pub fn statistic(&mut self) -> Option<Vec<u8>> {
        self.cmd_statistics()
    }

    pub fn kill(&mut self, pid: u32) -> bool {
        let read_response = pid as u64 != self.thread_id;
        self.cmd_process_kill(pid, read_response)
    }

    pub fn refresh(&mut self, options: u8) -> bool {
        self.cmd_refresh(options)
    }

    pub fn dump_debug_info(&mut self) -> bool {
        self.cmd_debug()
    }

    pub fn set_server_option(&mut self, option: u16) -> bool {
        self.cmd_set_option(option)
    }

    pub fn set_autocommit(&mut self, mode: bool) -> bool {
        self.query(if mode {
            b"SET AUTOCOMMIT=1"
        } else {
            b"SET AUTOCOMMIT=0"
        })
    }

    /// mysqlnd_conn_data::tx_cor_options_to_string
    fn tx_cor_options_to_string(mode: u32) -> String {
        let mut s = String::new();
        let add = |s: &mut String, t: &str| {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(t);
        };
        if mode & TRANS_COR_AND_CHAIN != 0 && mode & TRANS_COR_AND_NO_CHAIN == 0 {
            add(&mut s, "AND CHAIN");
        } else if mode & TRANS_COR_AND_NO_CHAIN != 0 && mode & TRANS_COR_AND_CHAIN == 0 {
            add(&mut s, "AND NO CHAIN");
        }
        if mode & TRANS_COR_RELEASE != 0 && mode & TRANS_COR_NO_RELEASE == 0 {
            add(&mut s, "RELEASE");
        } else if mode & TRANS_COR_NO_RELEASE != 0 && mode & TRANS_COR_RELEASE == 0 {
            add(&mut s, "NO RELEASE");
        }
        s
    }

    /// mysqlnd_escape_string_for_tx_name_in_comment
    fn escape_tx_name(name: Option<&[u8]>) -> Option<Vec<u8>> {
        let name = name?;
        let mut ret = b" /*".to_vec();
        let mut warned = false;
        for &v in name {
            if v == 0 {
                break;
            }
            if v.is_ascii_alphanumeric() || v == b'-' || v == b'_' || v == b' ' || v == b'=' {
                ret.push(v);
            } else if !warned {
                php_error_docref(
                    E_WARNING,
                    "Transaction name has been truncated, since it can only contain the A-Z, a-z, 0-9, \"\\\", \"-\", \"_\", and \"=\" characters",
                );
                warned = true;
            }
        }
        ret.extend_from_slice(b"*/");
        Some(ret)
    }

    /// mysqlnd_conn_data::tx_commit_or_rollback
    pub fn tx_commit_or_rollback(&mut self, commit: bool, flags: u32, name: Option<&[u8]>) -> bool {
        let tmp = Self::tx_cor_options_to_string(flags);
        let name_esc = Self::escape_tx_name(name);
        let mut q: Vec<u8> = if commit {
            b"COMMIT".to_vec()
        } else {
            b"ROLLBACK".to_vec()
        };
        if let Some(n) = &name_esc {
            q.extend_from_slice(n);
        }
        q.push(b' ');
        q.extend_from_slice(tmp.as_bytes());
        self.query(&q)
    }

    /// mysqlnd_conn_data::tx_begin
    pub fn tx_begin(&mut self, mode: u32, name: Option<&[u8]>) -> bool {
        let mut tmp = String::new();
        if mode & TRANS_START_WITH_CONSISTENT_SNAPSHOT != 0 {
            tmp.push_str("WITH CONSISTENT SNAPSHOT");
        }
        if mode & TRANS_START_READ_WRITE != 0 {
            if !tmp.is_empty() {
                tmp.push_str(", ");
            }
            tmp.push_str("READ WRITE");
        } else if mode & TRANS_START_READ_ONLY != 0 {
            if !tmp.is_empty() {
                tmp.push_str(", ");
            }
            tmp.push_str("READ ONLY");
        }
        let name_esc = Self::escape_tx_name(name);
        let mut q = b"START TRANSACTION".to_vec();
        if let Some(n) = &name_esc {
            q.extend_from_slice(n);
        }
        q.push(b' ');
        q.extend_from_slice(tmp.as_bytes());
        let ret = self.query(&q);
        if ret
            && mode & (TRANS_START_READ_WRITE | TRANS_START_READ_ONLY) != 0
            && self.error_info.error_no == 1064
        {
            self.error_info.set_str(
                CR_NOT_IMPLEMENTED,
                UNKNOWN_SQLSTATE,
                "This server version doesn't support 'READ WRITE' and 'READ ONLY'. Minimum 5.6.5 is required",
            );
        }
        ret
    }

    pub fn tx_savepoint(&mut self, name: Option<&[u8]>) -> bool {
        let name = match name {
            Some(n) => n,
            None => {
                self.error_info.set_str(
                    CR_UNKNOWN_ERROR,
                    UNKNOWN_SQLSTATE,
                    "Savepoint name not provided",
                );
                return false;
            }
        };
        let mut q = b"SAVEPOINT `".to_vec();
        q.extend_from_slice(cstr(name));
        q.push(b'`');
        self.query(&q)
    }

    pub fn tx_savepoint_release(&mut self, name: Option<&[u8]>) -> bool {
        let name = match name {
            Some(n) => n,
            None => {
                self.error_info.set_str(
                    CR_UNKNOWN_ERROR,
                    UNKNOWN_SQLSTATE,
                    "Savepoint name not provided",
                );
                return false;
            }
        };
        let mut q = b"RELEASE SAVEPOINT `".to_vec();
        q.extend_from_slice(cstr(name));
        q.push(b'`');
        self.query(&q)
    }

    /// mysqlnd_conn_data::set_client_option. `value` is the option's
    /// argument (strings, or the number as decimal string for ints).
    pub fn set_client_option(&mut self, option: i64, value: OptValue) -> bool {
        match option {
            MYSQL_OPT_READ_TIMEOUT => self.vio.timeout_read = value.uint(),
            MYSQL_OPT_WRITE_TIMEOUT => {}
            MYSQLND_OPT_SSL_KEY => self.vio.ssl_key = value.string(),
            MYSQLND_OPT_SSL_CERT => self.vio.ssl_cert = value.string(),
            MYSQLND_OPT_SSL_CA => self.vio.ssl_ca = value.string(),
            MYSQLND_OPT_SSL_CAPATH => self.vio.ssl_capath = value.string(),
            MYSQLND_OPT_SSL_CIPHER => self.vio.ssl_cipher = value.string(),
            MYSQLND_OPT_SSL_PASSPHRASE => self.vio.ssl_passphrase = value.string(),
            MYSQL_OPT_SSL_VERIFY_SERVER_CERT => {
                self.vio.ssl_verify_peer = match value.uint() {
                    1 => super::vio::SslPeer::Verify,
                    2 => super::vio::SslPeer::DontVerify,
                    _ => super::vio::SslPeer::Default,
                }
            }
            MYSQL_OPT_CONNECT_TIMEOUT => self.vio.timeout_connect = value.uint(),
            MYSQLND_OPT_NET_READ_BUFFER_SIZE => {
                self.vio.net_read_buffer_size = value.uint() as usize
            }
            MYSQLND_OPT_NET_CMD_BUFFER_SIZE => {
                let v = value.uint() as usize;
                if v < MYSQLND_NET_CMD_BUFFER_MIN_SIZE {
                    return false;
                }
                self.pfc.cmd_buffer_length = v;
            }
            MYSQL_OPT_COMPRESS => {
                self.pfc.flags |= super::vio::MYSQLND_PROTOCOL_FLAG_USE_COMPRESSION;
            }
            MYSQL_SERVER_PUBLIC_KEY => self.pfc.sha256_server_public_key = value.string(),
            MYSQLND_OPT_INT_AND_FLOAT_NATIVE => {
                self.options.int_and_float_native = value.uint() != 0
            }
            MYSQL_OPT_LOCAL_INFILE => {
                if value.uint() != 0 {
                    self.options.flags |= CLIENT_LOCAL_FILES;
                } else {
                    self.options.flags &= !CLIENT_LOCAL_FILES;
                }
            }
            MYSQL_OPT_LOAD_DATA_LOCAL_DIR => {
                self.options.local_infile_directory = match value.string() {
                    Some(s) if !s.is_empty() => Some(s),
                    _ => None,
                }
            }
            MYSQL_INIT_COMMAND => {
                self.options
                    .init_commands
                    .push(value.bytes().unwrap_or_default());
            }
            MYSQL_READ_DEFAULT_FILE | MYSQL_READ_DEFAULT_GROUP => {}
            MYSQL_SET_CHARSET_NAME => {
                let name = value.string().unwrap_or_default();
                if charset::find_charset_name(&name).is_none() {
                    self.error_info.set_str(
                        CR_CANT_FIND_CHARSET,
                        UNKNOWN_SQLSTATE,
                        "Unknown character set",
                    );
                    return false;
                }
                self.options.charset_name = Some(name);
            }
            MYSQL_OPT_NAMED_PIPE => self.options.protocol = 3,
            MYSQL_OPT_PROTOCOL => {
                let v = value.uint();
                if v < MYSQL_PROTOCOL_LAST {
                    self.options.protocol = v;
                }
            }
            MYSQLND_OPT_MAX_ALLOWED_PACKET => {
                let v = value.uint();
                if v > (1 << 16) {
                    self.options.max_allowed_packet = v;
                }
            }
            MYSQLND_OPT_AUTH_PROTOCOL => self.options.auth_protocol = value.string(),
            MYSQL_OPT_CAN_HANDLE_EXPIRED_PASSWORDS => {
                if value.uint() != 0 {
                    self.options.flags |= CLIENT_CAN_HANDLE_EXPIRED_PASSWORDS;
                } else {
                    self.options.flags &= !CLIENT_CAN_HANDLE_EXPIRED_PASSWORDS;
                }
            }
            MYSQL_OPT_CONNECT_ATTR_RESET => {
                if let Some(a) = self.options.connect_attr.as_mut() {
                    a.clear();
                }
            }
            MYSQL_OPT_CONNECT_ATTR_DELETE => {
                if let (Some(a), Some(k)) = (self.options.connect_attr.as_mut(), value.bytes()) {
                    a.retain(|(key, _)| *key != k);
                }
            }
            _ => return false,
        }
        true
    }

    /// mysqlnd_conn_data::set_client_option_2d (MYSQL_OPT_CONNECT_ATTR_ADD)
    pub fn connect_attr_add(&mut self, key: &[u8], value: &[u8]) {
        let a = self.options.connect_attr.get_or_insert_with(Vec::new);
        if let Some(e) = a.iter_mut().find(|(k, _)| k == key) {
            e.1 = value.to_vec();
        } else {
            a.push((key.to_vec(), value.to_vec()));
        }
    }

    /// mysqlnd_conn_data::get_updated_connect_flags
    fn get_updated_connect_flags(&self, mut mysql_flags: u32) -> u32 {
        mysql_flags |= MYSQLND_CAPABILITIES;
        mysql_flags |= self.options.flags;
        if self.pfc.flags & super::vio::MYSQLND_PROTOCOL_FLAG_USE_COMPRESSION != 0 {
            mysql_flags |= CLIENT_COMPRESS;
        }
        if self.vio.ssl_key.is_some()
            || self.vio.ssl_cert.is_some()
            || self.vio.ssl_ca.is_some()
            || self.vio.ssl_capath.is_some()
            || self.vio.ssl_cipher.is_some()
        {
            mysql_flags |= CLIENT_SSL;
        }
        if self
            .options
            .connect_attr
            .as_ref()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
        {
            mysql_flags |= CLIENT_CONNECT_ATTRS;
        }
        mysql_flags
    }

    /// mysqlnd_conn_data::get_scheme: (transport, unix_socket)
    fn get_scheme(hostname: &[u8], socket: &mut Option<Vec<u8>>, mut port: u32) -> (String, bool) {
        if hostname.eq_ignore_ascii_case(b"localhost") {
            if socket.is_none() {
                *socket = Some(b"/tmp/mysql.sock".to_vec());
            }
            let s = String::from_utf8_lossy(socket.as_ref().unwrap()).into_owned();
            return (format!("unix://{}", s), true);
        }
        if port == 0 {
            port = 3306;
        }
        let h = String::from_utf8_lossy(hostname).into_owned();
        let is_ipv6 = match h.find(':') {
            Some(p) => h[p + 1..].contains(':'),
            None => false,
        };
        if !h.starts_with('[') && is_ipv6 {
            return (format!("tcp://[{}]:{}", h, port), false);
        }
        let has_port = if h.starts_with('[') {
            match h.find(']') {
                Some(p) => h.as_bytes().get(p + 1) == Some(&b':'),
                None => false,
            }
        } else {
            h.contains(':')
        };
        if has_port {
            (format!("tcp://{}", h), false)
        } else {
            (format!("tcp://{}:{}", h, port), false)
        }
    }

    /// mysqlnd_conn::connect + mysqlnd_conn_data::connect
    #[allow(clippy::too_many_arguments)]
    pub fn connect(
        &mut self,
        hostname: Option<&[u8]>,
        username: Option<&[u8]>,
        password: Option<&[u8]>,
        database: Option<&[u8]>,
        port: u32,
        socket: Option<&[u8]>,
        mut mysql_flags: u32,
    ) -> bool {
        // mysqlnd_conn::connect
        self.connect_attr_add(b"_client_name", b"mysqlnd");
        if let Some(h) = hostname {
            if !h.is_empty() {
                self.connect_attr_add(b"_server_host", h);
            }
        }
        let mut reconnect = false;
        let mut saved_compression = false;
        self.error_info.reset();
        self.upsert.set_affected_rows_to_error();
        if self.state > ConnState::Alloced {
            if self.state < ConnState::QuitSent {
                self.stats.inc(STAT_CLOSE_IMPLICIT);
                reconnect = true;
                self.send_close();
            }
            self.free_contents();
            if self.pfc.compressed {
                saved_compression = true;
                self.pfc.compressed = false;
            }
            if self.pfc.ssl {
                self.pfc.ssl = false;
            }
        } else {
            self.set_client_option(
                MYSQLND_OPT_MAX_ALLOWED_PACKET,
                OptValue::Int(MYSQLND_ASSEMBLED_PACKET_MAX_SIZE as i64),
            );
        }
        let hostname: Vec<u8> = match hostname {
            Some(h) if !h.is_empty() => h.to_vec(),
            _ => b"localhost".to_vec(),
        };
        let username = username.unwrap_or(b"").to_vec();
        let password = password.unwrap_or(b"").to_vec();
        let database: Vec<u8> = match database {
            Some(d) if !d.is_empty() && d[0] != 0 => d.to_vec(),
            _ => Vec::new(),
        };
        if !database.is_empty() {
            mysql_flags |= CLIENT_CONNECT_WITH_DB;
        }
        let mut socket: Option<Vec<u8>> = socket.filter(|s| !s.is_empty()).map(|s| s.to_vec());
        let (transport, unix_socket) = Self::get_scheme(&hostname, &mut socket, port);
        mysql_flags = self.get_updated_connect_flags(mysql_flags);

        let ok = self.connect_handshake(&transport, &username, &password, &database, mysql_flags);
        if ok {
            self.state = ConnState::Ready;
            if saved_compression {
                self.pfc.compressed = true;
            }
            self.pfc.compressed = mysql_flags & CLIENT_COMPRESS != 0;
            self.scheme = Some(transport.clone());
            self.port = port;
            if !unix_socket {
                let mut hi = hostname.clone();
                hi.extend_from_slice(b" via TCP/IP");
                self.host_info = Some(hi);
            } else {
                self.host_info = Some(b"Localhost via UNIX socket".to_vec());
            }
            self.error_info.reset();
            if self.execute_init_commands() {
                self.stats
                    .inc2(STAT_CONNECT_SUCCESS, 1, STAT_OPENED_CONNECTIONS, 1);
                if reconnect {
                    global_inc(STAT_RECONNECT);
                }
                if self.persistent {
                    self.stats.inc2(
                        STAT_PCONNECT_SUCCESS,
                        1,
                        STAT_OPENED_PERSISTENT_CONNECTIONS,
                        1,
                    );
                }
                return true;
            }
        }
        // err:
        if self.error_info.error_no == 0 {
            let t = if !transport.is_empty() {
                transport.clone()
            } else {
                self.scheme.clone().unwrap_or_default()
            };
            let msg = format!("Unknown error while trying to connect via {}", t);
            self.error_info
                .set_str(CR_CONNECTION_ERROR, UNKNOWN_SQLSTATE, &msg);
        }
        self.free_contents();
        self.stats.inc(STAT_CONNECT_FAILURE);
        false
    }

    /// mysqlnd_conn_data::connect_handshake + mysqlnd_command::handshake
    fn connect_handshake(
        &mut self,
        scheme: &str,
        user: &[u8],
        passwd: &[u8],
        db: &[u8],
        mysql_flags: u32,
    ) -> bool {
        if !self.vio.connect(scheme, &mut self.error_info) {
            return false;
        }
        self.pfc.reset();
        let greet = match self.read_greet() {
            Ok(g) => g,
            Err(()) => {
                php_error_docref(
                    E_WARNING,
                    format!(
                        "Error while reading greeting packet. PID={}",
                        std::process::id()
                    ),
                );
                self.server_capabilities = 0;
                return false;
            }
        };
        if greet.error_no != 0 {
            self.error_info
                .set(greet.error_no, &greet.sqlstate, &greet.error);
            self.server_capabilities = 0;
            return false;
        }
        if greet.pre41 {
            let v = String::from_utf8_lossy(&greet.server_version);
            let v: String = v.chars().take(32).collect();
            let msg = format!(
                "Connecting to 3.22, 3.23 & 4.0 is not supported. Server is {}",
                v
            );
            self.error_info
                .set_str(CR_NOT_IMPLEMENTED, UNKNOWN_SQLSTATE, &msg);
            self.server_capabilities = 0;
            return false;
        }
        self.thread_id = greet.thread_id as u64;
        self.protocol_version = greet.protocol_version as u32;
        self.server_version = Some(greet.server_version.clone());
        let mut charset_no = greet.charset_no;
        match charset::find_charset_nr(charset_no) {
            Some(cs) => self.greet_charset = Some(cs),
            None => {
                charset_no = if self.get_server_version() >= 50500 {
                    charset::UTF8_MB4_DEFAULT_ID
                } else {
                    charset::UTF8_MB3_DEFAULT_ID
                };
                self.greet_charset = charset::find_charset_nr(charset_no);
            }
        }
        self.server_capabilities = greet.server_capabilities;
        let auth_protocol = greet
            .auth_protocol
            .as_ref()
            .map(|p| String::from_utf8_lossy(p).into_owned());
        if !super::auth::connect_run_authentication(
            self,
            user,
            passwd,
            db,
            &greet.auth_plugin_data,
            auth_protocol.as_deref(),
            charset_no,
            greet.server_capabilities,
            mysql_flags,
        ) {
            self.server_capabilities = 0;
            return false;
        }
        self.upsert.reset();
        self.upsert.server_status = greet.server_status;
        true
    }

    /// mysqlnd_conn_data::execute_init_commands
    fn execute_init_commands(&mut self) -> bool {
        let cmds = self.options.init_commands.clone();
        for command in cmds {
            self.stats.inc(STAT_INIT_COMMAND_EXECUTED_COUNT);
            if !self.query(&command) {
                self.stats.inc(STAT_INIT_COMMAND_FAILED_COUNT);
                return false;
            }
            loop {
                if self.last_query_type == QueryType::Select {
                    if let Some(mut r) = self.use_result_internal() {
                        r.free_result_with(self, true);
                    }
                }
                if !self.next_result() {
                    break;
                }
            }
        }
        true
    }

    /// mysqlnd_send_close
    pub fn send_close(&mut self) -> bool {
        let mut ret = true;
        let state = self.state;
        if state >= ConnState::Ready {
            global_dec(STAT_OPENED_CONNECTIONS);
            if self.persistent {
                global_dec(STAT_OPENED_PERSISTENT_CONNECTIONS);
            }
        }
        match state {
            ConnState::Ready => {
                if self.vio.has_stream() {
                    ret = self.cmd_quit();
                    self.vio.close_stream();
                }
                self.state = ConnState::QuitSent;
            }
            ConnState::SendingLoadData
            | ConnState::NextResultPending
            | ConnState::QuerySent
            | ConnState::FetchingData => {
                global_inc(STAT_CLOSE_IN_MIDDLE);
                self.state = ConnState::QuitSent;
                self.vio.close_stream();
            }
            ConnState::Alloced => {
                self.state = ConnState::QuitSent;
                self.vio.close_stream();
            }
            ConnState::QuitSent => {
                self.vio.close_stream();
            }
        }
        ret
    }

    /// mysqlnd_conn::close (the close type maps to a statistic)
    pub fn close(&mut self, close_type: CloseType) -> bool {
        if self.state >= ConnState::Ready {
            self.stats.inc(match close_type {
                CloseType::Explicit => STAT_CLOSE_EXPLICIT,
                CloseType::Implicit => STAT_CLOSE_IMPLICIT,
                CloseType::Disconnected => STAT_CLOSE_DISCONNECT,
            });
        }
        self.send_close()
    }

    /// The use_result part of mysqlnd_conn_data::use_result that needs no
    /// connection reference (init commands).
    fn use_result_internal(&mut self) -> Option<Box<ResultSet>> {
        let mut r = self.current_result.take()?;
        if self.last_query_type != QueryType::Select || self.state != ConnState::FetchingData {
            self.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            self.current_result = Some(r);
            return None;
        }
        self.stats.inc(STAT_UNBUFFERED_SETS);
        r.init_use_result(self, false);
        Some(r)
    }

    /// mysqlnd_conn_data::change_user
    pub fn change_user(
        &mut self,
        user: Option<&[u8]>,
        passwd: Option<&[u8]>,
        db: Option<&[u8]>,
        silent: bool,
    ) -> bool {
        self.error_info.reset();
        self.upsert.set_affected_rows_to_error();
        let user = user.unwrap_or(b"").to_vec();
        let passwd = passwd.unwrap_or(b"").to_vec();
        let db = cstr(db.unwrap_or(b"")).to_vec();
        let plugin_data = self.authentication_plugin_data.clone();
        let proto = self.options.auth_protocol.clone();
        let caps = self.server_capabilities;
        super::auth::run_authentication(
            self,
            &user,
            &passwd,
            &db,
            &plugin_data,
            proto.as_deref(),
            0,
            caps,
            silent,
            true,
            0,
        )
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // mysqlnd_conn_data::free_reference reaching 0: send_close + dtor
        self.send_close();
    }
}

/// The close types of mysqlnd_close()
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseType {
    Explicit,
    Implicit,
    Disconnected,
}

/// The value of a client option (char* in C).
#[derive(Clone, Debug)]
pub enum OptValue {
    Int(i64),
    Str(Vec<u8>),
    Null,
}

impl OptValue {
    pub fn uint(&self) -> u32 {
        match self {
            OptValue::Int(n) => *n as u32,
            OptValue::Str(s) => String::from_utf8_lossy(s)
                .trim()
                .parse::<i64>()
                .unwrap_or(0) as u32,
            OptValue::Null => 0,
        }
    }
    pub fn string(&self) -> Option<String> {
        match self {
            OptValue::Str(s) => Some(String::from_utf8_lossy(cstr(s)).into_owned()),
            OptValue::Int(n) => Some(n.to_string()),
            OptValue::Null => None,
        }
    }
    pub fn bytes(&self) -> Option<Vec<u8>> {
        match self {
            OptValue::Str(s) => Some(cstr(s).to_vec()),
            OptValue::Int(n) => Some(n.to_string().into_bytes()),
            OptValue::Null => None,
        }
    }
}

/// The part of a byte string a C function sees (up to the first NUL).
pub fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&c| c == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

// ── API on connection references (results keep a reference) ─────────

/// mysqlnd_conn_data::use_result
pub fn use_result(conn: &ConnRef) -> Option<Box<ResultSet>> {
    let mut c = conn.borrow_mut();
    let mut r = c.current_result.take()?;
    if c.last_query_type != QueryType::Select || c.state != ConnState::FetchingData {
        c.error_info.set_str(
            CR_COMMANDS_OUT_OF_SYNC,
            UNKNOWN_SQLSTATE,
            MYSQLND_OUT_OF_SYNC,
        );
        // C frees current_result? no: returns NULL, current_result kept
        c.current_result = Some(r);
        return None;
    }
    c.stats.inc(STAT_UNBUFFERED_SETS);
    r.conn = Some(conn.clone());
    r.init_use_result(&mut c, false);
    Some(r)
}

/// mysqlnd_conn_data::store_result
pub fn store_result(conn: &ConnRef) -> Option<Box<ResultSet>> {
    let mut c = conn.borrow_mut();
    let mut r = c.current_result.take()?;
    if c.last_query_type != QueryType::Select || c.state != ConnState::FetchingData {
        c.error_info.set_str(
            CR_COMMANDS_OUT_OF_SYNC,
            UNKNOWN_SQLSTATE,
            MYSQLND_OUT_OF_SYNC,
        );
        c.current_result = Some(r);
        return None;
    }
    c.stats.inc(STAT_BUFFERED_SETS);
    r.conn = Some(conn.clone());
    r.rtype = ResultType::Normal;
    if r.store_result(&mut c, false) {
        Some(r)
    } else {
        // free_result(TRUE)
        drop(c);
        r.free_result(true);
        None
    }
}
