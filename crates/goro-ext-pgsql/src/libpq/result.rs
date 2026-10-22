//! PGresult (fe-exec.c) and the error message layout of
//! fe-protocol3.c (pqBuildErrorMessage3, reportErrorPosition).

use super::encoding;
use super::*;

/// PGresAttDesc
#[derive(Clone, Debug, Default)]
pub struct AttDesc {
    pub name: Vec<u8>,
    pub tableid: Oid,
    pub columnid: i32,
    pub format: i32,
    pub typid: Oid,
    pub typlen: i32,
    pub atttypmod: i32,
}

/// PGresult
#[derive(Clone, Debug)]
pub struct PgResult {
    pub status: ExecStatus,
    pub attrs: Vec<AttDesc>,
    pub tuples: Vec<Vec<Option<Vec<u8>>>>,
    pub params: Vec<Oid>,
    pub cmd_status: Vec<u8>,
    pub err_msg: Option<Vec<u8>>,
    /// (field code, value), most recent first (pqSaveMessageField pushes
    /// at the head)
    pub err_fields: Vec<(u8, Vec<u8>)>,
    pub err_query: Option<Vec<u8>>,
    pub binary: bool,
    pub client_encoding: i32,
    pub memory_size: usize,
}

impl PgResult {
    /// PQmakeEmptyPGresult (without a connection)
    pub fn new(status: ExecStatus) -> PgResult {
        PgResult {
            status,
            attrs: Vec::new(),
            tuples: Vec::new(),
            params: Vec::new(),
            cmd_status: Vec::new(),
            err_msg: None,
            err_fields: Vec::new(),
            err_query: None,
            binary: false,
            client_encoding: encoding::PG_SQL_ASCII,
            // sizeof(PGresult)
            memory_size: 208,
        }
    }

    pub fn ntuples(&self) -> usize {
        self.tuples.len()
    }

    pub fn nfields(&self) -> usize {
        self.attrs.len()
    }

    /// PQgetvalue: "" for NULL
    pub fn value(&self, row: usize, col: usize) -> &[u8] {
        match self.tuples.get(row).and_then(|r| r.get(col)) {
            Some(Some(v)) => v,
            _ => b"",
        }
    }

    pub fn is_null(&self, row: usize, col: usize) -> bool {
        matches!(self.tuples.get(row).and_then(|r| r.get(col)), Some(None))
    }

    pub fn get_length(&self, row: usize, col: usize) -> usize {
        match self.tuples.get(row).and_then(|r| r.get(col)) {
            Some(Some(v)) => v.len(),
            _ => 0,
        }
    }

    pub fn fname(&self, col: usize) -> Option<&[u8]> {
        self.attrs.get(col).map(|a| a.name.as_slice())
    }

    pub fn ftype(&self, col: usize) -> Oid {
        self.attrs.get(col).map(|a| a.typid).unwrap_or(0)
    }

    pub fn fsize(&self, col: usize) -> i32 {
        self.attrs.get(col).map(|a| a.typlen).unwrap_or(0)
    }

    pub fn fmod(&self, col: usize) -> i32 {
        self.attrs.get(col).map(|a| a.atttypmod).unwrap_or(0)
    }

    pub fn ftable(&self, col: usize) -> Oid {
        self.attrs.get(col).map(|a| a.tableid).unwrap_or(0)
    }

    pub fn ftablecol(&self, col: usize) -> i32 {
        self.attrs.get(col).map(|a| a.columnid).unwrap_or(0)
    }

    pub fn fformat(&self, col: usize) -> i32 {
        self.attrs.get(col).map(|a| a.format).unwrap_or(0)
    }

