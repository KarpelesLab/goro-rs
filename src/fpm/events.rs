//! sapi/fpm/fpm/fpm_events.c (with the epoll module), fpm_signals.c and
//! fpm_process_ctl.c: the master's event loop, its signal pipe and the
//! process control state machine.

use std::sync::atomic::{AtomicI32, Ordering};

use super::Fpm;
use super::scoreboard::{self, Timeval};
use super::zlog::{self, DEBUG, ERROR, NOTICE, SYSERROR, WARNING};
use crate::zlog;

pub const PCTL_STATE_NORMAL: i32 = 1;
pub const PCTL_STATE_RELOADING: i32 = 2;
pub const PCTL_STATE_TERMINATING: i32 = 3;
pub const PCTL_STATE_FINISHING: i32 = 4;

/// The master's pid, for the signal handler (fpm_globals.parent_pid).
pub static PARENT_PID: AtomicI32 = AtomicI32::new(0);
/// sp[1]: the write end of the signal pipe.
static SIG_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

/// FPM_IDLE_SERVER_MAINTENANCE_HEARTBEAT (1s)
const IDLE_SERVER_MAINTENANCE_HEARTBEAT: u64 = 1000;
/// FPM_PCTL_MIN_HEARTBEAT
const PCTL_MIN_HEARTBEAT: i32 = 130;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    Signal,
    ChildStdout(i32),
    ChildStderr(i32),
    Ondemand(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerKind {
    Heartbeat,
    Maintenance,
    Pctl,
    ChildrenBury,
    ChildFree(i32),
}

struct FdEvent {
    fd: i32,
    kind: FdKind,
}

struct TimerEvent {
    kind: TimerKind,
    timeout: Timeval,
    frequency: Timeval,
    persist: bool,
}

/// The event queues (fpm_event_queue_fd / fpm_event_queue_timer) and the
/// epoll descriptor.
#[derive(Default)]
pub struct Events {
    fds: Vec<FdEvent>,
    timers: Vec<TimerEvent>,
    epollfd: i32,
    nepollfds: usize,
}

impl Events {
    pub fn epollfd_raw(&self) -> i32 {
        self.epollfd
    }
}

pub fn mechanism_name() -> &'static str {
    "epoll"
}

fn tv_add_ms(now: Timeval, ms: u64) -> (Timeval, Timeval) {
    let freq = if ms >= 1000 {
        Timeval {
            sec: (ms / 1000) as i64,
            usec: ((ms % 1000) * 1000) as i64,
        }
    } else {
        Timeval {
            sec: 0,
            usec: (ms * 1000) as i64,
        }
    };
    (add(now, freq), freq)
}

fn add(a: Timeval, b: Timeval) -> Timeval {
    let mut sec = a.sec + b.sec;
    let mut usec = a.usec + b.usec;
    if usec >= 1_000_000 {
        sec += 1;
        usec -= 1_000_000;
    }
    Timeval { sec, usec }
}

fn le(a: &Timeval, b: &Timeval) -> bool {
    !b.lt(a)
}

extern "C" fn sig_handler(signo: libc::c_int) {
    if PARENT_PID.load(Ordering::Relaxed) != unsafe { libc::getpid() } {
        return;
    }
    let c: u8 = match signo {
        libc::SIGTERM => b'T',
        libc::SIGINT => b'I',
        libc::SIGUSR1 => b'1',
        libc::SIGUSR2 => b'2',
        libc::SIGQUIT => b'Q',
        libc::SIGCHLD => b'C',
        _ => 0,
    };
    unsafe {
        let saved = *libc::__errno_location();
        libc::write(
            SIG_WRITE_FD.load(Ordering::Relaxed),
            &c as *const u8 as *const libc::c_void,
            1,
        );
        *libc::__errno_location() = saved;
    }
}

/// The child's SIGQUIT handler (sig_soft_quit()): closing the listening
/// socket makes accept() return right away.
extern "C" fn sig_soft_quit(_signo: libc::c_int) {
    unsafe {
        let saved = *libc::__errno_location();
        let fd = super::fastcgi::LISTEN_FD.load(Ordering::Relaxed);
        libc::close(fd);
        libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        super::fastcgi::TERMINATE.store(true, Ordering::Relaxed);
        *libc::__errno_location() = saved;
    }
}

