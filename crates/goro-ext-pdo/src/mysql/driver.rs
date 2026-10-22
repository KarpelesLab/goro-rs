//! The MySQL database handle (ext/pdo_mysql/mysql_driver.c) over mysqlnd.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ext_mysqli::mysqlnd::conn::{CloseType, Conn, ConnRef, OptValue};
use goro_ext_mysqli::mysqlnd::*;

use super::*;
use crate::core::*;

fn vstr(s: impl AsRef<[u8]>) -> Value {
    Value::String(PhpString::from_bytes(s.as_ref()))
}

/// pdo_mysql_error_info
#[derive(Clone, Debug, Default)]
pub struct EInfo {
    pub errcode: u32,
    pub errmsg: Option<Vec<u8>>,
}

/// pdo_mysql_db_handle
#[derive(Default)]
pub struct H {
    pub server: Option<ConnRef>,
    pub assume_national_character_set_strings: bool,
    pub attached: bool,
    pub buffered: bool,
    pub emulate_prepare: bool,
    pub fetch_table_names: bool,
    pub local_infile: bool,
    pub einfo: EInfo,
}

/// Run `f` on the MySQL handle of a PDO handle
pub fn with_h<R>(dbh: &DbhRef, f: impl FnOnce(&mut H) -> R) -> Option<R> {
    let mut d = dbh.borrow_mut();
    let h = d.driver_data.as_mut()?.downcast_mut::<H>()?;
    Some(f(h))
}

/// H->server
pub fn server(dbh: &DbhRef) -> Option<ConnRef> {
    with_h(dbh, |h| h.server.clone()).flatten()
}

const MSG_2014_PENDING: &[u8] = b"Cannot execute queries while there are pending result sets. Consider unsetting the previous PDOStatement or calling PDOStatement::closeCursor()";
const MSG_2014_UNBUFFERED: &[u8] = b"Cannot execute queries while other unbuffered queries are active.  Consider using PDOStatement::fetchAll().  Alternatively, if your code is only ever going to run against mysql, you may enable query buffering by setting the Pdo\\Mysql::ATTR_USE_BUFFERED_QUERY attribute.";
const MSG_2057: &[u8] = b"A stored procedure returning result sets of different size was called. This is not supported by libmysql";

/// _pdo_mysql_error: the error code; throws when the handle is not set up
/// yet (and a VM is at hand).
pub fn mysql_error(vm: Option<&mut Vm>, dbh: &DbhRef, stmt: Option<&StmtRef>) -> u32 {
    let conn = server(dbh);
    let ps = stmt.and_then(|s| {
        let b = s.borrow();
        b.driver_data
            .as_ref()
            .and_then(|d| d.downcast_ref::<S>())
            .and_then(|s| s.stmt.clone())
    });
    let (errcode, msg, sqlstate) = {
        let errcode = match &ps {
            Some(p) => p.borrow().error_info.error_no,
            None => conn
                .as_ref()
                .map(|c| c.borrow().error_info.error_no)
                .unwrap_or(0),
        };
        if errcode == 0 {
            match stmt {
                Some(s) => {
                    let mut b = s.borrow_mut();
                    b.error_code = PDO_ERR_NONE.to_vec();
                    if let Some(sd) = b.driver_data.as_mut().and_then(|d| d.downcast_mut::<S>()) {
                        sd.einfo = EInfo::default();
                    }
                }
                None => {
                    dbh.borrow_mut().error_code = PDO_ERR_NONE.to_vec();
                    with_h(dbh, |h| h.einfo = EInfo::default());
                }
            }
            return 0;
        }
        let more = conn
            .as_ref()
            .map(|c| c.borrow().more_results())
            .unwrap_or(false);
        let msg: Vec<u8> = if errcode == 2014 {
            if more {
                MSG_2014_PENDING.to_vec()
            } else {
                MSG_2014_UNBUFFERED.to_vec()
            }
        } else if errcode == 2057 {
            MSG_2057.to_vec()
        } else {
            match &ps {
                Some(p) => p.borrow().error_info.error.clone(),
                None => conn
                    .as_ref()
                    .map(|c| c.borrow().error_info.error.clone())
                    .unwrap_or_default(),
            }
        };
        let sqlstate = match &ps {
            Some(p) => p.borrow().sqlstate(),
            None => conn
                .as_ref()
                .map(|c| c.borrow().sqlstate())
                .unwrap_or_else(|| b"HY000".to_vec()),
        };
        (errcode, msg, sqlstate_buf(&sqlstate))
    };
    let einfo = EInfo {
        errcode,
        errmsg: Some(msg.clone()),
    };
    match stmt {
        Some(s) => {
            let mut b = s.borrow_mut();
            b.error_code = sqlstate.clone();
            if let Some(sd) = b.driver_data.as_mut().and_then(|d| d.downcast_mut::<S>()) {
                sd.einfo = einfo;
            }
        }
        None => {
            dbh.borrow_mut().error_code = sqlstate.clone();
            with_h(dbh, |h| h.einfo = einfo);
        }
    }
    if dbh.borrow().methods.is_none() {
        if let Some(vm) = vm {
            throw_exception(vm, errcode, &msg, &sqlstate);
        }
    }
    errcode
}