    /// PQfnumber: unquoted names are downcased, "quoted" ones kept
    pub fn fnumber(&self, name: &[u8]) -> i32 {
        // fast path: an exact match
        if let Some(p) = self.attrs.iter().position(|a| a.name == name) {
            return p as i32;
        }
        let mut field = Vec::with_capacity(name.len());
        let mut in_quotes = false;
        let mut all_lower = true;
        let mut i = 0;
        while i < name.len() {
            let c = name[i];
            if in_quotes {
                if c == b'"' {
                    if name.get(i + 1) == Some(&b'"') {
                        field.push(b'"');
                        i += 2;
                        continue;
                    }
                    in_quotes = false;
                } else {
                    field.push(c);
                }
            } else if c == b'"' {
                in_quotes = true;
            } else {
                let l = c.to_ascii_lowercase();
                if l != c {
                    all_lower = false;
                }
                field.push(l);
            }
            i += 1;
        }
        let _ = all_lower;
        self.attrs
            .iter()
            .position(|a| a.name == field)
            .map(|p| p as i32)
            .unwrap_or(-1)
    }

    /// PQresultErrorField
    pub fn error_field(&self, code: u8) -> Option<&[u8]> {
        self.err_fields
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_slice())
    }

    /// PQresultErrorMessage
    pub fn error_message(&self) -> &[u8] {
        self.err_msg.as_deref().unwrap_or(b"")
    }

    /// PQcmdTuples
    pub fn cmd_tuples(&self) -> &[u8] {
        let s = self.cmd_status.as_slice();
        let p = if s.starts_with(b"INSERT ") {
            // skip the oid
            let mut p = 7;
            while p < s.len() && s[p] != b' ' {
                p += 1;
            }
            if p >= s.len() {
                return b"";
            }
            p + 1
        } else if s.starts_with(b"DELETE ")
            || s.starts_with(b"UPDATE ")
            || s.starts_with(b"SELECT ")
        {
            7
        } else if s.starts_with(b"FETCH ") {
            6
        } else if s.starts_with(b"MOVE ") || s.starts_with(b"COPY ") {
            5
        } else if s.starts_with(b"MERGE ") {
            6
        } else {
            return b"";
        };
        let rest = &s[p..];
        if rest.is_empty() || !rest.iter().all(|c| c.is_ascii_digit()) {
            return b"";
        }
        rest
    }

    /// PQoidValue
    pub fn oid_value(&self) -> Oid {
        let s = self.cmd_status.as_slice();
        if !s.starts_with(b"INSERT ") {
            return 0;
        }
        let rest = &s[7..];
        let end = rest
            .iter()
            .position(|c| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        std::str::from_utf8(&rest[..end])
            .ok()
            .and_then(|t| t.parse::<u64>().ok())
            .map(|v| v as Oid)
            .unwrap_or(0)
    }
}

/// PQresStatus
pub fn res_status(s: ExecStatus) -> &'static str {
    match s {
        ExecStatus::EmptyQuery => "PGRES_EMPTY_QUERY",
        ExecStatus::CommandOk => "PGRES_COMMAND_OK",
        ExecStatus::TuplesOk => "PGRES_TUPLES_OK",
        ExecStatus::CopyOut => "PGRES_COPY_OUT",
        ExecStatus::CopyIn => "PGRES_COPY_IN",
        ExecStatus::BadResponse => "PGRES_BAD_RESPONSE",
        ExecStatus::NonfatalError => "PGRES_NONFATAL_ERROR",
        ExecStatus::FatalError => "PGRES_FATAL_ERROR",
        ExecStatus::CopyBoth => "PGRES_COPY_BOTH",
        ExecStatus::SingleTuple => "PGRES_SINGLE_TUPLE",
        ExecStatus::PipelineSync => "PGRES_PIPELINE_SYNC",
        ExecStatus::PipelineAborted => "PGRES_PIPELINE_ABORTED",
        ExecStatus::TuplesChunk => "PGRES_TUPLES_CHUNK",
    }
}

const DISPLAY_SIZE: usize = 60;
const MIN_RIGHT_CUT: usize = 10;

