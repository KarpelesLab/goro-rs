//! The PostgreSQL database handle (ext/pdo_pgsql/pgsql_driver.c) over
//! goro-ext-pgsql's libpq port.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_pgsql::libpq::{
    ConnStatus, ExecStatus, PG_DIAG_SQLSTATE, PgConn, PgResult, TransStatus,
};

use super::statement::{S, STMT_METHODS};
use super::*;
use crate::core::*;

pub type ConnRef = Rc<RefCell<PgConn>>;

pub const PGRES_FATAL_ERROR: i64 = ExecStatus::FatalError as i64;
const CONNECTION_FAILURE_SQLSTATE: &[u8] = b"08006";

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// pdo_pgsql_error_info
#[derive(Clone, Debug, Default)]
pub struct EInfo {
    pub errcode: i64,
    pub errmsg: Option<Vec<u8>>,
}

/// pdo_pgsql_db_handle
#[derive(Default)]
pub struct H {
    pub server: Option<ConnRef>,
    pub attached: bool,
    pub einfo: EInfo,
    pub pgoid: i64,
    pub stmt_counter: u32,
    pub emulate_prepares: bool,
    pub disable_prepares: bool,
    /// the resource ids of the open large object streams
    pub lob_streams: Vec<i64>,
    pub notice_callback: Option<Value>,
    pub default_fetching_laziness: bool,
    pub running_stmt: Option<Weak<RefCell<Stmt>>>,
    /// the notice processor argument
    pub id: usize,
}

/// Run `f` on the PostgreSQL handle of a PDO handle
pub fn with_h<R>(dbh: &DbhRef, f: impl FnOnce(&mut H) -> R) -> Option<R> {
    let mut d = dbh.try_borrow_mut().ok()?;
    let h = d.driver_data.as_mut()?.downcast_mut::<H>()?;
    Some(f(h))
}

/// H->server
pub fn server(dbh: &DbhRef) -> Option<ConnRef> {
    with_h(dbh, |h| h.server.clone()).flatten()
}

/// _pdo_pgsql_trim_message
pub fn trim_message(m: &[u8]) -> Vec<u8> {
    let m = match m.iter().position(|&c| c == 0) {
        Some(p) => &m[..p],
        None => m,
    };
    if m.is_empty() {
        return Vec::new();
    }
    let mut i = m.len() - 1;
    if i > 1 && (m[i - 1] == b'\r' || m[i - 1] == b'\n') && m[i] == b'.' {
        i -= 1;
    }
    while i > 0 && (m[i] == b'\r' || m[i] == b'\n') {
        i -= 1;
    }
    m[..i + 1].to_vec()
}

/// _pdo_pgsql_error
pub fn pgsql_error(
    vm: Option<&mut Vm>,
    dbh: &DbhRef,
    stmt: Option<&StmtRef>,
    errcode: i64,
    sqlstate: Option<&[u8]>,
    msg: Option<&[u8]>,
) -> i64 {
    let errmsg = server(dbh).map(|c| c.borrow().error_message().to_vec());
    let state: Vec<u8> = match sqlstate {
        Some(s) if !s.is_empty() && s.len() < 6 => s.to_vec(),
        _ => b"HY000".to_vec(),
    };
    let einfo_msg = match msg {
        Some(m) => Some(m.to_vec()),
        None => errmsg.map(|e| trim_message(&e)),
    };
    with_h(dbh, |h| {
        h.einfo.errcode = errcode;
        h.einfo.errmsg = einfo_msg.clone();
    });
    match stmt {
        Some(s) => s.borrow_mut().error_code = state.clone(),
        None => dbh.borrow_mut().error_code = state.clone(),
    }
    if dbh.borrow().methods.is_none() {
        if let Some(vm) = vm {
            throw_exception(
                vm,
                errcode as u32,
                einfo_msg.as_deref().unwrap_or(b""),
                &state,
            );
        }
    }
    errcode
}

/// pdo_pgsql_sqlstate()
pub fn sqlstate(r: Option<&PgResult>) -> Option<Vec<u8>> {
    r.and_then(|r| r.error_field(PG_DIAG_SQLSTATE).map(|s| s.to_vec()))
}

/// PQresultStatus(res) (NULL → PGRES_FATAL_ERROR)
pub fn status_of(r: Option<&PgResult>) -> ExecStatus {
    r.map(|r| r.status).unwrap_or(ExecStatus::FatalError)
}

