//! multicast.c (the RFC 3678 API, as on Linux)

use goro_core::array::PhpArray;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::zpp;
use crate::*;

/// The result of the option helpers (SUCCESS / FAILURE / 1 = not handled).
#[derive(PartialEq, Eq)]
pub enum OptRes {
    Success,
    Failure,
    NotHandled,
}

/// php_string_to_if_index()
pub fn string_to_if_index(vm: &mut Vm, f: &str, val: &[u8]) -> Option<u32> {
    let c = cstring(val);
    let ind = unsafe { libc::if_nametoindex(c.as_ptr()) };
    if ind == 0 {
        vm.emit_warning(&format!(
            "{}(): No interface with name \"{}\" could be found",
            f,
            String::from_utf8_lossy(c_bytes(val))
        ));
        None
    } else {
        Some(ind)
    }
}

/// php_get_if_index_from_zval()
fn get_if_index_from_zval(vm: &mut Vm, f: &str, val: &Value) -> Option<u32> {
    match val.deref() {
        Value::Long(n) => {
            if n < 0 || n > u32::MAX as i64 {
                zpp::plain_value_error(vm, "Index must be between 0 and 4294967295");
                return None;
            }
            Some(n as u32)
        }
        other => {
            let s = vm.value_to_string_checked(&other).ok()?;
            string_to_if_index(vm, f, s.as_bytes())
        }
    }
}

#[repr(C)]
struct IfReq {
    name: [libc::c_char; libc::IFNAMSIZ],
    /// ifr_ifru: ifr_addr / ifr_ifindex
    data: [u8; 24],
}

#[repr(C)]
struct IfConf {
    ifc_len: i32,
    ifc_buf: *mut libc::c_char,
}

/// php_if_index_to_addr4()
pub fn if_index_to_addr4(vm: &mut Vm, f: &str, if_index: u32, sock: &Sock) -> Option<[u8; 4]> {
    if if_index == 0 {
        return Some([0; 4]);
    }
    let mut req = IfReq {
        name: [0; libc::IFNAMSIZ],
        data: [0; 24],
    };
    req.data[..4].copy_from_slice(&(if_index as i32).to_ne_bytes());
    let fd = sock.fd.get();
    if unsafe { libc::ioctl(fd, libc::SIOCGIFNAME, &mut req as *mut IfReq) } == -1
        || unsafe { libc::ioctl(fd, libc::SIOCGIFADDR, &mut req as *mut IfReq) } == -1
    {
        vm.emit_warning(&format!(
            "{}(): Failed obtaining address for interface {}: error {}",
            f,
            if_index,
            errno()
        ));
        return None;
    }
    // struct sockaddr_in: family (2), port (2), addr (4)
    let mut a = [0u8; 4];
    a.copy_from_slice(&req.data[4..8]);
    Some(a)
}

/// php_add4_to_if_index()
pub fn add4_to_if_index(vm: &mut Vm, f: &str, addr: [u8; 4], sock: &Sock) -> Option<u32> {
    if addr == [0; 4] {
        return Some(0);
    }
    let fd = sock.fd.get();
    let entry = std::mem::size_of::<IfReq>();
    let mut size = 0usize;
    let mut lastsize: i32 = 0;
    let mut buf: Vec<u8>;
    loop {
        size += 5 * entry;
        buf = vec![0u8; size];
        let mut conf = IfConf {
            ifc_len: size as i32,
            ifc_buf: buf.as_mut_ptr() as *mut libc::c_char,
        };
        if unsafe { libc::ioctl(fd, libc::SIOCGIFCONF, &mut conf as *mut IfConf) } == -1
            && (errno() != libc::EINVAL || lastsize != 0)
        {
            vm.emit_warning(&format!(
                "{}(): Failed obtaining interfaces list: error {}",
                f,
                errno()
            ));
            return None;
        }
        if conf.ifc_len == lastsize {
            break;
        }
        lastsize = conf.ifc_len;
    }
    let mut p = 0usize;
    while p + entry <= lastsize as usize {
        let mut req = IfReq {
            name: [0; libc::IFNAMSIZ],
            data: [0; 24],
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                buf[p..].as_ptr(),
                &mut req as *mut IfReq as *mut u8,
                entry,
            );
        }
        let family = u16::from_ne_bytes([req.data[0], req.data[1]]) as i32;
        if family == libc::AF_INET && req.data[4..8] == addr {
            if unsafe { libc::ioctl(fd, libc::SIOCGIFINDEX, &mut req as *mut IfReq) } == -1 {
                vm.emit_warning(&format!(
                    "{}(): Error converting interface name to index: error {}",
                    f,
                    errno()
                ));
                return None;
            }
            let idx = i32::from_ne_bytes([req.data[0], req.data[1], req.data[2], req.data[3]]);
            return Some(idx as u32);
        }
        p += entry;
    }
    vm.emit_warning(&format!(
        "{}(): The interface with IP address {} was not found",
        f,
        inet_ntop4(&addr)
    ));
    None
}

