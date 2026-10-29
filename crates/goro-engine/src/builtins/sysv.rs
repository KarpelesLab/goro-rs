//! System V IPC: ext/shmop, ext/sysvsem, ext/sysvshm and ext/sysvmsg
//! (PHP manual: Shared Memory, Semaphore functions) over the kernel's
//! shmget/semget/msgget calls. Messages and checks follow the extensions'
//! tests and PHP 8.5's observed output.
//!
//! The layouts inside the IPC objects are goro's own (PHP's are not
//! documented): a sysvshm segment holds a small header and a list of
//! (key, serialized value) records; a sysvsem semaphore is a set of three
//! (the semaphore, a user count and an initialisation lock).

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::class::NativeHooks;
use crate::func::Args;
use crate::registry::{ConstVal, Module};
use crate::vm::{R, Vm};

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn strerror(e: i32) -> String {
    super::exec::strerror(e)
}

fn with_native<T: 'static, U>(o: &ObjRef, f: impl FnOnce(&mut T) -> U) -> Option<U> {
    let mut n = o.obj().native.borrow_mut();
    let any: &mut dyn std::any::Any = &mut **n.as_mut()?;
    any.downcast_mut::<T>().map(f)
}

fn new_obj(vm: &mut Vm, class: &str, data: Box<dyn goro_runtime::NativeData>) -> R<Value> {
    let Some(cls) = vm.find_class(class.as_bytes(), false)? else {
        return Err(vm.fatal(&format!("Class \"{class}\" not found")));
    };
    let o = vm.instantiate_raw(&cls)?;
    *o.obj().native.borrow_mut() = Some(data);
    Ok(Value::Object(o))
}

fn no_hooks() -> NativeHooks {
    NativeHooks::default()
}

// ================================================================ shmop

pub static SHMOP: Module = Module {
    name: "shmop",
    functions: &[
        (
            "function shmop_open(int $key, string $mode, int $permissions, int $size): Shmop|false",
            shmop_open,
        ),
        (
            "function shmop_read(Shmop $shmop, int $offset, int $size): string",
            shmop_read,
        ),
        ("function shmop_close(Shmop $shmop): void", shmop_close),
        ("function shmop_size(Shmop $shmop): int", shmop_size),
        (
            "function shmop_write(Shmop $shmop, string $data, int $offset): int",
            shmop_write,
        ),
        ("function shmop_delete(Shmop $shmop): bool", shmop_delete),
    ],
    classes: &[(
        "shmop",
        "<?php #[NotSerializable] #[NotCloneable] final class Shmop {
            public function __construct() { throw new Error('Cannot directly construct Shmop, use shmop_open() instead'); }
        }",
    )],
    hooks: &[("shmop", no_hooks)],
    listed: true,
    ..Module::EMPTY
};

struct Shm {
    id: i32,
    addr: *mut u8,
    size: usize,
    read_only: bool,
}

impl goro_runtime::NativeData for Shm {}

impl Drop for Shm {
    fn drop(&mut self) {
        if !self.addr.is_null() {
            // SAFETY: an address returned by shmat().
            unsafe { libc::shmdt(self.addr as *const libc::c_void) };
        }
    }
}

fn shm_obj<U>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut Shm) -> U) -> R<U> {
    match a.obj(0).and_then(|o| with_native::<Shm, U>(&o, f)) {
        Some(u) => Ok(u),
        None => vm.fthrow(a, "Error", "Shmop object is not initialized"),
    }
}

