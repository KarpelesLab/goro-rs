//! The scoreboard: per pool counters and one slot per worker process, in
//! memory shared by the master and its workers (an anonymous shared
//! mapping made before the workers are forked). It feeds the status page
//! and fpm_get_status() (PHP manual: FPM status page) and lets the master
//! see which workers are idle.

use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU8, AtomicU64, Ordering::Relaxed};

/// Worker states (the status page's "state").
pub const IDLE: u8 = 0;
pub const READING: u8 = 1;
pub const RUNNING: u8 = 2;
pub const FINISHING: u8 = 3;
pub const STARTING: u8 = 4;

pub fn state_name(s: u8) -> &'static str {
    match s {
        IDLE => "Idle",
        READING => "Reading headers",
        RUNNING => "Running",
        FINISHING => "Finishing",
        _ => "Starting",
    }
}

const URI: usize = 1024;
const QUERY: usize = 512;
const SCRIPT: usize = 512;
const SMALL: usize = 64;

/// A fixed-size text field.
#[repr(C)]
pub struct Text<const N: usize> {
    len: AtomicU64,
    bytes: [AtomicU8; N],
}

impl<const N: usize> Text<N> {
    pub fn set(&self, v: &[u8]) {
        let n = v.len().min(N);
        for (i, &b) in v[..n].iter().enumerate() {
            self.bytes[i].store(b, Relaxed);
        }
        self.len.store(n as u64, Relaxed);
    }

    pub fn get(&self) -> Vec<u8> {
        let n = (self.len.load(Relaxed) as usize).min(N);
        self.bytes[..n].iter().map(|b| b.load(Relaxed)).collect()
    }
}

/// One worker process.
#[repr(C)]
pub struct Slot {
    pub pid: AtomicI32,
    pub state: AtomicU8,
    /// Process start, µs since the epoch.
    pub started: AtomicI64,
    pub requests: AtomicU64,
    /// Start and end of the current or last request, µs.
    pub req_start: AtomicI64,
    pub req_end: AtomicI64,
    /// When the process last became idle, µs.
    pub idle_since: AtomicI64,
    pub content_length: AtomicU64,
    /// f64 bits.
    pub last_cpu: AtomicU64,
    pub last_memory: AtomicU64,
    /// The response status of the request.
    pub status: AtomicI64,
    /// Peak memory of the request's engine.
    pub memory: AtomicU64,
    /// The response of the current request was ended (END_REQUEST sent).
    pub done: AtomicU8,
    /// The request was reported as slow.
    pub slow_logged: AtomicU8,
    pub method: Text<SMALL>,
    pub uri: Text<URI>,
    pub query: Text<QUERY>,
    pub script: Text<SCRIPT>,
    pub user: Text<SMALL>,
}

/// A pool's shared counters.
#[repr(C)]
pub struct Head {
    pub start_time: AtomicI64,
    pub accepted: AtomicU64,
    pub max_active: AtomicU64,
    pub max_children_reached: AtomicU64,
    pub slow_requests: AtomicU64,
    pub nslots: AtomicU64,
}

/// A pool's scoreboard.
pub struct Board {
    base: *mut u8,
    size: usize,
    nslots: usize,
}

// SAFETY: the mapping is only used through atomics.
unsafe impl Send for Board {}

impl Board {
    /// A shared anonymous mapping for `nslots` workers (zero-filled).
    pub fn new(nslots: usize) -> Option<Board> {
        let size = std::mem::size_of::<Head>() + nslots * std::mem::size_of::<Slot>();
        // SAFETY: an anonymous shared mapping, checked below.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return None;
        }
        let b = Board {
            base: p as *mut u8,
            size,
            nslots,
        };
        b.head().nslots.store(nslots as u64, Relaxed);
        b.head().start_time.store(super::log::now_secs(), Relaxed);
        Some(b)
    }

    pub fn head(&self) -> &Head {
        // SAFETY: the mapping starts with a Head (zero is a valid state).
        unsafe { &*(self.base as *const Head) }
    }

    pub fn slot(&self, i: usize) -> &Slot {
        assert!(i < self.nslots);
        // SAFETY: slot i lies inside the mapping; all-zero is valid.
        unsafe {
            &*(self
                .base
                .add(std::mem::size_of::<Head>() + i * std::mem::size_of::<Slot>())
                as *const Slot)
        }
    }

    pub fn nslots(&self) -> usize {
        self.nslots
    }

    /// A free slot (no process), cleared.
    pub fn alloc(&self) -> Option<usize> {
        (0..self.nslots).find(|&i| {
            let s = self.slot(i);
            if s.pid.load(Relaxed) == 0 && s.state.load(Relaxed) != STARTING {
                s.state.store(STARTING, Relaxed);
                s.requests.store(0, Relaxed);
                s.req_start.store(0, Relaxed);
                s.req_end.store(0, Relaxed);
                s.content_length.store(0, Relaxed);
                s.last_cpu.store(0f64.to_bits(), Relaxed);
                s.last_memory.store(0, Relaxed);
                s.started.store(super::log::now_us(), Relaxed);
                s.idle_since.store(super::log::now_us(), Relaxed);
                s.method.set(b"");
                s.uri.set(b"");
                s.query.set(b"");
                s.script.set(b"");
                s.user.set(b"");
                true
            } else {
                false
            }
        })
    }

    pub fn free(&self, i: usize) {
        let s = self.slot(i);
        s.pid.store(0, Relaxed);
        s.state.store(IDLE, Relaxed);
    }

    /// (idle, active) process counts.
    pub fn counts(&self) -> (u64, u64) {
        let mut idle = 0;
        let mut active = 0;
        for i in 0..self.nslots {
            let s = self.slot(i);
            if s.pid.load(Relaxed) == 0 {
                continue;
            }
            match s.state.load(Relaxed) {
                IDLE => idle += 1,
                STARTING => {}
                _ => active += 1,
            }
        }
        (idle, active)
    }

    /// Slots in use.
    pub fn used(&self) -> impl Iterator<Item = (usize, &Slot)> {
        (0..self.nslots)
            .map(|i| (i, self.slot(i)))
            .filter(|(_, s)| s.pid.load(Relaxed) != 0)
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        // SAFETY: unmapping our own mapping.
        unsafe {
            libc::munmap(self.base as *mut libc::c_void, self.size);
        }
    }
}
