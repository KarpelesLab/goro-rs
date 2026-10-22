//! sendrecvmsg.c and conversions.c: socket_sendmsg(), socket_recvmsg(),
//! socket_cmsg_space() and the IPV6_PKTINFO option, with the PHP array <->
//! native structure conversions and their error paths.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::funcs::{assign_ref, vstr};
use crate::mcast::OptRes;
use crate::opts::OptRes2;
use crate::zpp::{self, Arg};
use crate::*;

const MAX_USER_BUFF_SIZE: i64 = 100 * 1024 * 1024;
const DEFAULT_BUFF_SIZE: usize = 8192;

const SCM_RIGHTS: i32 = 1;
const SCM_CREDENTIALS: i32 = 2;
const IPV6_PKTINFO: i32 = 50;
const IPV6_HOPLIMIT: i32 = 52;
const IPV6_TCLASS: i32 = 67;

const CMSGHDR_SIZE: usize = 16;

fn cmsg_align(n: usize) -> usize {
    (n + 7) & !7
}

fn cmsg_space(n: usize) -> usize {
    cmsg_align(n) + CMSGHDR_SIZE
}

fn cmsg_len(n: usize) -> usize {
    CMSGHDR_SIZE + n
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Anc {
    Pktinfo,
    Int,
    Ucred,
    Rights,
}

/// The ancillary data registry: (size, var_el_size, kind)
fn anc_entry(level: i32, ty: i32) -> Option<(usize, usize, Anc)> {
    match (level, ty) {
        (libc::IPPROTO_IPV6, IPV6_PKTINFO) => Some((20, 0, Anc::Pktinfo)),
        (libc::IPPROTO_IPV6, IPV6_HOPLIMIT) => Some((4, 0, Anc::Int)),
        (libc::IPPROTO_IPV6, IPV6_TCLASS) => Some((4, 0, Anc::Int)),
        (libc::SOL_SOCKET, SCM_CREDENTIALS) => Some((12, 0, Anc::Ucred)),
        (libc::SOL_SOCKET, SCM_RIGHTS) => Some((0, 4, Anc::Rights)),
        _ => None,
    }
}

fn skey(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

fn new_arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

/// An error of a conversion: Some(None) when an exception was thrown.
type ConvErr = Option<Option<String>>;

/// do_from_to_zval_err()
fn conv_error(err: &mut ConvErr, keys: &[String], what: &str, msg: String) {
    if err.is_some() {
        return;
    }
    let path = keys.join(" > ");
    let path = if path.is_empty() {
        "unavailable".to_string()
    } else {
        path
    };
    *err = Some(Some(format!(
        "error converting {} data (path: {}): {}",
        what, path, msg
    )));
}

/// err_msg_dispose()
fn dispose(vm: &mut Vm, f: &str, err: ConvErr) {
    if let Some(Some(m)) = err {
        vm.emit_warning(&format!("{}(): {}", f, m));
    }
}

// ---------------------------------------------------------------------------
// PHP -> native
// ---------------------------------------------------------------------------

struct Ser<'a> {
    vm: &'a mut Vm,
    f: &'a str,
    sock: &'a Sock,
    keys: Vec<String>,
    err: ConvErr,
    fill_sockaddr: bool,
}

/// The native msghdr parts.
#[derive(Default)]
struct Msg {
    name: Option<Vec<u8>>,
    iov: Vec<Vec<u8>>,
    control: Option<Vec<u8>>,
}

impl Ser<'_> {
    fn err(&mut self, msg: String) {
        conv_error(&mut self.err, &self.keys, "user", msg);
    }
    fn failed(&self) -> bool {
        self.err.is_some()
    }
    fn push(&mut self, k: &str) {
        self.keys.push(k.to_string());
    }
    fn pop(&mut self) {
        self.keys.pop();
    }

    /// zval_get_tmp_string()
    fn tmp_string(&mut self, v: &Value) -> Option<Vec<u8>> {
        match self.vm.value_to_string_checked(&v.deref()) {
            Ok(s) => Some(s.as_bytes().to_vec()),
            Err(_) => {
                if self.err.is_none() {
                    self.err = Some(None);
                }
                None
            }
        }
    }

    /// from_zval_integer_common()
    fn integer_common(&mut self, v: &Value) -> Option<i64> {
        let v = v.deref();
        match &v {
            Value::Long(n) => Some(*n),
            Value::Double(d) => Some(dval_to_lval(*d)),
            Value::Object(_) | Value::String(_) => {
                let s = match self.vm.value_to_string_checked(&v) {
                    Ok(s) => s,
                    Err(_) => {
                        if self.err.is_none() {
                            self.err = Some(None);
                        }
                        return None;
                    }
                };
                match goro_core::value::parse_numeric_str(s.as_bytes(), false) {
                    Some((goro_core::value::NumStr::Long(n), false)) => Some(n),
                    Some((goro_core::value::NumStr::Double(d), false)) => Some(dval_to_lval(d)),
                    _ => {
                        self.err(format!(
                            "expected an integer, but got a non numeric string (possibly from a converted object): '{}'",
                            String::from_utf8_lossy(c_bytes(s.as_bytes()))
                        ));
                        None
                    }
                }
            }
            _ => {
                self.err(
                    "expected an integer, either of a PHP integer type or of a convertible type"
                        .to_string(),
                );
                None
            }
        }
    }

    fn write_int(&mut self, v: &Value) -> Option<i32> {
        let l = self.integer_common(v)?;
        if l > i32::MAX as i64 || l < i32::MIN as i64 {
            self.err("given PHP integer is out of bounds for a native int".to_string());
            return None;
        }
        Some(l as i32)
    }

    fn write_uint32(&mut self, v: &Value) -> Option<u32> {
        let l = self.integer_common(v)?;
        if !(0..=0xFFFF_FFFF).contains(&l) {
            self.err(
                "given PHP integer is out of bounds for an unsigned 32-bit integer".to_string(),
            );
            return None;
        }
        Some(l as u32)
    }

    fn write_net_uint16(&mut self, v: &Value) -> Option<u16> {
        let l = self.integer_common(v)?;
        if !(0..=0xFFFF).contains(&l) {
            self.err(
                "given PHP integer is out of bounds for an unsigned 16-bit integer".to_string(),
            );
            return None;
        }
        Some((l as u16).to_be())
    }

    fn write_sa_family(&mut self, v: &Value) -> Option<u16> {
        let l = self.integer_common(v)?;
        if !(0..=0xFFFF).contains(&l) {
            self.err("given PHP integer is out of bounds for a sa_family_t value".to_string());
            return None;
        }
        Some(l as u16)
    }

    fn write_pid(&mut self, v: &Value) -> Option<i32> {
        let l = self.integer_common(v)?;
        if l < 0 || l as i32 as i64 != l {
            self.err("given PHP integer is out of bounds for a pid_t value".to_string());
            return None;
        }
        Some(l as i32)
    }

    fn write_uid(&mut self, v: &Value) -> Option<u32> {
        let l = self.integer_common(v)?;
        if l < 0 || l as u32 as i64 != l {
            self.err("given PHP integer is out of bounds for a uid_t value".to_string());
            return None;
        }
        Some(l as u32)
    }

    /// from_zval_write_aggregation(): the table of an array container.
    fn container(&mut self, v: &Value) -> Option<PhpArray> {
        match v.deref() {
            Value::Array(a) => Some(a.borrow().clone()),
            _ => {
                self.err("expected an array here".to_string());
                None
            }
        }
    }

    /// Run `field` on the value of `key` when present.
    fn field<T>(
        &mut self,
        ht: &PhpArray,
        key: &str,
        required: bool,
        mut conv: impl FnMut(&mut Self, &Value) -> Option<T>,
    ) -> Option<Option<T>> {
        if self.failed() {
            return None;
        }
        match ht.get_str(key.as_bytes()).cloned() {
            Some(v) => {
                self.push(key);
                let r = conv(self, &v);
                self.pop();
                if self.failed() { None } else { Some(r) }
            }
            None => {
                if required {
                    self.err(format!("The key '{}' is required", key));
                    return None;
                }
                Some(None)
            }
        }
    }

    fn sin_addr(&mut self, v: &Value) -> Option<[u8; 4]> {
        let s = self.tmp_string(v)?;
        match set_inet_addr(self.vm, self.f, &s, self.sock) {
            Some(a) => Some(a),
            None => {
                self.err(format!(
                    "could not resolve address '{}' to get an AF_INET address",
                    String::from_utf8_lossy(c_bytes(&s))
                ));
                None
            }
        }
    }

    fn sin6_addr(&mut self, v: &Value) -> Option<[u8; 16]> {
        let s = self.tmp_string(v)?;
        match set_inet6_addr(self.vm, self.f, &s, self.sock) {
            Some((a, _)) => Some(a),
            None => {
                self.err(format!(
                    "could not resolve address '{}' to get an AF_INET6 address",
                    String::from_utf8_lossy(c_bytes(&s))
                ));
                None
            }
        }
    }

    fn sockaddr_in(&mut self, v: &Value) -> Option<libc::sockaddr_in> {
        let ht = self.container(v)?;
        let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        if let Some(fam) = self.field(&ht, "family", false, |s, v| s.write_sa_family(v))? {
            sin.sin_family = fam;
        }
        if let Some(a) = self.field(&ht, "addr", false, |s, v| s.sin_addr(v))? {
            sin.sin_addr.s_addr = u32::from_ne_bytes(a);
        }
        if let Some(p) = self.field(&ht, "port", false, |s, v| s.write_net_uint16(v))? {
            sin.sin_port = p;
        }
        Some(sin)
    }

    fn sockaddr_in6(&mut self, v: &Value) -> Option<libc::sockaddr_in6> {
        let ht = self.container(v)?;
        let mut sin6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
        if let Some(fam) = self.field(&ht, "family", false, |s, v| s.write_sa_family(v))? {
            sin6.sin6_family = fam;
        }
        if let Some(a) = self.field(&ht, "addr", false, |s, v| s.sin6_addr(v))? {
            sin6.sin6_addr.s6_addr = a;
        }
        if let Some(p) = self.field(&ht, "port", false, |s, v| s.write_net_uint16(v))? {
            sin6.sin6_port = p;
        }
        if let Some(x) = self.field(&ht, "flowinfo", false, |s, v| s.write_uint32(v))? {
            sin6.sin6_flowinfo = x;
        }
        if let Some(x) = self.field(&ht, "scope_id", false, |s, v| s.write_uint32(v))? {
            sin6.sin6_scope_id = x;
        }
        Some(sin6)
    }

    fn sun_path(&mut self, v: &Value) -> Option<Vec<u8>> {
        let s = self.tmp_string(v)?;
        if s.is_empty() {
            self.err("the path is must not be empty".to_string());
            return None;
        }
        if s.len() >= 108 {
            self.err("the path is too long, the maximum permitted length is 107".to_string());
            return None;
        }
        Some(s)
    }

    fn sockaddr_un(&mut self, v: &Value) -> Option<libc::sockaddr_un> {
        let ht = self.container(v)?;
        let mut sun: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        if let Some(fam) = self.field(&ht, "family", false, |s, v| s.write_sa_family(v))? {
            sun.sun_family = fam;
        }
        if let Some(p) = self.field(&ht, "path", false, |s, v| s.sun_path(v))? {
            for (i, c) in p.iter().enumerate() {
                sun.sun_path[i] = *c as libc::c_char;
            }
            sun.sun_path[p.len()] = 0;
        }
        Some(sun)
    }

    /// from_zval_write_sockaddr_aux()
    fn sockaddr_aux(&mut self, v: &Value) -> Option<Vec<u8>> {
        let ht = match v.deref() {
            Value::Array(a) => a.borrow().clone(),
            _ => {
                self.err("expected an array here".to_string());
                return None;
            }
        };
        let family = match ht.get_str(b"family").cloned() {
            Some(fv) if !matches!(fv.deref(), Value::Null) => {
                self.push("family");
                let r = self.write_int(&fv);
                self.pop();
                r?
            }
            _ => self.sock.family.get(),
        };
        let st = self.sock.family.get();
        match family {
            libc::AF_INET => {
                if st != libc::AF_INET && st != libc::AF_INET6 {
                    self.err(format!(
                        "the specified family (number {}) is not supported on this socket",
                        family
                    ));
                    return None;
                }
                if !self.fill_sockaddr {
                    return Some(vec![0u8; std::mem::size_of::<libc::sockaddr_in>()]);
                }
                let mut sin = self.sockaddr_in(v)?;
                sin.sin_family = libc::AF_INET as libc::sa_family_t;
                Some(as_bytes(&sin).to_vec())
            }
            libc::AF_INET6 => {
                if st != libc::AF_INET6 {
                    self.err(
                        "the specified family (AF_INET6) is not supported on this socket"
                            .to_string(),
                    );
                    return None;
                }
                if !self.fill_sockaddr {
                    return Some(vec![0u8; std::mem::size_of::<libc::sockaddr_in6>()]);
                }
                let mut sin6 = self.sockaddr_in6(v)?;
                sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                Some(as_bytes(&sin6).to_vec())
            }
            libc::AF_UNIX => {
                if st != libc::AF_UNIX {
                    self.err(
                        "the specified family (AF_UNIX) is not supported on this socket"
                            .to_string(),
                    );
                    return None;
                }
                if !self.fill_sockaddr {
                    return Some(vec![0u8; std::mem::size_of::<libc::sockaddr_un>()]);
                }
                let mut sun = self.sockaddr_un(v)?;
                sun.sun_family = libc::AF_UNIX as libc::sa_family_t;
                let path = &sun.sun_path;
                let len = if path[0] == 0 {
                    1 + path[1..].iter().take_while(|&&c| c != 0).count()
                } else {
                    path.iter().take_while(|&&c| c != 0).count()
                };
                Some(as_bytes(&sun)[..2 + len].to_vec())
            }
            _ => {
                self.err(
                    "the only families currently supported are AF_INET, AF_INET6 and AF_UNIX"
                        .to_string(),
                );
                None
            }
        }
    }

    /// from_zval_write_ifindex()
    fn ifindex(&mut self, v: &Value) -> Option<u32> {
        match v.deref() {
            Value::Long(n) => {
                if n < 0 || n > u32::MAX as i64 {
                    self.err(format!(
                        "the interface index cannot be negative or larger than 4294967295; given {}",
                        n
                    ));
                    None
                } else {
                    Some(n as u32)
                }
            }
            other => {
                let s = self.tmp_string(&other)?;
                let c = cstring(&s);
                let r = unsafe { libc::if_nametoindex(c.as_ptr()) };
                if r == 0 {
                    self.err(format!(
                        "no interface with name \"{}\" could be found",
                        String::from_utf8_lossy(c_bytes(&s))
                    ));
                    None
                } else {
                    Some(r)
                }
            }
        }
    }

    /// from_zval_write_in6_pktinfo()
    fn in6_pktinfo(&mut self, v: &Value) -> Option<[u8; 20]> {
        let ht = self.container(v)?;
        let mut out = [0u8; 20];
        if let Some(a) = self.field(&ht, "addr", true, |s, v| s.sin6_addr(v))? {
            out[..16].copy_from_slice(&a);
        }
        if let Some(i) = self.field(&ht, "ifindex", true, |s, v| s.ifindex(v))? {
            out[16..].copy_from_slice(&i.to_ne_bytes());
        }
        Some(out)
    }

    fn ucred(&mut self, v: &Value) -> Option<[u8; 12]> {
        let ht = self.container(v)?;
        let mut out = [0u8; 12];
        if let Some(p) = self.field(&ht, "pid", true, |s, v| s.write_pid(v))? {
            out[..4].copy_from_slice(&p.to_ne_bytes());
        }
        if let Some(u) = self.field(&ht, "uid", true, |s, v| s.write_uid(v))? {
            out[4..8].copy_from_slice(&u.to_ne_bytes());
        }
        if let Some(g) = self.field(&ht, "gid", true, |s, v| s.write_uid(v))? {
            out[8..].copy_from_slice(&g.to_ne_bytes());
        }
        Some(out)
    }

    /// from_zval_write_fd_array()
    fn fd_array(&mut self, v: &Value, n: usize) -> Option<Vec<u8>> {
        let ht = self.container(v)?;
        let mut fds = vec![0i32; n];
        for (idx, (_, el)) in ht.iter().enumerate() {
            let i = idx + 1;
            self.push(&format!("element #{}", i));
            let el = el.deref();
            match &el {
                Value::Object(o) if o.borrow().class_name.eq_ignore_ascii_case(b"Socket") => {
                    let s = sock_of(o);
                    if s.is_invalid() {
                        self.err("socket is already closed".to_string());
                    } else if i < n {
                        // php-src stores it at iarr[i] (sic)
                        fds[i] = s.fd.get();
                    }
                }
                Value::Resource(id) => {
                    if !goro_ext_standard::sockets_streams::ext_is_stream(id.id()) {
                        self.err("resource is not a stream".to_string());
                    } else {
                        match goro_ext_standard::sockets_streams::ext_stream_fd(
                            self.vm,
                            id.id(),
                            self.f,
                        ) {
                            Some(fd) => fds[i - 1] = fd,
                            None => self.err("cast stream to file descriptor failed".to_string()),
                        }
                    }
                }
                _ => self.err("expected a Socket object or a stream resource".to_string()),
            }
            self.pop();
            if self.failed() {
                return None;
            }
        }
        Some(fds.iter().flat_map(|f| f.to_ne_bytes()).collect())
    }

    /// from_zval_write_control()
    fn control(&mut self, el: &Value, buf: &mut Vec<u8>) {
        let Some(ht) = self.container(el) else { return };
        let level = match self.field(&ht, "level", false, |s, v| s.write_int(v)) {
            Some(l) => l.unwrap_or(0),
            None => return,
        };
        let ty = match self.field(&ht, "type", false, |s, v| s.write_int(v)) {
            Some(t) => t.unwrap_or(0),
            None => return,
        };
        let Some((size, _var, kind)) = anc_entry(level, ty) else {
            self.err(format!(
                "cmsghdr with level {} and type {} not supported",
                level, ty
            ));
            return;
        };
        let data_len = if kind == Anc::Rights {
            let Some(d) = ht.get_str(b"data").cloned() else {
                self.err("cmsghdr should have a 'data' element here".to_string());
                return;
            };
            match d.deref() {
                Value::Array(a) => {
                    let n = a.borrow().len();
                    if n == 0 {
                        self.err("expected at least one element in this array".to_string());
                        return;
                    }
                    n * 4
                }
                _ => {
                    self.err("expected an array here".to_string());
                    return;
                }
            }
        } else {
            size
        };
        let mut data = vec![0u8; data_len];
        if let Some(d) = ht.get_str(b"data").cloned() {
            self.push("data");
            let r: Option<Vec<u8>> = match kind {
                Anc::Pktinfo => self.in6_pktinfo(&d).map(|a| a.to_vec()),
                Anc::Int => self.write_int(&d).map(|i| i.to_ne_bytes().to_vec()),
                Anc::Ucred => self.ucred(&d).map(|a| a.to_vec()),
                Anc::Rights => self.fd_array(&d, data_len / 4),
            };
            self.pop();
            if let Some(r) = r {
                data[..r.len().min(data_len)].copy_from_slice(&r[..r.len().min(data_len)]);
            }
        }
        let space = cmsg_space(data_len);
        let off = buf.len();
        buf.resize(off + space, 0);
        buf[off..off + 8].copy_from_slice(&(cmsg_len(data_len) as u64).to_ne_bytes());
        buf[off + 8..off + 12].copy_from_slice(&level.to_ne_bytes());
        buf[off + 12..off + 16].copy_from_slice(&ty.to_ne_bytes());
        buf[off + CMSGHDR_SIZE..off + CMSGHDR_SIZE + data_len].copy_from_slice(&data);
    }

    fn control_array(&mut self, v: &Value) -> Option<Vec<u8>> {
        let ht = self.container(v)?;
        if ht.is_empty() {
            return None;
        }
        let mut buf = Vec::new();
        for (i, (_, el)) in ht.iter().enumerate() {
            if self.failed() {
                break;
            }
            self.push(&format!("element #{}", i));
            self.control(el, &mut buf);
            self.pop();
        }
        Some(buf)
    }

    fn iov_array(&mut self, v: &Value) -> Option<Vec<Vec<u8>>> {
        let ht = self.container(v)?;
        let mut out = Vec::new();
        for (i, (_, el)) in ht.iter().enumerate() {
            self.push(&format!("element #{}", i + 1));
            let s = self.tmp_string(el);
            self.pop();
            out.push(s?);
            if self.failed() {
                return None;
            }
        }
        Some(out)
    }

    /// from_zval_write_msghdr_send()
    fn msghdr_send(&mut self, v: &Value) -> Option<Msg> {
        let ht = self.container(v)?;
        let mut m = Msg {
            name: self.field(&ht, "name", false, |s, v| s.sockaddr_aux(v))?,
            ..Default::default()
        };
        if let Some(iov) = self.field(&ht, "iov", false, |s, v| s.iov_array(v))? {
            m.iov = iov;
        }
        m.control = self.field(&ht, "control", false, |s, v| s.control_array(v))?;
        Some(m)
    }

    /// from_zval_write_msghdr_recv()
    fn msghdr_recv(&mut self, v: &Value) -> Option<Msg> {
        self.fill_sockaddr = false;
        let ht = self.container(v);
        let ht = ht?;
        let mut m = Msg {
            name: self.field(&ht, "name", false, |s, v| s.sockaddr_aux(v))?,
            ..Default::default()
        };
        let bs = self.field(&ht, "buffer_size", false, |s, v| {
            let l = s.integer_common(v)?;
            if !(0..=MAX_USER_BUFF_SIZE).contains(&l) {
                s.err(format!(
                    "the buffer size must be between 1 and {}; given {}",
                    MAX_USER_BUFF_SIZE, l
                ));
                return None;
            }
            Some(l as usize)
        })?;
        let cl = self.field(&ht, "controllen", true, |s, v| {
            let len = s.write_uint32(v)?;
            if len == 0 {
                s.err("controllen cannot be 0".to_string());
                return None;
            }
            Some(len as usize)
        })?;
        self.fill_sockaddr = true;
        m.iov = vec![vec![0u8; bs.unwrap_or(DEFAULT_BUFF_SIZE)]];
        m.control = cl.map(|l| vec![0u8; l]);
        Some(m)
    }
}

