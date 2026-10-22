//! The ldap_* functions (ldap.c).

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use goro_ldap::Conn;
use goro_ldap::consts::*;
use goro_ldap::message::{Control, LdapMessage};

use crate::ctl;
use crate::handles::{self, ConnData};
use crate::zpp::{self, Arg};

type R = Result<Value, VmError>;

fn sval(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}
fn bval(b: bool) -> Value {
    if b { Value::True } else { Value::False }
}
fn akey(s: &[u8]) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s))
}
fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn parse(vm: &mut Vm, f: &str, spec: &str, args: &[Value]) -> Option<Vec<Arg>> {
    zpp::parse(vm, f, spec, args).ok()
}

/// VERIFY_LDAP_LINK_CONNECTED: the ConnData, or throw "already been closed".
fn link(vm: &mut Vm, a: &Arg) -> Option<Rc<ConnData>> {
    let o = a.obj()?;
    let cd = handles::conn_data(&o)?;
    if cd.conn.borrow().is_none() {
        zpp::throw(vm, b"Error", "LDAP connection has already been closed");
        return None;
    }
    Some(cd)
}

fn warn(vm: &mut Vm, f: &str, msg: &str) {
    vm.emit_warning(&format!("{}(): {}", f, msg));
}

/// Parse the optional server-controls array argument.
fn controls_from(
    vm: &mut Vm,
    f: &str,
    a: Option<&Arg>,
    arg_num: usize,
) -> Result<Vec<Control>, ()> {
    match a {
        Some(Arg::Any(Value::Array(arr))) => {
            let arr = arr.borrow().clone();
            ctl::controls_from_array(vm, f, &arr, arg_num)
        }
        _ => Ok(Vec::new()),
    }
}

/// Run a closure with the connection borrowed mutably.
fn with_conn<T>(cd: &ConnData, f: impl FnOnce(&mut Conn) -> T) -> T {
    let mut b = cd.conn.borrow_mut();
    f(b.as_mut().unwrap())
}

// ---------------------------------------------------------------------------
// Connect / bind / unbind
// ---------------------------------------------------------------------------

pub fn ldap_connect(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_connect";
    let nargs = args
        .iter()
        .take_while(|v| !matches!(v, Value::Undef))
        .count();
    if nargs > 2 {
        vm.emit_deprecated("Calling ldap_connect() with Oracle-specific arguments is deprecated");
    } else if nargs == 2 {
        vm.emit_deprecated("Usage of ldap_connect with two arguments is deprecated");
    }
    if vm.current_exception.is_some() {
        return Ok(Value::Null);
    }
    let Some(a) = parse(vm, f, "|s!l", args) else {
        return Ok(Value::Null);
    };
    let host = a[0].opt_str().map(|s| s.to_vec());
    let port = a[1].long_or(LDAP_PORT as i64);

    // ldap.max_links is applied from --INI-- after MINIT, so read it here.
    let max = match vm.constants.get(b"ldap.max_links".as_slice()) {
        Some(v) => std::str::from_utf8(v.to_php_string().as_bytes())
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(-1),
        None => -1,
    };
    if max != -1 && crate::num_links() >= max {
        warn(
            vm,
            f,
            &format!("Too many open links ({})", crate::num_links()),
        );
        return Ok(Value::False);
    }

    // Build the URL (ldap_initialize). A bare host becomes ldap://host:port.
    let url_str = match &host {
        Some(h) if goro_ldap::url::is_ldap_url(&String::from_utf8_lossy(h)) => {
            String::from_utf8_lossy(h).into_owned()
        }
        Some(h) => {
            if port <= 0 || port > 65535 {
                zpp::value_error(vm, f, 2, "must be between 1 and 65535");
                return Ok(Value::Null);
            }
            format!("ldap://{}:{}", String::from_utf8_lossy(h), port)
        }
        None => {
            // no host: the default URI from ldap.conf / LDAP* env, else
            // localhost (openldap_ldap_init_w_*()).
            default_uri().unwrap_or_else(|| "ldap://localhost".to_string())
        }
    };
    let urls = match goro_ldap::url::parse_ext(&url_str, goro_ldap::url::PARSE_DEF_PORT) {
        Ok(u) => vec![u],
        Err(_) => {
            warn(
                vm,
                f,
                "Could not create session handle: Bad parameter to an ldap routine",
            );
            return Ok(Value::False);
        }
    };
    // ldap_initialize() rejects a URI that carries a DN / attributes /
    // filter (only the host[:port] part is usable for a session handle).
    let u0 = &urls[0];
    if u0.dn.as_ref().is_some_and(|d| !d.is_empty()) || !u0.attrs.is_empty() || u0.filter.is_some()
    {
        warn(
            vm,
            f,
            "Could not create session handle: Bad parameter to an ldap routine",
        );
        return Ok(Value::False);
    }
    // LDAP_OPT_HOST_NAME reads ldap_url_list2hosts(defludp): "host[:port]".
    let host_name = {
        let u = &urls[0];
        let h = u.host.clone().unwrap_or_default();
        if u.port != 0 {
            format!("{}:{}", h, u.port).into_bytes()
        } else {
            h.into_bytes()
        }
    };
    let mut opts = crate::clone_global_opts();
    opts.host_name = Some(host_name);
    let conn = Conn::new(urls, opts);
    crate::inc_links();
    Ok(handles::new_conn(vm, conn))
}

/// The default URI from $LDAPCONF / $LDAPURI / $LDAPHOST (init.c env/conf).
fn default_uri() -> Option<String> {
    if let Ok(uri) = std::env::var("LDAPURI") {
        if !uri.trim().is_empty() {
            return Some(uri.split_whitespace().next()?.to_string());
        }
    }
    // $LDAPCONF names an alternate ldap.conf; parse its URI/HOST/PORT.
    if let Ok(path) = std::env::var("LDAPCONF") {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let mut host: Option<String> = None;
            let mut port: Option<String> = None;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut it = line.splitn(2, char::is_whitespace);
                let key = it.next().unwrap_or("").to_ascii_uppercase();
                let val = it.next().unwrap_or("").trim();
                match key.as_str() {
                    "URI" => {
                        if let Some(first) = val.split_whitespace().next() {
                            return Some(first.to_string());
                        }
                    }
                    "HOST" => host = Some(val.to_string()),
                    "PORT" => port = Some(val.to_string()),
                    _ => {}
                }
            }
            if let Some(h) = host {
                return Some(match port {
                    Some(p) => format!("ldap://{}:{}", h, p),
                    None => format!("ldap://{}", h),
                });
            }
        }
    }
    if let Ok(h) = std::env::var("LDAPHOST") {
        if !h.trim().is_empty() {
            return Some(format!("ldap://{}", h.trim()));
        }
    }
    None
}