pub fn signal_name(sig: i32) -> &'static str {
    match sig {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGILL => "SIGILL",
        libc::SIGTRAP => "SIGTRAP",
        libc::SIGABRT => "SIGABRT",
        libc::SIGBUS => "SIGBUS",
        libc::SIGFPE => "SIGFPE",
        libc::SIGKILL => "SIGKILL",
        libc::SIGUSR1 => "SIGUSR1",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGUSR2 => "SIGUSR2",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGALRM => "SIGALRM",
        libc::SIGTERM => "SIGTERM",
        libc::SIGCHLD => "SIGCHLD",
        libc::SIGCONT => "SIGCONT",
        libc::SIGSTOP => "SIGSTOP",
        libc::SIGTSTP => "SIGTSTP",
        libc::SIGTTIN => "SIGTTIN",
        libc::SIGTTOU => "SIGTTOU",
        libc::SIGURG => "SIGURG",
        libc::SIGXCPU => "SIGXCPU",
        libc::SIGXFSZ => "SIGXFSZ",
        libc::SIGVTALRM => "SIGVTALRM",
        libc::SIGPROF => "SIGPROF",
        libc::SIGWINCH => "SIGWINCH",
        libc::SIGIO => "SIGIO",
        libc::SIGPWR => "SIGPWR",
        libc::SIGSYS => "SIGSYS",
        _ => "",
    }
}

fn sigset(sigs: &[i32]) -> libc::sigset_t {
    let mut s: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut s);
        for &g in sigs {
            libc::sigaddset(&mut s, g);
        }
    }
    s
}

/// fpm_signals_block(): the signals of fpm_signals_init_mask()
pub fn signals_block() -> i32 {
    let s = sigset(&[libc::SIGUSR1, libc::SIGUSR2, libc::SIGCHLD]);
    if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &s, std::ptr::null_mut()) } < 0 {
        zlog!(SYSERROR, "failed to block signals");
        return -1;
    }
    0
}

/// fpm_signals_child_block()
pub fn signals_child_block() -> i32 {
    let s = sigset(&[
        libc::SIGUSR1,
        libc::SIGUSR2,
        libc::SIGCHLD,
        libc::SIGTERM,
        libc::SIGQUIT,
    ]);
    if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &s, std::ptr::null_mut()) } < 0 {
        zlog!(SYSERROR, "failed to block child signals");
        return -1;
    }
    0
}

/// fpm_signals_unblock()
pub fn signals_unblock() -> i32 {
    let mut all: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe { libc::sigfillset(&mut all) };
    if unsafe { libc::sigprocmask(libc::SIG_UNBLOCK, &all, std::ptr::null_mut()) } < 0 {
        zlog!(SYSERROR, "failed to unblock signals");
        return -1;
    }
    0
}

fn set_nonblock(fd: i32) -> i32 {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return -1;
        }
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK)
    }
}

pub fn set_blocked(fd: i32, blocked: bool) -> i32 {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return -1;
        }
        let f = if blocked {
            flags & !libc::O_NONBLOCK
        } else {
            flags | libc::O_NONBLOCK
        };
        libc::fcntl(fd, libc::F_SETFL, f)
    }
}

fn sigaction(sig: i32, handler: libc::sighandler_t, flags: i32, fill_mask: bool) -> i32 {
    let mut act: libc::sigaction = unsafe { std::mem::zeroed() };
    act.sa_sigaction = handler;
    act.sa_flags = flags;
    unsafe {
        if fill_mask {
            libc::sigfillset(&mut act.sa_mask);
        } else {
            libc::sigemptyset(&mut act.sa_mask);
        }
        libc::sigaction(sig, &act, std::ptr::null_mut())
    }
}