/// zend_dval_to_lval()
fn dval_to_lval(d: f64) -> i64 {
    if !d.is_finite() || d >= 9.223_372_036_854_776e18 || d < -9.223_372_036_854_776e18 {
        0
    } else {
        d as i64
    }
}

/// LONG_CHECK_VALID_INT()
fn check_int(vm: &mut Vm, f: &str, l: i64, n: usize) -> bool {
    if l < i32::MIN as i64 || l > i32::MAX as i64 {
        zpp::value_error(vm, f, n, "must be between -2147483648 and 2147483647");
        return false;
    }
    true
}

/// An 8-byte aligned buffer with a copy of `data` (for msg_control).
fn aligned(data: &[u8]) -> Vec<u64> {
    let mut v = vec![0u64; data.len().div_ceil(8).max(1)];
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), v.as_mut_ptr() as *mut u8, data.len());
    }
    v
}

pub fn socket_sendmsg(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Oa|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let flags = a[2].long_or(0);
    if !check_int(vm, f, flags, 3) {
        return Value::Null;
    }
    let sock = sock_of(&a[0].obj().unwrap());
    if !ensure_valid(vm, f, &sock) {
        return Value::Null;
    }
    let Arg::Arr(arr) = &a[1] else {
        return Value::Null;
    };
    let msgv = Value::Array(arr.clone());
    let mut ser = Ser {
        vm: &mut *vm,
        f,
        sock: &sock,
        keys: vec!["msghdr".to_string()],
        err: None,
        fill_sockaddr: true,
    };
    let m = ser.msghdr_send(&msgv);
    let err = ser.err.take();
    if err.is_some() || m.is_none() {
        dispose(vm, f, err);
        return Value::False;
    }
    let mut m = m.unwrap();
    let mut iovs: Vec<libc::iovec> = m
        .iov
        .iter_mut()
        .map(|b| libc::iovec {
            iov_base: b.as_mut_ptr() as *mut libc::c_void,
            iov_len: b.len(),
        })
        .collect();
    let mut ctl = m.control.as_ref().map(|c| (aligned(c), c.len()));
    let mut hdr: libc::msghdr = unsafe { std::mem::zeroed() };
    if let Some(n) = m.name.as_mut() {
        hdr.msg_name = n.as_mut_ptr() as *mut libc::c_void;
        hdr.msg_namelen = n.len() as libc::socklen_t;
    }
    if !iovs.is_empty() {
        hdr.msg_iov = iovs.as_mut_ptr();
        hdr.msg_iovlen = iovs.len();
    }
    if let Some((c, len)) = ctl.as_mut() {
        hdr.msg_control = c.as_mut_ptr() as *mut libc::c_void;
        hdr.msg_controllen = *len;
    }
    vm.flush_cli_output();
    let res = unsafe { libc::sendmsg(sock.fd.get(), &hdr, flags as i32) };
    if res != -1 {
        Value::Long(res as i64)
    } else {
        sock_error(vm, f, &sock, "Error in sendmsg", errno());
        Value::False
    }
}