fn shmop_open(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(0) as libc::key_t;
    let mode = a.str(1);
    let perms = a.int(2) as libc::c_int;
    let size = a.int(3);
    let (flags, read_only) = match mode.as_bytes() {
        b"a" => (0, true),
        b"c" => (libc::IPC_CREAT, false),
        b"n" => (libc::IPC_CREAT | libc::IPC_EXCL, false),
        b"w" => (0, false),
        _ => {
            return vm.fthrow(
                a,
                "ValueError",
                "Argument #2 ($mode) must be a valid access mode",
            );
        }
    };
    if flags & libc::IPC_CREAT != 0 && size < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($size) must be greater than 0 for the \"c\" and \"n\" access modes",
        );
    }
    let req = if flags & libc::IPC_CREAT != 0 {
        size as usize
    } else {
        0
    };
    // SAFETY: plain System V calls.
    let id = unsafe { libc::shmget(key, req, flags | perms) };
    if id < 0 {
        let e = errno();
        vm.fwarn(
            a,
            &format!(
                "Unable to attach or create shared memory segment \"{}\"",
                strerror(e)
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let mut ds: libc::shmid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::shmctl(id, libc::IPC_STAT, &mut ds) } != 0 {
        let e = errno();
        vm.fwarn(
            a,
            &format!(
                "Unable to get shared memory segment information \"{}\"",
                strerror(e)
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let addr = unsafe {
        libc::shmat(
            id,
            std::ptr::null(),
            if read_only { libc::SHM_RDONLY } else { 0 },
        )
    };
    if addr as isize == -1 {
        let e = errno();
        vm.fwarn(
            a,
            &format!(
                "Unable to attach to shared memory segment \"{}\"",
                strerror(e)
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    new_obj(
        vm,
        "Shmop",
        Box::new(Shm {
            id,
            addr: addr as *mut u8,
            size: ds.shm_segsz,
            read_only,
        }),
    )
}

fn shmop_read(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let off = a.int(1);
    let n = a.int(2);
    let (addr, size) = shm_obj(vm, a, |s| (s.addr, s.size))?;
    if off < 0 || off as u64 > size as u64 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($offset) must be between 0 and the segment size",
        );
    }
    if n < 0 || (off as u64).saturating_add(n as u64) > size as u64 {
        return vm.fthrow(a, "ValueError", "Argument #3 ($size) is out of range");
    }
    // SAFETY: the range lies inside the attached segment.
    let b = unsafe { std::slice::from_raw_parts(addr.add(off as usize), n as usize) };
    Ok(Value::Str(PhpStr::from(b)))
}

fn shmop_close(vm: &mut Vm, _a: &mut Args) -> R<Value> {
    let _ = vm;
    Ok(Value::Null)
}

fn shmop_size(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let size = shm_obj(vm, a, |s| s.size)?;
    Ok(Value::Int(size as i64))
}

fn shmop_write(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let data = a.str(1);
    let off = a.int(2);
    let (addr, size, ro) = shm_obj(vm, a, |s| (s.addr, s.size, s.read_only))?;
    if ro {
        return vm.throw("Error", "Read-only segment cannot be written");
    }
    if off < 0 || off as u64 > size as u64 {
        return vm.fthrow(a, "ValueError", "Argument #3 ($offset) is out of range");
    }
    let n = data.len().min(size - off as usize);
    // SAFETY: the range lies inside the attached, writable segment.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), addr.add(off as usize), n) };
    Ok(Value::Int(n as i64))
}

fn shmop_delete(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = shm_obj(vm, a, |s| s.id)?;
    if unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) } != 0 {
        vm.fwarn(a, "Can't mark segment for deletion (are you the owner?)")?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

// ============================================================== sysvsem

pub static SYSVSEM: Module = Module {
    name: "sysvsem",
    functions: &[
        (
            "function sem_get(int $key, int $max_acquire = 1, int $permissions = 0666, bool $auto_release = true): SysvSemaphore|false",
            sem_get,
        ),
        (
            "function sem_acquire(SysvSemaphore $semaphore, bool $non_blocking = false): bool",
            sem_acquire,
        ),
        (
            "function sem_release(SysvSemaphore $semaphore): bool",
            sem_release,
        ),
        (
            "function sem_remove(SysvSemaphore $semaphore): bool",
            sem_remove,
        ),
    ],
    classes: &[(
        "sysvsemaphore",
        "<?php #[NotSerializable] #[NotCloneable] final class SysvSemaphore {
            public function __construct() { throw new Error('Cannot directly construct SysvSemaphore, use sem_get() instead'); }
        }",
    )],
    hooks: &[("sysvsemaphore", no_hooks)],
    listed: true,
    ..Module::EMPTY
};

const SEM_VALUE: u16 = 0;
const SEM_USERS: u16 = 1;
const SEM_LOCK: u16 = 2;

struct Sem {
    key: i64,
    id: i32,
    /// Acquisitions held by this object.
    count: i64,
    auto_release: bool,
}

impl goro_runtime::NativeData for Sem {}

fn semop(id: i32, ops: &mut [libc::sembuf]) -> bool {
    // SAFETY: a valid operation array.
    unsafe { libc::semop(id, ops.as_mut_ptr(), ops.len()) == 0 }
}

fn op(num: u16, delta: i16, flags: i16) -> libc::sembuf {
    libc::sembuf {
        sem_num: num,
        sem_op: delta,
        sem_flg: flags,
    }
}

impl Drop for Sem {
    fn drop(&mut self) {
        // Releases what this object still holds (PHP manual: sem_get,
        // auto_release), and leaves the user count.
        if self.auto_release {
            for _ in 0..self.count {
                if !semop(self.id, &mut [op(SEM_VALUE, 1, libc::SEM_UNDO as i16)]) {
                    break;
                }
            }
        }
        semop(
            self.id,
            &mut [op(
                SEM_USERS,
                -1,
                (libc::SEM_UNDO | libc::IPC_NOWAIT) as i16,
            )],
        );
    }
}

fn sem_obj<U>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut Sem) -> U) -> R<U> {
    match a.obj(0).and_then(|o| with_native::<Sem, U>(&o, f)) {
        Some(u) => Ok(u),
        None => vm.fthrow(a, "Error", "SysvSemaphore object is not initialized"),
    }
}

fn sem_get(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(0);
    let max = a.int_or(1, 1);
    let perms = a.int_or(2, 0o666) as libc::c_int;
    let auto = a.bool_or(3, true);
    let id = unsafe { libc::semget(key as libc::key_t, 3, perms | libc::IPC_CREAT) };
    if id < 0 {
        let e = errno();
        vm.fwarn(a, &format!("Failed for key 0x{:x}: {}", key, strerror(e)))?;
        return Ok(Value::Bool(false));
    }
    // Initialisation under the lock semaphore: the first user sets the
    // value to max_acquire.
    let locked = loop {
        if semop(
            id,
            &mut [op(SEM_LOCK, 0, 0), op(SEM_LOCK, 1, libc::SEM_UNDO as i16)],
        ) {
            break true;
        }
        if errno() != libc::EINTR {
            break false;
        }
    };
    if !locked {
        let e = errno();
        vm.fwarn(
            a,
            &format!(
                "Failed acquiring SYSVSEM_SETVAL for key 0x{:x}: {}",
                key,
                strerror(e)
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    let users = unsafe { libc::semctl(id, SEM_USERS as libc::c_int, libc::GETVAL) };
    let mut ok = true;
    if users == 0
        && unsafe {
            libc::semctl(
                id,
                SEM_VALUE as libc::c_int,
                libc::SETVAL,
                max as libc::c_int,
            )
        } != 0
    {
        let e = errno();
        vm.fwarn(a, &format!("Failed for key 0x{:x}: {}", key, strerror(e)))?;
        ok = false;
    }
    if ok {
        semop(id, &mut [op(SEM_USERS, 1, libc::SEM_UNDO as i16)]);
    }
    semop(id, &mut [op(SEM_LOCK, -1, libc::SEM_UNDO as i16)]);
    if !ok {
        return Ok(Value::Bool(false));
    }
    new_obj(
        vm,
        "SysvSemaphore",
        Box::new(Sem {
            key,
            id,
            count: 0,
            auto_release: auto,
        }),
    )
}

fn sem_acquire(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let nb = a.bool_or(1, false);
    let (id, key) = sem_obj(vm, a, |s| (s.id, s.key))?;
    vm.out.flush();
    let flags = libc::SEM_UNDO | if nb { libc::IPC_NOWAIT } else { 0 };
    let ok = loop {
        if semop(id, &mut [op(SEM_VALUE, -1, flags as i16)]) {
            break true;
        }
        if errno() != libc::EINTR {
            break false;
        }
    };
    if !ok {
        let e = errno();
        if !(nb && e == libc::EAGAIN) {
            vm.fwarn(
                a,
                &format!("Failed to acquire key 0x{:x}: {}", key, strerror(e)),
            )?;
        }
        return Ok(Value::Bool(false));
    }
    sem_obj(vm, a, |s| s.count += 1)?;
    Ok(Value::Bool(true))
}

fn sem_release(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (id, key, count) = sem_obj(vm, a, |s| (s.id, s.key, s.count))?;
    if count == 0 {
        vm.fwarn(
            a,
            &format!(
                "SysV semaphore for key 0x{:x} is not currently acquired",
                key
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    if !semop(id, &mut [op(SEM_VALUE, 1, libc::SEM_UNDO as i16)]) {
        let e = errno();
        vm.fwarn(
            a,
            &format!("Failed to release key 0x{:x}: {}", key, strerror(e)),
        )?;
        return Ok(Value::Bool(false));
    }
    sem_obj(vm, a, |s| s.count -= 1)?;
    Ok(Value::Bool(true))
}

fn sem_remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (id, key) = sem_obj(vm, a, |s| (s.id, s.key))?;
    let mut buf: libc::semid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::semctl(id, 0, libc::IPC_STAT, &mut buf as *mut libc::semid_ds) } < 0
        || unsafe { libc::semctl(id, 0, libc::IPC_RMID) } < 0
    {
        vm.fwarn(
            a,
            &format!(
                "SysV semaphore for key 0x{:x} does not (any longer) exist",
                key
            ),
        )?;
        return Ok(Value::Bool(false));
    }
    sem_obj(vm, a, |s| s.count = 0)?;
    Ok(Value::Bool(true))
}

// ============================================================== sysvshm

pub static SYSVSHM: Module = Module {
    name: "sysvshm",
    functions: &[
        (
            "function shm_attach(int $key, ?int $size = null, int $permissions = 0666): SysvSharedMemory|false",
            shm_attach,
        ),
        ("function shm_detach(SysvSharedMemory $shm): true", shm_detach),
        (
            "function shm_has_var(SysvSharedMemory $shm, int $key): bool",
            shm_has_var,
        ),
        ("function shm_remove(SysvSharedMemory $shm): bool", shm_remove),
        (
            "function shm_put_var(SysvSharedMemory $shm, int $key, mixed $value): bool",
            shm_put_var,
        ),
        (
            "function shm_get_var(SysvSharedMemory $shm, int $key): mixed",
            shm_get_var,
        ),
        (
            "function shm_remove_var(SysvSharedMemory $shm, int $key): bool",
            shm_remove_var,
        ),
    ],
    classes: &[(
        "sysvsharedmemory",
        "<?php #[NotSerializable] #[NotCloneable] final class SysvSharedMemory {
            public function __construct() { throw new Error('Cannot directly construct SysvSharedMemory, use shm_attach() instead'); }
        }",
    )],
    hooks: &[("sysvsharedmemory", no_hooks)],
    ini: &[("sysvshm.init_mem", "10000")],
    listed: true,
    ..Module::EMPTY
};

/// Segment header: magic, used bytes (records end), segment size.
const SHM_MAGIC: u64 = 0x676f_726f_7368_6d31;
const SHM_HEADER: usize = 24;

struct Sysv {
    key: i64,
    id: i32,
    /// Null once detached.
    addr: *mut u8,
    size: usize,
}

impl goro_runtime::NativeData for Sysv {}

impl Drop for Sysv {
    fn drop(&mut self) {
        if !self.addr.is_null() {
            unsafe { libc::shmdt(self.addr as *const libc::c_void) };
        }
    }
}

impl Sysv {
    // The segment is memory shared with other processes, not data owned
    // by this object.
    #[allow(clippy::mut_from_ref)]
    fn mem(&self) -> &mut [u8] {
        // SAFETY: the attached segment of `size` bytes.
        unsafe { std::slice::from_raw_parts_mut(self.addr, self.size) }
    }
    fn used(&self) -> usize {
        let m = self.mem();
        (u64::from_ne_bytes(m[8..16].try_into().unwrap()) as usize).clamp(SHM_HEADER, self.size)
    }
    fn set_used(&self, n: usize) {
        self.mem()[8..16].copy_from_slice(&(n as u64).to_ne_bytes());
    }
    /// (record offset, record length, value offset, value length) of `key`.
    fn find(&self, key: i64) -> Option<(usize, usize, usize, usize)> {
        let m = self.mem();
        let end = self.used();
        let mut p = SHM_HEADER;
        while p + 16 <= end {
            let k = i64::from_ne_bytes(m[p..p + 8].try_into().unwrap());
            let len = u64::from_ne_bytes(m[p + 8..p + 16].try_into().unwrap()) as usize;
            let rec = 16 + len;
            if p + rec > end {
                return None;
            }
            if k == key {
                return Some((p, rec, p + 16, len));
            }
            p += rec;
        }
        None
    }
    fn remove(&self, key: i64) -> bool {
        let Some((p, rec, _, _)) = self.find(key) else {
            return false;
        };
        let end = self.used();
        self.mem().copy_within(p + rec..end, p);
        self.set_used(end - rec);
        true
    }
}

fn sysv_obj<U>(vm: &mut Vm, a: &Args, f: impl FnOnce(&mut Sysv) -> U) -> R<U> {
    let r = a.obj(0).and_then(|o| {
        with_native::<Sysv, Option<U>>(&o, |s| if s.addr.is_null() { None } else { Some(f(s)) })
    });
    match r {
        Some(Some(u)) => Ok(u),
        _ => vm.throw("Error", "Shared memory block has already been destroyed"),
    }
}

fn shm_attach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(0);
    let size = match a.opt_int(1) {
        Some(s) => s,
        None => vm
            .get_ini(b"sysvshm.init_mem")
            .map(|v| v.to_string_lossy().trim().parse().unwrap_or(10000))
            .unwrap_or(10000),
    };
    let perms = a.int_or(2, 0o666) as libc::c_int;
    if size < 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($size) must be greater than 0",
        );
    }
    let k = key as libc::key_t;
    let mut id = unsafe { libc::shmget(k, 0, 0) };
    if id < 0 {
        if (size as usize) < SHM_HEADER {
            vm.fwarn(
                a,
                &format!("Failed for key 0x{:x}: memorysize too small", key),
            )?;
            return Ok(Value::Bool(false));
        }
        id = unsafe { libc::shmget(k, size as usize, libc::IPC_CREAT | libc::IPC_EXCL | perms) };
        if id < 0 {
            let e = errno();
            vm.fwarn(a, &format!("Failed for key 0x{:x}: {}", key, strerror(e)))?;
            return Ok(Value::Bool(false));
        }
    }
    let mut ds: libc::shmid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::shmctl(id, libc::IPC_STAT, &mut ds) } != 0 {
        let e = errno();
        vm.fwarn(a, &format!("Failed for key 0x{:x}: {}", key, strerror(e)))?;
        return Ok(Value::Bool(false));
    }
    let addr = unsafe { libc::shmat(id, std::ptr::null(), 0) };
    if addr as isize == -1 {
        let e = errno();
        vm.fwarn(a, &format!("Failed for key 0x{:x}: {}", key, strerror(e)))?;
        return Ok(Value::Bool(false));
    }
    let s = Sysv {
        key,
        id,
        addr: addr as *mut u8,
        size: ds.shm_segsz,
    };
    if s.size < SHM_HEADER {
        vm.fwarn(
            a,
            &format!("Failed for key 0x{:x}: memorysize too small", key),
        )?;
        return Ok(Value::Bool(false));
    }
    let m = s.mem();
    if u64::from_ne_bytes(m[0..8].try_into().unwrap()) != SHM_MAGIC {
        m[0..8].copy_from_slice(&SHM_MAGIC.to_ne_bytes());
        m[16..24].copy_from_slice(&(s.size as u64).to_ne_bytes());
        s.set_used(SHM_HEADER);
    }
    new_obj(vm, "SysvSharedMemory", Box::new(s))
}

fn shm_detach(vm: &mut Vm, a: &mut Args) -> R<Value> {
    sysv_obj(vm, a, |s| {
        unsafe { libc::shmdt(s.addr as *const libc::c_void) };
        s.addr = std::ptr::null_mut();
    })?;
    Ok(Value::Bool(true))
}

fn shm_remove(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (id, key) = sysv_obj(vm, a, |s| (s.id, s.key))?;
    if unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) } < 0 {
        let e = errno();
        vm.fwarn(
            a,
            &format!("Failed for key 0x{:x}, id {}: {}", key, id, strerror(e)),
        )?;
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(true))
}

