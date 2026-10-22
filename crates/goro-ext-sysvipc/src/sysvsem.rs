//! ext/sysvsem (sysvsem.c): System V semaphores.

use std::cell::Cell;

use goro_core::value::Value;
use goro_core::vm::Vm;

use crate::zpp;
use crate::{R, errno, native, new_object, strerror, warn, xlong};

const CLASS: &str = "SysvSemaphore";

const SYSVSEM_SEM: u16 = 0;
const SYSVSEM_USAGE: u16 = 1;
const SYSVSEM_SETVAL: u16 = 2;

const FUNCTIONS: &[(&str, goro_core::vm::BuiltinFn)] = &[
    ("sem_get", sem_get),
    ("sem_acquire", sem_acquire),
    ("sem_release", sem_release),
    ("sem_remove", sem_remove),
];

pub fn register(vm: &mut Vm) {
    if crate::register_ext(vm, b"sysvsem", FUNCTIONS, &[]) {
        crate::register_class(
            vm,
            CLASS.as_bytes(),
            "Cannot directly construct SysvSemaphore, use sem_get() instead",
        );
    }
}

/// sysvsem_sem
struct Sem {
    key: i32,
    semid: i32,
    count: Cell<i32>,
    auto_release: bool,
}

impl Drop for Sem {
    /// sysvsem_free_obj()
    fn drop(&mut self) {
        let count = self.count.get();
        if count == -1 || !self.auto_release {
            return;
        }
        let mut sop = [
            libc::sembuf {
                sem_num: SYSVSEM_USAGE,
                sem_op: -1,
                sem_flg: libc::SEM_UNDO as i16,
            },
            libc::sembuf {
                sem_num: SYSVSEM_SEM,
                sem_op: count as i16,
                sem_flg: libc::SEM_UNDO as i16,
            },
        ];
        let n = if count != 0 { 2 } else { 1 };
        unsafe {
            libc::semop(self.semid, sop.as_mut_ptr(), n);
        }
    }
}

fn semop_retry(semid: i32, sop: &mut [libc::sembuf]) -> bool {
    loop {
        if unsafe { libc::semop(semid, sop.as_mut_ptr(), sop.len()) } != -1 {
            return true;
        }
        if errno() != libc::EINTR {
            return false;
        }
    }
}

fn sem_get(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sem_get";
    let Ok(a) = zpp::parse(vm, f, CLASS, "l|llb", args) else {
        return Ok(Value::Null);
    };
    let key = a[0].long_or(0);
    let max_acquire = a[1].long_or(1);
    let perm = a[2].long_or(0o666);
    let auto_release = a[3].bool_or(true);
    let semid = unsafe { libc::semget(key as libc::key_t, 3, perm as i32 | libc::IPC_CREAT) };
    if semid == -1 {
        let e = errno();
        warn(
            vm,
            f,
            &format!("Failed for key 0x{}: {}", xlong(key), strerror(e)),
        );
        return Ok(Value::False);
    }
    // Wait for sem 1 to be zero, increment it and the usage count.
    let mut sop = [
        libc::sembuf {
            sem_num: SYSVSEM_SETVAL,
            sem_op: 0,
            sem_flg: 0,
        },
        libc::sembuf {
            sem_num: SYSVSEM_SETVAL,
            sem_op: 1,
            sem_flg: libc::SEM_UNDO as i16,
        },
        libc::sembuf {
            sem_num: SYSVSEM_USAGE,
            sem_op: 1,
            sem_flg: libc::SEM_UNDO as i16,
        },
    ];
    if !semop_retry(semid, &mut sop) {
        let e = errno();
        warn(
            vm,
            f,
            &format!(
                "Failed acquiring SYSVSEM_SETVAL for key 0x{}: {}",
                xlong(key),
                strerror(e)
            ),
        );
    }
    let count = unsafe { libc::semctl(semid, SYSVSEM_USAGE as i32, libc::GETVAL) };
    if count == -1 {
        let e = errno();
        warn(
            vm,
            f,
            &format!("Failed for key 0x{}: {}", xlong(key), strerror(e)),
        );
    }
    // If we are the only user, set the max.
    if count == 1
        && unsafe {
            libc::semctl(
                semid,
                SYSVSEM_SEM as i32,
                libc::SETVAL,
                max_acquire as libc::c_int,
            )
        } == -1
    {
        let e = errno();
        warn(
            vm,
            f,
            &format!("Failed for key 0x{}: {}", xlong(key), strerror(e)),
        );
    }
    // Set semaphore 1 back to zero.
    let mut sop = [libc::sembuf {
        sem_num: SYSVSEM_SETVAL,
        sem_op: -1,
        sem_flg: libc::SEM_UNDO as i16,
    }];
    if !semop_retry(semid, &mut sop) {
        let e = errno();
        warn(
            vm,
            f,
            &format!(
                "Failed releasing SYSVSEM_SETVAL for key 0x{}: {}",
                xlong(key),
                strerror(e)
            ),
        );
    }
    let sem = Sem {
        key: key as i32,
        semid,
        count: Cell::new(0),
        auto_release,
    };
    Ok(new_object(vm, CLASS.as_bytes(), sem))
}

