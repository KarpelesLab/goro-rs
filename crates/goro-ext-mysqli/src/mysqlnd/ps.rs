//! Prepared statements (mysqlnd_ps.c) and the COM_STMT_EXECUTE request
//! encoding (mysqlnd_ps_codec.c).

use goro_core::value::Value;

use super::conn::{Conn, ConnRef};
use super::result::{Buffered, ResultSet, ResultType};
use super::stats::*;
use super::wire::RowPacket;
use super::*;

pub const MYSQLND_NOT_BOUND_AS_BLOB: &str =
    "Can't send long data for non-string/non-binary data types";
pub const MYSQLND_STMT_NOT_PREPARED: &str = "Statement not prepared";

pub const MYSQLND_PARAM_BIND_BLOB_USED: u32 = 1;

pub const STMT_ATTR_UPDATE_MAX_LENGTH: i64 = 0;
pub const STMT_ATTR_CURSOR_TYPE: i64 = 1;
pub const CURSOR_TYPE_NO_CURSOR: u32 = 0;
pub const CURSOR_TYPE_READ_ONLY: u32 = 1;

/// enum_mysqlnd_stmt_state
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StmtState {
    Initted = 0,
    Prepared = 1,
    Executed = 2,
    WaitingUseOrStore = 3,
    UseOrStoreCalled = 4,
    UserFetching = 5,
}

/// MYSQLND_PARAM_BIND
#[derive(Clone, Debug)]
pub struct ParamBind {
    /// Value::Undef when not bound; may be a Value::Reference
    pub zv: Value,
    pub ptype: u8,
    pub flags: u32,
}

/// MYSQLND_RESULT_BIND
#[derive(Clone, Debug)]
pub struct ResultBind {
    pub zv: Value,
    pub bound: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RsetHandler {
    Use,
    Store,
}

/// The PHP conversions the execute request needs (they may call user code
/// or raise diagnostics, so they are provided by the mysqli layer).
pub trait Convert {
    /// zval_get_double
    fn to_double(&mut self, v: &Value) -> f64;
    /// convert_to_long
    fn to_long(&mut self, v: &Value) -> i64;
    /// try_convert_to_string: None when an exception was thrown
    fn to_string(&mut self, v: &Value) -> Option<Vec<u8>>;
}

/// MYSQLND_STMT_DATA
pub struct Stmt {
    pub conn: Option<ConnRef>,
    pub stmt_id: u32,
    pub state: StmtState,
    pub field_count: u32,
    pub param_count: u32,
    pub result: Option<Box<ResultSet>>,
    pub error_info: ErrorInfo,
    pub upsert: UpsertStatus,
    pub param_bind: Option<Vec<ParamBind>>,
    pub result_bind: Option<Vec<ResultBind>>,
    pub send_types_to_server: bool,
    pub flags: u32,
    pub update_max_length: bool,
    pub cursor_exists: bool,
    pub default_rset_handler: RsetHandler,
    pub execute_count: u64,
    /// the statement was destroyed (dtor ran)
    pub closed: bool,
}

fn deref(v: &Value) -> Value {
    match v {
        Value::Reference(r) => r.borrow().clone(),
        other => other.clone(),
    }
}

fn is_null(v: &Value) -> bool {
    matches!(deref(v), Value::Null)
}

impl Stmt {
    /// mysqlnd_object_factory::get_prepared_statement
    pub fn new(conn: ConnRef) -> Stmt {
        Stmt {
            conn: Some(conn),
            stmt_id: 0,
            state: StmtState::Initted,
            field_count: 0,
            param_count: 0,
            result: None,
            error_info: ErrorInfo::default(),
            upsert: UpsertStatus::default(),
            param_bind: None,
            result_bind: None,
            send_types_to_server: false,
            flags: 0,
            update_max_length: false,
            cursor_exists: false,
            default_rset_handler: RsetHandler::Use,
            execute_count: 0,
            closed: false,
        }
    }

    fn conn(&self) -> Option<ConnRef> {
        self.conn.clone()
    }

    fn reset_errors(&mut self, c: &mut Conn) {
        self.error_info.reset();
        c.error_info.reset();
    }

    fn check_state(&self, c: &Conn) -> bool {
        if self.state != StmtState::WaitingUseOrStore {
            return false;
        }
        if self.cursor_exists {
            c.state == ConnState::Ready
        } else {
            c.state == ConnState::FetchingData
        }
    }

