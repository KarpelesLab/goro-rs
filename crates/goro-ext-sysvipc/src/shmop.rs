//! ext/shmop (shmop.c): System V shared memory segments as byte arrays.

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::zpp::{self, Arg};
use crate::{R, errno, native, new_object, strerror, warn};

const CLASS: &str = "Shmop";

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("shmop_open", shmop_open),
    ("shmop_read", shmop_read),
    ("shmop_close", shmop_close),
    ("shmop_size", shmop_size),
    ("shmop_write", shmop_write),
    ("shmop_delete", shmop_delete),
];

pub fn register(vm: &mut Vm) {
    if crate::register_ext(vm, b"shmop", FUNCTIONS, &[]) {
        crate::register_class(
            vm,
            CLASS.as_bytes(),
            "Cannot directly construct Shmop, use shmop_open() instead",
        );
    }
}

/// php_shmop
struct Shmop {
    shmid: i32,
    shmatflg: i32,
    addr: *mut u8,
    size: i64,
}

impl Drop for Shmop {
    /// shmop_free_obj()
    fn drop(&mut self) {
        unsafe {
            libc::shmdt(self.addr as *const libc::c_void);
        }
    }
}

fn shmop_open(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shmop_open";
    let Ok(a) = zpp::parse(vm, f, CLASS, "lsll", args) else {
        return Ok(Value::Null);
    };
    let key = a[0].long_or(0);
    let flags = a[1].str().to_vec();
    let mode = a[2].long_or(0);
    let size = a[3].long_or(0);
    if flags.len() != 1 {
        zpp::value_error(vm, f, 2, "must be a valid access mode");
        return Ok(Value::Null);
    }
    let mut shmflg = mode as i32;
    let mut shmatflg = 0;
    let mut seg_size: i64 = 0;
    match flags[0] {
        b'a' => shmatflg |= libc::SHM_RDONLY,
        b'c' => {
            shmflg |= libc::IPC_CREAT;
            seg_size = size;
        }
        b'n' => {
            shmflg |= libc::IPC_CREAT | libc::IPC_EXCL;
            seg_size = size;
        }
        b'w' => {}
        _ => {
            zpp::value_error(vm, f, 2, "must be a valid access mode");
            return Ok(Value::False);
        }
    }
    if shmflg & libc::IPC_CREAT != 0 && seg_size < 1 {
        zpp::value_error(
            vm,
            f,
            4,
            "must be greater than 0 for the \"c\" and \"n\" access modes",
        );
        return Ok(Value::False);
    }
    let shmid = unsafe { libc::shmget(key as libc::key_t, seg_size as usize, shmflg) };
    if shmid == -1 {
        let e = errno();
        warn(
            vm,
            f,
            &format!(
                "Unable to attach or create shared memory segment \"{}\"",
                strerror(e)
            ),
        );
        return Ok(Value::False);
    }
    let mut shm: libc::shmid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::shmctl(shmid, libc::IPC_STAT, &mut shm) } != 0 {
        let e = errno();
        warn(
            vm,
            f,
            &format!(
                "Unable to get shared memory segment information \"{}\"",
                strerror(e)
            ),
        );
        return Ok(Value::False);
    }
    if shm.shm_segsz > i64::MAX as usize {
        zpp::value_error(vm, f, 4, "is too large");
        return Ok(Value::False);
    }
    let addr = unsafe { libc::shmat(shmid, std::ptr::null(), shmatflg) };
    if addr as isize == -1 {
        let e = errno();
        warn(
            vm,
            f,
            &format!(
                "Unable to attach to shared memory segment \"{}\"",
                strerror(e)
            ),
        );
        return Ok(Value::False);
    }
    let seg = Shmop {
        shmid,
        shmatflg,
        addr: addr as *mut u8,
        size: shm.shm_segsz as i64,
    };
    Ok(new_object(vm, CLASS.as_bytes(), seg))
}

fn seg_arg(
    vm: &mut Vm,
    f: &str,
    spec: &str,
    args: &[Value],
) -> Option<(Vec<Arg>, std::rc::Rc<Shmop>)> {
    let a = zpp::parse(vm, f, CLASS, spec, args).ok()?;
    let s = native::<Shmop>(&a[0])?;
    Some((a, s))
}

fn shmop_read(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shmop_read";
    let Some((a, s)) = seg_arg(vm, f, "Oll", args) else {
        return Ok(Value::Null);
    };
    let start = a[1].long_or(0);
    let count = a[2].long_or(0);
    if start < 0 || start > s.size {
        zpp::value_error(vm, f, 2, "must be between 0 and the segment size");
        return Ok(Value::Null);
    }
    if count < 0 || start > i64::MAX - count || start + count > s.size {
        zpp::value_error(vm, f, 3, "is out of range");
        return Ok(Value::Null);
    }
    let bytes = (if count != 0 { count } else { s.size - start }) as i32;
    let n = bytes.max(0) as usize;
    let data = unsafe { std::slice::from_raw_parts(s.addr.add(start as usize), n) }.to_vec();
    Ok(Value::String(PhpString::from_vec(data)))
}

fn shmop_close(vm: &mut Vm, args: &[Value]) -> R {
    vm.emit_function_deprecation(
        "Function shmop_close() is deprecated since 8.0, as Shmop objects are freed automatically",
    )?;
    let args = &vm.zpp_self_check_flags(b"shmop_close", args, goro_core::arginfo::CHECK_ARITY)?[..];
    let _ = seg_arg(vm, "shmop_close", "O", args);
    Ok(Value::Null)
}

fn shmop_size(vm: &mut Vm, args: &[Value]) -> R {
    let Some((_, s)) = seg_arg(vm, "shmop_size", "O", args) else {
        return Ok(Value::Null);
    };
    Ok(Value::Long(s.size))
}

fn shmop_write(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shmop_write";
    let Some((a, s)) = seg_arg(vm, f, "Osl", args) else {
        return Ok(Value::Null);
    };
    let data = a[1].str().to_vec();
    let offset = a[2].long_or(0);
    if s.shmatflg & libc::SHM_RDONLY == libc::SHM_RDONLY {
        zpp::throw(vm, b"Error", "Read-only segment cannot be written");
        return Ok(Value::Null);
    }
    if offset < 0 || offset > s.size {
        zpp::value_error(vm, f, 3, "is out of range");
        return Ok(Value::Null);
    }
    let writesize = (data.len() as i64).min(s.size - offset);
    unsafe {
        std::ptr::copy_nonoverlapping(
            data.as_ptr(),
            s.addr.add(offset as usize),
            writesize as usize,
        );
    }
    Ok(Value::Long(writesize))
}

fn shmop_delete(vm: &mut Vm, args: &[Value]) -> R {
    let f = "shmop_delete";
    let Some((_, s)) = seg_arg(vm, f, "O", args) else {
        return Ok(Value::Null);
    };
    if unsafe { libc::shmctl(s.shmid, libc::IPC_RMID, std::ptr::null_mut()) } != 0 {
        warn(
            vm,
            f,
            "Can't mark segment for deletion (are you the owner?)",
        );
        return Ok(Value::False);
    }
    Ok(Value::True)
}