// ---------------------------------------------------------------------------
// native -> PHP
// ---------------------------------------------------------------------------

struct Res<'a> {
    vm: &'a mut Vm,
    f: &'a str,
    keys: Vec<String>,
    err: ConvErr,
}

impl Res<'_> {
    fn err(&mut self, msg: String) {
        conv_error(&mut self.err, &self.keys, "native", msg);
    }

    fn sockaddr(&mut self, sa: &[u8]) -> Value {
        if sa.len() < 2 {
            return Value::Null;
        }
        let family = u16::from_ne_bytes([sa[0], sa[1]]) as i32;
        if family == 0 {
            return Value::Null;
        }
        let mut out = PhpArray::new();
        match family {
            libc::AF_INET if sa.len() >= std::mem::size_of::<libc::sockaddr_in>() => {
                let sin = unsafe { &*(sa.as_ptr() as *const libc::sockaddr_in) };
                out.set(skey("family"), Value::Long(sin.sin_family as i64));
                out.set(
                    skey("addr"),
                    vstr(inet_ntop4(&sin.sin_addr.s_addr.to_ne_bytes())),
                );
                out.set(skey("port"), Value::Long(u16::from_be(sin.sin_port) as i64));
            }
            libc::AF_INET6 if sa.len() >= std::mem::size_of::<libc::sockaddr_in6>() => {
                let sin6 = unsafe { &*(sa.as_ptr() as *const libc::sockaddr_in6) };
                out.set(skey("family"), Value::Long(sin6.sin6_family as i64));
                out.set(skey("addr"), vstr(inet_ntop6(&sin6.sin6_addr.s6_addr)));
                out.set(
                    skey("port"),
                    Value::Long(u16::from_be(sin6.sin6_port) as i64),
                );
                out.set(skey("flowinfo"), Value::Long(sin6.sin6_flowinfo as i64));
                out.set(skey("scope_id"), Value::Long(sin6.sin6_scope_id as i64));
            }
            libc::AF_UNIX => {
                out.set(skey("family"), Value::Long(family as i64));
                let path = &sa[2..sa.len().min(110)];
                self.keys.push("path".to_string());
                match path.iter().position(|&c| c == 0) {
                    Some(p) => out.set(skey("path"), vstr(&path[..p])),
                    None if path.len() < 108 => out.set(skey("path"), vstr(path)),
                    None => {
                        out.set(skey("path"), Value::Null);
                        self.err("could not find a NUL in the path".to_string());
                    }
                }
                self.keys.pop();
            }
            _ => self.err(format!(
                "cannot read struct sockaddr with family {}; not supported",
                family
            )),
        }
        new_arr(out)
    }

    fn in6_pktinfo(&mut self, d: &[u8]) -> Value {
        let mut a = [0u8; 16];
        a.copy_from_slice(&d[..16]);
        let idx = u32::from_ne_bytes([d[16], d[17], d[18], d[19]]);
        let mut out = PhpArray::new();
        out.set(skey("addr"), vstr(inet_ntop6(&a)));
        out.set(skey("ifindex"), Value::Long(idx as i64));
        new_arr(out)
    }

    fn fd_array(&mut self, data: &[u8], cmsg_len_v: usize) -> Value {
        let n = (cmsg_len_v - CMSGHDR_SIZE) / 4;
        let mut out = PhpArray::new();
        for i in 0..n {
            let fd = i32::from_ne_bytes([
                data[i * 4],
                data[i * 4 + 1],
                data[i * 4 + 2],
                data[i * 4 + 3],
            ]);
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            if unsafe { libc::fstat(fd, &mut st) } == -1 {
                self.err(format!(
                    "error creating resource for received file descriptor {}: fstat() call failed with errno {}",
                    fd,
                    errno()
                ));
                return new_arr(out);
            }
            if st.st_mode & libc::S_IFMT == libc::S_IFSOCK {
                let (obj, s) = new_socket(self.vm);
                if !crate::funcs::import_fd(self.vm, self.f, fd, &s) {
                    self.err(format!(
                        "error getting protocol descriptor {}: getsockopt() call failed with errno {}",
                        fd,
                        errno()
                    ));
                    return new_arr(out);
                }
                out.push(obj);
            } else {
                let sid = goro_ext_standard::sockets_streams::ext_stream_from_fd(fd, "rw");
                out.push(Value::resource(&sid));
            }
        }
        new_arr(out)
    }

    /// to_zval_read_control()
    fn control(&mut self, buf: &[u8]) -> Value {
        let len = u64::from_ne_bytes(buf[..8].try_into().unwrap()) as usize;
        let level = i32::from_ne_bytes(buf[8..12].try_into().unwrap());
        let ty = i32::from_ne_bytes(buf[12..16].try_into().unwrap());
        let mut out = PhpArray::new();
        out.set(skey("level"), Value::Long(level as i64));
        out.set(skey("type"), Value::Long(ty as i64));
        self.keys.push("data".to_string());
        let data = match anc_entry(level, ty) {
            None => {
                self.err(format!(
                    "cmsghdr with level {} and type {} not supported",
                    level, ty
                ));
                Value::Null
            }
            Some((size, _, _)) if cmsg_len(size) > len => {
                self.err(format!(
                    "the cmsghdr structure is unexpectedly small; expected a length of at least {}, but got {}",
                    cmsg_len(size),
                    len
                ));
                Value::Null
            }
            Some((_, _, kind)) => {
                let d = &buf[CMSGHDR_SIZE..len.min(buf.len()).max(CMSGHDR_SIZE)];
                match kind {
                    Anc::Pktinfo => self.in6_pktinfo(d),
                    Anc::Int => Value::Long(i32::from_ne_bytes(d[..4].try_into().unwrap()) as i64),
                    Anc::Ucred => {
                        let mut o = PhpArray::new();
                        o.set(
                            skey("pid"),
                            Value::Long(i32::from_ne_bytes(d[..4].try_into().unwrap()) as i64),
                        );
                        o.set(
                            skey("uid"),
                            Value::Long(u32::from_ne_bytes(d[4..8].try_into().unwrap()) as i64),
                        );
                        o.set(
                            skey("gid"),
                            Value::Long(u32::from_ne_bytes(d[8..12].try_into().unwrap()) as i64),
                        );
                        new_arr(o)
                    }
                    Anc::Rights => self.fd_array(d, len),
                }
            }
        };
        self.keys.pop();
        out.set(skey("data"), data);
        new_arr(out)
    }

    fn control_array(&mut self, hdr: &libc::msghdr) -> Value {
        let mut out = PhpArray::new();
        let mut i = 1;
        let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(hdr) };
        while !cmsg.is_null() && self.err.is_none() {
            let len = unsafe { (*cmsg).cmsg_len };
            let avail = hdr.msg_controllen - (cmsg as usize - hdr.msg_control as usize);
            let n = len.min(avail).max(CMSGHDR_SIZE);
            let bytes = unsafe { std::slice::from_raw_parts(cmsg as *const u8, n) }.to_vec();
            self.keys.push(format!("element #{}", i));
            i += 1;
            let v = self.control(&bytes);
            self.keys.pop();
            out.push(v);
            cmsg = unsafe { libc::CMSG_NXTHDR(hdr, cmsg) };
        }
        new_arr(out)
    }
}