fn shm_has_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(1);
    let r = sysv_obj(vm, a, |s| s.find(key).is_some())?;
    Ok(Value::Bool(r))
}

fn shm_put_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(1);
    sysv_obj(vm, a, |_| ())?;
    let data = vm.serialize_value(&a.val(2))?;
    let r = a.obj(0).and_then(|o| {
        with_native::<Sysv, Option<bool>>(&o, |s| {
            if s.addr.is_null() {
                return None;
            }
            s.remove(key);
            let end = s.used();
            let rec = 16 + data.len();
            if end + rec > s.size {
                return Some(false);
            }
            let m = s.mem();
            m[end..end + 8].copy_from_slice(&key.to_ne_bytes());
            m[end + 8..end + 16].copy_from_slice(&(data.len() as u64).to_ne_bytes());
            m[end + 16..end + rec].copy_from_slice(&data);
            s.set_used(end + rec);
            Some(true)
        })
    });
    match r.flatten() {
        Some(true) => Ok(Value::Bool(true)),
        Some(false) => {
            vm.fwarn(a, "Not enough shared memory left")?;
            Ok(Value::Bool(false))
        }
        None => vm.throw(
            "Error",
            "Shared memory block has been destroyed by the serialization function",
        ),
    }
}

fn shm_get_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(1);
    let data = sysv_obj(vm, a, |s| {
        s.find(key)
            .map(|(_, _, off, len)| s.mem()[off..off + len].to_vec())
    })?;
    let Some(data) = data else {
        vm.fwarn(a, &format!("Variable key {} doesn't exist", key))?;
        return Ok(Value::Bool(false));
    };
    match vm.unserialize_quiet(&data) {
        Some(v) => Ok(v),
        None => {
            vm.fwarn(a, "Variable data in shared memory is corrupted")?;
            Ok(Value::Bool(false))
        }
    }
}