/// pdo_pgsql_fetch_error_func
fn fetch_error_func(_vm: &mut Vm, dbh: &DbhRef, _stmt: Option<&StmtRef>, info: &mut PhpArray) {
    let einfo = with_h(dbh, |h| h.einfo.clone()).unwrap_or_default();
    if einfo.errcode != 0 {
        info.push(Value::Long(einfo.errcode));
    } else {
        info.push(Value::Null);
    }
    if let Some(m) = einfo.errmsg {
        info.push(vstr(m));
    }
}

/// pdo_pgsql_close_lob_streams: queued, closed when control returns to PHP
pub fn close_lob_streams(dbh: &DbhRef) {
    let ids = with_h(dbh, |h| std::mem::take(&mut h.lob_streams)).unwrap_or_default();
    queue_stream_close(ids);
}

/// pgsql_handle_closer
fn handle_closer(dbh: &DbhRef) {
    close_lob_streams(dbh);
    let h = dbh.borrow_mut().driver_data.take();
    if let Some(h) = h {
        if let Ok(h) = h.downcast::<H>() {
            unregister_handle(h.id);
            if let Some(c) = &h.server {
                if let Ok(mut c) = c.try_borrow_mut() {
                    c.finish();
                }
            }
        }
    }
}

/// pgsql_handle_preparer
fn handle_preparer(
    vm: &mut Vm,
    dbh: &DbhRef,
    sql: &[u8],
    stmt: &StmtRef,
    driver_options: Option<&Arr>,
) -> bool {
    let (emulate_default, disable_default, lazy_default) = with_h(dbh, |h| {
        (
            h.emulate_prepares,
            h.disable_prepares,
            h.default_fetching_laziness,
        )
    })
    .unwrap_or((false, false, false));
    let mut s = S {
        conn: server(dbh),
        ..S::default()
    };
    let scrollable =
        attr_lval(driver_options, PDO_ATTR_CURSOR, PDO_CURSOR_FWDONLY) == PDO_CURSOR_SCROLL;
    let mut emulate = false;
    let mut execute_only = false;
    if scrollable {
        let n = with_h(dbh, |h| {
            h.stmt_counter += 1;
            h.stmt_counter
        })
        .unwrap_or(1);
        s.cursor_name = Some(format!("pdo_crsr_{:08x}", n).into_bytes());
        emulate = true;
    } else if driver_options.is_some() {
        if attr_lval(
            driver_options,
            PDO_ATTR_EMULATE_PREPARES,
            emulate_default as i64,
        ) == 1
        {
            emulate = true;
        }
        if attr_lval(
            driver_options,
            PDO_PGSQL_ATTR_DISABLE_PREPARES,
            disable_default as i64,
        ) == 1
        {
            execute_only = true;
        }
    } else {
        emulate = emulate_default;
        execute_only = disable_default;
    }
    {
        let mut st = stmt.borrow_mut();
        st.methods = Some(&STMT_METHODS);
        if emulate {
            st.supports_placeholders = PDO_PLACEHOLDER_NONE;
        } else {
            st.supports_placeholders = PDO_PLACEHOLDER_NAMED;
            st.named_rewrite_template = Some("$%d");
        }
    }
    let prefetch = driver_options.and_then(|o| o.borrow().get_int(PDO_ATTR_PREFETCH).cloned());
    s.is_unbuffered = match prefetch {
        Some(v) => match get_long_param(vm, &v) {
            Some(l) => l == 0,
            None => {
                vm.current_exception = None;
                lazy_default
            }
        },
        None => lazy_default,
    };
    stmt.borrow_mut().driver_data = Some(Box::new(s));

    let mut nsql = Vec::new();
    let ret = crate::parser::parse_params(vm, stmt, sql, &mut nsql);
    let query = match ret {
        -1 => {
            let ec = stmt.borrow().error_code.clone();
            dbh.borrow_mut().error_code = ec;
            return false;
        }
        1 => nsql,
        _ => sql.to_vec(),
    };
    let stmt_name = if !emulate && !execute_only {
        let n = with_h(dbh, |h| {
            h.stmt_counter += 1;
            h.stmt_counter
        })
        .unwrap_or(1);
        Some(format!("pdo_stmt_{:08x}", n).into_bytes())
    } else {
        None
    };
    super::statement::with_s(stmt, |s| {
        s.query = query;
        s.stmt_name = stmt_name;
    });
    true
}