pub fn socket_recvmsg(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Oa|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let flags = a[2].long_or(0);
    if !check_int(vm, f, flags, 3) {
        return Value::Null;
    }
    let sock = sock_of(&a[0].obj().unwrap());
    if !ensure_valid(vm, f, &sock) {
        return Value::Null;
    }
    let Arg::Arr(arr) = &a[1] else {
        return Value::Null;
    };
    let msgv = Value::Array(arr.clone());
    let mut ser = Ser {
        vm: &mut *vm,
        f,
        sock: &sock,
        keys: vec!["msghdr".to_string()],
        err: None,
        fill_sockaddr: false,
    };
    let m = ser.msghdr_recv(&msgv);
    let err = ser.err.take();
    if err.is_some() || m.is_none() {
        dispose(vm, f, err);
        return Value::False;
    }
    let mut m = m.unwrap();
    let mut iovs: Vec<libc::iovec> = m
        .iov
        .iter_mut()
        .map(|b| libc::iovec {
            iov_base: b.as_mut_ptr() as *mut libc::c_void,
            iov_len: b.len(),
        })
        .collect();
    let mut ctl = m.control.as_ref().map(|c| (aligned(c), c.len()));
    let mut hdr: libc::msghdr = unsafe { std::mem::zeroed() };
    if let Some(n) = m.name.as_mut() {
        hdr.msg_name = n.as_mut_ptr() as *mut libc::c_void;
        hdr.msg_namelen = n.len() as libc::socklen_t;
    }
    hdr.msg_iov = iovs.as_mut_ptr();
    hdr.msg_iovlen = iovs.len();
    if let Some((c, len)) = ctl.as_mut() {
        hdr.msg_control = c.as_mut_ptr() as *mut libc::c_void;
        hdr.msg_controllen = *len;
    }
    vm.flush_cli_output();
    let res = unsafe { libc::recvmsg(sock.fd.get(), &mut hdr, flags as i32) };
    if res == -1 {
        let e = errno();
        set_last_error(e);
        vm.emit_warning(&format!(
            "{}(): Error in recvmsg [{}]: {}",
            f,
            e,
            sockets_strerror(e)
        ));
        return Value::False;
    }
    let mut r = Res {
        vm: &mut *vm,
        f,
        keys: vec!["msghdr".to_string()],
        err: None,
    };
    let mut out = PhpArray::new();
    // name
    r.keys.push("name".to_string());
    let name = match &m.name {
        Some(n) => r.sockaddr(n),
        None => Value::Null,
    };
    r.keys.pop();
    out.set(skey("name"), name);
    // control
    if r.err.is_none() {
        r.keys.push("control".to_string());
        let c = r.control_array(&hdr);
        r.keys.pop();
        out.set(skey("control"), c);
    }
    // iov
    if r.err.is_none() {
        let mut iov = PhpArray::new();
        let mut left = res as usize;
        for b in &m.iov {
            if left == 0 {
                break;
            }
            let n = b.len().min(left);
            iov.push(vstr(&b[..n]));
            left -= n;
        }
        out.set(skey("iov"), new_arr(iov));
    }
    if r.err.is_none() {
        out.set(skey("flags"), Value::Long(hdr.msg_flags as i64));
    }
    let err = r.err.take();
    if err.is_none() {
        assign_ref(&args[1], new_arr(out));
    } else {
        dispose(vm, f, err);
        assign_ref(&args[1], Value::False);
    }
    Value::Long(res as i64)
}