    /// mysqlnd_stmt::prepare
    pub fn prepare(&mut self, query: &[u8]) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        self.upsert.set_affected_rows_to_error();
        {
            let mut c = conn.borrow_mut();
            c.upsert.set_affected_rows_to_error();
            self.reset_errors(&mut c);
        }
        if self.state > StmtState::Initted {
            // Create a new prepared statement and destroy the previous one.
            let mut old = std::mem::replace(self, Stmt::new(conn.clone()));
            // keep the error info objects of the handle
            self.error_info = std::mem::take(&mut old.error_info);
            self.upsert = old.upsert;
            old.dtor(true);
        }
        let mut c = conn.borrow_mut();
        if !c.send_command(COM_STMT_PREPARE, query, false) {
            let e = c.error_info.clone();
            self.error_info.copy_from(&e);
            return false;
        }
        // mysqlnd_stmt_read_prepare_response
        let resp = match c.read_prepare_response() {
            Ok(r) => r,
            Err(()) => return false,
        };
        if resp.error_code == 0xFF {
            self.error_info.copy_from(&resp.error_info);
            c.error_info.copy_from(&resp.error_info);
            return false;
        }
        self.stmt_id = resp.stmt_id;
        c.upsert.warning_count = resp.warning_count;
        self.upsert.affected_rows = 0;
        self.field_count = resp.field_count;
        c.field_count = resp.field_count;
        self.param_count = resp.param_count;
        if self.param_count > 0 {
            // mysqlnd_stmt_skip_metadata
            for _ in 0..self.param_count {
                let mut ei = ErrorInfo::default();
                if c.read_field(true, &mut ei).is_err() {
                    return false;
                }
            }
            if !self.prepare_read_eof(&mut c) {
                return false;
            }
        }
        if self.field_count > 0 {
            let mut result = Box::new(ResultSet::new(self.field_count));
            result.conn = Some(conn.clone());
            result.rtype = ResultType::PsBuf;
            let ok = result.read_result_metadata(&mut c);
            self.result = Some(result);
            if !ok || !self.prepare_read_eof(&mut c) {
                return false;
            }
        }
        self.state = StmtState::Prepared;
        true
    }

    /// mysqlnd_stmt_prepare_read_eof
    fn prepare_read_eof(&mut self, c: &mut Conn) -> bool {
        match c.read_eof() {
            Err(()) => {
                if let Some(mut r) = self.result.take() {
                    r.free_result_contents();
                    // memset(stmt, 0): the statement is unusable
                    self.reset_to_zero();
                }
                false
            }
            Ok(eof) => {
                self.upsert.server_status = eof.server_status;
                self.upsert.warning_count = eof.warning_count;
                self.state = StmtState::Prepared;
                true
            }
        }
    }

    fn reset_to_zero(&mut self) {
        self.conn = None;
        self.stmt_id = 0;
        self.state = StmtState::Initted;
        self.field_count = 0;
        self.param_count = 0;
        self.result = None;
        self.param_bind = None;
        self.result_bind = None;
    }

    /// mysqlnd_stmt::execute
    pub fn execute(&mut self, cv: &mut dyn Convert) -> bool {
        self.send_execute(cv) && self.parse_execute_response(false)
    }