/// reportErrorPosition: the "LINE n: ..." line and its cursor
fn report_error_position(msg: &mut Vec<u8>, query: &[u8], loc: i64, enc: i32) {
    let loc = loc - 1;
    if loc < 0 {
        return;
    }
    let loc = loc as usize;
    let mut wquery: Vec<u8> = query.to_vec();
    if let Some(p) = wquery.iter().position(|&c| c == 0) {
        wquery.truncate(p);
    }
    let slen = wquery.len() + 1;
    let mut qidx = vec![0usize; slen];
    let mut scridx = vec![0usize; slen];
    let mb = encoding::max_length(enc) != 1;
    let mut qoffset = 0usize;
    let mut scroffset = 0usize;
    let mut loc_line = 1;
    let mut ibeg = 0usize;
    let mut iend: Option<usize> = None;
    let mut cno = 0usize;
    while qoffset < wquery.len() {
        let ch = wquery[qoffset];
        qidx[cno] = qoffset;
        scridx[cno] = scroffset;
        if ch == b'\t' {
            wquery[qoffset] = b' ';
        } else if ch == b'\r' || ch == b'\n' {
            if cno < loc {
                if ch == b'\r' || cno == 0 || wquery[qidx[cno - 1]] != b'\r' {
                    loc_line += 1;
                }
                ibeg = cno + 1;
            } else {
                iend = Some(cno);
                break;
            }
        }
        if mb {
            let mut w = encoding::dsplen(enc, &wquery[qoffset..]);
            if w == 0 {
                w = 1;
            }
            scroffset += w;
            let l = encoding::mblen(enc, &wquery[qoffset..]);
            qoffset += l.min(wquery.len() - qoffset).max(1);
        } else {
            scroffset += 1;
            qoffset += 1;
        }
        cno += 1;
    }
    let mut iend = match iend {
        Some(e) => e,
        None => {
            qidx[cno] = qoffset;
            scridx[cno] = scroffset;
            cno
        }
    };
    if loc > cno {
        return;
    }
    let mut beg_trunc = false;
    let mut end_trunc = false;
    if scridx[iend] - scridx[ibeg] > DISPLAY_SIZE {
        if scridx[ibeg] + DISPLAY_SIZE >= scridx[loc] + MIN_RIGHT_CUT {
            while scridx[iend] - scridx[ibeg] > DISPLAY_SIZE {
                iend -= 1;
            }
            end_trunc = true;
        } else {
            while scridx[loc] + MIN_RIGHT_CUT < scridx[iend] {
                iend -= 1;
                end_trunc = true;
            }
            while scridx[iend] - scridx[ibeg] > DISPLAY_SIZE {
                ibeg += 1;
                beg_trunc = true;
            }
        }
    }
    let prefix_start = msg.len();
    msg.extend_from_slice(format!("LINE {}: ", loc_line).as_bytes());
    if beg_trunc {
        msg.extend_from_slice(b"...");
    }
    let mut scr = 0usize;
    let mut i = prefix_start;
    while i < msg.len() {
        let mut w = encoding::dsplen(enc, &msg[i..]);
        if w == 0 {
            w = 1;
        }
        scr += w;
        i += encoding::mblen(enc, &msg[i..]).max(1);
    }
    msg.extend_from_slice(&wquery[qidx[ibeg]..qidx[iend]]);
    if end_trunc {
        msg.extend_from_slice(b"...");
    }
    msg.push(b'\n');
    scr += scridx[loc] - scridx[ibeg];
    msg.extend(std::iter::repeat_n(b' ', scr));
    msg.extend_from_slice(b"^\n");
}