pub fn socket_cmsg_space(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "ll|l", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let level = a[0].long_or(0);
    let ty = a[1].long_or(0);
    let n = a[2].long_or(0);
    if !check_int(vm, f, level, 1) || !check_int(vm, f, ty, 2) || !check_int(vm, f, n, 3) {
        return Value::Null;
    }
    if n < 0 {
        zpp::value_error(vm, f, 3, "must be greater than or equal to 0");
        return Value::Null;
    }
    let Some((size, var, _)) = anc_entry(level as i32, ty as i32) else {
        zpp::plain_value_error(
            vm,
            &format!("Pair level {} and/or type {} is not supported", level, ty),
        );
        return Value::Null;
    };
    Value::Long(cmsg_space(size + n as usize * var) as i64)
}

/// php_do_setsockopt_ipv6_rfc3542()
pub fn setsockopt_ipv6_rfc3542(
    vm: &mut Vm,
    f: &str,
    sock: &Sock,
    level: i32,
    optname: i32,
    arg4: &Value,
) -> OptRes {
    if optname != IPV6_PKTINFO {
        return OptRes::NotHandled;
    }
    let mut ser = Ser {
        vm: &mut *vm,
        f,
        sock,
        keys: vec!["in6_pktinfo".to_string()],
        err: None,
        fill_sockaddr: true,
    };
    let r = ser.in6_pktinfo(arg4);
    let err = ser.err.take();
    if err.is_some() || r.is_none() {
        dispose(vm, f, err);
        return OptRes::Failure;
    }
    let opt = r.unwrap();
    let rv = unsafe {
        libc::setsockopt(
            sock.fd.get(),
            level,
            optname,
            opt.as_ptr() as *const libc::c_void,
            opt.len() as libc::socklen_t,
        )
    };
    if rv != 0 {
        sock_error(vm, f, sock, "Unable to set socket option", errno());
        return OptRes::Failure;
    }
    OptRes::Success
}

/// php_do_getsockopt_ipv6_rfc3542()
pub fn getsockopt_ipv6_rfc3542(
    vm: &mut Vm,
    f: &str,
    sock: &Sock,
    level: i32,
    optname: i32,
) -> OptRes2 {
    if optname != IPV6_PKTINFO {
        return OptRes2::NotHandled;
    }
    let mut buf = [0u8; 20];
    let mut len = buf.len() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            sock.fd.get(),
            level,
            optname,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut len,
        )
    };
    if r != 0 {
        sock_error(vm, f, sock, "unable to get socket option", errno());
        return OptRes2::Failure;
    }
    let mut res = Res {
        vm: &mut *vm,
        f,
        keys: vec!["in6_pktinfo".to_string()],
        err: None,
    };
    OptRes2::Value(res.in6_pktinfo(&buf))
}
