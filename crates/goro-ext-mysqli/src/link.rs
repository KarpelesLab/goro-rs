//! Connection level functions (mysqli_nonapi.c, mysqli_api.c, mysqli.c,
//! mysqli_report.c, mysqli_exception.c, mysqli_driver.c).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::PhpObject;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::mysqlnd::conn::{CloseType, Conn, ConnRef, OptValue};
use crate::mysqlnd::stats::{self, STAT_CONNECT_REUSED};
use crate::mysqlnd::*;
use crate::objects::*;
use crate::zpp::*;

// ── MyG(): the mysqli globals ─────────────────────────────────────────

pub struct Globals {
    pub num_links: i64,
    pub num_active_persistent: i64,
    pub num_inactive_persistent: i64,
    pub error_no: i64,
    pub error_msg: Option<Vec<u8>>,
    pub report_mode: i64,
    /// mysqli.rollback_on_cached_plink (PHP_INI_SYSTEM)
    pub rollback_on_cached_plink: bool,
}

thread_local! {
    pub static MYG: RefCell<Globals> = const { RefCell::new(Globals {
        num_links: 0,
        num_active_persistent: 0,
        num_inactive_persistent: 0,
        error_no: 0,
        error_msg: None,
        report_mode: MYSQLI_REPORT_ERROR | MYSQLI_REPORT_STRICT,
        rollback_on_cached_plink: false,
    }) };
    /// EG(persistent_list) entries of mysqli: hash_key -> free links
    static PLIST: RefCell<PList> = RefCell::new(PList(HashMap::new()));
}

/// The cached persistent links. They live until the process exits: closing
/// them while thread-locals are being torn down would touch the (already
/// destroyed) statistics, so they are left for the OS to close.
struct PList(HashMap<String, Vec<ConnRef>>);