fn shm_remove_var(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(1);
    let ok = sysv_obj(vm, a, |s| s.remove(key))?;
    if !ok {
        vm.fwarn(a, &format!("Variable key {} doesn't exist", key))?;
    }
    Ok(Value::Bool(ok))
}

// ============================================================== sysvmsg

pub static SYSVMSG: Module = Module {
    name: "sysvmsg",
    functions: &[
        (
            "function msg_get_queue(int $key, int $permissions = 0666): SysvMessageQueue|false",
            msg_get_queue,
        ),
        (
            "function msg_send(SysvMessageQueue $queue, int $message_type, $message, bool $serialize = true, bool $blocking = true, &$error_code = null): bool",
            msg_send,
        ),
        (
            "function msg_receive(SysvMessageQueue $queue, int $desired_message_type, &$received_message_type, int $max_message_size, mixed &$message, bool $unserialize = true, int $flags = 0, &$error_code = null): bool",
            msg_receive,
        ),
        (
            "function msg_remove_queue(SysvMessageQueue $queue): bool",
            msg_remove_queue,
        ),
        (
            "function msg_stat_queue(SysvMessageQueue $queue): array|false",
            msg_stat_queue,
        ),
        (
            "function msg_set_queue(SysvMessageQueue $queue, array $data): bool",
            msg_set_queue,
        ),
        ("function msg_queue_exists(int $key): bool", msg_queue_exists),
    ],
    classes: &[(
        "sysvmessagequeue",
        "<?php #[NotSerializable] #[NotCloneable] final class SysvMessageQueue {
            public function __construct() { throw new Error('Cannot directly construct SysvMessageQueue, use msg_get_queue() instead'); }
        }",
    )],
    hooks: &[("sysvmessagequeue", no_hooks)],
    constants: &[
        ("MSG_IPC_NOWAIT", ConstVal::Int(1)),
        ("MSG_EAGAIN", ConstVal::Int(libc::EAGAIN as i64)),
        ("MSG_ENOMSG", ConstVal::Int(libc::ENOMSG as i64)),
        ("MSG_NOERROR", ConstVal::Int(2)),
        ("MSG_EXCEPT", ConstVal::Int(4)),
    ],
    listed: true,
    ..Module::EMPTY
};

