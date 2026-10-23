//! sapi/fpm/fpm/fpm_scoreboard.c and fpm_shm.c: the per-pool statistics in
//! shared memory, written by the workers and read by the master and the
//! status page.

use std::sync::atomic::{AtomicI32, Ordering};

use super::zlog::{DEBUG, ERROR, WARNING};
use crate::zlog;

pub const ACTION_SET: i32 = 0;
pub const ACTION_INC: i32 = 1;
const SPINLOCK_MAX_RETRIES: i32 = 50000;

/// enum fpm_request_stage_e
pub const REQUEST_CREATING: i32 = 0;
pub const REQUEST_ACCEPTING: i32 = 1;
pub const REQUEST_READING_HEADERS: i32 = 2;
pub const REQUEST_INFO: i32 = 3;
pub const REQUEST_EXECUTING: i32 = 4;
pub const REQUEST_END: i32 = 5;
pub const REQUEST_FINISHED: i32 = 6;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Timeval {
    pub sec: i64,
    pub usec: i64,
}

impl Timeval {
    pub fn sub(&self, o: &Timeval) -> Timeval {
        let mut sec = self.sec - o.sec;
        let mut usec = self.usec - o.usec;
        if usec < 0 {
            sec -= 1;
            usec += 1_000_000;
        }
        Timeval { sec, usec }
    }
    pub fn is_set(&self) -> bool {
        self.sec != 0 || self.usec != 0
    }
    pub fn lt(&self, o: &Timeval) -> bool {
        if self.sec == o.sec {
            self.usec < o.usec
        } else {
            self.sec < o.sec
        }
    }
}

/// fpm_clock_get(): CLOCK_MONOTONIC
pub fn clock_get() -> Timeval {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    Timeval {
        sec: ts.tv_sec,
        usec: ts.tv_nsec / 1000,
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Tms {
    pub utime: i64,
    pub stime: i64,
    pub cutime: i64,
    pub cstime: i64,
}

pub fn times() -> Tms {
    let mut t: libc::tms = unsafe { std::mem::zeroed() };
    unsafe {
        libc::times(&mut t);
    }
    Tms {
        utime: t.tms_utime,
        stime: t.tms_stime,
        cutime: t.tms_cutime,
        cstime: t.tms_cstime,
    }
}

pub fn tick() -> f64 {
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) as f64 }
}

/// struct fpm_scoreboard_proc_s
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Proc {
    pub lock: i32,
    _pad: [u8; 12],
    pub used: i32,
    pub start_epoch: i64,
    pub pid: i32,
    pub requests: u64,
    pub request_stage: i32,
    pub accepted: Timeval,
    pub duration: Timeval,
    pub accepted_epoch: i64,
    pub tv: Timeval,
    pub request_uri: [u8; 512],
    pub query_string: [u8; 2048],
    pub request_method: [u8; 16],
    pub content_length: usize,
    pub script_filename: [u8; 512],
    pub auth_user: [u8; 32],
    pub cpu_accepted: Tms,
    pub cpu_duration: Timeval,
    pub last_request_cpu: Tms,
    pub last_request_cpu_duration: Timeval,
    pub memory: usize,
}

/// struct fpm_scoreboard_s (the procs follow it)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Scoreboard {
    pub lock: i32,
    _pad: [u8; 12],
    pub writer_active: i32,
    pub reader_count: u32,
    pub pool: [u8; 32],
    pub pm: i32,
    pub start_epoch: i64,
    pub idle: i32,
    pub active: i32,
    pub active_max: i32,
    pub requests: u64,
    pub max_children_reached: u32,
    pub lq: i32,
    pub lq_max: i32,
    pub lq_len: u32,
    pub nprocs: u32,
    pub free_proc: i32,
    pub slow_rq: u64,
    pub memory_peak: usize,
    pub shared: *mut Scoreboard,
}

/// A C string field.
pub fn cstr(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(z) => &b[..z],
        None => b,
    }
}

/// strlcpy()
pub fn strlcpy(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len() - 1);
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

static SHM_ALLOCATED: AtomicI32 = AtomicI32::new(0);

pub fn shm_size_allocated() -> usize {
    SHM_ALLOCATED.load(Ordering::Relaxed) as usize
}