impl Fpm {
    /// fpm_signals_init_main()
    pub fn signals_init_main(&mut self) -> i32 {
        let mut sp = [0i32; 2];
        if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, sp.as_mut_ptr()) } < 0 {
            zlog!(SYSERROR, "failed to init signals: socketpair()");
            return -1;
        }
        if set_nonblock(sp[0]) < 0 || set_nonblock(sp[1]) < 0 {
            zlog!(SYSERROR, "failed to init signals: fd_set_blocked()");
            return -1;
        }
        unsafe {
            if libc::fcntl(sp[0], libc::F_SETFD, libc::FD_CLOEXEC) < 0
                || libc::fcntl(sp[1], libc::F_SETFD, libc::FD_CLOEXEC) < 0
            {
                zlog!(
                    SYSERROR,
                    "failed to init signals: fcntl(F_SETFD, FD_CLOEXEC)"
                );
                return -1;
            }
        }
        self.sp = sp;
        SIG_WRITE_FD.store(sp[1], Ordering::Relaxed);
        let h = sig_handler as extern "C" fn(libc::c_int) as libc::sighandler_t;
        for s in [
            libc::SIGTERM,
            libc::SIGINT,
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGCHLD,
            libc::SIGQUIT,
        ] {
            if sigaction(s, h, 0, true) < 0 {
                zlog!(SYSERROR, "failed to init signals: sigaction()");
                return -1;
            }
        }
        zlog!(DEBUG, "Unblocking all signals");
        if signals_unblock() < 0 {
            return -1;
        }
        0
    }

    /// fpm_signals_init_child()
    pub fn signals_init_child(&mut self) -> i32 {
        unsafe {
            libc::close(self.sp[0]);
            libc::close(self.sp[1]);
        }
        let dfl = libc::SIG_DFL;
        let quit = sig_soft_quit as extern "C" fn(libc::c_int) as libc::sighandler_t;
        if sigaction(libc::SIGTERM, dfl, 0, false) < 0
            || sigaction(libc::SIGINT, dfl, 0, false) < 0
            || sigaction(libc::SIGUSR1, dfl, 0, false) < 0
            || sigaction(libc::SIGUSR2, dfl, 0, false) < 0
            || sigaction(libc::SIGCHLD, dfl, 0, false) < 0
            || sigaction(libc::SIGQUIT, quit, libc::SA_RESTART, false) < 0
        {
            zlog!(SYSERROR, "failed to init child signals: sigaction()");
            return -1;
        }
        if signals_unblock() < 0 {
            return -1;
        }
        0
    }

    /// fpm_event_pre_init()
    pub fn event_pre_init(&mut self) -> Result<(), ()> {
        match self.gc.events_mechanism.as_deref() {
            None => Ok(()),
            Some(m) if m.eq_ignore_ascii_case("epoll") => Ok(()),
            Some(m) => {
                zlog!(
                    ERROR,
                    "event mechanism '{}' is not available on this system",
                    m
                );
                Err(())
            }
        }
    }

    /// fpm_event_init_main()
    pub fn event_init_main(&mut self) -> i32 {
        let mut max = 1usize;
        for wp in &self.pools {
            if wp.config.catch_workers_output != 0 && wp.config.pm_max_children > 0 {
                max += wp.config.pm_max_children as usize * 2;
            }
        }
        let fd = unsafe { libc::epoll_create((max + 1) as i32) };
        if fd < 0 {
            zlog!(ERROR, "epoll: unable to initialize");
            zlog!(ERROR, "Unable to initialize the event module epoll");
            return -1;
        }
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        self.ev.epollfd = fd;
        self.ev.nepollfds = max;
        zlog!(
            DEBUG,
            "event module is epoll and {} fds have been reserved",
            max
        );
        0
    }

    /// fpm_event_add() for a descriptor.
    pub fn event_add_fd(&mut self, fd: i32, kind: FdKind, edge: bool) {
        if self.ev.fds.iter().any(|e| e.kind == kind) {
            return;
        }
        let mut e = libc::epoll_event {
            events: (libc::EPOLLIN | if edge { libc::EPOLLET } else { 0 }) as u32,
            u64: fd as u64,
        };
        if unsafe { libc::epoll_ctl(self.ev.epollfd, libc::EPOLL_CTL_ADD, fd, &mut e) } == -1 {
            zlog!(ERROR, "epoll: unable to add fd {}", fd);
            return;
        }
        self.ev.fds.insert(0, FdEvent { fd, kind });
    }

    /// fpm_event_del() for a descriptor.
    pub fn event_del_fd(&mut self, kind: FdKind) {
        if let Some(i) = self.ev.fds.iter().position(|e| e.kind == kind) {
            let fd = self.ev.fds[i].fd;
            self.ev.fds.remove(i);
            let mut e = libc::epoll_event {
                events: libc::EPOLLIN as u32,
                u64: fd as u64,
            };
            if unsafe { libc::epoll_ctl(self.ev.epollfd, libc::EPOLL_CTL_DEL, fd, &mut e) } == -1 {
                zlog!(ERROR, "epoll: unable to remove fd {}", fd);
            }
        }
    }

    /// fpm_event_add() for a timer (in ms).
    pub fn event_add_timer(&mut self, kind: TimerKind, ms: u64, persist: bool) {
        let (timeout, frequency) = tv_add_ms(scoreboard::clock_get(), ms);
        if let Some(t) = self.ev.timers.iter_mut().find(|t| t.kind == kind) {
            t.timeout = timeout;
            t.frequency = frequency;
            t.persist = persist;
            return;
        }
        self.ev.timers.insert(
            0,
            TimerEvent {
                kind,
                timeout,
                frequency,
                persist,
            },
        );
    }

    /// fpm_event_fire() of a descriptor event.
    fn fire_fd(&mut self, kind: FdKind) {
        match kind {
            FdKind::Signal => self.got_signal(),
            FdKind::ChildStdout(pid) => self.stdio_child_said(pid, true),
            FdKind::ChildStderr(pid) => self.stdio_child_said(pid, false),
            FdKind::Ondemand(i) => self.pctl_on_socket_accept(i),
        }
    }

    pub fn fire_child_stdio(&mut self, pid: i32, stdout: bool) {
        let kind = if stdout {
            FdKind::ChildStdout(pid)
        } else {
            FdKind::ChildStderr(pid)
        };
        if self.ev.fds.iter().any(|e| e.kind == kind) {
            self.fire_fd(kind);
        }
    }

    fn fire_timer(&mut self, kind: TimerKind) {
        match kind {
            TimerKind::Heartbeat => self.pctl_heartbeat(true),
            TimerKind::Maintenance => self.pctl_maintenance_heartbeat(true),
            TimerKind::Pctl => self.pctl(0, Action::Timeout),
            TimerKind::ChildrenBury => self.children_bury(),
            TimerKind::ChildFree(pid) => self.postponed_child_free(pid),
        }
    }

    /// fpm_got_signal()
    fn got_signal(&mut self) {
        loop {
            let mut c = 0u8;
            let res = loop {
                let r =
                    unsafe { libc::read(self.sp[0], &mut c as *mut u8 as *mut libc::c_void, 1) };
                if r == -1 && zlog::errno() == libc::EINTR {
                    continue;
                }
                break r;
            };
            if res <= 0 {
                let e = zlog::errno();
                if res < 0 && e != libc::EAGAIN && e != libc::EWOULDBLOCK {
                    zlog!(SYSERROR, "unable to read from the signal pipe");
                }
                return;
            }
            match c {
                b'C' => {
                    zlog!(DEBUG, "received SIGCHLD");
                    self.event_add_timer(TimerKind::ChildrenBury, 0, false);
                }
                b'I' => {
                    zlog!(DEBUG, "received SIGINT");
                    zlog!(NOTICE, "Terminating ...");
                    self.pctl(PCTL_STATE_TERMINATING, Action::Set);
                }
                b'T' => {
                    zlog!(DEBUG, "received SIGTERM");
                    zlog!(NOTICE, "Terminating ...");
                    self.pctl(PCTL_STATE_TERMINATING, Action::Set);
                }
                b'Q' => {
                    zlog!(DEBUG, "received SIGQUIT");
                    zlog!(NOTICE, "Finishing ...");
                    self.pctl(PCTL_STATE_FINISHING, Action::Set);
                }
                b'1' => {
                    zlog!(DEBUG, "received SIGUSR1");
                    self.stdio_restore_original_stderr(false);
                    if self.stdio_open_error_log(true) == 0 {
                        zlog!(NOTICE, "error log file re-opened");
                    } else {
                        zlog!(ERROR, "unable to re-opened error log file");
                    }
                    let ret = self.log_open(true);
                    if ret == 0 {
                        zlog!(NOTICE, "access log file re-opened");
                    } else if ret == -1 {
                        zlog!(ERROR, "unable to re-opened access log file");
                    }
                    self.stdio_redirect_stderr_to_error_log();
                }
                b'2' => {
                    zlog!(DEBUG, "received SIGUSR2");
                    zlog!(NOTICE, "Reloading in progress ...");
                    self.pctl(PCTL_STATE_RELOADING, Action::Set);
                }
                _ => {}
            }
        }
    }

    /// fpm_event_loop()
    pub fn event_loop(&mut self, err: bool) -> ! {
        let sfd = self.sp[0];
        self.event_add_fd(sfd, FdKind::Signal, false);
        if self.heartbeat > 0 {
            self.pctl_heartbeat(false);
        }
        if !err {
            self.pctl_maintenance_heartbeat(false);
            zlog!(
                DEBUG,
                "{} bytes have been reserved in SHM",
                scoreboard::shm_size_allocated()
            );
            zlog!(NOTICE, "ready to handle connections");
        }
        let mut events: Vec<libc::epoll_event> =
            vec![libc::epoll_event { events: 0, u64: 0 }; self.ev.nepollfds.max(1)];
        loop {
            let now = scoreboard::clock_get();
            let mut ms: Option<Timeval> = None;
            for t in &self.ev.timers {
                match ms {
                    None => ms = Some(t.timeout),
                    Some(m) if t.timeout.lt(&m) => ms = Some(t.timeout),
                    _ => {}
                }
            }
            let timeout: i32 = match ms {
                Some(m) if m.is_set() && now.lt(&m) => {
                    let d = m.sub(&now);
                    (d.sec * 1000 + d.usec / 1000 + 1) as i32
                }
                _ => 1000,
            };
            let n = events.len();
            let ret = unsafe {
                libc::epoll_wait(self.ev.epollfd, events.as_mut_ptr(), n as i32, timeout)
            };
            if ret == -1 {
                let e = zlog::errno();
                if e != libc::EINTR {
                    zlog!(WARNING, "epoll_wait() returns {}", e);
                }
            }
            if ret > 0 {
                let fired: Vec<i32> = events[..ret as usize]
                    .iter()
                    .map(|e| e.u64 as i32)
                    .collect();
                for fd in fired {
                    let kind = self.ev.fds.iter().find(|e| e.fd == fd).map(|e| e.kind);
                    if let Some(k) = kind {
                        self.fire_fd(k);
                    }
                }
                zlog!(DEBUG, "event module triggered {} events", ret);
            }
            // trigger timers
            let kinds: Vec<TimerKind> = self.ev.timers.iter().map(|t| t.kind).collect();
            for k in kinds {
                let now = scoreboard::clock_get();
                let Some(i) = self.ev.timers.iter().position(|t| t.kind == k) else {
                    continue;
                };
                if le(&self.ev.timers[i].timeout, &now) {
                    if self.ev.timers[i].persist {
                        let f = self.ev.timers[i].frequency;
                        self.ev.timers[i].timeout = add(now, f);
                    } else {
                        self.ev.timers.remove(i);
                    }
                    self.fire_timer(k);
                }
            }
        }
    }

    /// fpm_pctl_heartbeat()
    fn pctl_heartbeat(&mut self, timeout: bool) {
        if timeout {
            let now = scoreboard::clock_get();
            self.pctl_check_request_timeout(now);
            return;
        }
        self.heartbeat = self.heartbeat.max(PCTL_MIN_HEARTBEAT);
        zlog!(
            DEBUG,
            "heartbeat have been set up with a timeout of {}ms",
            self.heartbeat
        );
        let hb = self.heartbeat as u64;
        self.event_add_timer(TimerKind::Heartbeat, hb, true);
    }

    /// fpm_pctl_perform_idle_server_maintenance_heartbeat()
    fn pctl_maintenance_heartbeat(&mut self, timeout: bool) {
        if timeout {
            if self.pctl_can_spawn_children() {
                self.pctl_perform_idle_server_maintenance();
            }
            return;
        }
        self.event_add_timer(
            TimerKind::Maintenance,
            IDLE_SERVER_MAINTENANCE_HEARTBEAT,
            true,
        );
    }

    pub fn pctl_can_spawn_children(&self) -> bool {
        self.pctl_state == PCTL_STATE_NORMAL
    }

    /// fpm_pctl_kill_all()
    pub fn pctl_kill_all(&self, signo: i32) {
        let mut alive = 0;
        for c in self.children.iter().filter(|c| c.linked) {
            let res = unsafe { libc::kill(c.pid, signo) };
            zlog!(
                DEBUG,
                "[pool {}] sending signal {} {} to child {}",
                self.pools[c.pool].config.name(),
                signo,
                signal_name(signo),
                c.pid
            );
            if res == 0 {
                alive += 1;
            }
        }
        if alive != 0 {
            zlog!(DEBUG, "{} child(ren) still alive", alive);
        }
    }

    /// fpm_pctl_exit()
    fn pctl_exit(&mut self) -> ! {
        zlog!(NOTICE, "exiting, bye-bye!");
        self.conf_unlink_pid();
        self.sockets_cleanup(Cleanup::ParentExitMain);
        std::process::exit(0);
    }

    /// fpm_pctl_exec()
    fn pctl_exec(&mut self) -> ! {
        zlog!(DEBUG, "Blocking some signals before reexec");
        if signals_block() < 0 {
            zlog!(WARNING, "concurrent reloads may be unstable");
        }
        let argv: Vec<String> = self
            .argv
            .iter()
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        let mut s = format!("reloading: execvp(\"{}\", {{\"{}\"", argv[0], argv[0]);
        for a in argv.iter().skip(1).take(10) {
            s.push_str(&format!(", \"{}\"", a));
        }
        s.push_str("})");
        zlog!(NOTICE, "{}", s);
        self.sockets_cleanup(Cleanup::ParentExec);
        if self.ev.epollfd > 0 {
            unsafe { libc::close(self.ev.epollfd) };
        }
        self.stdio_restore_original_stderr(true);
        let cargs: Vec<std::ffi::CString> = self
            .argv
            .iter()
            .map(|a| std::ffi::CString::new(a.clone()).unwrap_or_default())
            .collect();
        let mut ptrs: Vec<*const libc::c_char> = cargs.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        // the executable that is running (argv[0] may be relative to a
        // directory execvp() does not search)
        let exe = std::ffi::CString::new(self.argv[0].clone()).unwrap_or_default();
        unsafe {
            libc::execvp(exe.as_ptr(), ptrs.as_ptr());
        }
        zlog!(SYSERROR, "failed to reload: execvp() failed");
        std::process::exit(70);
    }

    fn pctl_action_last(&mut self) {
        match self.pctl_state {
            PCTL_STATE_RELOADING => self.pctl_exec(),
            PCTL_STATE_FINISHING | PCTL_STATE_TERMINATING => self.pctl_exit(),
            _ => {}
        }
    }

    fn pctl_action_next(&mut self) {
        if self.running_children == 0 {
            self.pctl_action_last();
        }
        let (sig, timeout) = if self.pctl_signal_sent == 0 {
            (
                if self.pctl_state == PCTL_STATE_TERMINATING {
                    libc::SIGTERM
                } else {
                    libc::SIGQUIT
                },
                self.gc.process_control_timeout,
            )
        } else {
            (
                if self.pctl_signal_sent == libc::SIGQUIT {
                    libc::SIGTERM
                } else {
                    libc::SIGKILL
                },
                1,
            )
        };
        self.pctl_kill_all(sig);
        self.pctl_signal_sent = sig;
        self.event_add_timer(TimerKind::Pctl, timeout.max(0) as u64 * 1000, false);
    }

    /// fpm_pctl()
    pub fn pctl(&mut self, new_state: i32, action: Action) {
        match action {
            Action::Set => {
                if self.pctl_state == new_state {
                    return;
                }
                let names = ["", "normal", "reloading", "terminating", "finishing"];
                let blocked = match self.pctl_state {
                    PCTL_STATE_NORMAL => false,
                    PCTL_STATE_RELOADING => {
                        new_state != PCTL_STATE_FINISHING && new_state != PCTL_STATE_TERMINATING
                    }
                    PCTL_STATE_FINISHING => new_state != PCTL_STATE_TERMINATING,
                    _ => true,
                };
                if blocked {
                    zlog!(
                        DEBUG,
                        "not switching to '{}' state, because already in '{}' state",
                        names[new_state as usize],
                        names[self.pctl_state as usize]
                    );
                    return;
                }
                self.pctl_signal_sent = 0;
                self.pctl_state = new_state;
                zlog!(DEBUG, "switching to '{}' state", names[new_state as usize]);
                self.pctl_action_next();
            }
            Action::Timeout => self.pctl_action_next(),
            Action::LastChildExited => self.pctl_action_last(),
        }
    }

    /// fpm_pctl_child_exited()
    pub fn pctl_child_exited(&mut self) {
        if self.pctl_state == PCTL_STATE_NORMAL {
            return;
        }
        if self.running_children == 0 {
            self.pctl(0, Action::LastChildExited);
        }
    }

    /// fpm_pctl_check_request_timeout()
    fn pctl_check_request_timeout(&mut self, now: Timeval) {
        for i in 0..self.pools.len() {
            let c = &self.pools[i].config;
            let (track, term, slow) = (
                c.request_terminate_timeout_track_finished != 0,
                c.request_terminate_timeout,
                c.request_slowlog_timeout,
            );
            if term != 0 || slow != 0 {
                let pids: Vec<i32> = self
                    .children
                    .iter()
                    .filter(|c| c.linked && c.pool == i)
                    .map(|c| c.pid)
                    .collect();
                for pid in pids {
                    self.request_check_timed_out(pid, now, term, slow, track);
                }
            }
        }
    }

    /// fpm_pctl_kill_idle_child()
    fn pctl_kill_idle_child(&mut self, pid: i32) {
        if let Some(c) = self.children.iter_mut().find(|c| c.pid == pid && c.linked) {
            if c.idle_kill {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            } else {
                c.idle_kill = true;
                unsafe { libc::kill(pid, libc::SIGQUIT) };
            }
        }
    }

    /// fpm_pctl_perform_idle_server_maintenance()
    fn pctl_perform_idle_server_maintenance(&mut self) {
        for i in 0..self.pools.len() {
            let mut cur_lq = 0u32;
            if self.pools[i].listen_address_domain == super::Domain::Inet {
                match super::sockets::get_listening_queue(self.pools[i].listening_socket) {
                    Some((cur, _)) => cur_lq = cur,
                    None => cur_lq = 0,
                }
            }
            let sb = self.pools[i].scoreboard;
            scoreboard::update_begin(sb);
            let mut idle = 0;
            let mut active = 0;
            let mut last_idle: Option<(i32, Timeval)> = None;
            for c in self.children.iter().filter(|c| c.linked && c.pool == i) {
                if self.request_is_idle(c) {
                    match last_idle {
                        None => last_idle = Some((c.pid, c.started)),
                        Some((_, t)) if c.started.lt(&t) => last_idle = Some((c.pid, c.started)),
                        _ => {}
                    }
                    idle += 1;
                } else {
                    active += 1;
                }
            }
            scoreboard::update_commit(
                idle,
                active,
                cur_lq as i32,
                -1,
                -1,
                -1,
                0,
                0,
                scoreboard::ACTION_SET,
                sb,
            );
            let name = self.pools[i].config.name().to_string();
            let pm = self.pools[i].config.pm;
            if pm == super::conf::PM_STYLE_ONDEMAND {
                zlog!(
                    DEBUG,
                    "[pool {}] currently {} active children, {} spare children",
                    name,
                    active,
                    idle
                );
                let Some((pid, _)) = last_idle else {
                    continue;
                };
                let last = self.request_last_activity(pid);
                let now = scoreboard::clock_get();
                if let Some(last) = last {
                    if last.sec < now.sec - self.pools[i].config.pm_process_idle_timeout as i64 {
                        self.pctl_kill_idle_child(pid);
                    }
                }
                continue;
            }
            if pm != super::conf::PM_STYLE_DYNAMIC {
                continue;
            }
            let wp = &self.pools[i];
            zlog!(
                DEBUG,
                "[pool {}] currently {} active children, {} spare children, {} running children. Spawning rate {}",
                name,
                active,
                idle,
                wp.running_children,
                wp.idle_spawn_rate
            );
            if idle > wp.config.pm_max_spare_servers {
                if let Some((pid, _)) = last_idle {
                    self.pctl_kill_idle_child(pid);
                    self.pools[i].idle_spawn_rate = 1;
                    continue;
                }
            }
            if idle < wp.config.pm_min_spare_servers {
                if wp.running_children >= wp.config.pm_max_children {
                    if !wp.warn_max_children && wp.shared.is_none() {
                        scoreboard::update(0, 0, 0, 0, 0, 1, 0, 0, scoreboard::ACTION_INC, sb);
                        zlog!(
                            WARNING,
                            "[pool {}] server reached pm.max_children setting ({}), consider raising it",
                            name,
                            wp.config.pm_max_children
                        );
                        self.pools[i].warn_max_children = true;
                    }
                    self.pools[i].idle_spawn_rate = 1;
                    continue;
                }
                if wp.idle_spawn_rate >= 8 {
                    zlog!(
                        WARNING,
                        "[pool {}] seems busy (you may need to increase pm.start_servers, or pm.min/max_spare_servers), spawning {} children, there are {} idle, and {} total children",
                        name,
                        wp.idle_spawn_rate,
                        idle,
                        wp.running_children
                    );
                }
                let mut to_fork = wp
                    .idle_spawn_rate
                    .min(wp.config.pm_min_spare_servers - idle);
                to_fork = to_fork.min(wp.config.pm_max_children - wp.running_children);
                if to_fork <= 0 {
                    if !wp.warn_max_children && wp.shared.is_none() {
                        scoreboard::update(0, 0, 0, 0, 0, 1, 0, 0, scoreboard::ACTION_INC, sb);
                        zlog!(
                            WARNING,
                            "[pool {}] server reached pm.max_children setting ({}), consider raising it",
                            name,
                            wp.config.pm_max_children
                        );
                        self.pools[i].warn_max_children = true;
                    }
                    self.pools[i].idle_spawn_rate = 1;
                    continue;
                }
                self.pools[i].warn_max_children = false;
                self.children_make(i, true, to_fork, true);
                zlog!(
                    DEBUG,
                    "[pool {}] {} child(ren) have been created dynamically",
                    name,
                    to_fork
                );
                if self.pools[i].idle_spawn_rate < self.pools[i].config.pm_max_spawn_rate {
                    self.pools[i].idle_spawn_rate *= 2;
                }
                continue;
            }
            self.pools[i].idle_spawn_rate = 1;
        }
    }

    /// fpm_pctl_on_socket_accept()
    fn pctl_on_socket_accept(&mut self, i: usize) {
        self.pools[i].socket_event_set = false;
        let wp = &self.pools[i];
        if wp.running_children >= wp.config.pm_max_children {
            if !wp.warn_max_children && wp.shared.is_none() {
                scoreboard::update(
                    0,
                    0,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    scoreboard::ACTION_INC,
                    wp.scoreboard,
                );
                zlog!(
                    WARNING,
                    "[pool {}] server reached max_children setting ({}), consider raising it",
                    wp.config.name(),
                    wp.config.pm_max_children
                );
                self.pools[i].warn_max_children = true;
            }
            return;
        }
        if self
            .children
            .iter()
            .filter(|c| c.linked && c.pool == i)
            .any(|c| self.request_is_idle(c))
        {
            return;
        }
        self.pools[i].warn_max_children = false;
        self.children_make(i, true, 1, true);
        zlog!(
            DEBUG,
            "[pool {}] got accept without idle child available .... I forked",
            self.pools[i].config.name()
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Set,
    Timeout,
    LastChildExited,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    Child,
    ParentExitMain,
    ParentExec,
}
