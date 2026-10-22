//! Protocol message tracing (fe-trace.c): one line per message.

use std::io::Write;

use super::{PQTRACE_REGRESS_MODE, PQTRACE_SUPPRESS_TIMESTAMPS};

struct Out<'a> {
    b: &'a [u8],
    p: usize,
    line: Vec<u8>,
    regress: bool,
}

impl Out<'_> {
    fn int32(&mut self, suppress: bool) -> i32 {
        if self.p + 4 > self.b.len() {
            self.p = self.b.len();
            return 0;
        }
        let v = i32::from_be_bytes([
            self.b[self.p],
            self.b[self.p + 1],
            self.b[self.p + 2],
            self.b[self.p + 3],
        ]);
        self.p += 4;
        if suppress && self.regress {
            self.line.extend_from_slice(b" NNNN");
        } else {
            self.line.extend_from_slice(format!(" {}", v).as_bytes());
        }
        v
    }
    fn int16(&mut self) -> i16 {
        if self.p + 2 > self.b.len() {
            self.p = self.b.len();
            return 0;
        }
        let v = i16::from_be_bytes([self.b[self.p], self.b[self.p + 1]]);
        self.p += 2;
        self.line.extend_from_slice(format!(" {}", v).as_bytes());
        v
    }
    fn byte1(&mut self) -> u8 {
        let v = self.b.get(self.p).copied().unwrap_or(0);
        self.p += 1;
        self.line.push(b' ');
        self.line.push(v);
        v
    }
    fn string(&mut self, suppress: bool) {
        let rest = &self.b[self.p.min(self.b.len())..];
        let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        if suppress && self.regress {
            self.line.extend_from_slice(b" \"SSSS\"");
        } else {
            self.line.extend_from_slice(b" \"");
            self.line.extend_from_slice(&rest[..n]);
            self.line.push(b'"');
        }
        self.p += n + 1;
    }
    fn nchar(&mut self, len: usize) {
        let end = (self.p + len).min(self.b.len());
        let v = &self.b[self.p.min(end)..end];
        self.line.extend_from_slice(b" '");
        for &c in v {
            if (0x20..0x7f).contains(&c) {
                self.line.push(c);
            } else {
                self.line
                    .extend_from_slice(format!("\\x{:02x}", c).as_bytes());
            }
        }
        self.line.push(b'\'');
        self.p = end;
    }
    fn more(&self) -> bool {
        self.p < self.b.len()
    }
}

fn timestamp() -> String {
    unsafe {
        let mut tv: libc::timeval = std::mem::zeroed();
        libc::gettimeofday(&mut tv, std::ptr::null_mut());
        let t = tv.tv_sec;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            tv.tv_usec
        )
    }
}