    /// mysqlnd_stmt::send_execute
    pub fn send_execute(&mut self, cv: &mut dyn Convert) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        self.upsert.set_affected_rows_to_error();
        conn.borrow_mut().upsert.set_affected_rows_to_error();
        if self.result.is_some() && self.state >= StmtState::Prepared && self.field_count > 0 {
            self.flush();
            if let Some(r) = self.result.as_mut() {
                r.free_result_buffers();
            }
            self.state = StmtState::Prepared;
        } else if self.state < StmtState::Prepared {
            self.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return false;
        }
        if self.param_count > 0 {
            let pb = match &self.param_bind {
                Some(p) => p,
                None => {
                    self.error_info.set_str(
                        CR_PARAMS_NOT_BOUND,
                        UNKNOWN_SQLSTATE,
                        "No data supplied for parameters in prepared statement",
                    );
                    return false;
                }
            };
            let not_bound = pb.iter().filter(|p| matches!(p.zv, Value::Undef)).count();
            if not_bound > 0 {
                let msg = format!(
                    "No data supplied for {} parameter{} in prepared statement",
                    not_bound,
                    if not_bound > 1 { "s" } else { "" }
                );
                self.error_info
                    .set_str(CR_PARAMS_NOT_BOUND, UNKNOWN_SQLSTATE, &msg);
                return false;
            }
        }
        let request = match self.generate_execute_request(cv) {
            Some(r) => r,
            None => {
                if self.error_info.error_no == 0 {
                    self.error_info.set_str(
                        CR_UNKNOWN_ERROR,
                        UNKNOWN_SQLSTATE,
                        "Couldn't generate the request. Possibly OOM.",
                    );
                }
                return false;
            }
        };
        let mut c = conn.borrow_mut();
        if !c.send_command(COM_STMT_EXECUTE, &request, false) {
            let e = c.error_info.clone();
            self.error_info.copy_from(&e);
            return false;
        }
        self.execute_count += 1;
        true
    }

    /// mysqlnd_stmt_execute_parse_response
    pub fn parse_execute_response(&mut self, _next_result: bool) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        let mut c = conn.borrow_mut();
        c.state = ConnState::QuerySent;
        let ret = c.query_read_result_set_header(Some(&mut self.result));
        if !ret {
            let e = c.error_info.clone();
            self.error_info.copy_from(&e);
            self.upsert.reset();
            self.upsert.affected_rows = c.upsert.affected_rows;
            self.state = StmtState::Prepared;
            self.send_types_to_server = true;
        } else {
            self.reset_errors(&mut c);
            self.upsert.warning_count = c.upsert.warning_count;
            self.upsert.affected_rows = c.upsert.affected_rows;
            self.upsert.server_status = c.upsert.server_status;
            self.upsert.last_insert_id = c.upsert.last_insert_id;
            self.state = StmtState::Executed;
            if c.last_query_type == QueryType::Upsert || c.last_query_type == QueryType::LoadLocal {
                return true;
            }
            if let Some(r) = self.result.as_mut() {
                r.rtype = ResultType::PsBuf;
                if r.conn.is_none() {
                    r.conn = Some(conn.clone());
                }
            }
            if let Some(rb) = self.result_bind.as_mut() {
                if c.field_count != self.field_count {
                    rb.resize(
                        c.field_count as usize,
                        ResultBind {
                            zv: Value::Undef,
                            bound: false,
                        },
                    );
                }
            }
            self.field_count = c.field_count;
            if let Some(r) = self.result.as_mut() {
                r.field_count = c.field_count;
            }
            if self.field_count > 0 {
                self.state = StmtState::WaitingUseOrStore;
                if self.flags & CURSOR_TYPE_READ_ONLY != 0 {
                    if self.upsert.server_status & SERVER_STATUS_CURSOR_EXISTS != 0 {
                        self.cursor_exists = true;
                        c.state = ConnState::Ready;
                        self.default_rset_handler = RsetHandler::Use;
                    } else {
                        self.default_rset_handler = RsetHandler::Store;
                    }
                } else {
                    self.default_rset_handler = RsetHandler::Use;
                }
            }
        }
        if self.upsert.server_status & SERVER_PS_OUT_PARAMS != 0 {
            drop(c);
            self.free_stmt_content();
            return self.parse_execute_response(false);
        }
        if ret && c.last_query_type == QueryType::Upsert && self.upsert.affected_rows != 0 {
            let n = self.upsert.affected_rows;
            c.stats.inc_v(STAT_ROWS_AFFECTED_PS, n);
        }
        ret
    }

    fn run_default_rset_handler(&mut self) {
        match self.default_rset_handler {
            RsetHandler::Use => {
                self.use_result();
            }
            RsetHandler::Store => {
                self.store_result();
            }
        }
    }

    /// mysqlnd_stmt::use_result (the result stays in stmt.result)
    pub fn use_result(&mut self) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        if self.result.is_none() {
            return false;
        }
        let mut c = conn.borrow_mut();
        if self.field_count == 0 || !self.check_state(&c) {
            c.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return false;
        }
        self.error_info.reset();
        c.stats.inc(STAT_PS_UNBUFFERED_SETS);
        let r = self.result.as_mut().unwrap();
        r.init_use_result(&mut c, true);
        self.state = StmtState::UseOrStoreCalled;
        true
    }

    fn send_cursor_fetch_command(&mut self, c: &mut Conn, max_rows: u32) -> bool {
        let mut buf = Vec::new();
        int4store(&mut buf, self.stmt_id);
        int4store(&mut buf, max_rows);
        if !c.send_command(COM_STMT_FETCH, &buf, false) {
            let e = c.error_info.clone();
            self.error_info.copy_from(&e);
            return false;
        }
        true
    }

    /// mysqlnd_stmt::store_result
    pub fn store_result(&mut self) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        if self.result.is_none() || self.field_count == 0 {
            return false;
        }
        let mut c = conn.borrow_mut();
        if !self.check_state(&c) {
            self.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return false;
        }
        self.default_rset_handler = RsetHandler::Store;
        self.reset_errors(&mut c);
        c.stats.inc(STAT_PS_BUFFERED_SETS);
        if self.cursor_exists && !self.send_cursor_fetch_command(&mut c, u32::MAX) {
            return false;
        }
        let fc = self.field_count;
        let r = self.result.as_mut().unwrap();
        r.rtype = ResultType::PsBuf;
        r.stored = Some(Buffered {
            is_ps: true,
            lengths: None,
            ..Default::default()
        });
        let _ = fc;
        if r.store_result_fetch_data(&mut c, true) {
            let rc = r.stored.as_ref().map(|s| s.row_count).unwrap_or(0);
            if let Some(s) = r.stored.as_mut() {
                s.current_row = 0;
            }
            self.upsert.affected_rows = rc;
            self.state = StmtState::UseOrStoreCalled;
            true
        } else {
            let e = r
                .stored
                .as_ref()
                .map(|s| s.error_info.clone())
                .unwrap_or_default();
            c.error_info.copy_from(&e);
            self.error_info.copy_from(&e);
            let mut r = self.result.take().unwrap();
            r.free_result_contents();
            self.state = StmtState::Prepared;
            false
        }
    }

    /// mysqlnd_stmt::get_result
    pub fn get_result(&mut self) -> Option<Box<ResultSet>> {
        let conn = self.conn()?;
        if self.result.is_none() || self.field_count == 0 {
            return None;
        }
        let mut c = conn.borrow_mut();
        if !self.check_state(&c) {
            self.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return None;
        }
        self.reset_errors(&mut c);
        c.stats.inc(STAT_BUFFERED_SETS);
        if self.cursor_exists && !self.send_cursor_fetch_command(&mut c, u32::MAX) {
            return None;
        }
        let fc = self.result.as_ref().unwrap().field_count;
        let mut result = Box::new(ResultSet::new(fc));
        result.meta = self.result.as_ref().unwrap().meta.clone();
        if let Some(m) = result.meta.as_mut() {
            m.current_field = 0;
        }
        // mysqlnd_res::store_result(result, conn, stmt)
        result.conn = Some(conn.clone());
        result.rtype = ResultType::Normal;
        if result.store_result(&mut c, true) {
            let rc = result.stored.as_ref().map(|s| s.row_count).unwrap_or(0);
            self.upsert.affected_rows = rc;
            self.state = StmtState::Prepared;
            result.rtype = ResultType::PsBuf;
            Some(result)
        } else {
            let e = c.error_info.clone();
            self.error_info.copy_from(&e);
            self.state = StmtState::Prepared;
            drop(c);
            result.free_result(true);
            None
        }
    }

    pub fn more_results(&self) -> bool {
        match &self.conn {
            Some(c) => c.borrow().upsert.server_status & SERVER_MORE_RESULTS_EXISTS != 0,
            None => false,
        }
    }

    /// mysqlnd_stmt::next_result
    pub fn next_result(&mut self) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        if self.result.is_none() {
            return false;
        }
        {
            let c = conn.borrow();
            if c.state != ConnState::NextResultPending
                || c.upsert.server_status & SERVER_MORE_RESULTS_EXISTS == 0
            {
                return false;
            }
        }
        self.free_stmt_result();
        self.parse_execute_response(true)
    }

    /// mysqlnd_stmt::fetch: Ok(Some(row)) with the row when fetched (the
    /// values for the bound variables), Ok(None) at the end.
    pub fn fetch(&mut self) -> Result<Option<Vec<Value>>, ()> {
        let conn = match self.conn() {
            Some(c) => c,
            None => return Err(()),
        };
        if self.result.is_none() || self.state < StmtState::WaitingUseOrStore {
            self.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return Err(());
        } else if self.state == StmtState::WaitingUseOrStore {
            self.run_default_rset_handler();
        }
        self.state = StmtState::UserFetching;
        self.reset_errors(&mut conn.borrow_mut());
        let want = self.result_bind.is_some();
        let r = match self.result.as_mut() {
            Some(r) => r,
            None => return Err(()),
        };
        let res = if r.unbuf.is_some() && self.cursor_exists {
            self.fetch_row_cursor(want)
        } else {
            let r = self.result.as_mut().unwrap();
            r.fetch_row(want)
        };
        if res.is_err() {
            let c = conn.borrow();
            if c.error_info.error_no != 0 {
                let e = c.error_info.clone();
                drop(c);
                self.error_info.copy_from(&e);
            }
        }
        res
    }

    /// mysqlnd_fetch_stmt_row_cursor
    fn fetch_row_cursor(&mut self, want: bool) -> Result<Option<Vec<Value>>, ()> {
        let conn = match self.conn() {
            Some(c) => c,
            None => return Err(()),
        };
        let mut c = conn.borrow_mut();
        if self.state < StmtState::UserFetching {
            c.error_info.set_str(
                CR_COMMANDS_OUT_OF_SYNC,
                UNKNOWN_SQLSTATE,
                MYSQLND_OUT_OF_SYNC,
            );
            return Err(());
        }
        let mut rp = match self
            .result
            .as_mut()
            .and_then(|r| r.unbuf.as_mut())
            .and_then(|u| u.row_packet.take())
        {
            Some(p) => p,
            None => return Err(()),
        };
        self.reset_errors(&mut c);
        if !self.send_cursor_fetch_command(&mut c, 1) {
            self.put_row_packet(rp);
            return Err(());
        }
        self.upsert.reset();
        let fields: Vec<super::result::Field> = self
            .result
            .as_ref()
            .map(|r| r.fields().to_vec())
            .unwrap_or_default();
        let mut out = None;
        let ret = match c.read_row(&mut rp) {
            Ok(Some(buf)) => {
                let mut row = Vec::new();
                if want {
                    let native = c.options.int_and_float_native;
                    match super::result::decode_binary_row(&buf, &fields, native, &mut c.stats) {
                        Some(r) => row = r,
                        None => {
                            self.put_row_packet(rp);
                            return Err(());
                        }
                    }
                }
                // We asked for one row, the next one should be EOF, eat it
                let r2 = c.read_row(&mut rp);
                c.stats.inc(STAT_ROWS_FETCHED_FROM_CLIENT_PS_CURSOR);
                if let Some(u) = self.result.as_mut().and_then(|r| r.unbuf.as_mut()) {
                    u.row_count += 1;
                }
                out = Some(row);
                r2.map(|_| ()).is_ok()
            }
            Ok(None) => {
                self.upsert.warning_count = rp.warning_count;
                c.upsert.warning_count = rp.warning_count;
                self.upsert.server_status = rp.server_status;
                c.upsert.server_status = rp.server_status;
                if let Some(u) = self.result.as_mut().and_then(|r| r.unbuf.as_mut()) {
                    u.eof_reached = rp.eof;
                }
                true
            }
            Err(()) => {
                if let Some(u) = self.result.as_mut().and_then(|r| r.unbuf.as_mut()) {
                    u.eof_reached = rp.eof;
                }
                false
            }
        };
        self.upsert.warning_count = rp.warning_count;
        c.upsert.warning_count = rp.warning_count;
        self.upsert.server_status = rp.server_status;
        c.upsert.server_status = rp.server_status;
        self.put_row_packet(rp);
        if ret { Ok(out) } else { Err(()) }
    }

    fn put_row_packet(&mut self, rp: RowPacket) {
        if let Some(u) = self.result.as_mut().and_then(|r| r.unbuf.as_mut()) {
            u.row_packet = Some(rp);
        }
    }

    /// mysqlnd_stmt::reset
    pub fn reset(&mut self) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        let mut ret = true;
        self.reset_errors(&mut conn.borrow_mut());
        if self.stmt_id != 0 {
            if let Some(pb) = self.param_bind.as_mut() {
                for p in pb.iter_mut() {
                    p.flags &= !MYSQLND_PARAM_BIND_BLOB_USED;
                }
            }
            self.flush();
            let mut c = conn.borrow_mut();
            if c.state == ConnState::Ready {
                let mut buf = Vec::new();
                int4store(&mut buf, self.stmt_id);
                ret = c.send_command(COM_STMT_RESET, &buf, false)
                    && c.send_command_handle_response(true, false, COM_STMT_RESET, true);
                if !ret {
                    let e = c.error_info.clone();
                    self.error_info.copy_from(&e);
                }
            }
            self.upsert = c.upsert;
        }
        ret
    }

    /// mysqlnd_stmt::flush
    pub fn flush(&mut self) {
        if self.stmt_id == 0 || self.conn.is_none() {
            return;
        }
        loop {
            if self.state == StmtState::WaitingUseOrStore {
                self.run_default_rset_handler();
                self.state = StmtState::UserFetching;
            }
            if let Some(r) = self.result.as_mut() {
                r.skip_result();
            }
            if !(self.more_results() && self.next_result()) {
                break;
            }
        }
    }

    /// mysqlnd_stmt::send_long_data
    pub fn send_long_data(&mut self, param_no: u32, data: &[u8]) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        let mut c = conn.borrow_mut();
        self.reset_errors(&mut c);
        if self.state < StmtState::Prepared {
            self.error_info.set_str(
                CR_NO_PREPARE_STMT,
                UNKNOWN_SQLSTATE,
                MYSQLND_STMT_NOT_PREPARED,
            );
            return false;
        }
        let pb = match self.param_bind.as_mut() {
            Some(p) => p,
            None => {
                self.error_info.set_str(
                    CR_COMMANDS_OUT_OF_SYNC,
                    UNKNOWN_SQLSTATE,
                    MYSQLND_OUT_OF_SYNC,
                );
                return false;
            }
        };
        if param_no >= self.param_count {
            self.error_info.set_str(
                CR_INVALID_PARAMETER_NO,
                UNKNOWN_SQLSTATE,
                "Invalid parameter number",
            );
            return false;
        }
        if pb[param_no as usize].ptype != MYSQL_TYPE_LONG_BLOB {
            self.error_info.set_str(
                CR_INVALID_BUFFER_USE,
                UNKNOWN_SQLSTATE,
                MYSQLND_NOT_BOUND_AS_BLOB,
            );
            return false;
        }
        let mut ret = false;
        if c.state == ConnState::Ready {
            pb[param_no as usize].flags |= MYSQLND_PARAM_BIND_BLOB_USED;
            let mut buf = Vec::with_capacity(6 + data.len());
            int4store(&mut buf, self.stmt_id);
            int2store(&mut buf, param_no);
            buf.extend_from_slice(data);
            ret = c.send_command(COM_STMT_SEND_LONG_DATA, &buf, false);
            if !ret {
                let e = c.error_info.clone();
                self.error_info.copy_from(&e);
            }
        }
        ret
    }

    /// mysqlnd_stmt::bind_parameters
    pub fn bind_parameters(&mut self, param_bind: Vec<ParamBind>) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        if self.state < StmtState::Prepared {
            self.error_info.set_str(
                CR_NO_PREPARE_STMT,
                UNKNOWN_SQLSTATE,
                MYSQLND_STMT_NOT_PREPARED,
            );
            return false;
        }
        self.reset_errors(&mut conn.borrow_mut());
        if self.param_count > 0 {
            let mut pb = param_bind;
            for p in pb.iter_mut() {
                p.flags = 0;
            }
            self.param_bind = Some(pb);
            self.send_types_to_server = true;
        }
        true
    }

    /// mysqlnd_stmt::bind_result
    pub fn bind_result(&mut self, result_bind: Vec<ResultBind>) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        if self.state < StmtState::Prepared {
            self.error_info.set_str(
                CR_NO_PREPARE_STMT,
                UNKNOWN_SQLSTATE,
                MYSQLND_STMT_NOT_PREPARED,
            );
            return false;
        }
        self.reset_errors(&mut conn.borrow_mut());
        if self.field_count > 0 {
            self.result_bind = None;
            let mut rb = result_bind;
            for r in rb.iter_mut() {
                r.bound = true;
            }
            self.result_bind = Some(rb);
        }
        true
    }

    pub fn num_rows(&self) -> u64 {
        self.result.as_ref().map(|r| r.num_rows()).unwrap_or(0)
    }

    pub fn sqlstate(&self) -> Vec<u8> {
        self.error_info.sqlstate_or_null()
    }

    /// mysqlnd_stmt::data_seek
    pub fn data_seek(&mut self, row: u64) -> bool {
        match self.result.as_mut() {
            Some(r) => r.data_seek(row),
            None => false,
        }
    }

    /// mysqlnd_stmt::result_metadata
    pub fn result_metadata(&mut self) -> Option<Box<ResultSet>> {
        let conn = self.conn()?;
        if self.field_count == 0 {
            return None;
        }
        let meta = self.result.as_ref()?.meta.clone()?;
        let mut r = Box::new(ResultSet::new(self.field_count));
        r.rtype = ResultType::Normal;
        let mut m = meta;
        m.current_field = 0;
        r.meta = Some(m);
        r.unbuf = Some(super::result::Unbuffered {
            eof_reached: true,
            is_ps: true,
            ..Default::default()
        });
        let _ = conn;
        Some(r)
    }

    /// mysqlnd_stmt::attr_set
    pub fn attr_set(&mut self, attr: i64, value: u64) -> bool {
        match attr {
            STMT_ATTR_UPDATE_MAX_LENGTH => {
                self.update_max_length = (value as u8) != 0;
            }
            STMT_ATTR_CURSOR_TYPE => {
                if value > CURSOR_TYPE_READ_ONLY as u64 {
                    self.error_info.set_str(
                        CR_NOT_IMPLEMENTED,
                        UNKNOWN_SQLSTATE,
                        "Not implemented",
                    );
                    return false;
                }
                self.flags = value as u32;
            }
            _ => {
                self.error_info
                    .set_str(CR_NOT_IMPLEMENTED, UNKNOWN_SQLSTATE, "Not implemented");
                return false;
            }
        }
        true
    }

    /// mysqlnd_stmt::attr_get
    pub fn attr_get(&self, attr: i64) -> Option<u64> {
        match attr {
            STMT_ATTR_UPDATE_MAX_LENGTH => Some(self.update_max_length as u64),
            STMT_ATTR_CURSOR_TYPE => Some(self.flags as u64),
            _ => None,
        }
    }

    /// mysqlnd_stmt::free_result
    pub fn free_result(&mut self) -> bool {
        if self.conn.is_none() {
            return false;
        }
        if self.result.is_none() {
            return true;
        }
        if self.state == StmtState::WaitingUseOrStore {
            self.default_rset_handler = RsetHandler::Use;
            self.use_result();
        }
        if self.state > StmtState::WaitingUseOrStore {
            if let Some(r) = self.result.as_mut() {
                r.skip_result();
            }
            self.result_bind = None;
            if let Some(r) = self.result.as_mut() {
                r.free_result_buffers();
            }
        }
        if self.state > StmtState::Prepared {
            self.state = StmtState::Prepared;
        }
        true
    }

    /// mysqlnd_stmt::free_stmt_result
    pub fn free_stmt_result(&mut self) {
        self.result_bind = None;
        if let Some(mut r) = self.result.take() {
            r.free_result(true);
        }
        self.error_info.error_list.clear();
    }

    /// mysqlnd_stmt::free_stmt_content
    pub fn free_stmt_content(&mut self) {
        self.param_bind = None;
        self.free_stmt_result();
    }

    /// mysqlnd_stmt::close_on_server
    fn close_on_server(&mut self, implicit: bool) -> bool {
        let conn = match self.conn() {
            Some(c) => c,
            None => return false,
        };
        self.reset_errors(&mut conn.borrow_mut());
        loop {
            if self.state == StmtState::WaitingUseOrStore {
                self.run_default_rset_handler();
                self.state = StmtState::UserFetching;
            }
            if let Some(r) = self.result.as_mut() {
                r.skip_result();
            }
            if !(self.more_results() && self.next_result()) {
                break;
            }
        }
        if self.stmt_id != 0 {
            global_inc(if implicit {
                STAT_FREE_RESULT_IMPLICIT
            } else {
                STAT_FREE_RESULT_EXPLICIT
            });
            let mut c = conn.borrow_mut();
            if c.state == ConnState::Ready {
                let mut buf = Vec::new();
                int4store(&mut buf, self.stmt_id);
                if !c.send_command(COM_STMT_CLOSE, &buf, false) {
                    let e = c.error_info.clone();
                    self.error_info.copy_from(&e);
                    return false;
                }
            }
        }
        let st = match self.execute_count {
            0 => STAT_PS_PREPARED_NEVER_EXECUTED,
            1 => STAT_PS_PREPARED_ONCE_USED,
            _ => STAT_LAST,
        };
        conn.borrow_mut().stats.inc(st);
        self.free_stmt_content();
        self.conn = None;
        true
    }

    /// mysqlnd_stmt::dtor
    pub fn dtor(&mut self, implicit: bool) -> bool {
        if self.closed {
            return true;
        }
        self.closed = true;
        global_inc(if implicit {
            STAT_STMT_CLOSE_IMPLICIT
        } else {
            STAT_STMT_CLOSE_EXPLICIT
        });
        self.close_on_server(implicit)
    }

    // ── mysqlnd_ps_codec.c: the execute request ───────────────────────

    /// mysqlnd_stmt_execute_generate_request
    fn generate_execute_request(&mut self, cv: &mut dyn Convert) -> Option<Vec<u8>> {
        let mut p = Vec::with_capacity(64);
        int4store(&mut p, self.stmt_id);
        p.push(self.flags as u8);
        int4store(&mut p, 1); // iteration count
        if self.param_count != 0 && !self.execute_store_params(&mut p, cv) {
            return None;
        }
        Some(p)
    }

    /// mysqlnd_stmt_execute_store_params
    fn execute_store_params(&mut self, p: &mut Vec<u8>, cv: &mut dyn Convert) -> bool {
        let n = self.param_count as usize;
        let null_count = n.div_ceil(8);
        let null_byte_offset = p.len();
        p.resize(p.len() + null_count, 0);
        let mut copies: Vec<Option<Value>> = vec![None; n];
        let mut resend_types_next_time = false;
        // 1. mysqlnd_stmt_execute_prepare_param_types
        {
            let pb = self.param_bind.as_ref().unwrap();
            for i in 0..n {
                let t = pb[i].ptype;
                let param = deref(&pb[i].zv);
                if !matches!(param, Value::Null)
                    && (t == MYSQL_TYPE_LONG || t == MYSQL_TYPE_LONGLONG || t == MYSQL_TYPE_TINY)
                    && !matches!(param, Value::Long(_))
                {
                    copies[i] = Some(param.clone());
                    let d = cv.to_double(&param);
                    if d >= i64::MAX as f64 || d < i64::MIN as f64 {
                        self.send_types_to_server = true;
                        resend_types_next_time = true;
                        match cv.to_string(&param) {
                            Some(s) => {
                                copies[i] =
                                    Some(Value::String(goro_core::string::PhpString::from_vec(s)))
                            }
                            None => return false,
                        }
                    } else {
                        copies[i] = Some(Value::Long(cv.to_long(&param)));
                    }
                }
            }
        }
        p.push(self.send_types_to_server as u8);
        if self.send_types_to_server {
            // mysqlnd_stmt_execute_store_types
            let pb = self.param_bind.as_ref().unwrap();
            for i in 0..n {
                let mut t = pb[i].ptype;
                if t == MYSQL_TYPE_LONG {
                    t = MYSQL_TYPE_LONGLONG;
                }
                let param = deref(&pb[i].zv);
                if !matches!(param, Value::Null)
                    && (t == MYSQL_TYPE_LONG || t == MYSQL_TYPE_LONGLONG)
                    && !matches!(param, Value::Long(_))
                {
                    let tmp = copies[i].clone().unwrap_or(param);
                    if matches!(tmp, Value::String(_)) {
                        t = MYSQL_TYPE_VAR_STRING;
                    }
                }
                int2store(p, t as u32);
            }
        }
        self.send_types_to_server = resend_types_next_time;
        // 2. mysqlnd_stmt_execute_calculate_param_values_size (conversions)
        {
            let pb = self.param_bind.clone().unwrap();
            for i in 0..n {
                let bind_var = &pb[i].zv;
                let the_var = deref(bind_var);
                if pb[i].ptype != MYSQL_TYPE_LONG_BLOB && matches!(the_var, Value::Null) {
                    continue;
                }
                if let Value::Reference(r) = bind_var {
                    for later in pb.iter().skip(i + 1) {
                        if let Value::Reference(r2) = &later.zv {
                            if std::rc::Rc::ptr_eq(r, r2) {
                                if copies[i].is_none() {
                                    copies[i] = Some(the_var.clone());
                                }
                                break;
                            }
                        }
                    }
                }
                match pb[i].ptype {
                    MYSQL_TYPE_DOUBLE => {
                        if !matches!(the_var, Value::Double(_)) && copies[i].is_none() {
                            copies[i] = Some(the_var.clone());
                        }
                    }
                    MYSQL_TYPE_LONGLONG | MYSQL_TYPE_LONG => {
                        let tmp = copies[i].clone().unwrap_or(the_var.clone());
                        if matches!(tmp, Value::String(_)) {
                            // use_string
                            if !self.to_string_param(i, &the_var, &mut copies, cv) {
                                return false;
                            }
                        } else if !matches!(tmp, Value::Long(_)) {
                            copies[i] = Some(Value::Long(cv.to_long(&tmp)));
                        }
                    }
                    MYSQL_TYPE_LONG_BLOB => {}
                    MYSQL_TYPE_VAR_STRING => {
                        if !self.to_string_param(i, &the_var, &mut copies, cv) {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }
        // 3. mysqlnd_stmt_execute_store_param_values
        let pb = self.param_bind.as_mut().unwrap();
        for i in 0..n {
            let param = deref(&pb[i].zv);
            let data = copies[i].clone().unwrap_or(param.clone());
            if !matches!(pb[i].zv, Value::Undef) && matches!(data, Value::Null) {
                p[null_byte_offset + i / 8] |= 1 << (i & 7);
                continue;
            }
            match pb[i].ptype {
                MYSQL_TYPE_DOUBLE => {
                    let d = match data {
                        Value::Double(d) => d,
                        ref other => cv.to_double(other),
                    };
                    p.extend_from_slice(&d.to_le_bytes());
                }
                MYSQL_TYPE_LONGLONG | MYSQL_TYPE_LONG | MYSQL_TYPE_TINY => match data {
                    Value::String(s) => {
                        net_store_length(p, s.len() as u64);
                        p.extend_from_slice(s.as_bytes());
                    }
                    ref other => {
                        let v = match other {
                            Value::Long(n) => *n,
                            o => cv.to_long(o),
                        };
                        match pb[i].ptype {
                            MYSQL_TYPE_LONGLONG => p.extend_from_slice(&v.to_le_bytes()),
                            MYSQL_TYPE_LONG => p.extend_from_slice(&(v as u32).to_le_bytes()),
                            _ => p.push(v as u8),
                        }
                    }
                },
                MYSQL_TYPE_LONG_BLOB => {
                    if pb[i].flags & MYSQLND_PARAM_BIND_BLOB_USED != 0 {
                        pb[i].flags &= !MYSQLND_PARAM_BIND_BLOB_USED;
                    } else {
                        net_store_length(p, 0);
                    }
                }
                MYSQL_TYPE_VAR_STRING => {
                    let s = match data {
                        Value::String(s) => s.as_bytes().to_vec(),
                        ref other => match cv.to_string(other) {
                            Some(s) => s,
                            None => return false,
                        },
                    };
                    net_store_length(p, s.len() as u64);
                    p.extend_from_slice(&s);
                }
                _ => {
                    p[null_byte_offset + i / 8] |= 1 << (i & 7);
                }
            }
        }
        true
    }

    /// the use_string conversion of calculate_param_values_size
    fn to_string_param(
        &self,
        i: usize,
        the_var: &Value,
        copies: &mut [Option<Value>],
        cv: &mut dyn Convert,
    ) -> bool {
        let cur = if matches!(the_var, Value::String(_)) {
            match &copies[i] {
                Some(c) => c.clone(),
                None => return true,
            }
        } else {
            copies[i].clone().unwrap_or(the_var.clone())
        };
        if matches!(cur, Value::String(_)) {
            copies[i] = Some(cur);
            return true;
        }
        match cv.to_string(&cur) {
            Some(s) => {
                copies[i] = Some(Value::String(goro_core::string::PhpString::from_vec(s)));
                true
            }
            None => false,
        }
    }
}

impl Drop for Stmt {
    fn drop(&mut self) {
        if !self.closed && self.conn.is_some() {
            // mysqli_stmt free_obj: php_clear_stmt_bind -> mysqli_stmt_close(stmt, true)
            self.dtor(true);
        }
    }
}
