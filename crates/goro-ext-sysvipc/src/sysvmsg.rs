//! ext/sysvmsg (sysvmsg.c): System V message queues.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::zpp::{self, Arg};
use crate::{R, assign_ref, bval, errno, native, new_object, strerror, warn, xlong};

const CLASS: &str = "SysvMessageQueue";

const PHP_MSG_IPC_NOWAIT: i64 = 1;
const PHP_MSG_NOERROR: i64 = 2;
const PHP_MSG_EXCEPT: i64 = 4;

const CONSTANTS: &[(&str, i64)] = &[
    ("MSG_IPC_NOWAIT", PHP_MSG_IPC_NOWAIT),
    ("MSG_EAGAIN", libc::EAGAIN as i64),
    ("MSG_ENOMSG", libc::ENOMSG as i64),
    ("MSG_NOERROR", PHP_MSG_NOERROR),
    ("MSG_EXCEPT", PHP_MSG_EXCEPT),
];

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("msg_get_queue", msg_get_queue),
    ("msg_send", msg_send),
    ("msg_receive", msg_receive),
    ("msg_remove_queue", msg_remove_queue),
    ("msg_stat_queue", msg_stat_queue),
    ("msg_set_queue", msg_set_queue),
    ("msg_queue_exists", msg_queue_exists),
];

pub fn register(vm: &mut Vm) {
    if crate::register_ext(vm, b"sysvmsg", FUNCTIONS, CONSTANTS) {
        crate::register_class(
            vm,
            CLASS.as_bytes(),
            "Cannot directly construct SysvMessageQueue, use msg_get_queue() instead",
        );
    }
}

/// sysvmsg_queue_t
struct Queue {
    id: i32,
}

fn queue_arg(vm: &mut Vm, f: &str, spec: &str, args: &[Value]) -> Option<(Vec<Arg>, Rc<Queue>)> {
    let a = zpp::parse(vm, f, CLASS, spec, args).ok()?;
    let q = native::<Queue>(&a[0])?;
    Some((a, q))
}

fn any(a: &Arg) -> Value {
    match a {
        Arg::Any(v) => v.clone(),
        _ => Value::Undef,
    }
}

fn msg_get_queue(vm: &mut Vm, args: &[Value]) -> R {
    let f = "msg_get_queue";
    let Ok(a) = zpp::parse(vm, f, CLASS, "l|l", args) else {
        return Ok(Value::Null);
    };
    let key = a[0].long_or(0);
    let perms = a[1].long_or(0o666);
    let mut id = unsafe { libc::msgget(key as libc::key_t, 0) };
    if id < 0 {
        // doesn't already exist; create it
        id = unsafe {
            libc::msgget(
                key as libc::key_t,
                libc::IPC_CREAT | libc::IPC_EXCL | perms as i32,
            )
        };
        if id < 0 {
            let e = errno();
            warn(
                vm,
                f,
                &format!("Failed for key 0x{}: {}", xlong(key), strerror(e)),
            );
            return Ok(Value::False);
        }
    }
    Ok(new_object(vm, CLASS.as_bytes(), Queue { id }))
}

fn msg_queue_exists(vm: &mut Vm, args: &[Value]) -> R {
    let Ok(a) = zpp::parse(vm, "msg_queue_exists", CLASS, "l", args) else {
        return Ok(Value::Null);
    };
    let key = a[0].long_or(0);
    Ok(bval(unsafe { libc::msgget(key as libc::key_t, 0) } >= 0))
}

fn msg_remove_queue(vm: &mut Vm, args: &[Value]) -> R {
    let Some((_, q)) = queue_arg(vm, "msg_remove_queue", "O", args) else {
        return Ok(Value::Null);
    };
    Ok(bval(
        unsafe { libc::msgctl(q.id, libc::IPC_RMID, std::ptr::null_mut()) } == 0,
    ))
}

fn msg_stat_queue(vm: &mut Vm, args: &[Value]) -> R {
    let Some((_, q)) = queue_arg(vm, "msg_stat_queue", "O", args) else {
        return Ok(Value::Null);
    };
    let mut st: libc::msqid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::msgctl(q.id, libc::IPC_STAT, &mut st) } != 0 {
        return Ok(Value::False);
    }
    let mut a = PhpArray::new();
    let fields: [(&str, i64); 10] = [
        ("msg_perm.uid", st.msg_perm.uid as i64),
        ("msg_perm.gid", st.msg_perm.gid as i64),
        ("msg_perm.mode", st.msg_perm.mode as i64),
        ("msg_stime", st.msg_stime),
        ("msg_rtime", st.msg_rtime),
        ("msg_ctime", st.msg_ctime),
        ("msg_qnum", st.msg_qnum as i64),
        ("msg_qbytes", st.msg_qbytes as i64),
        ("msg_lspid", st.msg_lspid as i64),
        ("msg_lrpid", st.msg_lrpid as i64),
    ];
    for (k, v) in fields {
        a.set(
            ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
            Value::Long(v),
        );
    }
    Ok(Value::Array(Rc::new(RefCell::new(a))))
}