/// pdo_mysql_fetch_error_func
fn fetch_error_func(_vm: &mut Vm, dbh: &DbhRef, stmt: Option<&StmtRef>, info: &mut PhpArray) {
    let einfo = match stmt {
        Some(s) => s
            .borrow()
            .driver_data
            .as_ref()
            .and_then(|d| d.downcast_ref::<S>())
            .map(|s| s.einfo.clone())
            .unwrap_or_default(),
        None => with_h(dbh, |h| h.einfo.clone()).unwrap_or_default(),
    };
    if einfo.errcode != 0 {
        info.push(Value::Long(einfo.errcode as i64));
        info.push(vstr(einfo.errmsg.unwrap_or_default()));
    }
}

/// mysql_handle_closer
fn handle_closer(dbh: &DbhRef) {
    let h = dbh.borrow_mut().driver_data.take();
    if let Some(h) = h {
        if let Ok(h) = h.downcast::<H>() {
            if let Some(c) = &h.server {
                if let Ok(mut c) = c.try_borrow_mut() {
                    c.close(CloseType::Explicit);
                }
            }
        }
    }
}

/// mysql_handle_preparer
fn handle_preparer(
    vm: &mut Vm,
    dbh: &DbhRef,
    sql: &[u8],
    stmt: &StmtRef,
    driver_options: Option<&Arr>,
) -> bool {
    {
        let mut s = stmt.borrow_mut();
        s.driver_data = Some(Box::new(S::default()));
        s.methods = Some(&super::statement::MYSQL_STMT_METHODS);
    }
    let emulate = with_h(dbh, |h| h.emulate_prepare).unwrap_or(true);
    let conn = server(dbh);
    let fallback = |stmt: &StmtRef| {
        stmt.borrow_mut().supports_placeholders = PDO_PLACEHOLDER_NONE;
        true
    };
    if emulate {
        return fallback(stmt);
    }
    let conn = match conn {
        Some(c) => c,
        None => return fallback(stmt),
    };
    if conn.borrow().get_server_version() < 40100 {
        return fallback(stmt);
    }
    stmt.borrow_mut().supports_placeholders = PDO_PLACEHOLDER_POSITIONAL;
    let mut nsql = Vec::new();
    let ret = crate::parser::parse_params(vm, stmt, sql, &mut nsql);
    let sql: Vec<u8> = match ret {
        1 => nsql,
        -1 => {
            let ec = stmt.borrow().error_code.clone();
            dbh.borrow_mut().error_code = ec;
            return false;
        }
        _ => sql.to_vec(),
    };
    let ps = Rc::new(RefCell::new(goro_ext_mysqli::mysqlnd::ps::Stmt::new(
        conn.clone(),
    )));
    let set_ps = |stmt: &StmtRef, ps: Option<Rc<RefCell<goro_ext_mysqli::mysqlnd::ps::Stmt>>>| {
        let mut s = stmt.borrow_mut();
        if let Some(sd) = s.driver_data.as_mut().and_then(|d| d.downcast_mut::<S>()) {
            sd.stmt = ps;
        }
    };
    set_ps(stmt, Some(ps.clone()));
    let ok = ps.borrow_mut().prepare(&sql);
    if !ok {
        // if the query isn't supported by the protocol, fallback to emulation
        if conn.borrow().error_info.error_no == 1295 {
            set_ps(stmt, None);
            ps.borrow_mut().dtor(false);
            return fallback(stmt);
        }
        mysql_error(Some(vm), dbh, None);
        return false;
    }
    let num_params = ps.borrow().param_count as i32;
    {
        let mut s = stmt.borrow_mut();
        let sd = s
            .driver_data
            .as_mut()
            .and_then(|d| d.downcast_mut::<S>())
            .unwrap();
        sd.num_params = num_params;
        sd.max_length = attr_lval(driver_options, PDO_ATTR_MAX_COLUMN_LEN, 0) != 0;
    }
    dbh.borrow_mut().alloc_own_columns = true;
    true
}