struct Queue {
    id: i32,
}

impl goro_runtime::NativeData for Queue {}

fn queue_id(vm: &mut Vm, a: &Args) -> R<i32> {
    match a
        .obj(0)
        .and_then(|o| with_native::<Queue, i32>(&o, |q| q.id))
    {
        Some(id) => Ok(id),
        None => vm.fthrow(a, "Error", "SysvMessageQueue object is not initialized"),
    }
}

fn msg_get_queue(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let key = a.int(0);
    let perms = a.int_or(1, 0o666) as libc::c_int;
    let id = unsafe { libc::msgget(key as libc::key_t, 0) };
    let id = if id < 0 {
        unsafe { libc::msgget(key as libc::key_t, libc::IPC_CREAT | libc::IPC_EXCL | perms) }
    } else {
        id
    };
    if id < 0 {
        let e = errno();
        vm.fwarn(a, &format!("Failed for key 0x{:x}: {}", key, strerror(e)))?;
        return Ok(Value::Bool(false));
    }
    new_obj(vm, "SysvMessageQueue", Box::new(Queue { id }))
}

fn msg_queue_exists(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Bool(
        unsafe { libc::msgget(a.int(0) as libc::key_t, 0) } >= 0,
    ))
}

fn msg_remove_queue(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = queue_id(vm, a)?;
    Ok(Value::Bool(
        unsafe { libc::msgctl(id, libc::IPC_RMID, std::ptr::null_mut()) } == 0,
    ))
}