/// pgsql_handle_in_transaction
pub fn in_transaction(dbh: &DbhRef) -> bool {
    match server(dbh) {
        Some(c) => match c.try_borrow() {
            Ok(c) => (c.transaction_status() as i32) > (TransStatus::Idle as i32),
            Err(_) => false,
        },
        None => false,
    }
}

/// pgsql_handle_doer
fn handle_doer(vm: &mut Vm, dbh: &DbhRef, sql: &[u8]) -> i64 {
    let Some(conn) = server(dbh) else {
        return -1;
    };
    let in_trans = in_transaction(dbh);
    let res = conn.borrow_mut().exec(sql);
    let Some(res) = res else {
        pgsql_error(Some(vm), dbh, None, PGRES_FATAL_ERROR, None, None);
        return -1;
    };
    let qs = res.status;
    if qs != ExecStatus::CommandOk && qs != ExecStatus::TuplesOk {
        let st = sqlstate(Some(&res));
        pgsql_error(Some(vm), dbh, None, qs as i64, st.as_deref(), None);
        return -1;
    }
    let oid = res.oid_value() as i64;
    with_h(dbh, |h| h.pgoid = oid);
    let ret = if qs == ExecStatus::CommandOk {
        atol(res.cmd_tuples())
    } else {
        0
    };
    if in_trans && !in_transaction(dbh) {
        close_lob_streams(dbh);
    }
    ret
}

/// pgsql_handle_quoter
fn handle_quoter(_vm: &mut Vm, dbh: &DbhRef, unquoted: &[u8], paramtype: i64) -> Option<Vec<u8>> {
    let conn = server(dbh)?;
    let mut out = vec![b'\''];
    if paramtype == PDO_PARAM_LOB {
        let esc = conn.borrow_mut().escape_bytea_conn(unquoted);
        out.extend_from_slice(&esc);
    } else {
        let (esc, err) = conn.borrow_mut().escape_string_conn(unquoted);
        if err {
            return None;
        }
        out.extend_from_slice(&esc);
    }
    out.push(b'\'');
    Some(out)
}

/// pdo_pgsql_last_insert_id
fn last_insert_id(vm: &mut Vm, dbh: &DbhRef, name: Option<&[u8]>) -> Option<Vec<u8>> {
    let conn = server(dbh)?;
    let res = match name {
        None => conn.borrow_mut().exec(b"SELECT LASTVAL()"),
        Some(n) => {
            conn.borrow_mut()
                .exec_params(b"SELECT CURRVAL($1)", &[], &[Some(n.to_vec())], &[], 0)
        }
    };
    let status = status_of(res.as_ref());
    match res {
        Some(r) if status == ExecStatus::TuplesOk => Some(r.value(0, 0).to_vec()),
        r => {
            let st = sqlstate(r.as_ref());
            pgsql_error(Some(vm), dbh, None, status as i64, st.as_deref(), None);
            None
        }
    }
}

/// pdo_libpq_version
pub fn libpq_version() -> String {
    let version = goro_ext_pgsql::libpq::LIB_VERSION;
    let major = version / 10000;
    if major >= 10 {
        format!("{}.{}", major, version % 10000)
    } else {
        format!("{}.{}.{}", major, version / 100 % 100, version % 100)
    }
}