/// The option array (Z_ARRVAL or Z_OBJPROP) of a multicast option.
pub fn opt_table(v: &Value) -> Option<PhpArray> {
    match v.deref() {
        Value::Array(a) => Some(a.borrow().clone()),
        Value::Object(o) => {
            let mut t = PhpArray::new();
            for (k, val) in o.borrow().properties.iter() {
                t.set(
                    goro_core::array::ArrayKey::String(goro_core::string::PhpString::from_bytes(k)),
                    val.clone(),
                );
            }
            Some(t)
        }
        _ => None,
    }
}

fn get_if_index_from_array(vm: &mut Vm, f: &str, ht: &PhpArray, key: &str) -> Option<u32> {
    match ht.get_str(key.as_bytes()) {
        None => Some(0),
        Some(v) => {
            let v = v.clone();
            get_if_index_from_zval(vm, f, &v)
        }
    }
}

fn get_address_from_array(
    vm: &mut Vm,
    f: &str,
    ht: &PhpArray,
    key: &str,
    sock: &Sock,
) -> Option<Vec<u8>> {
    let Some(v) = ht.get_str(key.as_bytes()).cloned() else {
        zpp::plain_value_error(vm, &format!("No key \"{}\" passed in optval", key));
        return None;
    };
    let s = vm.value_to_string_checked(&v.deref()).ok()?;
    set_inet46_addr(vm, f, s.as_bytes(), sock)
}

#[repr(C)]
struct GroupReq {
    gr_interface: u32,
    gr_group: libc::sockaddr_storage,
}

#[repr(C)]
struct GroupSourceReq {
    gsr_interface: u32,
    gsr_group: libc::sockaddr_storage,
    gsr_source: libc::sockaddr_storage,
}

fn copy_ss(dst: &mut libc::sockaddr_storage, src: &[u8]) {
    let n = src.len().min(std::mem::size_of::<libc::sockaddr_storage>());
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut _ as *mut u8, n);
    }
}

fn setsockopt_raw<T>(fd: i32, level: i32, optname: i32, v: &T) -> i32 {
    unsafe {
        libc::setsockopt(
            fd,
            level,
            optname,
            v as *const T as *const libc::c_void,
            std::mem::size_of::<T>() as libc::socklen_t,
        )
    }
}

/// php_do_mcast_opt()
fn do_mcast_opt(
    vm: &mut Vm,
    f: &str,
    sock: &Sock,
    level: i32,
    optname: i32,
    arg4: &Value,
) -> OptRes {
    let fd = sock.fd.get();
    let retval = match optname {
        libc::MCAST_JOIN_GROUP | libc::MCAST_LEAVE_GROUP => {
            let Some(ht) = opt_table(arg4) else {
                zpp::arg_type_error(
                    vm,
                    f,
                    4,
                    &format!(
                        "must be of type array when argument #3 ($option) is MCAST_LEAVE_GROUP, {} given",
                        Vm::value_type_name(&arg4.deref())
                    ),
                );
                return OptRes::Failure;
            };
            let Some(group) = get_address_from_array(vm, f, &ht, "group", sock) else {
                return OptRes::Failure;
            };
            let Some(if_index) = get_if_index_from_array(vm, f, &ht, "interface") else {
                return OptRes::Failure;
            };
            let mut greq: GroupReq = unsafe { std::mem::zeroed() };
            copy_ss(&mut greq.gr_group, &group);
            greq.gr_interface = if_index;
            setsockopt_raw(fd, level, optname, &greq)
        }
        libc::MCAST_BLOCK_SOURCE
        | libc::MCAST_UNBLOCK_SOURCE
        | libc::MCAST_JOIN_SOURCE_GROUP
        | libc::MCAST_LEAVE_SOURCE_GROUP => {
            let Some(ht) = opt_table(arg4) else {
                zpp::arg_type_error(
                    vm,
                    f,
                    4,
                    &format!(
                        "must be of type array when argument #3 ($option) is MCAST_LEAVE_SOURCE_GROUP, {} given",
                        Vm::value_type_name(&arg4.deref())
                    ),
                );
                return OptRes::Failure;
            };
            let Some(group) = get_address_from_array(vm, f, &ht, "group", sock) else {
                return OptRes::Failure;
            };
            let Some(source) = get_address_from_array(vm, f, &ht, "source", sock) else {
                return OptRes::Failure;
            };
            let Some(if_index) = get_if_index_from_array(vm, f, &ht, "interface") else {
                return OptRes::Failure;
            };
            let mut gsreq: GroupSourceReq = unsafe { std::mem::zeroed() };
            copy_ss(&mut gsreq.gsr_group, &group);
            copy_ss(&mut gsreq.gsr_source, &source);
            gsreq.gsr_interface = if_index;
            setsockopt_raw(fd, level, optname, &gsreq)
        }
        _ => {
            vm.emit_warning(&format!(
                "{}(): Unexpected option in php_do_mcast_opt (level {}, option {}). This is a bug.",
                f, level, optname
            ));
            return OptRes::Failure;
        }
    };
    if retval != 0 {
        sock_error(vm, f, sock, "Unable to set socket option", errno());
        return OptRes::Failure;
    }
    OptRes::Success
}