fn msg_stat_queue(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = queue_id(vm, a)?;
    let mut ds: libc::msqid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::msgctl(id, libc::IPC_STAT, &mut ds) } != 0 {
        return Ok(Value::Bool(false));
    }
    let mut arr = PhpArray::new();
    let mut put = |k: &str, v: i64| {
        arr.insert(Key::Str(PhpStr::from(k)), Value::Int(v));
    };
    put("msg_perm.uid", ds.msg_perm.uid as i64);
    put("msg_perm.gid", ds.msg_perm.gid as i64);
    put("msg_perm.mode", ds.msg_perm.mode as i64);
    put("msg_stime", ds.msg_stime);
    put("msg_rtime", ds.msg_rtime);
    put("msg_ctime", ds.msg_ctime);
    put("msg_qnum", ds.msg_qnum as i64);
    put("msg_qbytes", ds.msg_qbytes as i64);
    put("msg_lspid", ds.msg_lspid as i64);
    put("msg_lrpid", ds.msg_lrpid as i64);
    Ok(Value::Array(arr))
}

fn msg_set_queue(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = queue_id(vm, a)?;
    let data = a.array(1);
    let mut ds: libc::msqid_ds = unsafe { std::mem::zeroed() };
    if unsafe { libc::msgctl(id, libc::IPC_STAT, &mut ds) } != 0 {
        return Ok(Value::Bool(false));
    }
    let get = |k: &str| {
        data.get_str(&PhpStr::from(k)).map(|v| match v.deref() {
            Value::Int(i) => i,
            Value::Float(f) => f as i64,
            Value::Bool(b) => b as i64,
            Value::Str(s) => s.to_string_lossy().trim().parse().unwrap_or(0),
            _ => 0,
        })
    };
    if let Some(v) = get("msg_perm.uid") {
        ds.msg_perm.uid = v as libc::uid_t;
    }
    if let Some(v) = get("msg_perm.gid") {
        ds.msg_perm.gid = v as libc::gid_t;
    }
    if let Some(v) = get("msg_perm.mode") {
        ds.msg_perm.mode = v as _;
    }
    if let Some(v) = get("msg_qbytes") {
        ds.msg_qbytes = v as _;
    }
    Ok(Value::Bool(
        unsafe { libc::msgctl(id, libc::IPC_SET, &mut ds) } == 0,
    ))
}