/// pdo_pgsql_get_attribute
fn get_attribute(_vm: &mut Vm, dbh: &DbhRef, attr: i64) -> AttrResult {
    let Some(conn) = server(dbh) else {
        return AttrResult::Unsupported;
    };
    let ps = |c: &ConnRef, k: &[u8]| -> Vec<u8> {
        c.borrow().parameter_status(k).unwrap_or(b"").to_vec()
    };
    let v = match attr {
        PDO_ATTR_EMULATE_PREPARES => vbool(with_h(dbh, |h| h.emulate_prepares).unwrap_or(false)),
        PDO_PGSQL_ATTR_DISABLE_PREPARES => {
            vbool(with_h(dbh, |h| h.disable_prepares).unwrap_or(false))
        }
        PDO_ATTR_CLIENT_VERSION => vstr(libpq_version()),
        PDO_ATTR_SERVER_VERSION => vstr(ps(&conn, b"server_version")),
        PDO_ATTR_CONNECTION_STATUS => vstr(match conn.borrow().status() {
            ConnStatus::Started => "Waiting for connection to be made.",
            ConnStatus::Made | ConnStatus::Ok => "Connection OK; waiting to send.",
            ConnStatus::AwaitingResponse => "Waiting for a response from the server.",
            ConnStatus::AuthOk => {
                "Received authentication; waiting for backend start-up to finish."
            }
            ConnStatus::SslStartup => "Negotiating SSL encryption.",
            ConnStatus::Setenv => "Negotiating environment-driven parameter settings.",
            ConnStatus::Consume => "Flushing send queue/consuming extra data.",
            ConnStatus::GssStartup => "Negotiating GSSAPI.",
            ConnStatus::CheckTarget => "Connection OK; checking target server properties.",
            ConnStatus::CheckStandby => "Connection OK; checking if server in standby.",
            _ => "Bad connection.",
        }),
        PDO_ATTR_SERVER_INFO => {
            let pid = conn.borrow().backend_pid();
            let s = format!(
                "PID: {}; Client Encoding: {}; Is Superuser: {}; Session Authorization: {}; Date Style: {}",
                pid,
                String::from_utf8_lossy(&ps(&conn, b"client_encoding")),
                String::from_utf8_lossy(&ps(&conn, b"is_superuser")),
                String::from_utf8_lossy(&ps(&conn, b"session_authorization")),
                String::from_utf8_lossy(&ps(&conn, b"DateStyle")),
            );
            vstr(s)
        }
        _ => return AttrResult::Unsupported,
    };
    AttrResult::Value(v)
}

/// pdo_pgsql_check_liveness
fn check_liveness(_vm: &mut Vm, dbh: &DbhRef) -> bool {
    let Some(conn) = server(dbh) else {
        return false;
    };
    let mut c = conn.borrow_mut();
    if !c.consume_input() || c.status() == ConnStatus::Bad {
        c.reset();
    }
    c.status() == ConnStatus::Ok
}

/// pdo_pgsql_transaction_cmd
fn transaction_cmd(cmd: &[u8], dbh: &DbhRef) -> bool {
    let Some(conn) = server(dbh) else {
        return false;
    };
    let res = conn.borrow_mut().exec(cmd);
    let st = status_of(res.as_ref());
    if st != ExecStatus::CommandOk {
        let ss = sqlstate(res.as_ref());
        pgsql_error(None, dbh, None, st as i64, ss.as_deref(), None);
        return false;
    }
    true
}

fn handle_begin(dbh: &DbhRef) -> bool {
    transaction_cmd(b"BEGIN", dbh)
}

fn handle_commit(dbh: &DbhRef) -> bool {
    let ret = transaction_cmd(b"COMMIT", dbh);
    // When deferred constraints are used the commit could fail, and a
    // ROLLBACK implicitly ran. See bug #67462
    if ret {
        close_lob_streams(dbh);
    } else {
        let t = in_transaction(dbh);
        dbh.borrow_mut().in_txn = t;
    }
    ret
}

fn handle_rollback(dbh: &DbhRef) -> bool {
    let ret = transaction_cmd(b"ROLLBACK", dbh);
    if ret {
        close_lob_streams(dbh);
    }
    ret
}

/// pdo_pgsql_set_attr
fn set_attribute(vm: &mut Vm, dbh: &DbhRef, attr: i64, val: &Value) -> bool {
    match attr {
        PDO_ATTR_EMULATE_PREPARES => match get_bool_param(vm, val) {
            Some(b) => with_h(dbh, |h| h.emulate_prepares = b).is_some(),
            None => false,
        },
        PDO_PGSQL_ATTR_DISABLE_PREPARES => match get_bool_param(vm, val) {
            Some(b) => with_h(dbh, |h| h.disable_prepares = b).is_some(),
            None => false,
        },
        PDO_ATTR_PREFETCH => match get_bool_param(vm, val) {
            Some(b) => with_h(dbh, |h| h.default_fetching_laziness = !b).is_some(),
            None => false,
        },
        _ => false,
    }
}

pub static PGSQL_METHODS: DbhMethods = DbhMethods {
    closer: handle_closer,
    preparer: handle_preparer,
    doer: handle_doer,
    quoter: Some(handle_quoter),
    begin: Some(handle_begin),
    commit: Some(handle_commit),
    rollback: Some(handle_rollback),
    set_attribute: Some(set_attribute),
    last_id: Some(last_insert_id),
    fetch_err: Some(fetch_error_func),
    get_attribute: Some(get_attribute),
    check_liveness: Some(check_liveness),
    persistent_shutdown: None,
    in_transaction: Some(in_transaction),
    scanner: Some(crate::scanner::pgsql_scanner),
};