/// fpm_shm_alloc()
fn shm_alloc(size: usize) -> *mut u8 {
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANONYMOUS | libc::MAP_SHARED,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        zlog!(
            super::zlog::SYSERROR,
            "unable to allocate {} bytes in shared memory",
            size
        );
        return std::ptr::null_mut();
    }
    unsafe { std::ptr::write_bytes(p as *mut u8, 0, size) };
    SHM_ALLOCATED.fetch_add(size as i32, Ordering::Relaxed);
    p as *mut u8
}

/// fpm_shm_free()
fn shm_free(p: *mut u8, size: usize) {
    if p.is_null() {
        return;
    }
    unsafe {
        libc::munmap(p as *mut libc::c_void, size);
    }
    SHM_ALLOCATED.fetch_sub(size as i32, Ordering::Relaxed);
}

pub fn scoreboard_size(nprocs: usize) -> usize {
    std::mem::size_of::<Scoreboard>() + std::mem::size_of::<Proc>() * nprocs
}

/// Allocate a scoreboard for `nprocs` children.
pub fn alloc(pool: &str, pm: i32, nprocs: i32) -> *mut Scoreboard {
    let p = shm_alloc(scoreboard_size(nprocs as usize)) as *mut Scoreboard;
    if p.is_null() {
        return p;
    }
    let sb = unsafe { &mut *p };
    sb.pm = pm;
    sb.nprocs = nprocs as u32;
    sb.start_epoch = epoch();
    strlcpy(&mut sb.pool, pool.as_bytes());
    p
}

pub fn free(sb: *mut Scoreboard, nprocs: i32) {
    shm_free(sb as *mut u8, scoreboard_size(nprocs as usize));
}

pub fn epoch() -> i64 {
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

fn atomic(p: *mut i32) -> &'static AtomicI32 {
    unsafe { AtomicI32::from_ptr(p) }
}

/// fpm_spinlock()
fn spinlock(lock: *mut i32, try_once: bool) -> bool {
    let a = atomic(lock);
    if try_once {
        return a
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok();
    }
    loop {
        if a.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return true;
        }
        unsafe { libc::sched_yield() };
    }
}

/// fpm_spinlock_with_max_retries()
fn spinlock_with_max_retries(lock: *mut i32, max_retries: i32) -> bool {
    let a = atomic(lock);
    let mut retries = 0;
    loop {
        if a.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return true;
        }
        retries += 1;
        if retries > max_retries {
            return false;
        }
        unsafe { libc::sched_yield() };
    }
}

fn unlock(lock: *mut i32) {
    atomic(lock).store(0, Ordering::Release);
}

thread_local! {
    /// fpm_scoreboard / fpm_scoreboard_i of a worker
    static CURRENT: std::cell::Cell<(*mut Scoreboard, i32)> = const { std::cell::Cell::new((std::ptr::null_mut(), -1)) };
}

/// fpm_scoreboard_get()
pub fn get() -> *mut Scoreboard {
    CURRENT.with(|c| c.get().0)
}

/// fpm_scoreboard_child_use()
pub fn child_use(sb: *mut Scoreboard, index: i32, pid: i32) {
    CURRENT.with(|c| c.set((sb, index)));
    if let Some(p) = proc_get(sb, index) {
        unsafe {
            (*p).pid = pid;
            (*p).start_epoch = epoch();
        }
    }
}

fn get_for_update(sb: *mut Scoreboard) -> *mut Scoreboard {
    let sb = if sb.is_null() { get() } else { sb };
    if sb.is_null() {
        zlog!(
            WARNING,
            "Unable to update scoreboard: the SHM has not been found"
        );
    }
    sb
}

fn readers_decrement(sb: *mut Scoreboard) {
    unsafe {
        spinlock(&mut (*sb).lock, false);
        if (*sb).reader_count > 0 {
            (*sb).reader_count -= 1;
        }
        unlock(&mut (*sb).lock);
    }
}

