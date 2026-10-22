//! Query execution (fe-exec.c): the simple and extended query protocols,
//! the blocking PQexec family, COPY, the fastpath function call and
//! cancel requests.

use super::conn::{AsyncStatus, PgConn, QueryClass};
use super::result::PgResult;
use super::*;

/// One parameter of PQexecParams & co.: None is SQL NULL
pub type Param = Option<Vec<u8>>;

fn cstr(b: &mut Vec<u8>, s: &[u8]) {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    b.extend_from_slice(&s[..n]);
    b.push(0);
}

/// PQgetCopyData's outcome
pub enum CopyData {
    Row(Vec<u8>),
    /// no data yet (async mode only)
    WouldBlock,
    /// the COPY is done
    Done,
    Error,
}

impl PgConn {
    /// PQsendQueryStart
    fn send_query_start(&mut self, new_query: bool) -> bool {
        if new_query && !self.cmd_queued {
            self.clear_error_state();
        }
        if self.status != ConnStatus::Ok || self.socket() < 0 {
            self.append_error("no connection to the server");
            return false;
        }
        if self.async_status != AsyncStatus::Idle {
            self.append_error("another command is already in progress");
            return false;
        }
        self.result = None;
        self.next_result = None;
        self.partial_result_mode = false;
        self.single_row_mode = false;
        self.max_chunk_rows = 0;
        true
    }

    fn send_finish(&mut self, class: QueryClass, query: Option<&[u8]>) -> bool {
        self.queryclass = class;
        self.last_query = query.map(|q| {
            let n = q.iter().position(|&c| c == 0).unwrap_or(q.len());
            q[..n].to_vec()
        });
        self.cmd_queued = true;
        if self.flush() < 0 {
            return false;
        }
        self.async_status = AsyncStatus::Busy;
        true
    }

    /// PQsendQuery
    pub fn send_query(&mut self, query: &[u8]) -> bool {
        if !self.send_query_start(true) {
            return false;
        }
        let mut b = Vec::with_capacity(query.len() + 1);
        cstr(&mut b, query);
        self.put_msg(b'Q', &b);
        self.send_finish(QueryClass::Simple, Some(query))
    }