/// mysql_handle_doer
fn handle_doer(vm: &mut Vm, dbh: &DbhRef, sql: &[u8]) -> i64 {
    doer(Some(vm), dbh, sql)
}

fn doer(mut vm: Option<&mut Vm>, dbh: &DbhRef, sql: &[u8]) -> i64 {
    let conn = match server(dbh) {
        Some(c) => c,
        None => return -1,
    };
    if !conn.borrow_mut().query(sql) {
        mysql_error(vm.as_deref_mut(), dbh, None);
        return -1;
    }
    let c = conn.borrow().upsert.affected_rows;
    if c == u64::MAX {
        let code = mysql_error(vm.as_deref_mut(), dbh, None);
        return if code != 0 { -1 } else { 0 };
    }
    // MULTI_QUERY support - eat up all unfetched result sets
    loop {
        if !conn.borrow().more_results() {
            break;
        }
        if !conn.borrow_mut().next_result() {
            mysql_error(vm.as_deref_mut(), dbh, None);
            return -1;
        }
        if let Some(mut r) = goro_ext_mysqli::mysqlnd::conn::store_result(&conn) {
            r.free_result(false);
        }
    }
    c as i32 as i64
}

/// pdo_mysql_last_insert_id
fn last_insert_id(_vm: &mut Vm, dbh: &DbhRef, _name: Option<&[u8]>) -> Option<Vec<u8>> {
    let c = server(dbh)?;
    let id = c.borrow().upsert.last_insert_id;
    Some(id.to_string().into_bytes())
}

/// mysql_handle_quoter
fn handle_quoter(_vm: &mut Vm, dbh: &DbhRef, unquoted: &[u8], paramtype: i64) -> Option<Vec<u8>> {
    let conn = server(dbh)?;
    let national_default =
        with_h(dbh, |h| h.assume_national_character_set_strings).unwrap_or(false);
    let mut use_national = false;
    let mut use_binary = false;
    if paramtype & PDO_PARAM_LOB == PDO_PARAM_LOB {
        use_binary = true;
    } else {
        if national_default {
            use_national = true;
        }
        if paramtype & PDO_PARAM_STR_NATL == PDO_PARAM_STR_NATL {
            use_national = true;
        }
        if paramtype & PDO_PARAM_STR_CHAR == PDO_PARAM_STR_CHAR {
            use_national = false;
        }
    }
    let esc = conn.borrow().escape_string(unquoted);
    let mut out = Vec::with_capacity(esc.len() + 10);
    if use_national {
        out.extend_from_slice(b"N'");
    } else if use_binary {
        out.extend_from_slice(b"_binary'");
    } else {
        out.push(b'\'');
    }
    out.extend_from_slice(&esc);
    out.push(b'\'');
    Some(out)
}

/// mysql_handle_begin
fn handle_begin(dbh: &DbhRef) -> bool {
    doer(None, dbh, b"START TRANSACTION") >= 0
}

/// mysql_handle_commit
fn handle_commit(dbh: &DbhRef) -> bool {
    let conn = match server(dbh) {
        Some(c) => c,
        None => return false,
    };
    let ok = conn
        .borrow_mut()
        .tx_commit_or_rollback(true, TRANS_COR_NO_OPT, None);
    if !ok {
        mysql_error(None, dbh, None);
        return false;
    }
    true
}