/// pqBuildErrorMessage3
pub fn build_error_message(
    res: &PgResult,
    verbosity: Verbosity,
    show_context: ContextVisibility,
) -> Vec<u8> {
    let mut msg = Vec::new();
    if res.err_fields.is_empty() {
        match &res.err_msg {
            Some(m) if !m.is_empty() => msg.extend_from_slice(m),
            _ => msg.extend_from_slice(b"no error message available\n"),
        }
        return msg;
    }
    let f = |c: u8| res.error_field(c);
    let mut verbosity = verbosity;
    if let Some(v) = f(PG_DIAG_SEVERITY) {
        msg.extend_from_slice(v);
        msg.extend_from_slice(b":  ");
    }
    if verbosity == Verbosity::Sqlstate {
        if let Some(v) = f(PG_DIAG_SQLSTATE) {
            msg.extend_from_slice(v);
            msg.push(b'\n');
            return msg;
        }
        verbosity = Verbosity::Terse;
    }
    if verbosity == Verbosity::Verbose {
        if let Some(v) = f(PG_DIAG_SQLSTATE) {
            msg.extend_from_slice(v);
            msg.extend_from_slice(b": ");
        }
    }
    if let Some(v) = f(PG_DIAG_MESSAGE_PRIMARY) {
        msg.extend_from_slice(v);
    }
    let atoi = |v: &[u8]| -> i64 {
        let s = String::from_utf8_lossy(v);
        let t: String = s
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        t.parse().unwrap_or(0)
    };
    let mut querytext: Option<&[u8]> = None;
    let mut querypos = 0i64;
    if let Some(v) = f(PG_DIAG_STATEMENT_POSITION) {
        if verbosity != Verbosity::Terse && res.err_query.is_some() {
            querytext = res.err_query.as_deref();
            querypos = atoi(v);
        } else {
            msg.extend_from_slice(b" at character ");
            msg.extend_from_slice(v);
        }
    } else if let Some(v) = f(PG_DIAG_INTERNAL_POSITION) {
        let qt = f(PG_DIAG_INTERNAL_QUERY);
        if verbosity != Verbosity::Terse && qt.is_some() {
            querytext = qt;
            querypos = atoi(v);
        } else {
            msg.extend_from_slice(b" at character ");
            msg.extend_from_slice(v);
        }
    }
    msg.push(b'\n');
    if verbosity != Verbosity::Terse {
        if let Some(q) = querytext {
            if querypos > 0 {
                report_error_position(&mut msg, q, querypos, res.client_encoding);
            }
        }
        for (code, label) in [
            (PG_DIAG_MESSAGE_DETAIL, "DETAIL:  "),
            (PG_DIAG_MESSAGE_HINT, "HINT:  "),
            (PG_DIAG_INTERNAL_QUERY, "QUERY:  "),
        ] {
            if let Some(v) = f(code) {
                msg.extend_from_slice(label.as_bytes());
                msg.extend_from_slice(v);
                msg.push(b'\n');
            }
        }
        if show_context == ContextVisibility::Always
            || (show_context == ContextVisibility::Errors && res.status == ExecStatus::FatalError)
        {
            if let Some(v) = f(PG_DIAG_CONTEXT) {
                msg.extend_from_slice(b"CONTEXT:  ");
                msg.extend_from_slice(v);
                msg.push(b'\n');
            }
        }
    }
    if verbosity == Verbosity::Verbose {
        for (code, label) in [
            (PG_DIAG_SCHEMA_NAME, "SCHEMA NAME:  "),
            (PG_DIAG_TABLE_NAME, "TABLE NAME:  "),
            (PG_DIAG_COLUMN_NAME, "COLUMN NAME:  "),
            (PG_DIAG_DATATYPE_NAME, "DATATYPE NAME:  "),
            (PG_DIAG_CONSTRAINT_NAME, "CONSTRAINT NAME:  "),
        ] {
            if let Some(v) = f(code) {
                msg.extend_from_slice(label.as_bytes());
                msg.extend_from_slice(v);
                msg.push(b'\n');
            }
        }
        let valf = f(PG_DIAG_SOURCE_FILE);
        let vall = f(PG_DIAG_SOURCE_LINE);
        let val = f(PG_DIAG_SOURCE_FUNCTION);
        if val.is_some() || valf.is_some() || vall.is_some() {
            msg.extend_from_slice(b"LOCATION:  ");
            if let Some(v) = val {
                msg.extend_from_slice(v);
                msg.extend_from_slice(b", ");
            }
            if let (Some(a), Some(b)) = (valf, vall) {
                msg.extend_from_slice(a);
                msg.push(b':');
                msg.extend_from_slice(b);
            }
            msg.push(b'\n');
        }
    }
    msg
}