fn is_mcast_opt(optname: i32) -> bool {
    matches!(
        optname,
        libc::MCAST_JOIN_GROUP
            | libc::MCAST_LEAVE_GROUP
            | libc::MCAST_BLOCK_SOURCE
            | libc::MCAST_UNBLOCK_SOURCE
            | libc::MCAST_JOIN_SOURCE_GROUP
            | libc::MCAST_LEAVE_SOURCE_GROUP
    )
}

fn dosockopt(vm: &mut Vm, f: &str, sock: &Sock, level: i32, optname: i32, opt: &[u8]) -> OptRes {
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
        return OptRes::Failure;
    }
    OptRes::Success
}

/// php_do_setsockopt_ip_mcast()
pub fn setsockopt_ip_mcast(
    vm: &mut Vm,
    f: &str,
    sock: &Sock,
    level: i32,
    optname: i32,
    arg4: &Value,
) -> OptRes {
    if is_mcast_opt(optname) {
        return do_mcast_opt(vm, f, sock, level, optname, arg4);
    }
    match optname {
        libc::IP_MULTICAST_IF => {
            let Some(idx) = get_if_index_from_zval(vm, f, arg4) else {
                return OptRes::Failure;
            };
            let Some(addr) = if_index_to_addr4(vm, f, idx, sock) else {
                return OptRes::Failure;
            };
            dosockopt(vm, f, sock, level, optname, &addr)
        }
        libc::IP_MULTICAST_LOOP => {
            let v = arg4.deref().to_bool() as u8;
            dosockopt(vm, f, sock, level, optname, &[v])
        }
        libc::IP_MULTICAST_TTL => {
            let n = zpp::zval_get_long(vm, arg4);
            if !(0..=255).contains(&n) {
                zpp::value_error(vm, f, 4, "must be between 0 and 255");
                return OptRes::Failure;
            }
            dosockopt(vm, f, sock, level, optname, &[n as u8])
        }
        _ => OptRes::NotHandled,
    }
}

/// php_do_setsockopt_ipv6_mcast()
pub fn setsockopt_ipv6_mcast(
    vm: &mut Vm,
    f: &str,
    sock: &Sock,
    level: i32,
    optname: i32,
    arg4: &Value,
) -> OptRes {
    if is_mcast_opt(optname) {
        return do_mcast_opt(vm, f, sock, level, optname, arg4);
    }
    match optname {
        libc::IPV6_MULTICAST_IF => {
            let Some(idx) = get_if_index_from_zval(vm, f, arg4) else {
                return OptRes::Failure;
            };
            dosockopt(vm, f, sock, level, optname, &idx.to_ne_bytes())
        }
        libc::IPV6_MULTICAST_LOOP => {
            let v = arg4.deref().to_bool() as i32;
            dosockopt(vm, f, sock, level, optname, &v.to_ne_bytes())
        }
        libc::IPV6_MULTICAST_HOPS => {
            let n = zpp::zval_get_long(vm, arg4);
            if !(-1..=255).contains(&n) {
                zpp::value_error(vm, f, 4, "must be between -1 and 255");
                return OptRes::Failure;
            }
            dosockopt(vm, f, sock, level, optname, &(n as i32).to_ne_bytes())
        }
        _ => OptRes::NotHandled,
    }
}