/// mysql_handle_rollback
fn handle_rollback(dbh: &DbhRef) -> bool {
    let conn = match server(dbh) {
        Some(c) => c,
        None => return false,
    };
    let ok = match conn.try_borrow_mut() {
        Ok(mut c) => c.tx_commit_or_rollback(false, TRANS_COR_NO_OPT, None),
        Err(_) => false,
    };
    if !ok {
        mysql_error(None, dbh, None);
        return false;
    }
    true
}

/// mysql_handle_autocommit
fn handle_autocommit(vm: Option<&mut Vm>, dbh: &DbhRef) -> bool {
    let conn = match server(dbh) {
        Some(c) => c,
        None => return false,
    };
    let ac = dbh.borrow().auto_commit;
    if !conn.borrow_mut().set_autocommit(ac) {
        mysql_error(vm, dbh, None);
        return false;
    }
    true
}

/// pdo_mysql_set_attribute
fn set_attribute(vm: &mut Vm, dbh: &DbhRef, attr: i64, val: &Value) -> bool {
    match attr {
        PDO_ATTR_AUTOCOMMIT => {
            let bval = match get_bool_param(vm, val) {
                Some(b) => b,
                None => return false,
            };
            let cur = dbh.borrow().auto_commit;
            if cur ^ bval {
                dbh.borrow_mut().auto_commit = bval;
                if !handle_autocommit(Some(vm), dbh) {
                    return false;
                }
            }
            true
        }
        PDO_ATTR_DEFAULT_STR_PARAM => {
            let lval = match get_long_param(vm, val) {
                Some(l) => l,
                None => return false,
            };
            with_h(dbh, |h| {
                h.assume_national_character_set_strings = lval == PDO_PARAM_STR_NATL
            });
            true
        }
        PDO_MYSQL_ATTR_USE_BUFFERED_QUERY => {
            let bval = match get_bool_param(vm, val) {
                Some(b) => b,
                None => return false,
            };
            with_h(dbh, |h| h.buffered = bval);
            true
        }
        PDO_ATTR_EMULATE_PREPARES => {
            let bval = match get_bool_param(vm, val) {
                Some(b) => b,
                None => return false,
            };
            with_h(dbh, |h| h.emulate_prepare = bval);
            true
        }
        PDO_ATTR_FETCH_TABLE_NAMES => {
            let bval = match get_bool_param(vm, val) {
                Some(b) => b,
                None => return false,
            };
            with_h(dbh, |h| h.fetch_table_names = bval);
            true
        }
        PDO_ATTR_STRINGIFY_FETCHES => {
            let bval = match get_bool_param(vm, val) {
                Some(b) => b,
                None => return false,
            };
            if let Some(c) = server(dbh) {
                if !c.borrow_mut().set_client_option(
                    MYSQLND_OPT_INT_AND_FLOAT_NATIVE,
                    OptValue::Int(!bval as i64),
                ) {
                    mysql_error(Some(vm), dbh, None);
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

/// pdo_mysql_get_attribute
fn get_attribute(vm: &mut Vm, dbh: &DbhRef, attr: i64) -> AttrResult {
    let conn = match server(dbh) {
        Some(c) => c,
        None => return AttrResult::Unsupported,
    };
    let h = |f: &dyn Fn(&H) -> Value| with_h(dbh, |h| f(h)).unwrap_or(Value::Null);
    let v = match attr {
        PDO_ATTR_CLIENT_VERSION => vstr(goro_ext_mysqli::link::client_info(vm)),
        PDO_ATTR_SERVER_VERSION => vstr(conn.borrow().server_version.clone().unwrap_or_default()),
        PDO_ATTR_CONNECTION_STATUS => vstr(conn.borrow().host_info.clone().unwrap_or_default()),
        PDO_ATTR_SERVER_INFO => {
            let st = conn.borrow_mut().statistic();
            match st {
                Some(s) => vstr(s),
                None => {
                    mysql_error(Some(vm), dbh, None);
                    return AttrResult::Error;
                }
            }
        }
        PDO_ATTR_AUTOCOMMIT => vbool(dbh.borrow().auto_commit),
        PDO_ATTR_DEFAULT_STR_PARAM => h(&|h| {
            Value::Long(if h.assume_national_character_set_strings {
                PDO_PARAM_STR_NATL
            } else {
                PDO_PARAM_STR_CHAR
            })
        }),
        PDO_MYSQL_ATTR_USE_BUFFERED_QUERY => h(&|h| vbool(h.buffered)),
        PDO_ATTR_EMULATE_PREPARES => h(&|h| vbool(h.emulate_prepare)),
        PDO_MYSQL_ATTR_LOCAL_INFILE => h(&|h| vbool(h.local_infile)),
        PDO_MYSQL_ATTR_LOCAL_INFILE_DIRECTORY => {
            match conn.borrow().options.local_infile_directory.clone() {
                Some(d) => vstr(d),
                None => Value::Null,
            }
        }
        PDO_ATTR_FETCH_TABLE_NAMES => h(&|h| vbool(h.fetch_table_names)),
        _ => return AttrResult::Unsupported,
    };
    AttrResult::Value(v)
}

/// pdo_mysql_check_liveness
fn check_liveness(_vm: &mut Vm, dbh: &DbhRef) -> bool {
    match server(dbh) {
        Some(c) => c.borrow_mut().ping(),
        None => false,
    }
}

/// pdo_mysql_request_shutdown (mysqlnd_end_psession is a no-op)
fn request_shutdown(_dbh: &DbhRef) {}

/// pdo_mysql_in_transaction
fn in_transaction(dbh: &DbhRef) -> bool {
    match server(dbh) {
        Some(c) => match c.try_borrow() {
            Ok(c) => c.upsert.server_status & SERVER_STATUS_IN_TRANS != 0,
            Err(_) => false,
        },
        None => false,
    }
}

pub static MYSQL_METHODS: DbhMethods = DbhMethods {
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
    persistent_shutdown: Some(request_shutdown),
    in_transaction: Some(in_transaction),
    scanner: Some(crate::scanner::mysql_scanner),
};

/// pdo_mysql.default_socket
fn default_socket(vm: &Vm) -> Option<Vec<u8>> {
    match vm.constants.get(b"pdo_mysql.default_socket".as_slice()) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.as_bytes().to_vec()),
        _ => Some(b"/tmp/mysql.sock".to_vec()),
    }
}

/// pdo_mysql_handle_factory
fn handle_factory(vm: &mut Vm, dbh: &DbhRef, driver_options: Option<&Arr>) -> bool {
    goro_ext_mysqli::link::sync_globals(vm);
    let mut vars: Vec<(&str, Option<Vec<u8>>)> = vec![
        ("charset", None),
        ("dbname", Some(Vec::new())),
        ("host", Some(b"localhost".to_vec())),
        ("port", Some(b"3306".to_vec())),
        ("unix_socket", default_socket(vm)),
        ("user", None),
        ("password", None),
    ];
    let mut connect_opts: u32 = CLIENT_MULTI_RESULTS;
    match driver_options {
        None => connect_opts |= CLIENT_MULTI_STATEMENTS,
        Some(o) => {
            if attr_lval(Some(o), PDO_MYSQL_ATTR_MULTI_STATEMENTS, 1) != 0 {
                connect_opts |= CLIENT_MULTI_STATEMENTS;
            }
        }
    }
    let ds = dbh.borrow().data_source.clone().unwrap_or_default();
    parse_data_source(&ds, &mut vars);

    let persistent = dbh.borrow().is_persistent;
    let conn: ConnRef = Rc::new(RefCell::new(Conn::new(persistent)));
    {
        let mut d = dbh.borrow_mut();
        d.driver_data = Some(Box::new(H {
            server: Some(conn.clone()),
            buffered: true,
            emulate_prepare: true,
            ..Default::default()
        }));
        d.skip_param_evt = (1 << PDO_PARAM_EVT_FREE)
            | (1 << PDO_PARAM_EVT_EXEC_POST)
            | (1 << PDO_PARAM_EVT_FETCH_PRE)
            | (1 << PDO_PARAM_EVT_FETCH_POST)
            | (1 << PDO_PARAM_EVT_NORMALIZE);
    }

    let ok = factory_connect(vm, dbh, &conn, driver_options, &vars, connect_opts);
    dbh.borrow_mut().methods = Some(&MYSQL_METHODS);
    ok
}

fn factory_connect(
    vm: &mut Vm,
    dbh: &DbhRef,
    conn: &ConnRef,
    driver_options: Option<&Arr>,
    vars: &[(&str, Option<Vec<u8>>)],
    mut connect_opts: u32,
) -> bool {
    let opt = |conn: &ConnRef, o: i64, v: OptValue| conn.borrow_mut().set_client_option(o, v);
    if let Some(opts) = driver_options {
        let o = Some(opts);
        let connect_timeout = attr_lval(o, PDO_ATTR_TIMEOUT, 30);
        let buffered = attr_lval(o, PDO_MYSQL_ATTR_USE_BUFFERED_QUERY, 1) != 0;
        let cur_emulate = with_h(dbh, |h| h.emulate_prepare).unwrap_or(true);
        let emulate = attr_lval(o, PDO_ATTR_EMULATE_PREPARES, cur_emulate as i64) != 0;
        let national = attr_lval(o, PDO_ATTR_DEFAULT_STR_PARAM, 0) == PDO_PARAM_STR_NATL;
        with_h(dbh, |h| {
            h.buffered = buffered;
            h.emulate_prepare = emulate;
            h.assume_national_character_set_strings = national;
        });
        if attr_lval(o, PDO_MYSQL_ATTR_FOUND_ROWS, 0) != 0 {
            connect_opts |= CLIENT_FOUND_ROWS;
        }
        if attr_lval(o, PDO_MYSQL_ATTR_IGNORE_SPACE, 0) != 0 {
            connect_opts |= CLIENT_IGNORE_SPACE;
        }
        if !opt(
            conn,
            MYSQL_OPT_CONNECT_TIMEOUT,
            OptValue::Int(connect_timeout),
        ) {
            mysql_error(Some(vm), dbh, None);
            return false;
        }
        if attr_lval(o, PDO_MYSQL_ATTR_LOCAL_INFILE, 0) != 0 {
            with_h(dbh, |h| h.local_infile = true);
        }
        let dir = match attr_strval(vm, o, PDO_MYSQL_ATTR_LOCAL_INFILE_DIRECTORY) {
            Ok(d) => d,
            Err(()) => return false,
        };
        if let Some(dir) = dir {
            let fname = active_function_name(vm);
            if vm.check_open_basedir(&fname, &String::from_utf8_lossy(&dir))
                && !opt(conn, MYSQL_OPT_LOAD_DATA_LOCAL_DIR, OptValue::Str(dir))
            {
                mysql_error(Some(vm), dbh, None);
                return false;
            }
        }
        let init_cmd = match attr_strval(vm, o, PDO_MYSQL_ATTR_INIT_COMMAND) {
            Ok(c) => c,
            Err(()) => return false,
        };
        if let Some(cmd) = init_cmd {
            if !opt(conn, MYSQL_INIT_COMMAND, OptValue::Str(cmd)) {
                mysql_error(Some(vm), dbh, None);
                return false;
            }
        }
        if attr_lval(o, PDO_MYSQL_ATTR_COMPRESS, 0) != 0
            && !opt(conn, MYSQL_OPT_COMPRESS, OptValue::Null)
        {
            mysql_error(Some(vm), dbh, None);
            return false;
        }
        let mut ssl: Vec<Option<Vec<u8>>> = Vec::new();
        for a in [
            PDO_MYSQL_ATTR_SSL_KEY,
            PDO_MYSQL_ATTR_SSL_CERT,
            PDO_MYSQL_ATTR_SSL_CA,
            PDO_MYSQL_ATTR_SSL_CAPATH,
            PDO_MYSQL_ATTR_SSL_CIPHER,
        ] {
            match attr_strval(vm, o, a) {
                Ok(v) => ssl.push(v),
                Err(()) => return false,
            }
        }
        if ssl.iter().any(|s| s.is_some()) {
            // mysql_ssl_set
            for (o, v) in [
                MYSQLND_OPT_SSL_KEY,
                MYSQLND_OPT_SSL_CERT,
                MYSQLND_OPT_SSL_CA,
                MYSQLND_OPT_SSL_CAPATH,
                MYSQLND_OPT_SSL_CIPHER,
            ]
            .into_iter()
            .zip(ssl)
            {
                let v = match v {
                    Some(v) => OptValue::Str(v),
                    None => OptValue::Null,
                };
                opt(conn, o, v);
            }
        }
        let public_key = match attr_strval(vm, o, PDO_MYSQL_ATTR_SERVER_PUBLIC_KEY) {
            Ok(k) => k,
            Err(()) => return false,
        };
        if let Some(k) = public_key {
            if !opt(conn, MYSQL_SERVER_PUBLIC_KEY, OptValue::Str(k)) {
                mysql_error(Some(vm), dbh, None);
                return false;
            }
        }
        let verify = attr_lval(o, PDO_MYSQL_ATTR_SSL_VERIFY_SERVER_CERT, -1);
        if verify != -1 {
            connect_opts |= if verify != 0 {
                CLIENT_SSL_VERIFY_SERVER_CERT
            } else {
                CLIENT_SSL_DONT_VERIFY_SERVER_CERT
            };
        }
    }

    // Always explicitly set the LOCAL_INFILE option.
    let local_infile = with_h(dbh, |h| h.local_infile).unwrap_or(false);
    if !opt(
        conn,
        MYSQL_OPT_LOCAL_INFILE,
        OptValue::Int(local_infile as i64),
    ) {
        mysql_error(Some(vm), dbh, None);
        return false;
    }
    let stringify = dbh.borrow().stringify;
    let native = attr_lval(driver_options, PDO_ATTR_STRINGIFY_FETCHES, stringify as i64) == 0;
    if !opt(
        conn,
        MYSQLND_OPT_INT_AND_FLOAT_NATIVE,
        OptValue::Int(native as i64),
    ) {
        mysql_error(Some(vm), dbh, None);
        return false;
    }
    let get = |name: &str| -> Option<Vec<u8>> {
        vars.iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| v.clone())
    };
    if let Some(cs) = get("charset") {
        if !opt(conn, MYSQL_SET_CHARSET_NAME, OptValue::Str(cs)) {
            mysql_error(Some(vm), dbh, None);
            return false;
        }
    }
    let dbname = get("dbname");
    let host = get("host");
    let port: u32 = match get("port") {
        Some(p) => atoi(&p) as u32,
        None => 3306,
    };
    let unix_socket = if host.as_deref() == Some(b"localhost") {
        get("unix_socket")
    } else {
        None
    };
    {
        let mut d = dbh.borrow_mut();
        if d.username.is_none() {
            if let Some(u) = get("user") {
                d.username = Some(u);
            }
        }
        if d.password.is_none() {
            if let Some(p) = get("password") {
                d.password = Some(p);
            }
        }
    }
    let (user, pass) = {
        let d = dbh.borrow();
        (d.username.clone(), d.password.clone())
    };
    let ok = conn.borrow_mut().connect(
        host.as_deref(),
        user.as_deref(),
        pass.as_deref(),
        dbname.as_deref(),
        port,
        unix_socket.as_deref(),
        connect_opts,
    );
    flush_mysqlnd(vm);
    if !ok {
        mysql_error(Some(vm), dbh, None);
        return false;
    }
    if !dbh.borrow().auto_commit {
        handle_autocommit(Some(vm), dbh);
    }
    with_h(dbh, |h| h.attached = true);
    let mut d = dbh.borrow_mut();
    d.alloc_own_columns = true;
    d.max_escaped_char_length = 2;
    true
}

/// atoi()
fn atoi(s: &[u8]) -> i64 {
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

pub static MYSQL_DRIVER: Driver = Driver {
    name: "mysql",
    factory: handle_factory,
};
