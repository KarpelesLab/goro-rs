//! socket_get_option() / socket_set_option()

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::funcs::vstr;
use crate::mcast::{self, OptRes};
use crate::zpp;
use crate::*;

const SO_MEMINFO: i32 = 55;
const SK_MEMINFO_VARS: usize = 9;
const SO_ATTACH_REUSEPORT_CBPF: i32 = 51;
const SO_DETACH_BPF: i32 = 27;
const UDP_SEGMENT: i32 = 103;
const SKF_AD_OFF: i32 = -0x1000;
const SKF_AD_QUEUE: u32 = 24;
const SKF_AD_CPU: u32 = 36;

fn skey(s: &str) -> ArrayKey {
    ArrayKey::String(PhpString::from_bytes(s.as_bytes()))
}

fn arr(a: PhpArray) -> Value {
    Value::Array(Rc::new(RefCell::new(a)))
}

fn getsockopt_buf(fd: i32, level: i32, optname: i32, buf: &mut [u8]) -> Result<usize, i32> {
    let mut len = buf.len() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            fd,
            level,
            optname,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut len,
        )
    };
    if r != 0 {
        Err(errno())
    } else {
        Ok(len as usize)
    }
}

fn ne_i32(b: &[u8], off: usize) -> i32 {
    i32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn ne_i64(b: &[u8], off: usize) -> i64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    i64::from_ne_bytes(a)
}

pub fn socket_get_option(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Oll", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let sock = sock_of(&a[0].obj().unwrap());
    if !ensure_valid(vm, f, &sock) {
        return Value::Null;
    }
    let level = a[1].long_or(0) as i32;
    let optname = a[2].long_or(0) as i32;
    let fd = sock.fd.get();
    let fail = |vm: &mut Vm, e: i32| {
        sock_error(vm, f, &sock, "Unable to retrieve socket option", e);
        Value::False
    };

    if level == libc::IPPROTO_IP {
        if optname == libc::IP_MULTICAST_IF {
            let mut b = [0u8; 4];
            if let Err(e) = getsockopt_buf(fd, level, optname, &mut b) {
                return fail(vm, e);
            }
            return match mcast::add4_to_if_index(vm, f, b, &sock) {
                Some(i) => Value::Long(i as i64),
                None => Value::False,
            };
        }
    } else if level == libc::IPPROTO_IPV6 {
        match crate::conv::getsockopt_ipv6_rfc3542(vm, f, &sock, level, optname) {
            OptRes2::Value(v) => return v,
            OptRes2::Failure => return Value::False,
            OptRes2::NotHandled => {}
        }
    }

    if level == libc::IPPROTO_TCP && optname == libc::TCP_CONGESTION {
        let mut name = [0u8; 16];
        if let Err(e) = getsockopt_buf(fd, level, optname, &mut name) {
            return fail(vm, e);
        }
        let mut out = PhpArray::new();
        out.set(skey("name"), vstr(c_bytes(&name)));
        return arr(out);
    }

    if level == libc::SOL_SOCKET {
        match optname {
            libc::SO_LINGER => {
                let mut b = [0u8; 8];
                if let Err(e) = getsockopt_buf(fd, level, optname, &mut b) {
                    return fail(vm, e);
                }
                let mut out = PhpArray::new();
                out.set(skey("l_onoff"), Value::Long(ne_i32(&b, 0) as i64));
                out.set(skey("l_linger"), Value::Long(ne_i32(&b, 4) as i64));
                return arr(out);
            }
            libc::SO_RCVTIMEO | libc::SO_SNDTIMEO => {
                let mut b = [0u8; 16];
                if let Err(e) = getsockopt_buf(fd, level, optname, &mut b) {
                    return fail(vm, e);
                }
                let mut out = PhpArray::new();
                out.set(skey("sec"), Value::Long(ne_i64(&b, 0)));
                out.set(skey("usec"), Value::Long(ne_i64(&b, 8)));
                return arr(out);
            }
            SO_MEMINFO => {
                let mut b = [0u8; SK_MEMINFO_VARS * 4];
                let n = match getsockopt_buf(fd, level, optname, &mut b) {
                    Ok(n) => n,
                    Err(e) => return fail(vm, e),
                };
                if n != b.len() {
                    vm.emit_warning(&format!(
                        "{}(): Unable to retrieve all socket meminfo data",
                        f
                    ));
                    return Value::False;
                }
                let u = |i: usize| {
                    Value::Long(u32::from_ne_bytes([
                        b[i * 4],
                        b[i * 4 + 1],
                        b[i * 4 + 2],
                        b[i * 4 + 3],
                    ]) as i64)
                };
                let mut out = PhpArray::new();
                // SK_MEMINFO_RMEM_ALLOC, RCVBUF, WMEM_ALLOC, SNDBUF, FWD_ALLOC,
                // WMEM_QUEUED, OPTMEM, BACKLOG, DROPS
                for (i, k) in [
                    "rmem_alloc",
                    "rcvbuf",
                    "wmem_alloc",
                    "sndbuf",
                    "fwd_alloc",
                    "wmem_queued",
                    "optmem",
                    "backlog",
                    "drops",
                ]
                .iter()
                .enumerate()
                {
                    out.set(skey(k), u(i));
                }
                return arr(out);
            }
            _ => {}
        }
    }

    let mut b = [0u8; 4];
    let n = match getsockopt_buf(fd, level, optname, &mut b) {
        Ok(n) => n,
        Err(e) => return fail(vm, e),
    };
    let v = if n == 1 { b[0] as i32 } else { ne_i32(&b, 0) };
    Value::Long(v as i64)
}