/// fpm_scoreboard_update_begin()
pub fn update_begin(sb: *mut Scoreboard) {
    let sb = get_for_update(sb);
    if sb.is_null() {
        return;
    }
    let mut retries = 0;
    unsafe {
        loop {
            spinlock(&mut (*sb).lock, false);
            if (*sb).reader_count == 0 {
                if !spinlock_with_max_retries(&mut (*sb).writer_active, SPINLOCK_MAX_RETRIES) {
                    zlog!(
                        WARNING,
                        "scoreboard: writer {} waited too long for another writer to release lock.",
                        std::process::id()
                    );
                }
                unlock(&mut (*sb).lock);
                break;
            }
            unlock(&mut (*sb).lock);
            retries += 1;
            if retries > SPINLOCK_MAX_RETRIES {
                readers_decrement(sb);
                zlog!(
                    WARNING,
                    "scoreboard: writer detected a potential crashed reader, decrementing reader count."
                );
                retries = 0;
            }
            libc::sched_yield();
        }
    }
}

/// fpm_scoreboard_update_commit()
#[allow(clippy::too_many_arguments)]
pub fn update_commit(
    idle: i32,
    active: i32,
    lq: i32,
    lq_len: i32,
    requests: i32,
    max_children_reached: i32,
    slow_rq: i32,
    memory_peak: usize,
    action: i32,
    sb: *mut Scoreboard,
) {
    let sb = get_for_update(sb);
    if sb.is_null() {
        return;
    }
    let s = unsafe { &mut *sb };
    if action == ACTION_SET {
        if idle >= 0 {
            s.idle = idle;
        }
        if active >= 0 {
            s.active = active;
        }
        if lq >= 0 {
            s.lq = lq;
        }
        if lq_len >= 0 {
            s.lq_len = lq_len as u32;
        }
        if s.lq > s.lq_max {
            s.lq_max = s.lq;
        }
        if requests >= 0 {
            s.requests = requests as u64;
        }
        if max_children_reached >= 0 {
            s.max_children_reached = max_children_reached as u32;
        }
        if slow_rq > 0 {
            s.slow_rq = slow_rq as u64;
        }
    } else {
        s.idle = if s.idle + idle > 0 { s.idle + idle } else { 0 };
        s.active = if s.active + active > 0 {
            s.active + active
        } else {
            0
        };
        let r = s.requests as i64 + requests as i64;
        s.requests = if r > 0 { r as u64 } else { 0 };
        let m = s.max_children_reached as i64 + max_children_reached as i64;
        s.max_children_reached = if m > 0 { m as u32 } else { 0 };
        let q = s.slow_rq as i64 + slow_rq as i64;
        s.slow_rq = if q > 0 { q as u64 } else { 0 };
    }
    if s.active > s.active_max {
        s.active_max = s.active;
    }
    if s.memory_peak < memory_peak {
        s.memory_peak = memory_peak;
    }
    unlock(&mut s.writer_active);
}

/// fpm_scoreboard_update()
#[allow(clippy::too_many_arguments)]
pub fn update(
    idle: i32,
    active: i32,
    lq: i32,
    lq_len: i32,
    requests: i32,
    max_children_reached: i32,
    slow_rq: i32,
    memory_peak: usize,
    action: i32,
    sb: *mut Scoreboard,
) {
    update_begin(sb);
    update_commit(
        idle,
        active,
        lq,
        lq_len,
        requests,
        max_children_reached,
        slow_rq,
        memory_peak,
        action,
        sb,
    );
}

/// fpm_scoreboard_proc_get()
pub fn proc_get(sb: *mut Scoreboard, index: i32) -> Option<*mut Proc> {
    let sb = if sb.is_null() { get() } else { sb };
    if sb.is_null() {
        return None;
    }
    let index = if index < 0 {
        CURRENT.with(|c| c.get().1)
    } else {
        index
    };
    let nprocs = unsafe { (*sb).nprocs };
    if index < 0 || index as u32 >= nprocs {
        return None;
    }
    unsafe {
        let base = (sb as *mut u8).add(std::mem::size_of::<Scoreboard>()) as *mut Proc;
        Some(base.add(index as usize))
    }
}

/// fpm_scoreboard_proc_acquire()
pub fn proc_acquire(sb: *mut Scoreboard, index: i32, nohang: bool) -> Option<*mut Proc> {
    let p = proc_get(sb, index)?;
    if !spinlock(unsafe { &mut (*p).lock }, nohang) {
        return None;
    }
    Some(p)
}