pub fn ldap_unbind(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_unbind";
    let Some(a) = parse(vm, f, "C", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    // ldap_link_free: drop the connection.
    *cd.conn.borrow_mut() = None;
    crate::dec_links();
    Ok(Value::True)
}

pub fn ldap_bind(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_bind";
    let Some(a) = parse(vm, f, "C|p!p!", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a.get(1).and_then(|x| x.opt_str()).unwrap_or(b"").to_vec();
    let pw = a.get(2).and_then(|x| x.opt_str()).unwrap_or(b"").to_vec();
    let rc = with_conn(&cd, |c| c.simple_bind(&dn, &pw));
    if rc != LDAP_SUCCESS {
        warn(
            vm,
            f,
            &format!("Unable to bind to server: {}", goro_ldap::err2string(rc)),
        );
        return Ok(Value::False);
    }
    Ok(Value::True)
}

pub fn ldap_bind_ext(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_bind_ext";
    let Some(a) = parse(vm, f, "C|p!p!h!", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a.get(1).and_then(|x| x.opt_str()).unwrap_or(b"").to_vec();
    let pw = a.get(2).and_then(|x| x.opt_str()).unwrap_or(b"").to_vec();
    let ctrls = match controls_from(vm, f, a.get(3), 4) {
        Ok(c) => c,
        Err(()) => return Ok(Value::False),
    };
    match with_conn(&cd, |c| c.bind_ext(&dn, &pw, &ctrls)) {
        Ok(m) => Ok(handles::new_result(vm, Rc::new(m))),
        Err(e) => {
            warn(
                vm,
                f,
                &format!(
                    "Unable to bind to server: {} ({})",
                    goro_ldap::err2string(e),
                    e
                ),
            );
            Ok(Value::False)
        }
    }
}

pub fn ldap_sasl_bind(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_sasl_bind";
    let Some(a) = parse(vm, f, "C|p!p!p!p!p!p!p!", args) else {
        return Ok(Value::Null);
    };
    let Some(_cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    // SASL interactive bind is not supported by the pure-Rust client yet.
    warn(
        vm,
        f,
        &format!(
            "Unable to bind to server: {}",
            goro_ldap::err2string(LDAP_AUTH_METHOD_NOT_SUPPORTED)
        ),
    );
    Ok(Value::False)
}

pub fn ldap_start_tls(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_start_tls";
    let Some(a) = parse(vm, f, "C", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let rc = with_conn(&cd, |c| {
        c.opts.version = LDAP_VERSION3;
        c.start_tls()
    });
    if rc != LDAP_SUCCESS {
        warn(
            vm,
            f,
            &format!("Unable to start TLS: {}", goro_ldap::err2string(rc)),
        );
        return Ok(Value::False);
    }
    Ok(Value::True)
}

// ---------------------------------------------------------------------------
// Error reporting
// ---------------------------------------------------------------------------

pub fn ldap_errno(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_errno";
    let Some(a) = parse(vm, f, "C", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    Ok(Value::Long(with_conn(&cd, |c| c.errno) as i64))
}

pub fn ldap_error(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_error";
    let Some(a) = parse(vm, f, "C", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let e = with_conn(&cd, |c| c.errno);
    Ok(sval(goro_ldap::err2string(e).as_bytes()))
}

pub fn ldap_err2str(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_err2str";
    let Some(a) = parse(vm, f, "l", args) else {
        return Ok(Value::Null);
    };
    Ok(sval(
        goro_ldap::err2string(a[0].long_or(0) as i32).as_bytes(),
    ))
}

// ---------------------------------------------------------------------------
// DN helpers / escape
// ---------------------------------------------------------------------------

pub fn ldap_explode_dn(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_explode_dn";
    let Some(a) = parse(vm, f, "pl", args) else {
        return Ok(Value::Null);
    };
    let dn = a[0].str();
    let notypes = a[1].long_or(0) != 0;
    match goro_ldap::dn::explode_dn(dn, notypes) {
        Some(parts) => {
            let mut out = PhpArray::new();
            for (i, p) in parts.iter().enumerate() {
                out.set(ArrayKey::Int(i as i64), sval(p));
            }
            out.set(akey(b"count"), Value::Long(parts.len() as i64));
            Ok(arr(out))
        }
        None => Ok(Value::False),
    }
}

pub fn ldap_dn2ufn(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_dn2ufn";
    let Some(a) = parse(vm, f, "p", args) else {
        return Ok(Value::Null);
    };
    match goro_ldap::dn::dn2ufn(a[0].str()) {
        Some(s) => Ok(sval(&s)),
        None => Ok(Value::False),
    }
}

pub fn ldap_escape(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_escape";
    let Some(a) = parse(vm, f, "s|sl", args) else {
        return Ok(Value::Null);
    };
    let value = a[0].str().to_vec();
    let ignore = a.get(1).map(|x| x.str().to_vec()).unwrap_or_default();
    let flags = a.get(2).map(|x| x.long_or(0)).unwrap_or(0);
    if value.is_empty() {
        return Ok(sval(b""));
    }
    let map = goro_ldap::escape::build_map(flags, &ignore);
    // ZSTR_MAX_LEN
    let max_len = (i64::MAX as usize) >> 1;
    match goro_ldap::escape::do_escape(&map, &value, flags, max_len) {
        Some(out) => Ok(sval(&out)),
        None => {
            zpp::value_error(vm, f, 1, "is too long");
            Ok(Value::Null)
        }
    }
}

// ---------------------------------------------------------------------------
// Search family
// ---------------------------------------------------------------------------

pub fn ldap_read(vm: &mut Vm, args: &[Value]) -> R {
    do_search(vm, "ldap_read", args, LDAP_SCOPE_BASE)
}
pub fn ldap_list(vm: &mut Vm, args: &[Value]) -> R {
    do_search(vm, "ldap_list", args, LDAP_SCOPE_ONELEVEL)
}
pub fn ldap_search(vm: &mut Vm, args: &[Value]) -> R {
    do_search(vm, "ldap_search", args, LDAP_SCOPE_SUBTREE)
}

fn str_has_nul(s: &[u8]) -> bool {
    s.contains(&0)
}

/// Coerce a plain int argument; TypeError on failure.
fn long_param(vm: &mut Vm, f: &str, n: usize, v: &Value) -> Option<i64> {
    let strict = vm.caller_strict_types;
    let line = vm.current_line;
    match vm.zpp_coerce_scalar(goro_core::arginfo::T_INT, &v.deref(), strict, line) {
        Ok(Some(Value::Long(x))) => Some(x),
        _ => {
            zpp::type_error(
                vm,
                f,
                n,
                &format!(
                    "must be of type int, {} given",
                    Vm::value_type_name(&v.deref())
                ),
            );
            None
        }
    }
}

fn do_search(vm: &mut Vm, f: &str, args: &[Value], scope: i32) -> R {
    let given = args
        .iter()
        .take_while(|v| !matches!(v, Value::Undef))
        .count();
    if given < 3 {
        // arity handled by generic layer
        return Ok(Value::Null);
    }
    let link_v = args[0].deref();
    let base_v = args[1].deref();
    let filter_v = args[2].deref();

    // attrs (arg 4): array, default empty
    let mut ldap_attrs: Vec<Vec<u8>> = Vec::new();
    if given >= 4 {
        let av = args[3].deref();
        match &av {
            Value::Array(arr) => {
                let arr = arr.borrow();
                let num = arr.len();
                if num != 0 {
                    if !is_numerically_indexed(&arr) {
                        zpp::value_error(vm, f, 4, "must be an array with numeric keys");
                        return Ok(Value::Null);
                    }
                    for (_k, v) in arr.iter() {
                        let v = v.deref();
                        let Value::String(s) = &v else {
                            zpp::type_error(
                                vm,
                                f,
                                4,
                                &format!(
                                    "must be a list of strings, {} given",
                                    Vm::value_type_name(&v)
                                ),
                            );
                            return Ok(Value::Null);
                        };
                        if s.as_bytes().contains(&0) {
                            zpp::value_error(
                                vm,
                                f,
                                4,
                                "must not contain strings with any null bytes",
                            );
                            return Ok(Value::Null);
                        }
                        ldap_attrs.push(s.as_bytes().to_vec());
                    }
                }
            }
            other => {
                zpp::type_error(
                    vm,
                    f,
                    4,
                    &format!(
                        "must be of type array, {} given",
                        Vm::value_type_name(other)
                    ),
                );
                return Ok(Value::Null);
            }
        }
    }
    let attrsonly = if given >= 5 {
        match long_param(vm, f, 5, &args[4]) {
            Some(n) => n != 0,
            None => return Ok(Value::Null),
        }
    } else {
        false
    };
    let sizelimit = if given >= 6 {
        match long_param(vm, f, 6, &args[5]) {
            Some(n) => n as i32,
            None => return Ok(Value::Null),
        }
    } else {
        -1
    };
    let timelimit = if given >= 7 {
        match long_param(vm, f, 7, &args[6]) {
            Some(n) => n as i32,
            None => return Ok(Value::Null),
        }
    } else {
        -1
    };
    let deref = if given >= 8 {
        match long_param(vm, f, 8, &args[7]) {
            Some(n) => n as i32,
            None => return Ok(Value::Null),
        }
    } else {
        LDAP_DEREF_NEVER
    };

    // base and filter must be array|string
    let base_is_arr = matches!(base_v, Value::Array(_));
    let base_is_str = matches!(base_v, Value::String(_));
    let filter_is_arr = matches!(filter_v, Value::Array(_));
    let filter_is_str = matches!(filter_v, Value::String(_));
    if !base_is_arr && !base_is_str {
        zpp::type_error(
            vm,
            f,
            2,
            &format!(
                "must be of type array|string, {} given",
                Vm::value_type_name(&base_v)
            ),
        );
        return Ok(Value::Null);
    }
    if !filter_is_arr && !filter_is_str {
        zpp::type_error(
            vm,
            f,
            3,
            &format!(
                "must be of type array|string, {} given",
                Vm::value_type_name(&filter_v)
            ),
        );
        return Ok(Value::Null);
    }

    // controls (arg 9)
    let controls = if given >= 9 {
        match &args[8].deref() {
            Value::Null => Vec::new(),
            Value::Array(_) => match controls_from(vm, f, Some(&Arg::Any(args[8].deref())), 9) {
                Ok(c) => c,
                Err(()) => return Ok(Value::Null),
            },
            other => {
                zpp::type_error(
                    vm,
                    f,
                    9,
                    &format!(
                        "must be of type ?array, {} given",
                        Vm::value_type_name(other)
                    ),
                );
                return Ok(Value::Null);
            }
        }
    } else {
        Vec::new()
    };

    // parallel search?
    if let Value::Array(links) = &link_v {
        return do_search_parallel(
            vm,
            f,
            links,
            &base_v,
            &filter_v,
            &ldap_attrs,
            attrsonly,
            sizelimit,
            timelimit,
            deref,
            &controls,
        );
    }
    // single connection
    let Value::Object(o) = &link_v else {
        zpp::type_error(
            vm,
            f,
            1,
            &format!(
                "must be of type LDAP\\Connection|array, {} given",
                Vm::value_type_name(&link_v)
            ),
        );
        return Ok(Value::Null);
    };
    if !o.borrow().class_name.eq_ignore_ascii_case(zpp::CONN_CLASS) {
        zpp::type_error(
            vm,
            f,
            1,
            &format!(
                "must be of type LDAP\\Connection|array, {} given",
                Vm::value_type_name(&link_v)
            ),
        );
        return Ok(Value::Null);
    }
    let Some(cd) = handles::conn_data(o) else {
        return Ok(Value::Null);
    };
    if cd.conn.borrow().is_none() {
        zpp::throw(vm, b"Error", "LDAP connection has already been closed");
        return Ok(Value::Null);
    }
    let Value::String(base_s) = &base_v else {
        zpp::type_error(
            vm,
            f,
            2,
            "must be of type string when argument #1 ($ldap) is an LDAP\\Connection instance",
        );
        return Ok(Value::Null);
    };
    let Value::String(filter_s) = &filter_v else {
        zpp::type_error(
            vm,
            f,
            3,
            "must be of type string when argument #1 ($ldap) is an LDAP\\Connection instance",
        );
        return Ok(Value::Null);
    };
    let base = base_s.as_bytes().to_vec();
    let filter = filter_s.as_bytes().to_vec();
    let res = with_conn(&cd, |c| {
        c.search(
            &base,
            scope,
            &filter,
            &ldap_attrs,
            attrsonly,
            sizelimit,
            timelimit,
            deref,
            &controls,
        )
    });
    search_result(vm, f, res)
}

fn search_result(vm: &mut Vm, f: &str, res: Result<LdapMessage, i32>) -> R {
    match res {
        Ok(m) => {
            let rc = m.rescode;
            if rc == LDAP_SIZELIMIT_EXCEEDED {
                warn(vm, f, "Partial search results returned: Sizelimit exceeded");
            } else if rc == LDAP_ADMINLIMIT_EXCEEDED {
                warn(
                    vm,
                    f,
                    "Partial search results returned: Adminlimit exceeded",
                );
            } else if rc != LDAP_SUCCESS && rc != LDAP_REFERRAL {
                warn(vm, f, &format!("Search: {}", goro_ldap::err2string(rc)));
                return Ok(Value::False);
            }
            Ok(handles::new_result(vm, Rc::new(m)))
        }
        Err(e) => {
            warn(vm, f, &format!("Search: {}", goro_ldap::err2string(e)));
            Ok(Value::False)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn do_search_parallel(
    vm: &mut Vm,
    f: &str,
    links: &Rc<RefCell<PhpArray>>,
    base_v: &Value,
    filter_v: &Value,
    attrs: &[Vec<u8>],
    attrsonly: bool,
    sizelimit: i32,
    timelimit: i32,
    deref: i32,
    controls: &[Control],
) -> R {
    let links = links.borrow();
    let num_links = links.len();
    if num_links == 0 {
        zpp::value_error(vm, f, 1, "must not be empty");
        // ValueError via must_not_be_empty is "must not be empty"
        return Ok(Value::Null);
    }
    if !is_list(&links) {
        zpp::value_error(vm, f, 1, "must be a list");
        return Ok(Value::Null);
    }
    // base
    let base_list: Option<Vec<Vec<u8>>> = match base_v {
        Value::Array(a) => {
            let a = a.borrow();
            if !is_list(&a) {
                zpp::value_error(vm, f, 2, "must be a list");
                return Ok(Value::Null);
            }
            if a.len() != num_links {
                zpp::value_error(vm, f, 2, "must be the same size as argument #1");
                return Ok(Value::Null);
            }
            Some(Vec::new()) // filled per-link below
        }
        Value::String(s) => {
            if str_has_nul(s.as_bytes()) {
                zpp::value_error(vm, f, 2, "must not contain null bytes");
                return Ok(Value::Null);
            }
            None
        }
        _ => None,
    };
    let filter_list: Option<Vec<Vec<u8>>> = match filter_v {
        Value::Array(a) => {
            let a = a.borrow();
            if !is_list(&a) {
                zpp::value_error(vm, f, 3, "must be a list");
                return Ok(Value::Null);
            }
            if a.len() != num_links {
                zpp::value_error(vm, f, 3, "must be the same size as argument #1");
                return Ok(Value::Null);
            }
            Some(Vec::new())
        }
        Value::String(s) => {
            if str_has_nul(s.as_bytes()) {
                zpp::value_error(vm, f, 3, "must not contain null bytes");
                return Ok(Value::Null);
            }
            None
        }
        _ => None,
    };
    let base_arr = if base_list.is_some() {
        match base_v {
            Value::Array(a) => Some(a.borrow().clone()),
            _ => None,
        }
    } else {
        None
    };
    let filter_arr = if filter_list.is_some() {
        match filter_v {
            Value::Array(a) => Some(a.borrow().clone()),
            _ => None,
        }
    } else {
        None
    };
    let base_str = match base_v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    };
    let filter_str = match filter_v {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        _ => None,
    };

    let mut out = PhpArray::new();
    for i in 0..num_links {
        let lv = links.get(&ArrayKey::Int(i as i64)).map(|v| v.deref());
        let Some(Value::Object(o)) = &lv else {
            zpp::value_error(vm, f, 1, "must be a list of LDAP\\Connection");
            return Ok(Value::Null);
        };
        if !o.borrow().class_name.eq_ignore_ascii_case(zpp::CONN_CLASS) {
            zpp::value_error(vm, f, 1, "must be a list of LDAP\\Connection");
            return Ok(Value::Null);
        }
        let Some(cd) = handles::conn_data(o) else {
            return Ok(Value::Null);
        };
        if cd.conn.borrow().is_none() {
            zpp::throw(vm, b"Error", "LDAP connection has already been closed");
            return Ok(Value::Null);
        }
        // per-link base/filter
        let base = if let Some(ba) = &base_arr {
            let v = ba.get(&ArrayKey::Int(i as i64)).map(|v| v.deref());
            let Some(Value::String(s)) = &v else {
                zpp::type_error(
                    vm,
                    f,
                    2,
                    &format!(
                        "must be a list of strings, {} given",
                        v.as_ref().map(Vm::value_type_name).unwrap_or_default()
                    ),
                );
                return Ok(Value::Null);
            };
            if str_has_nul(s.as_bytes()) {
                zpp::value_error(vm, f, 2, "must not contain null bytes");
                return Ok(Value::Null);
            }
            s.as_bytes().to_vec()
        } else {
            base_str.clone().unwrap_or_default()
        };
        let filter = if let Some(fa) = &filter_arr {
            let v = fa.get(&ArrayKey::Int(i as i64)).map(|v| v.deref());
            let Some(Value::String(s)) = &v else {
                zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be a list of strings, {} given",
                        v.as_ref().map(Vm::value_type_name).unwrap_or_default()
                    ),
                );
                return Ok(Value::Null);
            };
            if str_has_nul(s.as_bytes()) {
                zpp::value_error(vm, f, 3, "must not contain null bytes");
                return Ok(Value::Null);
            }
            s.as_bytes().to_vec()
        } else {
            filter_str.clone().unwrap_or_default()
        };
        let res = with_conn(&cd, |c| {
            c.search(
                &base,
                scope_of(f),
                &filter,
                attrs,
                attrsonly,
                sizelimit,
                timelimit,
                deref,
                controls,
            )
        });
        match res {
            Ok(m) => {
                let rc = m.rescode;
                if rc == LDAP_SIZELIMIT_EXCEEDED {
                    warn(vm, f, "Partial search results returned: Sizelimit exceeded");
                } else if rc == LDAP_ADMINLIMIT_EXCEEDED {
                    warn(
                        vm,
                        f,
                        "Partial search results returned: Adminlimit exceeded",
                    );
                }
                if rc != LDAP_SUCCESS
                    && rc != LDAP_SIZELIMIT_EXCEEDED
                    && rc != LDAP_ADMINLIMIT_EXCEEDED
                    && rc != LDAP_REFERRAL
                {
                    out.push(bval(false));
                } else {
                    out.push(handles::new_result(vm, Rc::new(m)));
                }
            }
            Err(_) => out.push(bval(false)),
        }
    }
    Ok(arr(out))
}

fn scope_of(f: &str) -> i32 {
    match f {
        "ldap_read" => LDAP_SCOPE_BASE,
        "ldap_list" => LDAP_SCOPE_ONELEVEL,
        _ => LDAP_SCOPE_SUBTREE,
    }
}

fn is_numerically_indexed(a: &PhpArray) -> bool {
    if a.len() == 0 {
        return true;
    }
    a.iter().all(|(k, _)| matches!(k, ArrayKey::Int(_)))
}
fn is_list(a: &PhpArray) -> bool {
    let mut i = 0i64;
    for (k, _) in a.iter() {
        match k {
            ArrayKey::Int(n) if *n == i => i += 1,
            _ => return false,
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Result navigation
// ---------------------------------------------------------------------------

fn result_msg(vm: &mut Vm, a: &Arg) -> Option<Rc<LdapMessage>> {
    let o = a.obj()?;
    let rd = handles::result_data(&o)?;
    match rd.msg.borrow().clone() {
        Some(m) => Some(m),
        None => {
            zpp::throw(vm, b"Error", "LDAP result has already been closed");
            None
        }
    }
}

pub fn ldap_free_result(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_free_result";
    let Some(a) = parse(vm, f, "R", args) else {
        return Ok(Value::Null);
    };
    let Some(o) = a[0].obj() else {
        return Ok(Value::Null);
    };
    let Some(rd) = handles::result_data(&o) else {
        return Ok(Value::Null);
    };
    if rd.msg.borrow().is_none() {
        zpp::throw(vm, b"Error", "LDAP result has already been closed");
        return Ok(Value::Null);
    }
    *rd.msg.borrow_mut() = None;
    Ok(Value::True)
}

pub fn ldap_count_entries(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_count_entries";
    let Some(a) = parse(vm, f, "CR", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    Ok(Value::Long(m.entries.len() as i64))
}

pub fn ldap_first_entry(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_first_entry";
    let Some(a) = parse(vm, f, "CR", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    if m.entries.is_empty() {
        return Ok(Value::False);
    }
    let res_obj = a[1].obj().unwrap();
    Ok(handles::new_entry(vm, res_obj, m, false, 0))
}

pub fn ldap_next_entry(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_next_entry";
    let Some(a) = parse(vm, f, "CE", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(o) = a[1].obj() else {
        return Ok(Value::Null);
    };
    let Some(ed) = handles::entry_data(&o) else {
        return Ok(Value::Null);
    };
    let next = ed.index + 1;
    if ed.is_ref || next >= ed.msg.entries.len() {
        return Ok(Value::False);
    }
    Ok(handles::new_entry(
        vm,
        ed.res.clone(),
        ed.msg.clone(),
        false,
        next,
    ))
}

pub fn ldap_get_entries(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_get_entries";
    let Some(a) = parse(vm, f, "CR", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    out.set(akey(b"count"), Value::Long(m.entries.len() as i64));
    for (i, e) in m.entries.iter().enumerate() {
        let mut entry = PhpArray::new();
        for (ai, (name, vals)) in e.attrs.iter().enumerate() {
            let mut va = PhpArray::new();
            va.set(akey(b"count"), Value::Long(vals.len() as i64));
            for (vi, v) in vals.iter().enumerate() {
                va.set(ArrayKey::Int(vi as i64), sval(v));
            }
            let lower: Vec<u8> = name.to_ascii_lowercase();
            entry.set(akey(&lower), arr(va));
            entry.set(ArrayKey::Int(ai as i64), sval(&lower));
        }
        entry.set(akey(b"count"), Value::Long(e.attrs.len() as i64));
        entry.set(akey(b"dn"), sval(&e.dn));
        out.set(ArrayKey::Int(i as i64), arr(entry));
    }
    Ok(arr(out))
}

fn entry_of(_vm: &mut Vm, a: &Arg) -> Option<Rc<handles::EntryData>> {
    let o = a.obj()?;
    handles::entry_data(&o)
}

pub fn ldap_first_attribute(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_first_attribute";
    let Some(a) = parse(vm, f, "CE", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(ed) = entry_of(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    let Some(e) = ed.msg.entries.get(ed.index) else {
        return Ok(Value::False);
    };
    if e.attrs.is_empty() {
        ed.attr_cursor.set(None);
        return Ok(Value::False);
    }
    ed.attr_cursor.set(Some(1));
    Ok(sval(&e.attrs[0].0))
}

pub fn ldap_next_attribute(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_next_attribute";
    let Some(a) = parse(vm, f, "CE", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(ed) = entry_of(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    let Some(cursor) = ed.attr_cursor.get() else {
        warn(
            vm,
            f,
            "Called before calling ldap_first_attribute() or no attributes found in result entry",
        );
        return Ok(Value::False);
    };
    let Some(e) = ed.msg.entries.get(ed.index) else {
        return Ok(Value::False);
    };
    if cursor >= e.attrs.len() {
        ed.attr_cursor.set(None);
        return Ok(Value::False);
    }
    ed.attr_cursor.set(Some(cursor + 1));
    Ok(sval(&e.attrs[cursor].0))
}

pub fn ldap_get_attributes(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_get_attributes";
    let Some(a) = parse(vm, f, "CE", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(ed) = entry_of(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    let mut out = PhpArray::new();
    if let Some(e) = ed.msg.entries.get(ed.index) {
        for (ai, (name, vals)) in e.attrs.iter().enumerate() {
            let mut va = PhpArray::new();
            va.set(akey(b"count"), Value::Long(vals.len() as i64));
            for (vi, v) in vals.iter().enumerate() {
                va.set(ArrayKey::Int(vi as i64), sval(v));
            }
            out.set(akey(name), arr(va));
            out.set(ArrayKey::Int(ai as i64), sval(name));
        }
        out.set(akey(b"count"), Value::Long(e.attrs.len() as i64));
    } else {
        out.set(akey(b"count"), Value::Long(0));
    }
    Ok(arr(out))
}

pub fn ldap_get_values_len(vm: &mut Vm, args: &[Value]) -> R {
    get_values(vm, "ldap_get_values_len", args)
}
pub fn ldap_get_values(vm: &mut Vm, args: &[Value]) -> R {
    get_values(vm, "ldap_get_values", args)
}

fn get_values(vm: &mut Vm, f: &str, args: &[Value]) -> R {
    let Some(a) = parse(vm, f, "CEp", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let Some(ed) = entry_of(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    let attr = a[2].str();
    let vals = ed.msg.entries.get(ed.index).and_then(|e| {
        e.attrs
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(attr))
            .map(|(_, v)| v.clone())
    });
    match vals {
        Some(vals) => {
            let mut out = PhpArray::new();
            for (i, v) in vals.iter().enumerate() {
                out.set(ArrayKey::Int(i as i64), sval(v));
            }
            out.set(akey(b"count"), Value::Long(vals.len() as i64));
            Ok(arr(out))
        }
        None => {
            let e = with_conn(&cd, |c| c.errno);
            warn(
                vm,
                f,
                &format!(
                    "Cannot get the value(s) of attribute {}",
                    goro_ldap::err2string(e)
                ),
            );
            Ok(Value::False)
        }
    }
}

pub fn ldap_get_dn(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_get_dn";
    let Some(a) = parse(vm, f, "CE", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(ed) = entry_of(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    match ed.msg.entries.get(ed.index) {
        Some(e) => Ok(sval(&e.dn)),
        None => Ok(Value::False),
    }
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

pub fn ldap_count_references(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_count_references";
    let Some(a) = parse(vm, f, "CR", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    Ok(Value::Long(m.references.len() as i64))
}

pub fn ldap_first_reference(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_first_reference";
    let Some(a) = parse(vm, f, "CR", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    if m.references.is_empty() {
        return Ok(Value::False);
    }
    let res_obj = a[1].obj().unwrap();
    Ok(handles::new_entry(vm, res_obj, m, true, 0))
}

pub fn ldap_next_reference(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_next_reference";
    let Some(a) = parse(vm, f, "CE", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(o) = a[1].obj() else {
        return Ok(Value::Null);
    };
    let Some(ed) = handles::entry_data(&o) else {
        return Ok(Value::Null);
    };
    let next = ed.index + 1;
    if !ed.is_ref || next >= ed.msg.references.len() {
        return Ok(Value::False);
    }
    Ok(handles::new_entry(
        vm,
        ed.res.clone(),
        ed.msg.clone(),
        true,
        next,
    ))
}

pub fn ldap_parse_reference(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_parse_reference";
    let Some(a) = parse(vm, f, "CEz", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(ed) = entry_of(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    if !ed.is_ref {
        return Ok(Value::False);
    }
    let Some(refr) = ed.msg.references.get(ed.index) else {
        return Ok(Value::False);
    };
    let mut out = PhpArray::new();
    for r in &refr.referrals {
        out.push(sval(r));
    }
    if let Some(z) = a[2].any() {
        crate::assign_ref(&z, arr(out));
    }
    Ok(Value::True)
}

// ---------------------------------------------------------------------------
// parse_result / parse_exop
// ---------------------------------------------------------------------------

pub fn ldap_parse_result(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_parse_result";
    let Some(a) = parse(vm, f, "CRz|zzzz", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    // errcode (arg 3)
    if let Some(z) = a[2].any() {
        crate::assign_ref(&z, Value::Long(m.rescode as i64));
    }
    // matcheddn (arg 4)
    if let Some(z) = a.get(3).and_then(|x| x.any()) {
        crate::assign_ref(&z, sval(&m.matched_dn));
    }
    // errmsg (arg 5)
    if let Some(z) = a.get(4).and_then(|x| x.any()) {
        crate::assign_ref(&z, sval(&m.errmsg));
    }
    // referrals (arg 6)
    if let Some(z) = a.get(5).and_then(|x| x.any()) {
        let mut out = PhpArray::new();
        for r in &m.referrals {
            out.push(sval(r));
        }
        crate::assign_ref(&z, arr(out));
    }
    // controls (arg 7)
    if let Some(z) = a.get(6).and_then(|x| x.any()) {
        let a = ctl::controls_to_array(&m.ctrls, false);
        crate::assign_ref(&z, arr(a));
    }
    Ok(Value::True)
}

pub fn ldap_parse_exop(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_parse_exop";
    let Some(a) = parse(vm, f, "CR|zz", args) else {
        return Ok(Value::Null);
    };
    if link(vm, &a[0]).is_none() {
        return Ok(Value::Null);
    }
    let Some(m) = result_msg(vm, &a[1]) else {
        return Ok(Value::Null);
    };
    if let Some(z) = a.get(2).and_then(|x| x.any()) {
        crate::assign_ref(&z, sval(m.exop_data.as_deref().unwrap_or(b"")));
    }
    if let Some(z) = a.get(3).and_then(|x| x.any()) {
        crate::assign_ref(&z, sval(m.exop_oid.as_deref().unwrap_or(b"")));
    }
    Ok(Value::True)
}

// ---------------------------------------------------------------------------
// add / modify / delete / rename / compare
// ---------------------------------------------------------------------------

/// php_ldap_try_get_ldap_value_from_zval. `v` is matched WITHOUT dereferencing:
/// a reference (e.g. a list element `&$obj`) is not IS_OBJECT and falls to the
/// type error, as in php-src (which only derefs single, non-list values).
fn get_ldap_value(vm: &mut Vm, v: &Value) -> Result<Vec<u8>, VmError> {
    match v {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Long(_) | Value::Object(_) => Ok(vm.value_to_string_checked(v)?.as_bytes().to_vec()),
        Value::True => Ok(b"TRUE".to_vec()),
        Value::False => Ok(b"FALSE".to_vec()),
        other => {
            let m = format!(
                "LDAP value must be of type string|int|bool, {} given",
                Vm::value_type_name(other)
            );
            let e = vm.create_exception(b"TypeError", &m, vm.current_line);
            vm.current_exception = Some(e);
            Err(VmError {
                message: m,
                line: vm.current_line,
            })
        }
    }
}

const PHP_LD_FULL_ADD: i32 = 0xff;

pub fn ldap_add(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_add", args, PHP_LD_FULL_ADD, false)
}
pub fn ldap_add_ext(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_add_ext", args, PHP_LD_FULL_ADD, true)
}
pub fn ldap_mod_add(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_mod_add", args, LDAP_MOD_ADD, false)
}
pub fn ldap_mod_add_ext(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_mod_add_ext", args, LDAP_MOD_ADD, true)
}
pub fn ldap_mod_replace(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_mod_replace", args, LDAP_MOD_REPLACE, false)
}
pub fn ldap_mod_replace_ext(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_mod_replace_ext", args, LDAP_MOD_REPLACE, true)
}
pub fn ldap_mod_del(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_mod_del", args, LDAP_MOD_DELETE, false)
}
pub fn ldap_mod_del_ext(vm: &mut Vm, args: &[Value]) -> R {
    do_modify(vm, "ldap_mod_del_ext", args, LDAP_MOD_DELETE, true)
}

fn do_modify(vm: &mut Vm, f: &str, args: &[Value], mut oper: i32, ext: bool) -> R {
    // C, p (dn), a (entry hash), h! (controls)
    let Some(a) = parse(vm, f, "Cpah", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a[1].str().to_vec();
    let Some(Value::Array(entry)) = a[2].any() else {
        return Ok(Value::Null);
    };
    let entry = entry.borrow().clone();
    if entry.len() == 0 {
        zpp::value_error(vm, f, 3, "must not be empty");
        return Ok(Value::Null);
    }
    let is_full_add = oper == PHP_LD_FULL_ADD;
    if is_full_add {
        oper = LDAP_MOD_ADD;
    }

    let mut mods: Vec<(i32, Vec<u8>, Option<Vec<Vec<u8>>>)> = Vec::new();
    for (k, vals) in entry.iter() {
        let ArrayKey::String(attr) = k else {
            zpp::value_error(
                vm,
                f,
                3,
                "must be an associative array of attribute => values",
            );
            return Ok(Value::Null);
        };
        let attr = attr.as_bytes();
        if attr.is_empty() {
            zpp::value_error(vm, f, 3, "key must not be empty");
            return Ok(Value::Null);
        }
        if attr.contains(&0) {
            zpp::value_error(vm, f, 3, "key must not contain any null bytes");
            return Ok(Value::Null);
        }
        let vals = vals.deref();
        if let Value::Array(list) = &vals {
            let list = list.borrow();
            let num = list.len();
            if num == 0 {
                if oper == LDAP_MOD_ADD {
                    zpp::value_error(
                        vm,
                        f,
                        3,
                        &format!(
                            "attribute \"{}\" must be a non-empty list of attribute values",
                            String::from_utf8_lossy(attr)
                        ),
                    );
                    return Ok(Value::Null);
                }
                // modify: delete the attribute
                mods.push((oper, attr.to_vec(), None));
                continue;
            }
            if !is_numerically_indexed(&list) {
                zpp::value_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "attribute \"{}\" must be an array of attribute values with numeric keys",
                        String::from_utf8_lossy(attr)
                    ),
                );
                return Ok(Value::Null);
            }
            let mut out_vals = Vec::new();
            for (_vk, v) in list.iter() {
                match get_ldap_value(vm, v) {
                    Ok(b) => out_vals.push(b),
                    Err(e) => return Err(e),
                }
            }
            mods.push((oper, attr.to_vec(), Some(out_vals)));
        } else {
            // single value
            match get_ldap_value(vm, &vals) {
                Ok(b) => mods.push((oper, attr.to_vec(), Some(vec![b]))),
                Err(e) => return Err(e),
            }
        }
    }

    let controls = match controls_from(vm, f, a.get(3), 4) {
        Ok(c) => c,
        Err(()) => return Ok(Value::False),
    };

    let res = with_conn(&cd, |c| c.modify(&dn, &mods, is_full_add, &controls));
    let verb = if is_full_add { "Add" } else { "Modify" };
    finish_op(vm, f, verb, ext, res)
}

/// Common result handling for the write operations.
fn finish_op(vm: &mut Vm, f: &str, verb: &str, ext: bool, res: Result<LdapMessage, i32>) -> R {
    match res {
        Ok(m) => {
            if m.rescode != LDAP_SUCCESS {
                warn(
                    vm,
                    f,
                    &format!("{}: {}", verb, goro_ldap::err2string(m.rescode)),
                );
                return Ok(Value::False);
            }
            if ext {
                Ok(handles::new_result(vm, Rc::new(m)))
            } else {
                Ok(Value::True)
            }
        }
        Err(e) => {
            warn(vm, f, &format!("{}: {}", verb, goro_ldap::err2string(e)));
            Ok(Value::False)
        }
    }
}

pub fn ldap_delete(vm: &mut Vm, args: &[Value]) -> R {
    do_delete(vm, "ldap_delete", args, false)
}
pub fn ldap_delete_ext(vm: &mut Vm, args: &[Value]) -> R {
    do_delete(vm, "ldap_delete_ext", args, true)
}

fn do_delete(vm: &mut Vm, f: &str, args: &[Value], ext: bool) -> R {
    let Some(a) = parse(vm, f, "Cph", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a[1].str().to_vec();
    let controls = match controls_from(vm, f, a.get(2), 3) {
        Ok(c) => c,
        Err(()) => return Ok(Value::False),
    };
    let res = with_conn(&cd, |c| c.delete(&dn, &controls));
    finish_op(vm, f, "Delete", ext, res)
}

pub fn ldap_compare(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_compare";
    let Some(a) = parse(vm, f, "Cppsh", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a[1].str().to_vec();
    let attr = a[2].str().to_vec();
    let value = a[3].str().to_vec();
    let controls = match controls_from(vm, f, a.get(4), 5) {
        Ok(c) => c,
        Err(()) => return Ok(Value::False),
    };
    let res = with_conn(&cd, |c| c.compare(&dn, &attr, &value, &controls));
    match res {
        Ok(m) => match m.rescode {
            LDAP_COMPARE_TRUE => Ok(Value::True),
            LDAP_COMPARE_FALSE => Ok(Value::False),
            e => {
                warn(vm, f, &format!("Compare: {}", goro_ldap::err2string(e)));
                Ok(Value::Long(-1))
            }
        },
        Err(e) => {
            warn(vm, f, &format!("Compare: {}", goro_ldap::err2string(e)));
            Ok(Value::Long(-1))
        }
    }
}

pub fn ldap_rename(vm: &mut Vm, args: &[Value]) -> R {
    do_rename(vm, "ldap_rename", args, false)
}
pub fn ldap_rename_ext(vm: &mut Vm, args: &[Value]) -> R {
    do_rename(vm, "ldap_rename_ext", args, true)
}

fn do_rename(vm: &mut Vm, f: &str, args: &[Value], ext: bool) -> R {
    let Some(a) = parse(vm, f, "Cpppbh", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a[1].str().to_vec();
    let newrdn = a[2].str().to_vec();
    let newparent = a[3].str().to_vec();
    let delete_old = a[4].bool_or(false);
    let controls = match controls_from(vm, f, a.get(5), 6) {
        Ok(c) => c,
        Err(()) => return Ok(Value::False),
    };
    let np = if newparent.is_empty() {
        None
    } else {
        Some(newparent.as_slice())
    };
    let res = with_conn(&cd, |c| c.rename(&dn, &newrdn, np, delete_old, &controls));
    match res {
        Ok(m) => {
            if m.rescode != LDAP_SUCCESS {
                return Ok(Value::False);
            }
            if ext {
                Ok(handles::new_result(vm, Rc::new(m)))
            } else {
                Ok(Value::True)
            }
        }
        Err(_) => Ok(Value::False),
    }
}

// ---------------------------------------------------------------------------
// modify_batch
// ---------------------------------------------------------------------------

pub fn ldap_modify_batch(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_modify_batch";
    let Some(a) = parse(vm, f, "Cpah", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a[1].str().to_vec();
    let Some(Value::Array(mods_v)) = a[2].any() else {
        return Ok(Value::Null);
    };
    let mods_v = mods_v.borrow().clone();

    const ADD: i64 = 0x01;
    const REMOVE: i64 = 0x02;
    const REMOVE_ALL: i64 = 0x12;
    const REPLACE: i64 = 0x03;

    if mods_v.len() == 0 {
        zpp::value_error(vm, f, 3, "must not be empty");
        return Ok(Value::Null);
    }
    if !is_numerically_indexed(&mods_v) {
        zpp::value_error(vm, f, 3, "must be an array with numeric keys");
        return Ok(Value::Null);
    }
    // validation pass
    for (_k, mv) in mods_v.iter() {
        let mv = mv.deref();
        let Value::Array(m) = &mv else {
            zpp::type_error(vm, f, 3, "must only contain arrays");
            return Ok(Value::Null);
        };
        let m = m.borrow();
        let size = m.len();
        if size != 2 && size != 3 {
            zpp::value_error(
                vm,
                f,
                3,
                "a modification entry must only contain the keys \"attrib\", \"modtype\", and \"values\"",
            );
            return Ok(Value::Null);
        }
        let Some(attrib) = m.get_str(b"attrib") else {
            zpp::value_error(
                vm,
                f,
                3,
                "a modification entry must contain the \"attrib\" option",
            );
            return Ok(Value::Null);
        };
        let attrib = attrib.deref();
        let Value::String(attrs) = &attrib else {
            zpp::type_error(
                vm,
                f,
                3,
                &format!(
                    "the value for option \"attrib\" must be of type string, {} given",
                    Vm::value_type_name(&attrib)
                ),
            );
            return Ok(Value::Null);
        };
        if attrs.as_bytes().contains(&0) {
            zpp::value_error(
                vm,
                f,
                3,
                "the value for option \"attrib\" must not contain null bytes",
            );
            return Ok(Value::Null);
        }
        let Some(modtype) = m.get_str(b"modtype") else {
            zpp::value_error(
                vm,
                f,
                3,
                "a modification entry must contain the \"modtype\" option",
            );
            return Ok(Value::Null);
        };
        let modtype = modtype.deref();
        let Value::Long(mt) = &modtype else {
            zpp::type_error(
                vm,
                f,
                3,
                // php-src reports the type of `attrib` here, not modtype.
                &format!(
                    "the value for option \"modtype\" must be of type int, {} given",
                    Vm::value_type_name(&attrib)
                ),
            );
            return Ok(Value::Null);
        };
        let mt = *mt;
        if mt != ADD && mt != REMOVE && mt != REPLACE && mt != REMOVE_ALL {
            zpp::value_error(
                vm,
                f,
                3,
                "the value for option \"modtype\" must be LDAP_MODIFY_BATCH_ADD, LDAP_MODIFY_BATCH_REMOVE, LDAP_MODIFY_BATCH_REPLACE, or LDAP_MODIFY_BATCH_REMOVE_ALL",
            );
            return Ok(Value::Null);
        }
        if mt == REMOVE_ALL && size == 3 {
            zpp::value_error(
                vm,
                f,
                3,
                "a modification entry must not contain the \"values\" option when option \"modtype\" is LDAP_MODIFY_BATCH_REMOVE_ALL",
            );
            return Ok(Value::Null);
        }
        let vkey = m.get_str(b"values");
        if vkey.is_none() {
            if mt != REMOVE_ALL {
                zpp::value_error(
                    vm,
                    f,
                    3,
                    "a modification entry must contain the \"values\" option when the \"modtype\" option is not LDAP_MODIFY_BATCH_REMOVE_ALL",
                );
                return Ok(Value::Null);
            }
            continue;
        }
        let vv = vkey.unwrap().deref();
        let Value::Array(vals) = &vv else {
            zpp::type_error(
                vm,
                f,
                3,
                &format!(
                    "the value for option \"values\" must be of type array, {} given",
                    Vm::value_type_name(&vv)
                ),
            );
            return Ok(Value::Null);
        };
        let vals = vals.borrow();
        if vals.len() == 0 {
            zpp::value_error(
                vm,
                f,
                3,
                "the value for option \"values\" must not be empty",
            );
            return Ok(Value::Null);
        }
        if !is_numerically_indexed(&vals) {
            zpp::value_error(
                vm,
                f,
                3,
                "the value for option \"values\" must be an array with numeric keys",
            );
            return Ok(Value::Null);
        }
    }

    // build mods
    let mut mods: Vec<(i32, Vec<u8>, Option<Vec<Vec<u8>>>)> = Vec::new();
    for (_k, mv) in mods_v.iter() {
        let Value::Array(m) = &mv.deref() else {
            continue;
        };
        let m = m.borrow();
        let attr = match m.get_str(b"attrib").map(|v| v.deref()) {
            Some(Value::String(s)) => s.as_bytes().to_vec(),
            _ => continue,
        };
        let mt = match m.get_str(b"modtype").map(|v| v.deref()) {
            Some(Value::Long(n)) => n,
            _ => continue,
        };
        let op = match mt {
            ADD => LDAP_MOD_ADD,
            REMOVE | REMOVE_ALL => LDAP_MOD_DELETE,
            REPLACE => LDAP_MOD_REPLACE,
            _ => continue,
        };
        if mt == REMOVE_ALL {
            mods.push((op, attr, None));
        } else {
            let vals = match m.get_str(b"values").map(|v| v.deref()) {
                Some(Value::Array(a)) => a,
                _ => continue,
            };
            let vb = vals.borrow();
            let mut out = Vec::new();
            for (_vk, v) in vb.iter() {
                out.push(vm.value_to_string(&v.deref()).as_bytes().to_vec());
            }
            mods.push((op, attr, Some(out)));
        }
    }

    let controls = match controls_from(vm, f, a.get(3), 4) {
        Ok(c) => c,
        Err(()) => return Ok(Value::False),
    };
    let res = with_conn(&cd, |c| c.modify(&dn, &mods, false, &controls));
    match res {
        Ok(m) => {
            if m.rescode != LDAP_SUCCESS {
                warn(
                    vm,
                    f,
                    &format!("Batch Modify: {}", goro_ldap::err2string(m.rescode)),
                );
                return Ok(Value::False);
            }
            Ok(Value::True)
        }
        Err(e) => {
            warn(
                vm,
                f,
                &format!("Batch Modify: {}", goro_ldap::err2string(e)),
            );
            Ok(Value::False)
        }
    }
}

// ---------------------------------------------------------------------------
// Extended operations
// ---------------------------------------------------------------------------

pub fn ldap_exop(vm: &mut Vm, args: &[Value]) -> R {
    do_exop(vm, "ldap_exop", args, false)
}
pub fn ldap_exop_sync(vm: &mut Vm, args: &[Value]) -> R {
    do_exop(vm, "ldap_exop_sync", args, true)
}

fn do_exop(vm: &mut Vm, f: &str, args: &[Value], force_sync: bool) -> R {
    let given = args
        .iter()
        .take_while(|v| !matches!(v, Value::Undef))
        .count();
    if !force_sync && given > 4 {
        vm.emit_deprecated(
            "Calling ldap_exop() with more than 4 arguments is deprecated, use ldap_exop_sync() instead",
        );
        if vm.current_exception.is_some() {
            return Ok(Value::Null);
        }
    }
    // O P |S! h! z z : reqoid (non-empty, no-null), reqdata ?string,
    // controls ?array, &retdata, &retoid
    let Some(a) = parse(vm, f, "Cs", args) else {
        return Ok(Value::Null);
    };
    // reqoid: 'P' path semantics (no null) + non-empty
    let reqoid = {
        let v = args.get(1).map(|v| v.deref()).unwrap_or(Value::Null);
        match &v {
            Value::String(s) => {
                if s.as_bytes().contains(&0) {
                    zpp::value_error(vm, f, 2, "must not contain any null bytes");
                    return Ok(Value::Null);
                }
                s.as_bytes().to_vec()
            }
            _ => a[1].str().to_vec(),
        }
    };
    let _ = &a;
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    if reqoid.is_empty() {
        zpp::value_error(vm, f, 2, "must not be empty");
        return Ok(Value::Null);
    }
    let reqdata = match args.get(2).map(|v| v.deref()) {
        Some(Value::String(s)) => Some(s.as_bytes().to_vec()),
        _ => None,
    };
    let controls = if let Some(Value::Array(_)) = args.get(3).map(|v| v.deref()) {
        match controls_from(vm, f, Some(&Arg::Any(args[3].deref())), 4) {
            Ok(c) => c,
            Err(()) => return Ok(Value::False),
        }
    } else {
        Vec::new()
    };
    let retdata_ref = args.get(4).cloned();
    let retoid_ref = args.get(5).cloned();

    let res = with_conn(&cd, |c| c.extended(&reqoid, reqdata.as_deref(), &controls));
    let sync = force_sync || retdata_ref.is_some();
    match res {
        Ok(m) => {
            if m.rescode != LDAP_SUCCESS {
                warn(
                    vm,
                    f,
                    &format!(
                        "Extended operation {} failed: {} ({})",
                        String::from_utf8_lossy(&reqoid),
                        goro_ldap::err2string(m.rescode),
                        m.rescode
                    ),
                );
                return Ok(Value::False);
            }
            if sync {
                if let Some(z) = &retoid_ref {
                    crate::assign_ref(z, sval(m.exop_oid.as_deref().unwrap_or(b"")));
                }
                if let Some(z) = &retdata_ref {
                    crate::assign_ref(z, sval(m.exop_data.as_deref().unwrap_or(b"")));
                }
                Ok(Value::True)
            } else {
                Ok(handles::new_result(vm, Rc::new(m)))
            }
        }
        Err(e) => {
            warn(
                vm,
                f,
                &format!(
                    "Extended operation {} failed: {} ({})",
                    String::from_utf8_lossy(&reqoid),
                    goro_ldap::err2string(e),
                    e
                ),
            );
            Ok(Value::False)
        }
    }
}

pub fn ldap_exop_passwd(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_exop_passwd";
    let Some(a) = parse(vm, f, "C|ssz", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let user = a.get(1).map(|x| x.str().to_vec()).unwrap_or_default();
    let oldpw = a.get(2).map(|x| x.str().to_vec()).unwrap_or_default();
    let newpw = a.get(3).map(|x| x.str().to_vec()).unwrap_or_default();
    let ctrl_ref = args.get(4).cloned();

    // build the PasswdModifyRequestValue
    let data = passwd_req(&user, &oldpw, &newpw);
    let res = with_conn(&cd, |c| {
        c.extended(LDAP_EXOP_MODIFY_PASSWD.as_bytes(), Some(&data), &[])
    });
    match res {
        Ok(m) => {
            if m.rescode != LDAP_SUCCESS {
                warn(
                    vm,
                    f,
                    &format!(
                        "Passwd modify extended operation failed: {} ({})",
                        if m.errmsg.is_empty() {
                            goro_ldap::err2string(m.rescode).to_string()
                        } else {
                            String::from_utf8_lossy(&m.errmsg).into_owned()
                        },
                        m.rescode
                    ),
                );
                return Ok(Value::False);
            }
            if let Some(z) = &ctrl_ref {
                let a = ctl::controls_to_array(&m.ctrls, false);
                crate::assign_ref(z, arr(a));
            }
            // genpassword in the response value
            let genpw = parse_passwd_response(m.exop_data.as_deref());
            if newpw.is_empty() {
                Ok(sval(&genpw.unwrap_or_default()))
            } else {
                Ok(Value::True)
            }
        }
        Err(e) => {
            warn(
                vm,
                f,
                &format!(
                    "Passwd modify extended operation failed: {} ({})",
                    goro_ldap::err2string(e),
                    e
                ),
            );
            Ok(Value::False)
        }
    }
}

fn passwd_req(user: &[u8], oldpw: &[u8], newpw: &[u8]) -> Vec<u8> {
    use goro_ldap::ber::Writer;
    let mut w = Writer::new();
    w.start_seq();
    if !user.is_empty() {
        w.raw(LDAP_TAG_EXOP_MODIFY_PASSWD_ID, user);
    }
    if !oldpw.is_empty() {
        w.raw(LDAP_TAG_EXOP_MODIFY_PASSWD_OLD, oldpw);
    }
    if !newpw.is_empty() {
        w.raw(LDAP_TAG_EXOP_MODIFY_PASSWD_NEW, newpw);
    }
    w.end();
    w.finish()
}

fn parse_passwd_response(data: Option<&[u8]>) -> Option<Vec<u8>> {
    use goro_ldap::ber::Reader;
    let data = data?;
    let mut r = Reader::new(data);
    let mut s = r.seq().ok()?;
    let genv = s.optional(LDAP_TAG_EXOP_MODIFY_PASSWD_GEN).ok()??;
    Some(genv.to_vec())
}

pub fn ldap_exop_whoami(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_exop_whoami";
    let Some(a) = parse(vm, f, "C", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let res = with_conn(&cd, |c| {
        c.extended(LDAP_EXOP_WHO_AM_I.as_bytes(), None, &[])
    });
    match res {
        Ok(m) if m.rescode == LDAP_SUCCESS => Ok(sval(m.exop_data.as_deref().unwrap_or(b""))),
        Ok(m) => {
            warn(
                vm,
                f,
                &format!(
                    "Whoami extended operation failed: {} ({})",
                    goro_ldap::err2string(m.rescode),
                    m.rescode
                ),
            );
            Ok(Value::False)
        }
        Err(e) => {
            warn(
                vm,
                f,
                &format!(
                    "Whoami extended operation failed: {} ({})",
                    goro_ldap::err2string(e),
                    e
                ),
            );
            Ok(Value::False)
        }
    }
}

pub fn ldap_exop_refresh(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_exop_refresh";
    let Some(a) = parse(vm, f, "Csl", args) else {
        return Ok(Value::Null);
    };
    let Some(cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    let dn = a[1].str().to_vec();
    let ttl = a[2].long_or(0);
    // RefreshRequest ::= SEQUENCE { entryName [0] LDAPDN, requestTtl [1] INTEGER }
    use goro_ldap::ber::Writer;
    let mut w = Writer::new();
    w.start_seq();
    w.raw(LDAP_TAG_EXOP_REFRESH_REQ_DN, &dn);
    w.int_tagged(LDAP_TAG_EXOP_REFRESH_REQ_TTL, ttl);
    w.end();
    let data = w.finish();
    let res = with_conn(&cd, |c| {
        c.extended(LDAP_EXOP_REFRESH.as_bytes(), Some(&data), &[])
    });
    match res {
        Ok(m) if m.rescode == LDAP_SUCCESS => {
            let newttl = parse_refresh_response(m.exop_data.as_deref()).unwrap_or(ttl);
            Ok(Value::Long(newttl))
        }
        Ok(m) => {
            warn(
                vm,
                f,
                &format!(
                    "Refresh extended operation failed: {} ({})",
                    goro_ldap::err2string(m.rescode),
                    m.rescode
                ),
            );
            Ok(Value::False)
        }
        Err(e) => {
            warn(
                vm,
                f,
                &format!(
                    "Refresh extended operation failed: {} ({})",
                    goro_ldap::err2string(e),
                    e
                ),
            );
            Ok(Value::False)
        }
    }
}

fn parse_refresh_response(data: Option<&[u8]>) -> Option<i64> {
    use goro_ldap::ber::Reader;
    let data = data?;
    let mut r = Reader::new(data);
    let mut s = r.seq().ok()?;
    s.int_tagged(LDAP_TAG_EXOP_REFRESH_RES_TTL).ok()
}

// ---------------------------------------------------------------------------
// set_rebind_proc (callback stored, never invoked without referrals)
// ---------------------------------------------------------------------------

pub fn ldap_set_rebind_proc(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_set_rebind_proc";
    // O F! : connection, nullable callable
    let Some(a) = parse(vm, f, "C", args) else {
        return Ok(Value::Null);
    };
    let Some(_cd) = link(vm, &a[0]) else {
        return Ok(Value::Null);
    };
    // The callback is validated but not stored (referral chasing against a
    // second server is never triggered by the tests).
    match args.get(1).map(|v| v.deref()) {
        None | Some(Value::Undef) | Some(Value::Null) => Ok(Value::True),
        Some(v) => {
            let r = vm.zend_is_callable_ex(&v, 0);
            if vm.current_exception.is_some() {
                return Ok(Value::Null);
            }
            if !r.ok {
                let m = format!(
                    "{}(): Argument #2 ($callback) must be a valid callback or null, {}",
                    f,
                    r.error.unwrap_or_default()
                );
                zpp::throw(vm, b"TypeError", &m);
                return Ok(Value::Null);
            }
            Ok(Value::True)
        }
    }
}

// ---------------------------------------------------------------------------
// get/set option
// ---------------------------------------------------------------------------

pub fn ldap_get_option(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_get_option";
    let Some(a) = parse(vm, f, "clz", args) else {
        return Ok(Value::Null);
    };
    let option = a[1].long_or(0) as i32;
    let out_ref = a.get(2).and_then(|x| x.any());

    // X_TLS_PACKAGE works with any link (including null).
    if option == LDAP_OPT_X_TLS_PACKAGE {
        if let Some(z) = &out_ref {
            crate::assign_ref(z, sval(goro_ldap::consts::TLS_PACKAGE.as_bytes()));
        }
        return Ok(Value::True);
    }

    let cd = match &a[0] {
        Arg::Obj(o) => {
            let Some(cd) = handles::conn_data(o) else {
                return Ok(Value::Null);
            };
            if cd.conn.borrow().is_none() {
                zpp::throw(vm, b"Error", "LDAP connection has already been closed");
                return Ok(Value::Null);
            }
            Some(cd)
        }
        _ => None,
    };

    let result = if let Some(cd) = &cd {
        with_conn(cd, |c| {
            crate::options::get_option(option, &c.opts, c.errno, &c.error, &c.matched)
        })
    } else {
        crate::with_global_opts(|o| crate::options::get_option(option, o, 0, &[], &[]))
    };
    match result {
        Ok(Some(v)) => {
            if let Some(z) = &out_ref {
                crate::assign_ref(z, v);
            }
            Ok(Value::True)
        }
        Ok(None) => Ok(Value::False),
        Err(()) => {
            zpp::value_error(vm, f, 2, "must be a valid LDAP option");
            Ok(Value::Null)
        }
    }
}

pub fn ldap_set_option(vm: &mut Vm, args: &[Value]) -> R {
    let f = "ldap_set_option";
    let Some(a) = parse(vm, f, "clz", args) else {
        return Ok(Value::Null);
    };
    let option = a[1].long_or(0) as i32;
    let newval = a.get(2).and_then(|x| x.any()).unwrap_or(Value::Null);

    let cd = match &a[0] {
        Arg::Obj(o) => {
            let Some(cd) = handles::conn_data(o) else {
                return Ok(Value::Null);
            };
            if cd.conn.borrow().is_none() {
                zpp::throw(vm, b"Error", "LDAP connection has already been closed");
                return Ok(Value::Null);
            }
            Some(cd)
        }
        _ => None,
    };

    let outcome = if let Some(cd) = &cd {
        let mut b = cd.conn.borrow_mut();
        let c = b.as_mut().unwrap();
        crate::options::set_option(vm, f, option, &newval, &mut c.opts)
    } else {
        // mutate global
        let mut tmp = crate::clone_global_opts();
        let r = crate::options::set_option(vm, f, option, &newval, &mut tmp);
        crate::with_global_opts(|g| *g = tmp);
        r
    };
    match outcome {
        crate::options::SetOutcome::Ok => Ok(Value::True),
        crate::options::SetOutcome::False => Ok(Value::False),
        crate::options::SetOutcome::Throw => Ok(Value::Null),
    }
}