/// pqTraceOutputMessage
pub fn trace_message(f: &mut std::fs::File, flags: i32, from_server: bool, ty: u8, body: &[u8]) {
    let mut o = Out {
        b: body,
        p: 0,
        line: Vec::new(),
        regress: flags & PQTRACE_REGRESS_MODE != 0,
    };
    if flags & PQTRACE_SUPPRESS_TIMESTAMPS == 0 {
        o.line.extend_from_slice(timestamp().as_bytes());
        o.line.push(b'\t');
    }
    o.line
        .extend_from_slice(if from_server { b"B\t" } else { b"F\t" });
    o.line
        .extend_from_slice(format!("{}\t", body.len() + 4).as_bytes());
    let name: &str = if from_server {
        match ty {
            b'R' => "Authentication",
            b'S' => "ParameterStatus",
            b'K' => "BackendKeyData",
            b'Z' => "ReadyForQuery",
            b'T' => "RowDescription",
            b'D' => "DataRow",
            b'C' => "CommandComplete",
            b'E' => "ErrorResponse",
            b'N' => "NoticeResponse",
            b'A' => "NotificationResponse",
            b'I' => "EmptyQueryResponse",
            b'1' => "ParseComplete",
            b'2' => "BindComplete",
            b'3' => "CloseComplete",
            b'n' => "NoData",
            b't' => "ParameterDescription",
            b'G' => "CopyInResponse",
            b'H' => "CopyOutResponse",
            b'W' => "CopyBothResponse",
            b'd' => "CopyData",
            b'c' => "CopyDone",
            b's' => "PortalSuspended",
            b'V' => "FunctionCallResponse",
            b'v' => "NegotiateProtocolVersion",
            _ => "Unknown message",
        }
    } else {
        match ty {
            b'Q' => "Query",
            b'P' => "Parse",
            b'B' => "Bind",
            b'D' => "Describe",
            b'E' => "Execute",
            b'S' => "Sync",
            b'X' => "Terminate",
            b'C' => "Close",
            b'p' => "PasswordMessage",
            b'F' => "FunctionCall",
            b'd' => "CopyData",
            b'c' => "CopyDone",
            b'f' => "CopyFail",
            b'H' => "Flush",
            _ => "Unknown message",
        }
    };
    o.line.extend_from_slice(name.as_bytes());
    o.line.push(b'\t');
    match (from_server, ty) {
        (false, b'Q') | (true, b'C') => o.string(false),
        (false, b'P') => {
            o.string(false);
            o.string(false);
            let n = o.int16();
            for _ in 0..n.max(0) {
                o.int32(true);
            }
        }
        (false, b'B') => {
            o.string(false);
            o.string(false);
            let nf = o.int16();
            for _ in 0..nf.max(0) {
                o.int16();
            }
            let np = o.int16();
            for _ in 0..np.max(0) {
                let l = o.int32(false);
                if l >= 0 {
                    o.nchar(l as usize);
                }
            }
            let nr = o.int16();
            for _ in 0..nr.max(0) {
                o.int16();
            }
        }
        (false, b'D') | (false, b'C') => {
            o.byte1();
            o.string(false);
        }
        (false, b'E') => {
            o.string(false);
            o.int32(false);
        }
        (false, b'F') => {
            o.int32(true);
            let nf = o.int16();
            for _ in 0..nf.max(0) {
                o.int16();
            }
            let na = o.int16();
            for _ in 0..na.max(0) {
                let l = o.int32(false);
                if l >= 0 {
                    o.nchar(l as usize);
                }
            }
            o.int16();
        }
        (_, b'd') => {
            let l = body.len();
            o.nchar(l);
        }
        (false, b'f') => o.string(false),
        (true, b'R') => {
            o.int32(false);
            if o.more() {
                let l = body.len() - o.p;
                o.nchar(l);
            }
        }
        (true, b'S') => {
            o.string(false);
            o.string(false);
        }
        (true, b'K') => {
            o.int32(true);
            o.int32(true);
        }
        (true, b'Z') => {
            o.byte1();
        }
        (true, b'T') => {
            let n = o.int16();
            for _ in 0..n.max(0) {
                o.string(false);
                o.int32(true);
                o.int16();
                o.int32(true);
                o.int16();
                o.int32(false);
                o.int16();
            }
        }
        (true, b'D') => {
            let n = o.int16();
            for _ in 0..n.max(0) {
                let l = o.int32(false);
                if l >= 0 {
                    o.nchar(l as usize);
                }
            }
        }
        (true, b'E') | (true, b'N') => {
            while o.more() {
                let code = o.byte1();
                if code == 0 {
                    break;
                }
                let suppress = matches!(code, b'L' | b'F' | b'R');
                o.string(suppress);
            }
        }
        (true, b'A') => {
            o.int32(true);
            o.string(false);
            o.string(false);
        }
        (true, b't') => {
            let n = o.int16();
            for _ in 0..n.max(0) {
                o.int32(true);
            }
        }
        (true, b'G') | (true, b'H') | (true, b'W') => {
            o.byte1();
            let n = o.int16();
            for _ in 0..n.max(0) {
                o.int16();
            }
        }
        (true, b'V') => {
            let l = o.int32(false);
            if l >= 0 {
                o.nchar(l as usize);
            }
        }
        (false, b'p') => {
            let l = body.len();
            o.nchar(l);
        }
        _ => {}
    }
    o.line.push(b'\n');
    let _ = f.write_all(&o.line);
}