fn msg_set_queue(vm: &mut Vm, args: &[Value]) -> R {
    let Some((a, q)) = queue_arg(vm, "msg_set_queue", "Oh", args) else {
        return Ok(Value::Null);
    };
    let Value::Array(data) = any(&a[1]).deref() else {
        return Ok(Value::Null);
    };
    let mut st: libc::msqid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::msgctl(q.id, libc::IPC_STAT, &mut st) } != 0 {
        return Ok(Value::False);
    }
    let get = |k: &str| -> Option<Value> {
        data.borrow()
            .get(&ArrayKey::String(PhpString::from_bytes(k.as_bytes())))
            .map(|v| v.deref())
    };
    if let Some(v) = get("msg_perm.uid") {
        st.msg_perm.uid = v.to_long() as libc::uid_t;
    }
    if let Some(v) = get("msg_perm.gid") {
        st.msg_perm.gid = v.to_long() as libc::gid_t;
    }
    if let Some(v) = get("msg_perm.mode") {
        st.msg_perm.mode = v.to_long() as _;
    }
    if let Some(v) = get("msg_qbytes") {
        st.msg_qbytes = v.to_long() as _;
    }
    Ok(bval(
        unsafe { libc::msgctl(q.id, libc::IPC_SET, &mut st) } == 0,
    ))
}

fn msg_send(vm: &mut Vm, args: &[Value]) -> R {
    let f = "msg_send";
    let Some((a, q)) = queue_arg(vm, f, "Olz|bbz", args) else {
        return Ok(Value::Null);
    };
    let msgtype = a[1].long_or(0);
    let message = any(&a[2]).deref();
    let do_serialize = a[3].bool_or(true);
    let blocking = a[4].bool_or(true);
    let zerror = match &a[5] {
        Arg::Any(v) => Some(v.clone()),
        _ => None,
    };
    let text: Vec<u8> = if do_serialize {
        match goro_ext_standard::serialize::php_serialize(vm, &message) {
            Some(s) => s,
            None => return Ok(Value::Null),
        }
    } else {
        match &message {
            Value::String(s) => s.as_bytes().to_vec(),
            Value::Long(n) => n.to_string().into_bytes(),
            Value::False => b"0".to_vec(),
            Value::True => b"1".to_vec(),
            Value::Double(d) => format!("{:.6}", d).into_bytes(),
            other => {
                zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be of type string|int|float|bool, {} given",
                        Vm::value_type_name(other)
                    ),
                );
                return Ok(Value::Null);
            }
        }
    };
    let mut buf = Vec::with_capacity(8 + text.len() + 1);
    buf.extend_from_slice(&(msgtype as libc::c_long).to_ne_bytes());
    buf.extend_from_slice(&text);
    buf.push(0);
    let r = unsafe {
        libc::msgsnd(
            q.id,
            buf.as_ptr() as *const libc::c_void,
            text.len(),
            if blocking { 0 } else { libc::IPC_NOWAIT },
        )
    };
    if r == -1 {
        let e = errno();
        warn(vm, f, &format!("msgsnd failed: {}", strerror(e)));
        if let Some(z) = &zerror {
            assign_ref(z, Value::Long(e as i64));
        }
        return Ok(Value::False);
    }
    Ok(Value::True)
}

fn msg_receive(vm: &mut Vm, args: &[Value]) -> R {
    let f = "msg_receive";
    let Some((a, q)) = queue_arg(vm, f, "Olzlz|blz", args) else {
        return Ok(Value::Null);
    };
    let desired = a[1].long_or(0);
    let out_msgtype = any(&a[2]);
    let maxsize = a[3].long_or(0);
    let out_message = any(&a[4]);
    let do_unserialize = a[5].bool_or(true);
    let flags = a[6].long_or(0);
    let zerrcode = match &a[7] {
        Arg::Any(v) => Some(v.clone()),
        _ => None,
    };
    if maxsize <= 0 {
        zpp::value_error(vm, f, 4, "must be greater than 0");
        return Ok(Value::Null);
    }
    let mut realflags = 0;
    if flags & PHP_MSG_EXCEPT != 0 {
        realflags |= libc::MSG_EXCEPT;
    }
    if flags & PHP_MSG_NOERROR != 0 {
        realflags |= libc::MSG_NOERROR;
    }
    if flags & PHP_MSG_IPC_NOWAIT != 0 {
        realflags |= libc::IPC_NOWAIT;
    }
    let mut buf = vec![0u8; 8 + maxsize as usize + 1];
    let result = unsafe {
        libc::msgrcv(
            q.id,
            buf.as_mut_ptr() as *mut libc::c_void,
            maxsize as usize,
            desired as libc::c_long,
            realflags,
        )
    };
    if result < 0 {
        let e = errno();
        assign_ref(&out_msgtype, Value::Long(0));
        assign_ref(&out_message, Value::False);
        if let Some(z) = &zerrcode {
            assign_ref(z, Value::Long(e as i64));
        }
        return Ok(Value::False);
    }
    let mtype = i64::from_ne_bytes(buf[..8].try_into().unwrap());
    assign_ref(&out_msgtype, Value::Long(mtype));
    if let Some(z) = &zerrcode {
        assign_ref(z, Value::Long(0));
    }
    let text = &buf[8..8 + result as usize];
    if do_unserialize {
        match goro_ext_standard::serialize::php_var_unserialize(vm, text) {
            Some(v) => {
                assign_ref(&out_message, v);
                Ok(Value::True)
            }
            None => {
                if vm.current_exception.is_some() {
                    return Ok(Value::Null);
                }
                warn(vm, f, "Message corrupted");
                assign_ref(&out_message, Value::False);
                Ok(Value::False)
            }
        }
    } else {
        assign_ref(&out_message, Value::String(PhpString::from_bytes(text)));
        Ok(Value::True)
    }
}