impl std::ops::Deref for PList {
    type Target = HashMap<String, Vec<ConnRef>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for PList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Drop for PList {
    fn drop(&mut self) {
        for (_, links) in self.0.drain() {
            std::mem::forget(links);
        }
    }
}

pub fn report_mode() -> i64 {
    MYG.with(|g| g.borrow().report_mode)
}

pub fn set_report_mode(n: i64) {
    MYG.with(|g| g.borrow_mut().report_mode = n);
}

/// php_mysqli_set_error
fn set_connect_error(errno: i64, msg: &[u8]) {
    MYG.with(|g| {
        let mut g = g.borrow_mut();
        g.error_no = errno;
        g.error_msg = if msg.is_empty() {
            None
        } else {
            Some(msg.to_vec())
        };
    });
}

// ── INI ───────────────────────────────────────────────────────────────

pub fn ini_raw(vm: &Vm, key: &str) -> Option<Value> {
    vm.constants.get(key.as_bytes()).cloned()
}

pub fn ini_str(vm: &Vm, key: &str) -> Option<Vec<u8>> {
    match ini_raw(vm, key) {
        None | Some(Value::Null) | Some(Value::False) => None,
        Some(v) => Some(v.to_php_string().as_bytes().to_vec()),
    }
}

pub fn ini_long(vm: &Vm, key: &str, def: i64) -> i64 {
    match ini_raw(vm, key) {
        None => def,
        Some(Value::Long(n)) => n,
        Some(Value::True) => 1,
        Some(Value::False) | Some(Value::Null) => 0,
        Some(v) => {
            let s = v.to_php_string().to_string_lossy();
            let t = s.trim();
            let digits: String = t
                .chars()
                .enumerate()
                .take_while(|(i, c)| c.is_ascii_digit() || (*i == 0 && (*c == '-' || *c == '+')))
                .map(|(_, c)| c)
                .collect();
            digits.parse::<i64>().unwrap_or(0)
        }
    }
}

pub fn ini_bool(vm: &Vm, key: &str, def: bool) -> bool {
    match ini_raw(vm, key) {
        None => def,
        Some(Value::True) => true,
        Some(Value::False) | Some(Value::Null) => false,
        Some(Value::Long(n)) => n != 0,
        Some(v) => {
            let s = v.to_php_string().to_string_lossy();
            let t = s.trim().to_ascii_lowercase();
            if t == "on" || t == "yes" || t == "true" {
                true
            } else {
                t.parse::<i64>().map(|n| n != 0).unwrap_or(false)
            }
        }
    }
}

/// Refresh MYSQLND_G() from the INI settings
pub fn sync_globals(vm: &Vm) {
    let g = crate::mysqlnd::Globals {
        collect_statistics: ini_bool(vm, "mysqlnd.collect_statistics", true),
        net_cmd_buffer_size: ini_long(vm, "mysqlnd.net_cmd_buffer_size", 4096).max(4096) as usize,
        net_read_buffer_size: ini_long(vm, "mysqlnd.net_read_buffer_size", 32768).max(1) as usize,
        net_read_timeout: ini_long(vm, "mysqlnd.net_read_timeout", 86400),
        sha256_server_public_key: ini_str(vm, "mysqlnd.sha256_server_public_key")
            .map(|v| String::from_utf8_lossy(&v).into_owned()),
        default_socket_timeout: match ini_raw(vm, "default_socket_timeout") {
            Some(v) => v
                .to_php_string()
                .to_string_lossy()
                .trim()
                .parse()
                .unwrap_or(60.0),
            None => 60.0,
        },
    };
    crate::mysqlnd::set_globals(g);
    let ob = goro_core::basedir::open_basedir_value(vm);
    crate::mysqlnd::result::OPEN_BASEDIR.with(|b| *b.borrow_mut() = ob);
}

// ── diagnostics ───────────────────────────────────────────────────────

/// Emit the warnings mysqlnd queued (php_error_docref prefixes the active
/// function's name).
pub fn flush(vm: &mut Vm, f: &Func) {
    for w in crate::mysqlnd::take_warnings() {
        let msg = if w.docref {
            format!("{}({}): {}", f.name, w.param.unwrap_or_default(), w.msg)
        } else {
            w.msg
        };
        match w.level {
            E_NOTICE => {
                let line = vm.current_line;
                vm.emit_notice_at(&msg, line)
            }
            E_DEPRECATED => vm.emit_deprecated(&msg),
            _ => vm.emit_warning(&msg),
        }
    }
}

pub fn docref_warning(vm: &mut Vm, f: &Func, msg: &str) {
    vm.emit_warning(&format!("{}(): {}", f.name, msg));
}

/// php_mysqli_throw_sql_exception
pub fn throw_sql_exception(vm: &mut Vm, f: &Func, sqlstate: &[u8], errno: i64, msg: &[u8]) {
    flush(vm, f);
    if report_mode() & MYSQLI_REPORT_STRICT == 0 {
        let m = format!(
            "({}/{}): {}",
            String::from_utf8_lossy(sqlstate),
            errno,
            String::from_utf8_lossy(msg)
        );
        docref_warning(vm, f, &m);
        return;
    }
    let e = vm.create_exception(
        b"mysqli_sql_exception",
        &String::from_utf8_lossy(msg),
        vm.current_line,
    );
    if let Value::Object(o) = &e {
        let mut ob = o.borrow_mut();
        ob.set_property(b"message".to_vec(), vstr(msg));
        ob.set_property(
            b"sqlstate".to_vec(),
            vstr(if sqlstate.is_empty() {
                b"00000".as_slice()
            } else {
                sqlstate
            }),
        );
        ob.set_property(b"code".to_vec(), Value::Long(errno));
    }
    vm.current_exception = Some(e);
}

/// MYSQLI_REPORT_MYSQL_ERROR
pub fn report_conn_error(vm: &mut Vm, f: &Func, conn: &ConnRef) {
    flush(vm, f);
    if report_mode() & MYSQLI_REPORT_ERROR == 0 {
        return;
    }
    let (no, st, er) = {
        let c = conn.borrow();
        (
            c.error_info.error_no,
            c.sqlstate(),
            c.error_info.error.clone(),
        )
    };
    if no != 0 {
        throw_sql_exception(vm, f, &st, no as i64, &er);
    }
}

/// php_mysqli_report_index
pub fn report_index(vm: &mut Vm, f: &Func, query: &[u8], status: u32) {
    let index = if status & SERVER_QUERY_NO_GOOD_INDEX_USED != 0 {
        "Bad index"
    } else if status & SERVER_QUERY_NO_INDEX_USED != 0 {
        "No index"
    } else {
        return;
    };
    let mut m = format!("{} used in query/prepared statement ", index).into_bytes();
    m.extend_from_slice(query);
    throw_sql_exception(vm, f, b"00000", 0, &m);
}

fn vbool(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}

// ── property readers (mysqli_prop.c / mysqli_driver.c) ───────────────

pub fn client_info(vm: &Vm) -> String {
    let v = vm
        .constants
        .get(b"PHP_VERSION".as_slice())
        .map(|v| v.to_php_string().to_string_lossy())
        .unwrap_or_else(|| "8.5.0".to_string());
    format!("mysqlnd {}", v)
}

pub fn client_version(vm: &Vm) -> i64 {
    vm.constants
        .get(b"PHP_VERSION_ID".as_slice())
        .map(|v| v.to_long())
        .unwrap_or(80500)
}

pub fn driver_prop(vm: &mut Vm, name: &[u8], quiet: bool) -> Result<Value, ()> {
    match name {
        b"client_info" => Ok(vstr(client_info(vm))),
        b"client_version" => Ok(Value::Long(client_version(vm))),
        b"driver_version" => {
            if quiet {
                return Err(());
            }
            vm.emit_deprecated("The driver_version property is deprecated");
            Ok(Value::Long(101009))
        }
        b"report_mode" => Ok(Value::Long(report_mode())),
        _ => Err(()),
    }
}

/// CHECK_STATUS
fn check_status(vm: &mut Vm, o: &Obj, value: u8, quiet: bool) -> Result<Rc<Resource>, ()> {
    match get_res(o) {
        Some(r) if r.status.get() >= value => Ok(r),
        _ => {
            if !quiet {
                throw(vm, b"Error", "Property access is not allowed yet");
            }
            Err(())
        }
    }
}

/// MYSQLI_GET_MYSQL
fn get_mysql(vm: &mut Vm, o: &Obj, value: u8, quiet: bool) -> Result<ConnRef, ()> {
    let r = match get_res(o) {
        Some(r) => r,
        None => {
            if !quiet {
                let m = format!("{} object is already closed", ce_name(o));
                throw(vm, b"Error", &m);
            }
            return Err(());
        }
    };
    let m = match &r.kind {
        Kind::Link(m) => m.clone(),
        _ => return Err(()),
    };
    if r.status.get() < value {
        if !quiet {
            throw(vm, b"Error", "Property access is not allowed yet");
        }
        return Err(());
    }
    let c = m.borrow().mysql.clone();
    match c {
        Some(c) => Ok(c),
        None => {
            // ZEND_ASSERT(p)
            if !quiet {
                let m = format!("{} object is already closed", ce_name(o));
                throw(vm, b"Error", &m);
            }
            Err(())
        }
    }
}

pub fn link_prop(vm: &mut Vm, o: &Obj, name: &[u8], quiet: bool) -> Result<Value, ()> {
    match name {
        b"client_version" => return Ok(Value::Long(client_version(vm))),
        b"client_info" => return Ok(vstr(client_info(vm))),
        b"connect_errno" => return Ok(Value::Long(MYG.with(|g| g.borrow().error_no))),
        b"connect_error" => {
            return Ok(match MYG.with(|g| g.borrow().error_msg.clone()) {
                Some(m) => vstr(m),
                None => Value::Null,
            });
        }
        b"affected_rows" => {
            let r = check_status(vm, o, MYSQLI_STATUS_VALID, quiet)?;
            let conn = match &r.kind {
                Kind::Link(m) => m.borrow().mysql.clone(),
                _ => None,
            };
            let conn = match conn {
                Some(c) => c,
                None => return Err(()),
            };
            let rc = conn.borrow().upsert.affected_rows;
            return Ok(if rc == u64::MAX {
                Value::Long(-1)
            } else {
                long_or_str(rc)
            });
        }
        b"error_list" => {
            let r = check_status(vm, o, MYSQLI_STATUS_VALID, quiet)?;
            let conn = match &r.kind {
                Kind::Link(m) => m.borrow().mysql.clone(),
                _ => None,
            };
            return Ok(match conn {
                Some(c) => error_list(&c.borrow().error_info.error_list),
                None => empty_array(),
            });
        }
        _ => {}
    }
    let check = match name {
        b"errno" | b"error" => MYSQLI_STATUS_INITIALIZED,
        _ => MYSQLI_STATUS_VALID,
    };
    let conn = get_mysql(vm, o, check, quiet)?;
    let c = conn.borrow();
    Ok(match name {
        b"errno" => Value::Long(c.error_info.error_no as i64),
        b"error" => vstr(&c.error_info.error),
        b"field_count" => Value::Long(c.field_count as i64),
        b"host_info" => vstr(c.host_info.clone().unwrap_or_default()),
        b"info" => match &c.last_message {
            Some(m) => vstr(m),
            None => Value::Null,
        },
        b"insert_id" => long_or_str(c.upsert.last_insert_id),
        b"protocol_version" => Value::Long(c.protocol_version as i64),
        b"server_info" => vstr(c.server_version.clone().unwrap_or_default()),
        b"server_version" => Value::Long(c.get_server_version() as i64),
        b"sqlstate" => vstr(c.sqlstate()),
        b"thread_id" => Value::Long(c.thread_id as i64),
        b"warning_count" => Value::Long(c.upsert.warning_count as i64),
        _ => Value::Null,
    })
}

// ── connect ───────────────────────────────────────────────────────────

/// php_mysqli_close
pub fn php_mysqli_close(mysql: &mut MyMysql, close_type: CloseType, resource_status: u8) {
    if resource_status > MYSQLI_STATUS_INITIALIZED {
        MYG.with(|g| g.borrow_mut().num_links -= 1);
    }
    let conn = mysql.mysql.take();
    if let Some(conn) = conn {
        if !mysql.persistent {
            conn.borrow_mut().close(close_type);
        } else {
            let key = mysql.hash_key.clone().unwrap_or_default();
            let known = PLIST.with(|p| p.borrow().contains_key(&key));
            if known {
                // mysqlnd_end_psession
                {
                    let mut c = conn.borrow_mut();
                    c.current_result = None;
                    c.last_message = None;
                }
                let rollback = MYG.with(|g| g.borrow().rollback_on_cached_plink);
                if rollback
                    && !conn
                        .borrow_mut()
                        .tx_commit_or_rollback(false, TRANS_COR_NO_OPT, None)
                {
                    conn.borrow_mut().close(close_type);
                } else {
                    PLIST.with(|p| p.borrow_mut().entry(key).or_default().push(conn));
                    MYG.with(|g| g.borrow_mut().num_inactive_persistent += 1);
                }
                MYG.with(|g| g.borrow_mut().num_active_persistent -= 1);
            }
            mysql.persistent = false;
        }
    }
    mysql.hash_key = None;
}

/// php_mysqli_init
pub fn php_mysqli_init(vm: &mut Vm, this: Option<&Obj>) -> Value {
    if let Some(o) = this {
        if get_res(o).is_some() {
            return Value::Null;
        }
    }
    let mut mysql = MyMysql::new();
    mysql.mysql = Some(Rc::new(RefCell::new(Conn::new(true))));
    let res = Resource {
        status: std::cell::Cell::new(MYSQLI_STATUS_INITIALIZED),
        kind: Kind::Link(Rc::new(RefCell::new(mysql))),
    };
    match this {
        None => Value::Object(new_object(vm, b"mysqli", Base::Link, Some(res))),
        Some(o) => {
            set_res(o, Some(res));
            Value::Null
        }
    }
}

fn opt_bytes(a: &Arg) -> Option<Vec<u8>> {
    match a {
        Arg::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// mysqli_common_connect
pub fn common_connect(
    vm: &mut Vm,
    f: &Func,
    args: &[Value],
    is_real_connect: bool,
    in_ctor: bool,
) -> Value {
    sync_globals(vm);
    let object = f.this.clone();
    if in_ctor && args.is_empty() {
        if let Some(o) = &object {
            if get_res(o).is_some() {
                throw(vm, b"Error", "Cannot call constructor twice");
                return Value::Null;
            }
        }
        php_mysqli_init(vm, object.as_ref());
        return Value::Null;
    }
    let (hostname, username, passwd, dbname, port_arg, socket, mut flags);
    let mut mysql: Option<Rc<RefCell<MyMysql>>> = None;
    let mut mysqli_resource: Option<Rc<Resource>> = None;
    let object = if !is_real_connect {
        let a = match parse(vm, f, "|s!s!s!s!l!s!", &[], args) {
            Ok(a) => a,
            Err(()) => return Value::Null,
        };
        if in_ctor {
            if let Some(o) = &object {
                if get_res(o).is_some() {
                    throw(vm, b"Error", "Cannot call constructor twice");
                    return Value::Null;
                }
            }
        }
        if let Some(o) = &object {
            mysqli_resource = get_res(o);
            if let Some(r) = &mysqli_resource {
                if let Kind::Link(m) = &r.kind {
                    mysql = Some(m.clone());
                }
            }
        }
        hostname = opt_bytes(&a[0]);
        username = opt_bytes(&a[1]);
        passwd = opt_bytes(&a[2]);
        dbname = opt_bytes(&a[3]);
        port_arg = a[4].long();
        socket = opt_bytes(&a[5]);
        flags = CLIENT_MULTI_RESULTS as i64;
        object
    } else {
        let a = match parse(vm, f, "O|s!s!s!s!l!s!l", &[b"mysqli"], args) {
            Ok(a) => a,
            Err(()) => return Value::Null,
        };
        let o = a[0].obj().unwrap();
        mysqli_resource = get_res(&o);
        match fetch_conn(vm, &o, MYSQLI_STATUS_INITIALIZED) {
            Ok((m, _)) => mysql = Some(m),
            Err(()) => return Value::Null,
        }
        hostname = opt_bytes(&a[1]);
        username = opt_bytes(&a[2]);
        passwd = opt_bytes(&a[3]);
        dbname = opt_bytes(&a[4]);
        port_arg = a[5].long();
        socket = opt_bytes(&a[6]);
        flags = a[7].long_or(0);
        flags |= CLIENT_MULTI_RESULTS as i64;
        flags &= !(CLIENT_MULTI_STATEMENTS as i64);
        Some(o)
    };
    let self_alloced = mysql.is_none();
    let mysql = mysql.unwrap_or_else(|| Rc::new(RefCell::new(MyMysql::new())));
    let socket = match socket {
        Some(s) if !s.is_empty() => Some(s),
        _ => ini_str(vm, "mysqli.default_socket").filter(|s| !s.is_empty()),
    };
    let port = match port_arg {
        Some(p) if p != 0 => p,
        _ => ini_long(vm, "mysqli.default_port", 3306),
    };
    let passwd = match passwd {
        Some(p) => p,
        None => ini_str(vm, "mysqli.default_pw").unwrap_or_default(),
    };
    let username = match username {
        Some(u) => Some(u),
        None => ini_str(vm, "mysqli.default_user"),
    };
    let mut hostname = match hostname {
        Some(h) if !h.is_empty() => Some(h),
        _ => ini_str(vm, "mysqli.default_host"),
    };
    let has_conn = mysql.borrow().mysql.is_some();
    if has_conn {
        if let Some(r) = &mysqli_resource {
            if r.status.get() > MYSQLI_STATUS_INITIALIZED {
                let st = r.status.get();
                php_mysqli_close(&mut mysql.borrow_mut(), CloseType::Implicit, st);
            }
        }
    }
    let mut persistent = false;
    let mut new_connection = false;
    let mut reused = false;
    if let Some(h) = &hostname {
        if h.len() > 2 && h[..2].eq_ignore_ascii_case(b"p:") {
            let h2 = h[2..].to_vec();
            hostname = Some(h2.clone());
            if !ini_bool(vm, "mysqli.allow_persistent", true) {
                docref_warning(
                    vm,
                    f,
                    "Persistent connections are disabled. Downgrading to normal",
                );
            } else {
                persistent = true;
                let rb = ini_bool(vm, "mysqli.rollback_on_cached_plink", false);
                MYG.with(|g| g.borrow_mut().rollback_on_cached_plink = rb);
                let key = format!(
                    "mysqli_{}_{}{}{}{}{}",
                    String::from_utf8_lossy(&h2),
                    String::from_utf8_lossy(socket.as_deref().unwrap_or(b"")),
                    port,
                    String::from_utf8_lossy(username.as_deref().unwrap_or(b"")),
                    String::from_utf8_lossy(dbname.as_deref().unwrap_or(b"")),
                    String::from_utf8_lossy(&passwd)
                );
                {
                    let mut m = mysql.borrow_mut();
                    m.persistent = true;
                    m.hash_key = Some(key.clone());
                }
                let exists = PLIST.with(|p| p.borrow().contains_key(&key));
                if exists {
                    let cached = PLIST.with(|p| p.borrow_mut().get_mut(&key).and_then(|v| v.pop()));
                    if let Some(cached) = cached {
                        let old = mysql.borrow_mut().mysql.take();
                        if let Some(old) = old {
                            old.borrow_mut().close(CloseType::Implicit);
                        }
                        MYG.with(|g| g.borrow_mut().num_inactive_persistent -= 1);
                        let ok = cached.borrow_mut().change_user(
                            username.as_deref(),
                            Some(&passwd),
                            dbname.as_deref(),
                            true,
                        );
                        if ok {
                            // mysqlnd_restart_psession
                            {
                                let mut c = cached.borrow_mut();
                                c.stats.inc(STAT_CONNECT_REUSED);
                                c.current_result = None;
                                c.last_message = None;
                            }
                            MYG.with(|g| g.borrow_mut().num_active_persistent += 1);
                            let (no, er) = {
                                let c = cached.borrow();
                                (c.error_info.error_no as i64, c.error_info.error.clone())
                            };
                            set_connect_error(no, &er);
                            mysql.borrow_mut().mysql = Some(cached);
                            reused = true;
                        } else {
                            cached.borrow_mut().close(CloseType::Implicit);
                        }
                    }
                } else {
                    PLIST.with(|p| p.borrow_mut().insert(key, Vec::new()));
                }
            }
        }
    }
    flush(vm, f);
    if !reused {
        let max_links = ini_long(vm, "mysqli.max_links", -1);
        let num_links = MYG.with(|g| g.borrow().num_links);
        if max_links != -1 && num_links >= max_links {
            docref_warning(vm, f, &format!("Too many open links ({})", num_links));
            return connect_err(&mysql, self_alloced, is_real_connect);
        }
        let max_p = ini_long(vm, "mysqli.max_persistent", -1);
        let (ap, ip) = MYG.with(|g| {
            let g = g.borrow();
            (g.num_active_persistent, g.num_inactive_persistent)
        });
        if persistent && max_p != -1 && ap + ip >= max_p {
            docref_warning(
                vm,
                f,
                &format!("Too many open persistent links ({})", ap + ip),
            );
            return connect_err(&mysql, self_alloced, is_real_connect);
        }
        let has = mysql.borrow().mysql.is_some();
        if !has {
            mysql.borrow_mut().mysql = Some(Rc::new(RefCell::new(Conn::new(persistent))));
            new_connection = true;
        }
        let conn = mysql.borrow().mysql.clone().unwrap();
        let ok = conn.borrow_mut().connect(
            hostname.as_deref(),
            username.as_deref(),
            Some(&passwd),
            dbname.as_deref(),
            port as u32,
            socket.as_deref(),
            flags as u32,
        );
        flush(vm, f);
        if !ok {
            let (no, st, er) = {
                let c = conn.borrow();
                (
                    c.error_info.error_no as i64,
                    c.sqlstate(),
                    c.error_info.error.clone(),
                )
            };
            set_connect_error(no, &er);
            throw_sql_exception(vm, f, &st, no, &er);
            if !is_real_connect {
                conn.borrow_mut().close(CloseType::Disconnected);
                mysql.borrow_mut().mysql = None;
            }
            return connect_err(&mysql, self_alloced, is_real_connect);
        }
        let (no, er) = {
            let c = conn.borrow();
            (c.error_info.error_no as i64, c.error_info.error.clone())
        };
        set_connect_error(no, &er);
        let allow = ini_bool(vm, "mysqli.allow_local_infile", false);
        // MyG(local_infile_directory) && !php_check_open_basedir(...)
        let dir = ini_str(vm, "mysqli.local_infile_directory")
            .filter(|d| !d.is_empty())
            .filter(|d| vm.check_open_basedir(&f.name, &String::from_utf8_lossy(d)));
        let mut c = conn.borrow_mut();
        c.set_client_option(MYSQL_OPT_LOCAL_INFILE, OptValue::Int(allow as i64));
        if let Some(d) = dir {
            c.set_client_option(MYSQL_OPT_LOAD_DATA_LOCAL_DIR, OptValue::Str(d));
        }
    }
    // end:
    let resource = match mysqli_resource {
        Some(r) => r,
        None => Rc::new(Resource {
            status: std::cell::Cell::new(MYSQLI_STATUS_VALID),
            kind: Kind::Link(mysql.clone()),
        }),
    };
    resource.status.set(MYSQLI_STATUS_VALID);
    if persistent && (new_connection || is_real_connect) {
        MYG.with(|g| g.borrow_mut().num_active_persistent += 1);
    }
    MYG.with(|g| g.borrow_mut().num_links += 1);
    mysql.borrow_mut().multi_query = false;
    match object {
        Some(o) => {
            if let Some(d) = data_of(&o) {
                *d.res.borrow_mut() = Some(resource);
            }
            Value::True
        }
        None => {
            let o = new_object(vm, b"mysqli", Base::Link, None);
            if let Some(d) = data_of(&o) {
                *d.res.borrow_mut() = Some(resource);
            }
            Value::Object(o)
        }
    }
}

fn connect_err(mysql: &Rc<RefCell<MyMysql>>, self_alloced: bool, is_real_connect: bool) -> Value {
    {
        let mut m = mysql.borrow_mut();
        if m.hash_key.is_some() {
            m.hash_key = None;
            m.persistent = false;
        }
    }
    let _ = (self_alloced, is_real_connect);
    Value::False
}

pub fn mysqli_connect(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    common_connect(vm, f, args, false, false)
}

pub fn mysqli_ctor(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    common_connect(vm, f, args, false, true)
}

pub fn mysqli_real_connect(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    common_connect(vm, f, args, true, false)
}

pub fn mysqli_init(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    if f.is_method {
        let this = f.this.clone();
        php_mysqli_init(vm, this.as_ref());
        return Value::Null;
    }
    php_mysqli_init(vm, None)
}

pub fn mysqli_connect_errno(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    Value::Long(MYG.with(|g| g.borrow().error_no))
}

pub fn mysqli_connect_error(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    match MYG.with(|g| g.borrow().error_msg.clone()) {
        Some(m) => vstr(m),
        None => Value::Null,
    }
}

/// "O" + MYSQLI_FETCH_RESOURCE_CONN(VALID) prologue
fn link_args(
    vm: &mut Vm,
    f: &Func,
    spec: &str,
    args: &[Value],
    check: u8,
) -> Result<(Vec<Arg>, Rc<RefCell<MyMysql>>, ConnRef), ()> {
    sync_globals(vm);
    let a = parse(vm, f, spec, &[b"mysqli"], args)?;
    let o = a[0].obj().unwrap();
    let (m, c) = fetch_conn(vm, &o, check)?;
    Ok((a, m, c))
}

pub fn mysqli_affected_rows(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let rc = c.borrow().upsert.affected_rows;
    if rc == u64::MAX {
        return Value::Long(-1);
    }
    long_or_str(rc)
}

pub fn mysqli_autocommit(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Ob", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let ok = c.borrow_mut().set_autocommit(a[1].bool());
    if !ok {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_change_user(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Osss!", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let user = opt_bytes(&a[1]);
    let pw = opt_bytes(&a[2]);
    let db = opt_bytes(&a[3]);
    let rc = c
        .borrow_mut()
        .change_user(user.as_deref(), pw.as_deref(), db.as_deref(), false);
    report_conn_error(vm, f, &c);
    vbool(rc)
}

pub fn mysqli_character_set_name(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let n = c.borrow().charset_name();
    vstr(n)
}

pub fn mysqli_close(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "O", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let o = a[0].obj().unwrap();
    let (m, _) = match fetch_conn(vm, &o, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let status = get_res(&o).map(|r| r.status.get()).unwrap_or(0);
    php_mysqli_close(&mut m.borrow_mut(), CloseType::Explicit, status);
    if let Some(r) = get_res(&o) {
        r.status.set(MYSQLI_STATUS_UNKNOWN);
    }
    clear_resource(&o);
    flush(vm, f);
    Value::True
}

fn opt_name(a: &Arg) -> Option<Vec<u8>> {
    opt_bytes(a)
}

pub fn mysqli_commit(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "O|ls!", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let flags = a[1].long_or(0);
    let name = opt_name(&a[2]);
    let ok = c
        .borrow_mut()
        .tx_commit_or_rollback(true, flags as u32, name.as_deref());
    if !ok {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_rollback(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "O|ls!", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let flags = a[1].long_or(0);
    let name = opt_name(&a[2]);
    let ok = c
        .borrow_mut()
        .tx_commit_or_rollback(false, flags as u32, name.as_deref());
    if !ok {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_begin_transaction(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "O|ls!", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let flags = a[1].long_or(0);
    if flags < 0 {
        f.value_error(
            vm,
            f.arg_pos(2),
            "must be one of the MYSQLI_TRANS_* constants",
        );
        return Value::Null;
    }
    let name = opt_name(&a[2]);
    if let Some(n) = &name {
        if n.is_empty() {
            f.must_not_be_empty(vm, f.arg_pos(3));
            return Value::Null;
        }
    }
    let ok = c.borrow_mut().tx_begin(flags as u32, name.as_deref());
    if !ok {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_savepoint(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let name = a[1].str().unwrap_or(b"").to_vec();
    if name.is_empty() {
        f.must_not_be_empty(vm, f.arg_pos(2));
        return Value::Null;
    }
    if !c.borrow_mut().tx_savepoint(Some(&name)) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_release_savepoint(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let name = a[1].str().unwrap_or(b"").to_vec();
    if name.is_empty() {
        f.must_not_be_empty(vm, f.arg_pos(2));
        return Value::Null;
    }
    if !c.borrow_mut().tx_savepoint_release(Some(&name)) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_debug(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "s", &[], args).is_err() {
        return Value::Null;
    }
    Value::True
}

pub fn mysqli_dump_debug_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let ok = c.borrow_mut().dump_debug_info();
    flush(vm, f);
    vbool(ok)
}

pub fn mysqli_errno(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(c.borrow().error_info.error_no as i64)
}

pub fn mysqli_error(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vstr(c.borrow().error_info.error.clone())
}

pub fn mysqli_error_list(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    error_list(&c.borrow().error_info.error_list)
}

pub fn mysqli_field_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(c.borrow().field_count as i64)
}

pub fn mysqli_get_client_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if f.is_method {
        if parse(vm, f, "", &[], args).is_err() {
            return Value::Null;
        }
    } else {
        if parse(vm, f, "|O!", &[b"mysqli"], args).is_err() {
            return Value::Null;
        }
        if !args.is_empty() {
            vm.emit_deprecated(&format!(
                "{}(): Passing connection object as an argument is deprecated",
                f.name
            ));
        }
    }
    vstr(client_info(vm))
}

pub fn mysqli_get_client_version(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    Value::Long(client_version(vm))
}

pub fn mysqli_get_host_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vstr(c.borrow().host_info.clone().unwrap_or_default())
}

pub fn mysqli_get_proto_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(c.borrow().protocol_version as i64)
}

pub fn mysqli_get_server_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vstr(c.borrow().server_version.clone().unwrap_or_default())
}

pub fn mysqli_get_server_version(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(c.borrow().get_server_version() as i64)
}

pub fn mysqli_info(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    match &c.borrow().last_message {
        Some(m) => vstr(m),
        None => Value::Null,
    }
}

pub fn mysqli_insert_id(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    long_or_str(c.borrow().upsert.last_insert_id)
}

pub fn mysqli_kill(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Ol", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let pid = a[1].long_or(0);
    if pid <= 0 {
        f.value_error(vm, f.arg_pos(2), "must be greater than 0");
        return Value::Null;
    }
    let o = a[0].obj().unwrap();
    let (_, c) = match fetch_conn(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    if !c.borrow_mut().kill(pid as u32) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_more_results(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vbool(c.borrow().more_results())
}

pub fn mysqli_next_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    if !c.borrow_mut().next_result() {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

/// MYSQLI_DISABLE_MQ / MYSQLI_ENABLE_MQ
fn set_mq(m: &Rc<RefCell<MyMysql>>, c: &ConnRef, on: bool) {
    let cur = m.borrow().multi_query;
    if cur != on {
        c.borrow_mut().set_server_option(if on {
            MYSQL_OPTION_MULTI_STATEMENTS_ON
        } else {
            MYSQL_OPTION_MULTI_STATEMENTS_OFF
        });
        m.borrow_mut().multi_query = on;
    }
}

pub fn mysqli_multi_query(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, m, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let q = a[1].str().unwrap_or(b"").to_vec();
    set_mq(&m, &c, true);
    if !c.borrow_mut().query(&q) {
        let saved = c.borrow().error_info.clone();
        c.borrow_mut().error_info.error_list.clear();
        report_conn_error(vm, f, &c);
        set_mq(&m, &c, false);
        c.borrow_mut().error_info = saved;
        flush(vm, f);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

const MYSQLI_STORE_RESULT: i64 = 0;
const MYSQLI_USE_RESULT: i64 = 1;
const MYSQLI_ASYNC: i64 = 8;
const MYSQLI_STORE_RESULT_COPY_DATA: i64 = 16;

pub fn new_result_object(vm: &mut Vm, res: Box<crate::mysqlnd::result::ResultSet>) -> Value {
    let r = Resource {
        status: std::cell::Cell::new(MYSQLI_STATUS_VALID),
        kind: Kind::Result(Rc::new(RefCell::new(*res))),
    };
    Value::Object(new_object(vm, b"mysqli_result", Base::Result, Some(r)))
}

pub fn mysqli_query(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "Os|l", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let q = a[1].str().unwrap_or(b"").to_vec();
    if q.is_empty() {
        f.must_not_be_empty(vm, f.arg_pos(2));
        return Value::Null;
    }
    let resultmode = a[2].long_or(MYSQLI_STORE_RESULT);
    if (resultmode & !MYSQLI_ASYNC) != MYSQLI_USE_RESULT
        && MYSQLI_STORE_RESULT != (resultmode & !(MYSQLI_ASYNC | MYSQLI_STORE_RESULT_COPY_DATA))
    {
        f.value_error(
            vm,
            f.arg_pos(3),
            "must be either MYSQLI_USE_RESULT or MYSQLI_STORE_RESULT with MYSQLI_ASYNC as an optional bitmask flag",
        );
        return Value::Null;
    }
    let o = a[0].obj().unwrap();
    let (m, c) = match fetch_conn(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    set_mq(&m, &c, false);
    if resultmode & MYSQLI_ASYNC != 0 {
        if !c.borrow_mut().send_query(&q) {
            report_conn_error(vm, f, &c);
            return Value::False;
        }
        m.borrow_mut().async_result_fetch_type = resultmode & !MYSQLI_ASYNC;
        flush(vm, f);
        return Value::True;
    }
    if !c.borrow_mut().query(&q) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    if c.borrow().field_count == 0 {
        if report_mode() & MYSQLI_REPORT_INDEX != 0 {
            let st = c.borrow().upsert.server_status;
            report_index(vm, f, &q, st);
        }
        return Value::True;
    }
    let result = match resultmode & !(MYSQLI_ASYNC | MYSQLI_STORE_RESULT_COPY_DATA) {
        MYSQLI_USE_RESULT => crate::mysqlnd::conn::use_result(&c),
        _ => crate::mysqlnd::conn::store_result(&c),
    };
    let result = match result {
        Some(r) => r,
        None => {
            report_conn_error(vm, f, &c);
            return Value::False;
        }
    };
    flush(vm, f);
    if report_mode() & MYSQLI_REPORT_INDEX != 0 {
        let st = c.borrow().upsert.server_status;
        report_index(vm, f, &q, st);
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    }
    new_result_object(vm, result)
}

pub fn mysqli_real_query(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, m, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let q = a[1].str().unwrap_or(b"").to_vec();
    set_mq(&m, &c, false);
    if !c.borrow_mut().query(&q) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    if c.borrow().field_count == 0 && report_mode() & MYSQLI_REPORT_INDEX != 0 {
        let st = c.borrow().upsert.server_status;
        report_index(vm, f, &q, st);
    }
    Value::True
}

pub fn mysqli_reap_async_query(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, m, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    if !c.borrow_mut().reap_query() {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    if c.borrow().field_count == 0 {
        return Value::True;
    }
    let ft = m.borrow().async_result_fetch_type;
    let result = match ft {
        MYSQLI_STORE_RESULT => crate::mysqlnd::conn::store_result(&c),
        MYSQLI_USE_RESULT => crate::mysqlnd::conn::use_result(&c),
        _ => None,
    };
    match result {
        Some(r) => {
            flush(vm, f);
            new_result_object(vm, r)
        }
        None => {
            report_conn_error(vm, f, &c);
            Value::False
        }
    }
}

pub fn mysqli_store_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    sync_globals(vm);
    let a = match parse(vm, f, "O|l", &[b"mysqli"], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    if (f.is_method && args.len() == 1) || args.len() == 2 {
        vm.emit_deprecated(
            "Passing the $mode parameter is deprecated since 8.4, as it has been ignored since 8.1",
        );
        if vm.current_exception.is_some() {
            return Value::Null;
        }
    }
    let o = a[0].obj().unwrap();
    let (_, c) = match fetch_conn(vm, &o, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let r = crate::mysqlnd::conn::store_result(&c);
    match r {
        None => {
            report_conn_error(vm, f, &c);
            Value::False
        }
        Some(r) => {
            flush(vm, f);
            if report_mode() & MYSQLI_REPORT_INDEX != 0 {
                let st = c.borrow().upsert.server_status;
                report_index(vm, f, b"from previous query", st);
                if vm.current_exception.is_some() {
                    return Value::Null;
                }
            }
            new_result_object(vm, r)
        }
    }
}

pub fn mysqli_use_result(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let r = crate::mysqlnd::conn::use_result(&c);
    match r {
        None => {
            report_conn_error(vm, f, &c);
            Value::False
        }
        Some(r) => {
            flush(vm, f);
            if report_mode() & MYSQLI_REPORT_INDEX != 0 {
                let st = c.borrow().upsert.server_status;
                report_index(vm, f, b"from previous query", st);
                if vm.current_exception.is_some() {
                    return Value::Null;
                }
            }
            new_result_object(vm, r)
        }
    }
}

pub fn mysqli_real_escape_string(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let s = a[1].str().unwrap_or(b"").to_vec();
    let e = c.borrow().escape_string(&s);
    Value::String(PhpString::from_vec(e))
}

pub fn mysqli_select_db(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let db = a[1].str().unwrap_or(b"").to_vec();
    let db = crate::mysqlnd::conn::cstr(&db).to_vec();
    if !c.borrow_mut().select_db(&db) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

pub fn mysqli_sqlstate(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    vstr(c.borrow().sqlstate())
}

pub fn mysqli_ssl_set(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Os!s!s!s!s!", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let get = |i: usize| -> OptValue {
        match &a[i] {
            Arg::Str(s) if !s.is_empty() => OptValue::Str(s.clone()),
            _ => OptValue::Null,
        }
    };
    let mut cm = c.borrow_mut();
    cm.set_client_option(MYSQLND_OPT_SSL_KEY, get(1));
    cm.set_client_option(MYSQLND_OPT_SSL_CERT, get(2));
    cm.set_client_option(MYSQLND_OPT_SSL_CA, get(3));
    cm.set_client_option(MYSQLND_OPT_SSL_CAPATH, get(4));
    cm.set_client_option(MYSQLND_OPT_SSL_CIPHER, get(5));
    Value::True
}

pub fn mysqli_stat(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let r = c.borrow_mut().statistic();
    flush(vm, f);
    match r {
        Some(s) => vstr(s),
        None => Value::False,
    }
}

pub fn mysqli_refresh(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Ol", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let ok = c.borrow_mut().refresh(a[1].long_or(0) as u8);
    flush(vm, f);
    vbool(ok)
}

pub fn mysqli_thread_id(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(c.borrow().thread_id as i64)
}

pub fn mysqli_thread_safe(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    Value::True
}

pub fn mysqli_warning_count(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    Value::Long(c.borrow().upsert.warning_count as i64)
}

pub fn mysqli_ping(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let ok = c.borrow_mut().ping();
    report_conn_error(vm, f, &c);
    vbool(ok)
}

/// mysqli_options_get_option_zval_type
fn option_type(opt: i64) -> u8 {
    match opt {
        MYSQLND_OPT_NET_CMD_BUFFER_SIZE
        | MYSQLND_OPT_NET_READ_BUFFER_SIZE
        | MYSQLND_OPT_INT_AND_FLOAT_NATIVE
        | MYSQL_OPT_CONNECT_TIMEOUT
        | MYSQL_OPT_LOCAL_INFILE
        | MYSQL_OPT_NAMED_PIPE
        | MYSQL_OPT_PROTOCOL
        | MYSQL_OPT_READ_TIMEOUT
        | MYSQL_OPT_WRITE_TIMEOUT
        | MYSQL_OPT_SSL_VERIFY_SERVER_CERT
        | MYSQL_OPT_COMPRESS
        | MYSQL_OPT_CAN_HANDLE_EXPIRED_PASSWORDS => 1,
        MYSQL_READ_DEFAULT_FILE
        | MYSQL_READ_DEFAULT_GROUP
        | MYSQL_INIT_COMMAND
        | MYSQL_SET_CHARSET_NAME
        | MYSQL_SERVER_PUBLIC_KEY
        | MYSQL_OPT_LOAD_DATA_LOCAL_DIR => 2,
        _ => 0,
    }
}

pub fn mysqli_options(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Olz", args, MYSQLI_STATUS_INITIALIZED) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let opt = a[1].long_or(0);
    let value = match &a[2] {
        Arg::Any(v) => v.deref(),
        _ => Value::Null,
    };
    let ret = match option_type(opt) {
        2 => {
            let s = match &value {
                Value::String(s) => s.as_bytes().to_vec(),
                other => {
                    if matches!(other, Value::Array(_)) {
                        let line = vm.current_line;
                        vm.emit_warning_at("Array to string conversion", line);
                    }
                    let had = vm.current_exception.is_some();
                    let s = vm.value_to_string(other);
                    if !had && vm.current_exception.is_some() {
                        return Value::Null;
                    }
                    s.as_bytes().to_vec()
                }
            };
            !c.borrow_mut().set_client_option(opt, OptValue::Str(s))
        }
        1 => {
            let n = value.to_long();
            !c.borrow_mut()
                .set_client_option(opt, OptValue::Int(n as u32 as i64))
        }
        _ => true,
    };
    if ret && option_type(opt) != 0 {
        report_conn_error(vm, f, &c);
    }
    vbool(!ret)
}

pub fn mysqli_get_charset(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let cs = c.borrow().charset;
    let cs = match cs {
        Some(cs) => cs,
        None => {
            docref_warning(vm, f, "The connection has no charset associated");
            return Value::Null;
        }
    };
    let id = vm.alloc_object_id();
    let mut o = PhpObject::new(b"stdClass".to_vec(), id);
    o.set_property(b"charset".to_vec(), vstr(cs.name));
    o.set_property(b"collation".to_vec(), vstr(cs.collation));
    o.set_property(b"dir".to_vec(), vstr(""));
    o.set_property(b"min_length".to_vec(), Value::Long(cs.char_minlen as i64));
    o.set_property(b"max_length".to_vec(), Value::Long(cs.char_maxlen as i64));
    o.set_property(b"number".to_vec(), Value::Long(cs.nr as i64));
    o.set_property(b"state".to_vec(), Value::Long(1));
    o.set_property(b"comment".to_vec(), vstr(cs.comment));
    Value::Object(Rc::new(RefCell::new(o)))
}

pub fn mysqli_set_charset(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (a, _, c) = match link_args(vm, f, "Os", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let name = String::from_utf8_lossy(a[1].str().unwrap_or(b"")).into_owned();
    if !c.borrow_mut().set_charset(&name) {
        report_conn_error(vm, f, &c);
        return Value::False;
    }
    flush(vm, f);
    Value::True
}

fn stats_array(names: &[&str], values: &[u64]) -> Value {
    let mut arr = PhpArray::new();
    for (i, n) in names.iter().enumerate() {
        arr.set(
            ArrayKey::String(PhpString::from_bytes(n.as_bytes())),
            vstr(values.get(i).copied().unwrap_or(0).to_string()),
        );
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn mysqli_get_client_stats(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    stats_array(&stats::NAMES, &stats::global_values())
}

pub fn mysqli_get_connection_stats(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let v = c.borrow().stats.values.clone();
    stats_array(&stats::NAMES, &v)
}

pub fn mysqli_get_links_stats(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    if parse(vm, f, "", &[], args).is_err() {
        return Value::Null;
    }
    let (t, a, cp) = MYG.with(|g| {
        let g = g.borrow();
        (
            g.num_links,
            g.num_active_persistent,
            g.num_inactive_persistent,
        )
    });
    let mut arr = PhpArray::new();
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"total")),
        Value::Long(t),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"active_plinks")),
        Value::Long(a),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"cached_plinks")),
        Value::Long(cp),
    );
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn mysqli_report(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    set_report_mode(a[0].long_or(0));
    Value::True
}

/// mysqli_get_warnings (php_get_warnings)
pub fn get_warnings(c: &ConnRef) -> Vec<WarningItem> {
    let mut out = Vec::new();
    let ok = c.borrow_mut().query(b"SHOW WARNINGS");
    if !ok {
        return out;
    }
    let r = crate::mysqlnd::conn::use_result(c);
    let mut r = match r {
        Some(r) => r,
        None => return out,
    };
    while let Ok(Some(row)) = r.fetch_row(true) {
        let errno = row.get(1).map(|v| v.to_long()).unwrap_or(0);
        let reason = row
            .get(2)
            .map(|v| Value::String(v.to_php_string()))
            .unwrap_or_else(|| vstr(""));
        out.push(WarningItem {
            reason,
            sqlstate: vstr("HY000"),
            errorno: errno,
        });
    }
    r.free_result(false);
    out
}

pub fn new_warning_object(vm: &mut Vm, w: Vec<WarningItem>) -> Value {
    let r = Resource {
        status: std::cell::Cell::new(MYSQLI_STATUS_VALID),
        kind: Kind::Warning(Rc::new(w), std::cell::Cell::new(0)),
    };
    Value::Object(new_object(vm, b"mysqli_warning", Base::Warning, Some(r)))
}

pub fn mysqli_get_warnings(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let (_, _, c) = match link_args(vm, f, "O", args, MYSQLI_STATUS_VALID) {
        Ok(x) => x,
        Err(()) => return Value::Null,
    };
    let wc = c.borrow().upsert.warning_count;
    if wc == 0 {
        return Value::False;
    }
    let w = get_warnings(&c);
    flush(vm, f);
    if w.is_empty() {
        return Value::False;
    }
    new_warning_object(vm, w)
}

// ── mysqli_poll ───────────────────────────────────────────────────────

fn poll_array_to_conns(vm: &mut Vm, arr: &Value) -> Result<Vec<(Value, ConnRef)>, ()> {
    let mut out = Vec::new();
    let items: Vec<Value> = match arr.deref() {
        Value::Array(a) => a.borrow().iter().map(|(_, v)| v.deref()).collect(),
        _ => return Ok(out),
    };
    for (i, elem) in items.into_iter().enumerate() {
        let o = match &elem {
            Value::Object(o) if obj_is(vm, o, b"mysqli") => o.clone(),
            other => {
                let m = format!(
                    "Argument #{} must be an instance of mysqli, {} given",
                    i + 1,
                    Vm::value_type_name(other)
                );
                throw(vm, b"TypeError", &m);
                return Err(());
            }
        };
        let r = match get_res(&o) {
            Some(r) => r,
            None => {
                let m = format!("{} object is already closed", ce_name(&o));
                throw(vm, b"Error", &m);
                return Err(());
            }
        };
        if r.status.get() < MYSQLI_STATUS_VALID {
            let m = format!("{} object is not fully initialized", ce_name(&o));
            throw(vm, b"Error", &m);
            return Err(());
        }
        if let Kind::Link(m) = &r.kind {
            if let Some(c) = m.borrow().mysql.clone() {
                out.push((elem.clone(), c));
            }
        }
    }
    Ok(out)
}

fn write_ref(target: &Value, v: Value) {
    if let Value::Reference(r) = target {
        *r.borrow_mut() = v;
    }
}

fn list_value(items: Vec<Value>) -> Value {
    let mut arr = PhpArray::new();
    for v in items {
        arr.push(v);
    }
    Value::Array(Rc::new(RefCell::new(arr)))
}

pub fn mysqli_poll(vm: &mut Vm, f: &Func, args: &[Value]) -> Value {
    let a = match parse(vm, f, "a!a!al|l", &[], args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let sec = a[3].long_or(0);
    let usec = a[4].long_or(0);
    if sec < 0 {
        f.value_error(vm, 4, "must be greater than or equal to 0");
        return Value::Null;
    }
    if usec < 0 {
        f.value_error(vm, 5, "must be greater than or equal to 0");
        return Value::Null;
    }
    let r_given = !matches!(a[0], Arg::Null);
    let e_given = !matches!(a[1], Arg::Null);
    if !r_given && !e_given {
        throw(vm, b"ValueError", "No stream arrays were passed");
        return Value::Null;
    }
    let r_arr = if r_given {
        match poll_array_to_conns(vm, &args[0]) {
            Ok(v) => Some(v),
            Err(()) => return Value::Null,
        }
    } else {
        None
    };
    let e_arr = if e_given {
        match poll_array_to_conns(vm, &args[1]) {
            Ok(v) => Some(v),
            Err(()) => return Value::Null,
        }
    } else {
        None
    };
    // mysqlnd_poll
    let mut dont_poll: Vec<Value> = Vec::new();
    let mut r_list: Vec<(Value, ConnRef)> = Vec::new();
    if let Some(r) = r_arr {
        for (v, c) in r {
            let st = c.borrow().state;
            if st <= ConnState::Ready || st == ConnState::QuitSent {
                dont_poll.push(v);
            } else {
                r_list.push((v, c));
            }
        }
    }
    let e_list: Vec<(Value, ConnRef)> = e_arr.unwrap_or_default();
    let mut fds: Vec<libc::pollfd> = Vec::new();
    let mut r_idx = Vec::new();
    for (i, (_, c)) in r_list.iter().enumerate() {
        if let Some(fd) = c.borrow().vio.raw_fd() {
            r_idx.push((i, fds.len()));
            fds.push(libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
    }
    let mut e_idx = Vec::new();
    for (i, (_, c)) in e_list.iter().enumerate() {
        if let Some(fd) = c.borrow().vio.raw_fd() {
            e_idx.push((i, fds.len()));
            fds.push(libc::pollfd {
                fd,
                events: libc::POLLPRI,
                revents: 0,
            });
        }
    }
    let ret: Value;
    if fds.is_empty() {
        let msg = if !dont_poll.is_empty() {
            "All arrays passed are clear"
        } else {
            "No stream arrays were passed"
        };
        docref_warning(vm, f, msg);
        ret = Value::False;
    } else {
        let total_us = sec as i128 * 1_000_000 + usec as i128;
        let timeout_ms = (total_us / 1000).min(i32::MAX as i128) as i32;
        // data buffered in user space counts as readable
        let mut pre_ready = Vec::new();
        for (i, (_, c)) in r_list.iter().enumerate() {
            if c.borrow().vio.buffered() > 0 {
                pre_ready.push(i);
            }
        }
        // SAFETY: fds is a valid array of pollfd of the given length.
        let rc = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                if pre_ready.is_empty() { timeout_ms } else { 0 },
            )
        };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            docref_warning(
                vm,
                f,
                &format!(
                    "Unable to select [{}]: {} (max_fd={})",
                    e.raw_os_error().unwrap_or(0),
                    crate::mysqlnd::vio::strerror(&e),
                    fds.iter().map(|p| p.fd).max().unwrap_or(0)
                ),
            );
            ret = Value::False;
        } else {
            let mut nready = 0i64;
            let mut new_r = Vec::new();
            for (i, fi) in &r_idx {
                if fds[*fi].revents != 0 || pre_ready.contains(i) {
                    new_r.push(r_list[*i].0.clone());
                    nready += 1;
                }
            }
            let mut new_e = Vec::new();
            for (i, fi) in &e_idx {
                if fds[*fi].revents & libc::POLLPRI != 0 {
                    new_e.push(e_list[*i].0.clone());
                    nready += 1;
                }
            }
            if r_given {
                write_ref(&args[0], list_value(new_r));
            }
            if e_given {
                write_ref(&args[1], list_value(new_e));
            }
            ret = Value::Long(nready);
        }
    }
    // mysqlnd_dont_poll_zval_array_from_mysqlnd_array
    write_ref(
        &args[2],
        list_value(if r_given { dont_poll } else { Vec::new() }),
    );
    if matches!(ret, Value::False) {
        // arrays are still rewritten from the (unchanged) sets
        if r_given {
            let items: Vec<Value> = r_list.iter().map(|(v, _)| v.clone()).collect();
            write_ref(&args[0], list_value(items));
        }
        if e_given {
            let items: Vec<Value> = e_list.iter().map(|(v, _)| v.clone()).collect();
            write_ref(&args[1], list_value(items));
        }
    }
    ret
}