/// php_sysvsem_semop()
fn semop(vm: &mut Vm, args: &[Value], acquire: bool) -> R {
    let f = if acquire {
        "sem_acquire"
    } else {
        "sem_release"
    };
    let spec = if acquire { "O|b" } else { "O" };
    let Ok(a) = zpp::parse(vm, f, CLASS, spec, args) else {
        return Ok(Value::Null);
    };
    let Some(sem) = native::<Sem>(&a[0]) else {
        return Ok(Value::Null);
    };
    let nowait = acquire && a[1].bool_or(false);
    if !acquire && sem.count.get() == 0 {
        warn(
            vm,
            f,
            &format!(
                "SysV semaphore for key 0x{:x} is not currently acquired",
                sem.key
            ),
        );
        return Ok(Value::False);
    }
    let mut sop = [libc::sembuf {
        sem_num: SYSVSEM_SEM,
        sem_op: if acquire { -1 } else { 1 },
        sem_flg: (libc::SEM_UNDO | if nowait { libc::IPC_NOWAIT } else { 0 }) as i16,
    }];
    if !semop_retry(sem.semid, &mut sop) {
        let e = errno();
        if e != libc::EAGAIN {
            warn(
                vm,
                f,
                &format!(
                    "Failed to {} key 0x{:x}: {}",
                    if acquire { "acquire" } else { "release" },
                    sem.key,
                    strerror(e)
                ),
            );
        }
        return Ok(Value::False);
    }
    sem.count
        .set(sem.count.get() - if acquire { -1 } else { 1 });
    Ok(Value::True)
}

fn sem_acquire(vm: &mut Vm, args: &[Value]) -> R {
    semop(vm, args, true)
}

fn sem_release(vm: &mut Vm, args: &[Value]) -> R {
    semop(vm, args, false)
}

fn sem_remove(vm: &mut Vm, args: &[Value]) -> R {
    let f = "sem_remove";
    let Ok(a) = zpp::parse(vm, f, CLASS, "O", args) else {
        return Ok(Value::Null);
    };
    let Some(sem) = native::<Sem>(&a[0]) else {
        return Ok(Value::Null);
    };
    let mut buf: libc::semid_ds = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::semctl(
            sem.semid,
            0,
            libc::IPC_STAT,
            &mut buf as *mut libc::semid_ds,
        )
    } < 0
    {
        warn(
            vm,
            f,
            &format!(
                "SysV semaphore for key 0x{:x} does not (any longer) exist",
                sem.key
            ),
        );
        return Ok(Value::False);
    }
    if unsafe {
        libc::semctl(
            sem.semid,
            0,
            libc::IPC_RMID,
            &mut buf as *mut libc::semid_ds,
        )
    } < 0
    {
        let e = errno();
        warn(
            vm,
            f,
            &format!(
                "Failed for SysV semaphore for key 0x{:x}: {}",
                sem.key,
                strerror(e)
            ),
        );
        return Ok(Value::False);
    }
    sem.count.set(-1);
    Ok(Value::True)
}