/// msg_send(): without serialization, scalars are sent as their string
/// form (test sysvmsg/006.phpt).
fn msg_send(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = queue_id(vm, a)?;
    let mtype = a.int(1);
    let msg = a.val(2);
    let ser = a.bool_or(3, true);
    let blocking = a.bool_or(4, true);
    let body: Vec<u8> = if ser {
        vm.serialize_value(&msg)?
    } else {
        match &msg {
            Value::Str(s) => s.to_vec(),
            // Floats in their shortest round-trip form (the comparison of
            // test 006.phpt holds for PHP_INT_MAX + 1).
            Value::Float(f) => goro_runtime::convert::float_repr(*f).into_bytes(),
            Value::Int(_) | Value::Bool(_) => vm.to_php_str(&msg)?.to_vec(),
            other => {
                return vm.fthrow(
                    a,
                    "TypeError",
                    &format!(
                        "Argument #3 ($message) must be of type string|int|float|bool, {} given",
                        Vm::type_name_given(other)
                    ),
                );
            }
        }
    };
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend_from_slice(&(mtype as libc::c_long).to_ne_bytes());
    buf.extend_from_slice(&body);
    let flags = if blocking { 0 } else { libc::IPC_NOWAIT };
    vm.out.flush();
    let r = loop {
        let r = unsafe { libc::msgsnd(id, buf.as_ptr() as *const libc::c_void, body.len(), flags) };
        if r != 0 && errno() == libc::EINTR {
            continue;
        }
        break r;
    };
    if r != 0 {
        let e = errno();
        a.set_ref(5, Value::Int(e as i64));
        vm.fwarn(a, &format!("msgsnd failed: {}", strerror(e)))?;
        return Ok(Value::Bool(false));
    }
    a.set_ref(5, Value::Int(0));
    Ok(Value::Bool(true))
}