/// Result of an option reader that produces a value.
pub enum OptRes2 {
    Value(Value),
    Failure,
    NotHandled,
}

/// The option table of an array (or object) option value.
fn opt_table_or_error(vm: &mut Vm, f: &str, arg4: &Value, optname_str: &str) -> Option<PhpArray> {
    match mcast::opt_table(arg4) {
        Some(t) => Some(t),
        None => {
            zpp::arg_type_error(
                vm,
                f,
                4,
                &format!(
                    "must be of type array when argument #3 ($option) is {}, {} given",
                    optname_str,
                    Vm::value_type_name(&arg4.deref())
                ),
            );
            None
        }
    }
}

pub fn socket_set_option(vm: &mut Vm, f: &str, args: &[Value]) -> Value {
    let a = match zpp::parse(vm, f, "Ollz", args) {
        Ok(a) => a,
        Err(()) => return Value::Null,
    };
    let sock = sock_of(&a[0].obj().unwrap());
    if !ensure_valid(vm, f, &sock) {
        return Value::Null;
    }
    let level = a[1].long_or(0) as i32;
    let mut optname = a[2].long_or(0) as i32;
    let arg4 = a[3].any().map(|v| v.deref()).unwrap_or(Value::Null);
    set_errno(0);

    let handled = |r: OptRes| match r {
        OptRes::Success => Some(Value::True),
        OptRes::Failure => Some(Value::False),
        OptRes::NotHandled => None,
    };

    let mut default_case = false;
    if level == libc::IPPROTO_IP {
        let r = mcast::setsockopt_ip_mcast(vm, f, &sock, level, optname, &arg4);
        if let Some(v) = handled(r) {
            return finish(vm, v);
        }
        default_case = true;
    } else if level == libc::IPPROTO_IPV6 {
        let mut r = mcast::setsockopt_ipv6_mcast(vm, f, &sock, level, optname, &arg4);
        if r == OptRes::NotHandled {
            r = crate::conv::setsockopt_ipv6_rfc3542(vm, f, &sock, level, optname, &arg4);
        }
        if let Some(v) = handled(r) {
            return finish(vm, v);
        }
        default_case = true;
    }

    if !default_case && level == libc::IPPROTO_TCP && optname == libc::TCP_CONGESTION {
        let Value::String(s) = &arg4 else {
            zpp::arg_type_error(
                vm,
                f,
                4,
                &format!(
                    "must be of type string when argument #3 ($option) is TCP_CONGESTION, {} given",
                    Vm::value_type_name(&arg4)
                ),
            );
            return Value::Null;
        };
        let s = s.as_bytes().to_vec();
        return setopt_bytes(vm, f, &sock, level, optname, &s);
    }

    let opt: Vec<u8> = if default_case {
        (zpp::zval_get_long(vm, &arg4) as i32)
            .to_ne_bytes()
            .to_vec()
    } else {
        match optname {
            libc::SO_LINGER => {
                let Some(ht) = opt_table_or_error(vm, f, &arg4, "SO_LINGER") else {
                    return Value::Null;
                };
                let Some(onoff) = ht.get_str(b"l_onoff").cloned() else {
                    zpp::value_error(vm, f, 4, "must have key \"l_onoff\"");
                    return Value::Null;
                };
                let Some(linger) = ht.get_str(b"l_linger").cloned() else {
                    zpp::value_error(vm, f, 4, "must have key \"l_linger\"");
                    return Value::Null;
                };
                let on = zpp::zval_get_long(vm, &onoff);
                let li = zpp::zval_get_long(vm, &linger);
                if !(0..=65535).contains(&on) {
                    zpp::value_error(vm, f, 4, "\"l_onoff\" must be between 0 and 65535");
                    return Value::Null;
                }
                if !(0..=65535).contains(&li) {
                    zpp::value_error(vm, f, 4, "\"l_linger\" must be between 0 and 65535");
                    return Value::Null;
                }
                let mut v = (on as i32).to_ne_bytes().to_vec();
                v.extend_from_slice(&(li as i32).to_ne_bytes());
                v
            }
            libc::SO_RCVTIMEO | libc::SO_SNDTIMEO => {
                let name = if optname == libc::SO_RCVTIMEO {
                    "SO_RCVTIMEO"
                } else {
                    "SO_SNDTIMEO"
                };
                let Some(ht) = opt_table_or_error(vm, f, &arg4, name) else {
                    return Value::Null;
                };
                let Some(sec) = ht.get_str(b"sec").cloned() else {
                    zpp::value_error(vm, f, 4, "must have key \"sec\"");
                    return Value::Null;
                };
                let Some(usec) = ht.get_str(b"usec").cloned() else {
                    zpp::value_error(vm, f, 4, "must have key \"usec\"");
                    return Value::Null;
                };
                let mut v = zpp::zval_get_long(vm, &sec).to_ne_bytes().to_vec();
                v.extend_from_slice(&zpp::zval_get_long(vm, &usec).to_ne_bytes());
                v
            }
            libc::SO_BINDTODEVICE => {
                let Value::String(s) = &arg4 else {
                    zpp::arg_type_error(
                        vm,
                        f,
                        4,
                        &format!(
                            "must be of type string when argument #3 ($option) is SO_BINDTODEVICE, {} given",
                            Vm::value_type_name(&arg4)
                        ),
                    );
                    return Value::Null;
                };
                s.as_bytes().to_vec()
            }
            SO_ATTACH_REUSEPORT_CBPF => {
                let cbpf = zpp::zval_get_long(vm, &arg4);
                if cbpf == 0 {
                    optname = SO_DETACH_BPF;
                    1i32.to_ne_bytes().to_vec()
                } else {
                    let k = cbpf as u32;
                    match k {
                        SKF_AD_CPU | SKF_AD_QUEUE => {
                            let filter: [libc::sock_filter; 2] = [
                                libc::sock_filter {
                                    code: (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16,
                                    jt: 0,
                                    jf: 0,
                                    k: (SKF_AD_OFF as u32).wrapping_add(k),
                                },
                                libc::sock_filter {
                                    code: (libc::BPF_RET | libc::BPF_A) as u16,
                                    jt: 0,
                                    jf: 0,
                                    k: 0,
                                },
                            ];
                            let prog = libc::sock_fprog {
                                len: 2,
                                filter: filter.as_ptr() as *mut libc::sock_filter,
                            };
                            let r = unsafe {
                                libc::setsockopt(
                                    sock.fd.get(),
                                    level,
                                    optname,
                                    &prog as *const _ as *const libc::c_void,
                                    std::mem::size_of::<libc::sock_fprog>() as libc::socklen_t,
                                )
                            };
                            if r != 0 {
                                sock_error(vm, f, &sock, "Unable to set socket option", errno());
                                return Value::False;
                            }
                            return Value::True;
                        }
                        _ => {
                            vm.emit_warning(&format!("{}(): Unsupported CBPF filter", f));
                            return Value::False;
                        }
                    }
                }
            }
            UDP_SEGMENT => {
                let ov = zpp::zval_get_long(vm, &arg4) as i32;
                if !(0..=65535).contains(&ov) {
                    zpp::value_error(vm, f, 4, "must be of between 0 and 65535");
                    return Value::False;
                }
                ov.to_ne_bytes().to_vec()
            }
            _ => (zpp::zval_get_long(vm, &arg4) as i32)
                .to_ne_bytes()
                .to_vec(),
        }
    };
    setopt_bytes(vm, f, &sock, level, optname, &opt)
}

fn finish(vm: &mut Vm, v: Value) -> Value {
    if vm.current_exception.is_some() {
        Value::Null
    } else {
        v
    }
}

fn setopt_bytes(vm: &mut Vm, f: &str, sock: &Sock, level: i32, optname: i32, opt: &[u8]) -> Value {
    let r = unsafe {
        libc::setsockopt(
            sock.fd.get(),
            level,
            optname,
            opt.as_ptr() as *const libc::c_void,
            opt.len() as libc::socklen_t,
        )
    };
    if r != 0 {
        sock_error(vm, f, sock, "Unable to set socket option", errno());
        return Value::False;
    }
    Value::True
}