/// _pdo_pgsql_escape_credentials: addcslashes(str, "\\'")
fn escape_credentials(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 2);
    for &c in s {
        if c == b'\\' || c == b'\'' {
            out.push(b'\\');
        }
        out.push(c);
    }
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// pdo_pgsql_handle_factory
fn handle_factory(vm: &mut Vm, dbh: &DbhRef, driver_options: Option<&Arr>) -> bool {
    let id = Rc::as_ptr(dbh) as usize;
    {
        let mut d = dbh.borrow_mut();
        d.driver_data = Some(Box::new(H {
            pgoid: -1,
            id,
            ..H::default()
        }));
        d.skip_param_evt = (1 << PDO_PARAM_EVT_EXEC_POST)
            | (1 << PDO_PARAM_EVT_FETCH_PRE)
            | (1 << PDO_PARAM_EVT_FETCH_POST);
    }
    // PostgreSQL wants params in the connect string to be separated by
    // spaces: the PDO standard semicolons are converted
    let mut ds = dbh.borrow().data_source.clone().unwrap_or_default();
    for c in ds.iter_mut() {
        if *c == b';' {
            *c = b' ';
        }
    }
    dbh.borrow_mut().data_source = Some(ds.clone());
    let connect_timeout = if driver_options.is_some() {
        attr_lval(driver_options, PDO_ATTR_TIMEOUT, 30)
    } else {
        30
    };
    let (user, pass) = {
        let d = dbh.borrow();
        (d.username.clone(), d.password.clone())
    };
    let ds_nul = match ds.iter().position(|&c| c == 0) {
        Some(p) => &ds[..p],
        None => &ds[..],
    };
    let tmp_user = if !contains(ds_nul, b"user=") {
        user.as_deref().map(escape_credentials)
    } else {
        None
    };
    let tmp_pass = if !contains(ds_nul, b"password=") {
        pass.as_deref().map(escape_credentials)
    } else {
        None
    };
    let mut conn_str = ds.clone();
    conn_str.extend_from_slice(format!(" connect_timeout={}", connect_timeout).as_bytes());
    if let Some(u) = tmp_user {
        conn_str.extend_from_slice(b" user='");
        conn_str.extend_from_slice(&u);
        conn_str.push(b'\'');
    }
    if let Some(p) = tmp_pass {
        conn_str.extend_from_slice(b" password='");
        conn_str.extend_from_slice(&p);
        conn_str.push(b'\'');
    }
    let conn = match conn_str.iter().position(|&c| c == 0) {
        Some(p) => PgConn::connectdb(&conn_str[..p]),
        None => PgConn::connectdb(&conn_str),
    };
    let ok = conn.status() == ConnStatus::Ok;
    let conn = Rc::new(RefCell::new(conn));
    with_h(dbh, |h| h.server = Some(conn.clone()));
    if !ok {
        pgsql_error(
            Some(vm),
            dbh,
            None,
            PGRES_FATAL_ERROR,
            Some(CONNECTION_FAILURE_SQLSTATE),
            None,
        );
        dbh.borrow_mut().methods = Some(&PGSQL_METHODS);
        handle_closer(dbh);
        return false;
    }
    conn.borrow_mut().set_notice_processor(notice_processor, id);
    register_handle(id, dbh);
    with_h(dbh, |h| h.attached = true);
    let mut d = dbh.borrow_mut();
    d.methods = Some(&PGSQL_METHODS);
    d.alloc_own_columns = true;
    d.max_escaped_char_length = 2;
    true
}

/// ZEND_ATOL
pub fn atol(s: &[u8]) -> i64 {
    let t = String::from_utf8_lossy(s);
    let t = t.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let mut n: i64 = 0;
    for c in digits.bytes() {
        if !c.is_ascii_digit() {
            break;
        }
        n = n.wrapping_mul(10).wrapping_add((c - b'0') as i64);
    }
    if neg { -n } else { n }
}

pub static PGSQL_DRIVER: Driver = Driver {
    name: "pgsql",
    factory: handle_factory,
};