fn msg_receive(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = queue_id(vm, a)?;
    let want = a.int(1);
    let max = a.int(3);
    let unser = a.bool_or(5, true);
    let flags = a.int_or(6, 0);
    if max <= 0 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #4 ($max_message_size) must be greater than 0",
        );
    }
    let mut rflags = 0;
    if flags & 1 != 0 {
        rflags |= libc::IPC_NOWAIT;
    }
    if flags & 2 != 0 {
        rflags |= libc::MSG_NOERROR;
    }
    if flags & 4 != 0 {
        rflags |= libc::MSG_EXCEPT;
    }
    a.set_ref(2, Value::Int(0));
    a.set_ref(4, Value::Bool(false));
    let mut buf = vec![0u8; 8 + max as usize];
    vm.out.flush();
    let n = loop {
        let n = unsafe {
            libc::msgrcv(
                id,
                buf.as_mut_ptr() as *mut libc::c_void,
                max as usize,
                want as libc::c_long,
                rflags,
            )
        };
        if n < 0 && errno() == libc::EINTR {
            continue;
        }
        break n;
    };
    if n < 0 {
        a.set_ref(7, Value::Int(errno() as i64));
        return Ok(Value::Bool(false));
    }
    a.set_ref(7, Value::Int(0));
    let mtype = libc::c_long::from_ne_bytes(buf[..8].try_into().unwrap());
    a.set_ref(2, Value::Int(mtype as i64));
    let body = &buf[8..8 + n as usize];
    if unser {
        match vm.unserialize_quiet(body) {
            Some(v) => a.set_ref(4, v),
            None => {
                vm.fwarn(a, "Message corrupted")?;
                return Ok(Value::Bool(false));
            }
        }
    } else {
        a.set_ref(4, Value::Str(PhpStr::from(body)));
    }
    Ok(Value::Bool(true))
}

// ================================================================ ftok

/// ftok() (PHP manual: ftok), part of the standard functions.
pub static FTOK: Module = Module {
    name: "standard",
    functions: &[(
        "function ftok(string $filename, string $project_id): int",
        ftok,
    )],
    listed: false,
    ..Module::EMPTY
};

fn ftok(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let f = a.str(0);
    let p = a.str(1);
    if f.is_empty() {
        return vm.fthrow(a, "ValueError", "Argument #1 ($filename) must not be empty");
    }
    if p.len() != 1 {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #2 ($project_id) must be a single character",
        );
    }
    let Ok(c) = std::ffi::CString::new(f.as_bytes()) else {
        return vm.fthrow(
            a,
            "ValueError",
            "Argument #1 ($filename) must not contain any null bytes",
        );
    };
    // The path is relative to the script's working directory.
    use std::os::unix::ffi::OsStrExt;
    let path = vm.cwd.join(std::ffi::OsStr::from_bytes(c.as_bytes()));
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap_or(c);
    let k = unsafe { libc::ftok(cpath.as_ptr(), p.as_bytes()[0] as libc::c_int) };
    if k == -1 {
        let e = errno();
        vm.fwarn(a, &format!("ftok() failed - {}", strerror(e)))?;
    }
    Ok(Value::Int(k as i64))
}