    /// The Bind/Describe/Execute/Sync tail of pqSendQueryGuts
    fn send_bind_exec(
        &mut self,
        stmt: &[u8],
        params: &[Param],
        formats: &[i32],
        result_format: i32,
    ) {
        let mut b = Vec::new();
        cstr(&mut b, b"");
        cstr(&mut b, stmt);
        if formats.iter().any(|&f| f != 0) {
            b.extend_from_slice(&(params.len() as i16).to_be_bytes());
            for i in 0..params.len() {
                let f = formats.get(i).copied().unwrap_or(0);
                b.extend_from_slice(&(f as i16).to_be_bytes());
            }
        } else {
            b.extend_from_slice(&0i16.to_be_bytes());
        }
        b.extend_from_slice(&(params.len() as i16).to_be_bytes());
        for (i, p) in params.iter().enumerate() {
            match p {
                Some(v) => {
                    // text parameters stop at a NUL (strlen)
                    let mut v: &[u8] = v;
                    if formats.get(i).copied().unwrap_or(0) == 0 {
                        if let Some(n) = v.iter().position(|&c| c == 0) {
                            v = &v[..n];
                        }
                    }
                    b.extend_from_slice(&(v.len() as i32).to_be_bytes());
                    b.extend_from_slice(v);
                }
                None => b.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
        b.extend_from_slice(&1i16.to_be_bytes());
        b.extend_from_slice(&(result_format as i16).to_be_bytes());
        self.put_msg(b'B', &b);
        // Describe the portal
        self.put_msg(b'D', b"P\0");
        // Execute
        let mut e = Vec::new();
        cstr(&mut e, b"");
        e.extend_from_slice(&0i32.to_be_bytes());
        self.put_msg(b'E', &e);
        self.put_msg(b'S', &[]);
    }

    fn parse_msg(&mut self, name: &[u8], query: &[u8], types: &[Oid]) {
        let mut b = Vec::new();
        cstr(&mut b, name);
        cstr(&mut b, query);
        b.extend_from_slice(&(types.len() as i16).to_be_bytes());
        for t in types {
            b.extend_from_slice(&t.to_be_bytes());
        }
        self.put_msg(b'P', &b);
    }

    /// PQsendQueryParams
    pub fn send_query_params(
        &mut self,
        query: &[u8],
        types: &[Oid],
        params: &[Param],
        formats: &[i32],
        result_format: i32,
    ) -> bool {
        if !self.send_query_start(true) {
            return false;
        }
        if params.len() > 65535 {
            self.append_error("number of parameters must be between 0 and 65535");
            return false;
        }
        self.parse_msg(b"", query, types);
        self.send_bind_exec(b"", params, formats, result_format);
        self.send_finish(QueryClass::Extended, Some(query))
    }

    /// PQsendPrepare
    pub fn send_prepare(&mut self, name: &[u8], query: &[u8], types: &[Oid]) -> bool {
        if !self.send_query_start(true) {
            return false;
        }
        self.parse_msg(name, query, types);
        self.put_msg(b'S', &[]);
        self.send_finish(QueryClass::Prepare, Some(query))
    }

    /// PQsendQueryPrepared
    pub fn send_query_prepared(
        &mut self,
        name: &[u8],
        params: &[Param],
        formats: &[i32],
        result_format: i32,
    ) -> bool {
        if !self.send_query_start(true) {
            return false;
        }
        if params.len() > 65535 {
            self.append_error("number of parameters must be between 0 and 65535");
            return false;
        }
        self.send_bind_exec(name, params, formats, result_format);
        self.send_finish(QueryClass::Extended, None)
    }

    /// PQsendDescribePrepared / PQsendDescribePortal
    pub fn send_describe(&mut self, kind: u8, name: &[u8]) -> bool {
        if !self.send_query_start(true) {
            return false;
        }
        let mut b = vec![kind];
        cstr(&mut b, name);
        self.put_msg(b'D', &b);
        self.put_msg(b'S', &[]);
        self.send_finish(QueryClass::Describe, None)
    }

    /// PQsendClosePrepared / PQsendClosePortal
    pub fn send_close(&mut self, kind: u8, name: &[u8]) -> bool {
        if !self.send_query_start(true) {
            return false;
        }
        let mut b = vec![kind];
        cstr(&mut b, name);
        self.put_msg(b'C', &b);
        self.put_msg(b'S', &[]);
        self.send_finish(QueryClass::Close, None)
    }

    /// PQexecStart
    fn exec_start(&mut self) -> bool {
        if self.status == ConnStatus::Bad && self.socket() < 0 {
            // libpq would still try (and fail in send_query_start)
        }
        while let Some(r) = self.get_result() {
            match r.status {
                ExecStatus::CopyIn => {
                    if self.put_copy_end(Some(b"COPY terminated by new PQexec")) < 0 {
                        return false;
                    }
                }
                ExecStatus::CopyOut => {
                    self.append_error("PQexec not allowed during COPY OUT");
                    return false;
                }
                ExecStatus::CopyBoth => {
                    self.append_error("PQexec not allowed during COPY BOTH");
                    return false;
                }
                _ => {}
            }
            if self.status == ConnStatus::Bad {
                return false;
            }
        }
        true
    }

    /// PQexecFinish
    fn exec_finish(&mut self) -> Option<PgResult> {
        let mut last: Option<PgResult> = None;
        while let Some(r) = self.get_result() {
            let st = r.status;
            last = Some(r);
            if matches!(
                st,
                ExecStatus::CopyIn | ExecStatus::CopyOut | ExecStatus::CopyBoth
            ) || self.status == ConnStatus::Bad
            {
                break;
            }
        }
        last
    }

    /// PQexec
    pub fn exec(&mut self, query: &[u8]) -> Option<PgResult> {
        if !self.exec_start() {
            return None;
        }
        if !self.send_query(query) {
            return None;
        }
        self.exec_finish()
    }

    /// PQexecParams
    pub fn exec_params(
        &mut self,
        query: &[u8],
        types: &[Oid],
        params: &[Param],
        formats: &[i32],
        result_format: i32,
    ) -> Option<PgResult> {
        if !self.exec_start() {
            return None;
        }
        if !self.send_query_params(query, types, params, formats, result_format) {
            return None;
        }
        self.exec_finish()
    }

    /// PQprepare
    pub fn prepare(&mut self, name: &[u8], query: &[u8], types: &[Oid]) -> Option<PgResult> {
        if !self.exec_start() {
            return None;
        }
        if !self.send_prepare(name, query, types) {
            return None;
        }
        self.exec_finish()
    }

    /// PQexecPrepared
    pub fn exec_prepared(
        &mut self,
        name: &[u8],
        params: &[Param],
        formats: &[i32],
        result_format: i32,
    ) -> Option<PgResult> {
        if !self.exec_start() {
            return None;
        }
        if !self.send_query_prepared(name, params, formats, result_format) {
            return None;
        }
        self.exec_finish()
    }

    /// PQdescribePrepared
    pub fn describe_prepared(&mut self, name: &[u8]) -> Option<PgResult> {
        if !self.exec_start() {
            return None;
        }
        if !self.send_describe(b'S', name) {
            return None;
        }
        self.exec_finish()
    }

    /// PQclosePrepared
    pub fn close_prepared(&mut self, name: &[u8]) -> Option<PgResult> {
        if !self.exec_start() {
            return None;
        }
        if !self.send_close(b'S', name) {
            return None;
        }
        self.exec_finish()
    }

    /// PQsetSingleRowMode
    pub fn set_single_row_mode(&mut self) -> bool {
        if !self.can_change_result_mode() {
            return false;
        }
        self.partial_result_mode = true;
        self.single_row_mode = true;
        self.max_chunk_rows = 1;
        true
    }

    /// PQsetChunkedRowsMode
    pub fn set_chunked_rows_mode(&mut self, chunk: i32) -> bool {
        if chunk <= 0 || !self.can_change_result_mode() {
            return false;
        }
        self.partial_result_mode = true;
        self.single_row_mode = false;
        self.max_chunk_rows = chunk as usize;
        true
    }

    fn can_change_result_mode(&self) -> bool {
        self.async_status == AsyncStatus::Busy
            && self.cmd_queued
            && matches!(self.queryclass, QueryClass::Simple | QueryClass::Extended)
            && self.result.is_none()
    }

    /// PQconsumeInput
    pub fn consume_input(&mut self) -> bool {
        if self.nonblocking && self.flush() < 0 {
            return false;
        }
        if self.socket() < 0 {
            self.append_error("connection not open");
            return false;
        }
        if self.read_data() < 0 {
            return false;
        }
        self.parse_input();
        true
    }

    /// PQsetnonblocking: 0 ok, -1 failure
    pub fn set_nonblocking(&mut self, arg: bool) -> i32 {
        if arg == self.nonblocking {
            return 0;
        }
        if !self.cmd_queued || self.async_status == AsyncStatus::Idle {
            self.clear_error_state();
        }
        if self.flush() != 0 {
            return -1;
        }
        self.nonblocking = arg;
        0
    }

    /// PQnotifies
    pub fn notifies(&mut self) -> Option<Notify> {
        self.parse_input();
        self.notifies.pop_front()
    }

    // ── COPY ───────────────────────────────────────────────────────────

    /// PQputCopyData: 1 queued, 0 would block, -1 failure
    pub fn put_copy_data(&mut self, data: &[u8]) -> i32 {
        if self.async_status != AsyncStatus::CopyIn && self.async_status != AsyncStatus::CopyBoth {
            self.append_error("no COPY in progress");
            return -1;
        }
        // process any NOTICE or NOTIFY messages that might be pending
        self.parse_input();
        if !data.is_empty() {
            self.put_msg(b'd', data);
            if self.nonblocking {
                let r = self.flush();
                if r < 0 {
                    return -1;
                }
            }
        }
        1
    }

    /// PQputCopyEnd
    pub fn put_copy_end(&mut self, errormsg: Option<&[u8]>) -> i32 {
        if self.async_status != AsyncStatus::CopyIn && self.async_status != AsyncStatus::CopyBoth {
            self.append_error("no COPY in progress");
            return -1;
        }
        match errormsg {
            Some(m) => {
                let mut b = Vec::new();
                cstr(&mut b, m);
                self.put_msg(b'f', &b);
            }
            None => self.put_msg(b'c', &[]),
        }
        if self.queryclass != QueryClass::Simple {
            self.put_msg(b'S', &[]);
        }
        if self.async_status == AsyncStatus::CopyBoth {
            self.async_status = AsyncStatus::CopyOut;
        } else {
            self.async_status = AsyncStatus::Busy;
        }
        if self.flush() < 0 {
            return -1;
        }
        1
    }

    /// PQgetCopyData
    pub fn get_copy_data(&mut self, is_async: bool) -> CopyData {
        if self.async_status != AsyncStatus::CopyOut && self.async_status != AsyncStatus::CopyBoth {
            self.append_error("no COPY in progress");
            return CopyData::Error;
        }
        loop {
            // pqGetCopyData3 / getCopyDataMessage
            while let Some(ty) = self.peek_message_type() {
                match ty {
                    b'A' | b'N' | b'S' => {
                        let (ty, body) = self.take_message().unwrap();
                        self.trace_in(ty, &body);
                        self.handle_async_in_copy(ty, &body);
                    }
                    b'd' => {
                        let (ty, body) = self.take_message().unwrap();
                        self.trace_in(ty, &body);
                        return CopyData::Row(body);
                    }
                    _ => {
                        // CopyDone (or anything else) ends the COPY; the
                        // message is left for get_result
                        if ty == b'c' && self.async_status == AsyncStatus::CopyBoth {
                            self.async_status = AsyncStatus::CopyIn;
                        } else {
                            self.async_status = AsyncStatus::Busy;
                        }
                        return CopyData::Done;
                    }
                }
            }
            if is_async {
                return CopyData::WouldBlock;
            }
            if self.wait(true, false) != 0 || self.read_data() < 0 {
                return CopyData::Error;
            }
        }
    }

    fn handle_async_in_copy(&mut self, ty: u8, body: &[u8]) {
        match ty {
            b'N' => self.get_error_notice(body, false),
            b'A' => {
                let mut c = super::conn::Cur::new(body);
                let pid = c.int32().unwrap_or(0);
                let relname = c.cstr().unwrap_or(b"").to_vec();
                let extra = c.cstr().unwrap_or(b"").to_vec();
                self.notifies.push_back(Notify {
                    relname,
                    be_pid: pid,
                    extra,
                });
            }
            _ => {
                let mut c = super::conn::Cur::new(body);
                if let (Some(k), Some(v)) = (c.cstr(), c.cstr()) {
                    let (k, v) = (k.to_vec(), v.to_vec());
                    self.param_status.retain(|(a, _)| *a != k);
                    self.param_status.insert(0, (k, v));
                }
            }
        }
    }

    /// PQputline / PQputnbytes
    pub fn put_line(&mut self, s: &[u8]) -> i32 {
        let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        if self.put_copy_data(&s[..n]) > 0 {
            0
        } else {
            -1
        }
    }

    /// PQendcopy: 0 ok, 1 failure
    pub fn end_copy(&mut self) -> i32 {
        if !matches!(
            self.async_status,
            AsyncStatus::CopyIn | AsyncStatus::CopyOut | AsyncStatus::CopyBoth
        ) {
            self.append_error("no COPY in progress");
            return 1;
        }
        if self.async_status == AsyncStatus::CopyIn || self.async_status == AsyncStatus::CopyBoth {
            self.put_msg(b'c', &[]);
            if self.queryclass != QueryClass::Simple {
                self.put_msg(b'S', &[]);
            }
        }
        if self.flush() != 0 && !self.nonblocking {
            return 1;
        }
        // make sure no data is waiting to be sent, abort if we are
        // non-blocking and the flush fails
        if self.nonblocking && self.is_busy() {
            return 1;
        }
        self.async_status = AsyncStatus::Busy;
        // Return to active duty
        let r = self.get_result();
        if let Some(r) = r {
            if r.status == ExecStatus::CommandOk {
                // drain to idle
                while let Some(_x) = self.get_result() {}
                return 0;
            }
            // the error message is already in the connection; libpq
            // passes it to the notice processor as well
            let msg = self.error_message.clone();
            if !msg.is_empty() {
                (self.notice_processor)(self.notice_arg, &msg);
            }
        }
        while let Some(_x) = self.get_result() {}
        1
    }

    // ── fastpath function calls (fe-protocol3.c pqFunctionCall3) ─────

    /// PQfn: call function `fnid` with binary arguments (`Ok` ints are sent
    /// as 4-byte integers). Returns the result bytes (None for NULL).
    pub fn fn_call(&mut self, fnid: Oid, args: &[FnArg]) -> (Option<PgResult>, Option<Vec<u8>>) {
        self.clear_error_state();
        if self.status != ConnStatus::Ok || self.socket() < 0 {
            self.append_error("connection pointer is NULL");
            let r = self.make_empty_result(ExecStatus::FatalError);
            return (Some(r), None);
        }
        if self.async_status != AsyncStatus::Idle {
            self.append_error("another command is already in progress");
            return (None, None);
        }
        let mut b = Vec::new();
        b.extend_from_slice(&fnid.to_be_bytes());
        b.extend_from_slice(&1i16.to_be_bytes());
        b.extend_from_slice(&1i16.to_be_bytes());
        b.extend_from_slice(&(args.len() as i16).to_be_bytes());
        for a in args {
            match a {
                FnArg::Int(v) => {
                    b.extend_from_slice(&4i32.to_be_bytes());
                    b.extend_from_slice(&v.to_be_bytes());
                }
                FnArg::Int64(v) => {
                    b.extend_from_slice(&8i32.to_be_bytes());
                    b.extend_from_slice(&v.to_be_bytes());
                }
                FnArg::Bytes(v) => {
                    b.extend_from_slice(&(v.len() as i32).to_be_bytes());
                    b.extend_from_slice(v);
                }
            }
        }
        b.extend_from_slice(&1i16.to_be_bytes());
        self.put_msg(b'F', &b);
        if self.flush() != 0 {
            let r = self.make_empty_result(ExecStatus::FatalError);
            return (Some(r), None);
        }
        let mut value: Option<Vec<u8>> = None;
        let mut status = ExecStatus::FatalError;
        let mut err: Option<PgResult> = None;
        loop {
            let Some((ty, body)) = self.take_message() else {
                if self.wait(true, false) != 0 || self.read_data() < 0 {
                    let r = self.make_empty_result(ExecStatus::FatalError);
                    return (Some(r), None);
                }
                continue;
            };
            self.trace_in(ty, &body);
            match ty {
                b'V' => {
                    let mut c = super::conn::Cur::new(&body);
                    let len = c.int32().unwrap_or(-1);
                    if len >= 0 {
                        value = Some(c.bytes(len as usize).unwrap_or(b"").to_vec());
                    }
                    status = ExecStatus::CommandOk;
                }
                b'E' => {
                    self.get_error_notice(&body, true);
                    err = self.result.take();
                    status = ExecStatus::FatalError;
                }
                b'A' | b'N' | b'S' => self.handle_async_in_copy(ty, &body),
                b'Z' => {
                    self.xact_status = match body.first() {
                        Some(b'I') => TransStatus::Idle,
                        Some(b'T') => TransStatus::InTrans,
                        Some(b'E') => TransStatus::InError,
                        _ => TransStatus::Unknown,
                    };
                    if let Some(e) = err {
                        self.error_reported = self.error_message.len();
                        return (Some(e), None);
                    }
                    let r = self.make_empty_result(status);
                    return (Some(r), value);
                }
                _ => {
                    self.append_error(&format!("protocol error: id=0x{:x}", ty));
                }
            }
        }
    }

    // ── cancel ─────────────────────────────────────────────────────────

    /// PQgetCancel + PQcancel: Ok or the error text
    pub fn cancel(&mut self) -> Result<(), String> {
        if self.be_pid == 0 {
            return Err("PQcancel() -- no cancel object supplied".into());
        }
        let Some(addr) = self.current_addr() else {
            return Err("PQcancel() -- connect() failed: no address".into());
        };
        let mut pkt = Vec::with_capacity(16);
        pkt.extend_from_slice(&16i32.to_be_bytes());
        pkt.extend_from_slice(&80877102i32.to_be_bytes());
        pkt.extend_from_slice(&self.be_pid.to_be_bytes());
        pkt.extend_from_slice(&self.be_key.to_be_bytes());
        use std::io::{Read, Write};
        match addr {
            CancelAddr::Inet(a) => {
                let mut s = std::net::TcpStream::connect(a)
                    .map_err(|e| format!("PQcancel() -- connect() failed: {}", e))?;
                s.write_all(&pkt)
                    .map_err(|e| format!("PQcancel() -- send() failed: {}", e))?;
                let mut b = [0u8; 1];
                let _ = s.read(&mut b);
            }
            CancelAddr::Unix(p) => {
                let mut s = std::os::unix::net::UnixStream::connect(p)
                    .map_err(|e| format!("PQcancel() -- connect() failed: {}", e))?;
                s.write_all(&pkt)
                    .map_err(|e| format!("PQcancel() -- send() failed: {}", e))?;
                let mut b = [0u8; 1];
                let _ = s.read(&mut b);
            }
        }
        Ok(())
    }
}

/// A fastpath argument
pub enum FnArg {
    Int(i32),
    Int64(i64),
    Bytes(Vec<u8>),
}

pub enum CancelAddr {
    Inet(std::net::SocketAddr),
    Unix(String),
}

impl PgConn {
    /// PQsetClientEncoding: 0 ok, -1 failure
    pub fn set_client_encoding(&mut self, encoding: &[u8]) -> i32 {
        if self.status != ConnStatus::Ok {
            return -1;
        }
        let n = encoding
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(encoding.len());
        let mut enc = encoding[..n].to_vec();
        if enc == b"auto" {
            enc = b"UTF8".to_vec();
        }
        if enc.len() > 100 {
            return -1;
        }
        let mut q = b"set client_encoding to '".to_vec();
        q.extend_from_slice(&enc);
        q.push(b'\'');
        match self.exec(&q) {
            Some(r) if r.status == ExecStatus::CommandOk => 0,
            _ => -1,
        }
    }

    /// PQstatus
    pub fn status(&self) -> ConnStatus {
        self.status
    }
}