/// fpm_scoreboard_proc_release()
pub fn proc_release(p: *mut Proc) {
    unsafe {
        atomic(&mut (*p).lock).store(0, Ordering::Release);
    }
}

/// fpm_scoreboard_acquire()
fn acquire(sb: *mut Scoreboard) -> *mut Scoreboard {
    let s = if sb.is_null() { get() } else { sb };
    if s.is_null() {
        return s;
    }
    let mut retries = 0;
    unsafe {
        loop {
            spinlock(&mut (*s).lock, false);
            if atomic(&mut (*s).writer_active).load(Ordering::Acquire) == 0 {
                (*s).reader_count += 1;
                unlock(&mut (*s).lock);
                break;
            }
            unlock(&mut (*s).lock);
            libc::sched_yield();
            retries += 1;
            if retries > SPINLOCK_MAX_RETRIES {
                zlog!(
                    WARNING,
                    "scoreboard: reader waited too long for writer to release lock."
                );
                readers_decrement(s);
                return std::ptr::null_mut();
            }
        }
    }
    s
}

/// A copy of a scoreboard (fpm_scoreboard_copy()).
pub struct Copy {
    pub sb: Scoreboard,
    pub procs: Vec<Proc>,
}

/// fpm_scoreboard_copy()
pub fn copy(sb: *mut Scoreboard, copy_procs: bool) -> Option<Copy> {
    let sb = if sb.is_null() { get() } else { sb };
    let s = acquire(sb);
    if s.is_null() {
        zlog!(ERROR, "scoreboard: failed to lock (already locked)");
        return None;
    }
    let head = unsafe { *s };
    let mut procs = Vec::new();
    if copy_procs {
        for i in 0..head.nprocs as i32 {
            if let Some(p) = proc_acquire(s, i, false) {
                procs.push(unsafe { *p });
                proc_release(p);
            }
        }
    }
    readers_decrement(s);
    Some(Copy { sb: head, procs })
}

/// fpm_scoreboard_proc_free()
pub fn proc_free(sb: *mut Scoreboard, nprocs: i32, index: i32) {
    if sb.is_null() || index < 0 || index >= nprocs {
        return;
    }
    if let Some(p) = proc_get(sb, index) {
        unsafe {
            if (*p).used > 0 {
                std::ptr::write_bytes(p as *mut u8, 0, std::mem::size_of::<Proc>());
            }
            (*sb).free_proc = index;
        }
    }
}

/// fpm_scoreboard_proc_alloc(): the slot index.
pub fn proc_alloc(sb: *mut Scoreboard, nprocs: i32) -> Option<i32> {
    if sb.is_null() {
        return None;
    }
    let s = unsafe { &mut *sb };
    let used = |i: i32| proc_get(sb, i).is_some_and(|p| unsafe { (*p).used } != 0);
    let mut i = -1;
    if s.free_proc >= 0 && s.free_proc < nprocs && !used(s.free_proc) {
        i = s.free_proc;
    }
    if i < 0 {
        zlog!(
            DEBUG,
            "[pool {}] the proc->free_slot was not free. Let's search",
            String::from_utf8_lossy(cstr(&s.pool))
        );
        i = (0..nprocs).find(|&k| !used(k)).unwrap_or(nprocs);
    }
    if i < 0 || i >= nprocs {
        zlog!(
            ERROR,
            "[pool {}] no free scoreboard slot",
            String::from_utf8_lossy(cstr(&s.pool))
        );
        return None;
    }
    if let Some(p) = proc_get(sb, i) {
        unsafe { (*p).used = 1 };
    }
    s.free_proc = if i + 1 >= nprocs { 0 } else { i + 1 };
    Some(i)
}

/// fpm_request_get_stage_name()
pub fn stage_name(stage: i32) -> &'static str {
    match stage {
        REQUEST_CREATING => "Creating",
        REQUEST_ACCEPTING => "Idle",
        REQUEST_READING_HEADERS => "Reading headers",
        REQUEST_INFO => "Getting request information",
        REQUEST_EXECUTING => "Running",
        REQUEST_END => "Ending",
        _ => "Finishing",
    }
}
