//! sapi/fpm/fpm/fpm_children.c, fpm_stdio.c, fpm_unix.c, fpm_env.c and
//! fpm_request.c (the master's side): spawning and burying the workers,
//! their output pipes, users and environment.

use std::os::unix::ffi::OsStrExt;

use super::events::{FdKind, TimerKind};
use super::scoreboard::{self, Timeval};
use super::zlog::{self, DEBUG, ERROR, NOTICE, SYSERROR, WARNING, ZlogStream};
use super::{Child, Domain, Fpm};
use crate::zlog;

const FPM_STDIO_CMD_FLUSH: &[u8] = b"\0fscf\0";

fn cstring(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).unwrap_or_default()
}

fn is_id(name: &str) -> bool {
    name.bytes().all(|c| c.is_ascii_digit())
}

/// getpwnam(): (uid, gid, name, dir)
pub fn getpwnam(name: &str) -> Option<(u32, u32, String, String)> {
    let c = cstring(name);
    let p = unsafe { libc::getpwnam(c.as_ptr()) };
    if p.is_null() {
        return None;
    }
    let pw = unsafe { &*p };
    Some((
        pw.pw_uid,
        pw.pw_gid,
        unsafe { std::ffi::CStr::from_ptr(pw.pw_name) }
            .to_string_lossy()
            .into_owned(),
        unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) }
            .to_string_lossy()
            .into_owned(),
    ))
}

pub fn getpwuid(uid: u32) -> Option<(u32, u32, String, String)> {
    let p = unsafe { libc::getpwuid(uid) };
    if p.is_null() {
        return None;
    }
    let pw = unsafe { &*p };
    Some((
        pw.pw_uid,
        pw.pw_gid,
        unsafe { std::ffi::CStr::from_ptr(pw.pw_name) }
            .to_string_lossy()
            .into_owned(),
        unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) }
            .to_string_lossy()
            .into_owned(),
    ))
}

fn getgrnam(name: &str) -> Option<u32> {
    let c = cstring(name);
    let p = unsafe { libc::getgrnam(c.as_ptr()) };
    if p.is_null() {
        return None;
    }
    Some(unsafe { (*p).gr_gid })
}

fn str_purify_filename(s: &[u8]) -> String {
    let s: Vec<u8> = s
        .iter()
        .map(|&c| if c == b'\n' || c == b'\r' { b'?' } else { c })
        .collect();
    String::from_utf8_lossy(&s).into_owned()
}

impl Fpm {
    // ---- fpm_unix.c ----

    fn unix_get_passwd(
        &self,
        wp: usize,
        name: &str,
        flags: i32,
    ) -> Option<(u32, u32, String, String)> {
        let r = getpwnam(name);
        if r.is_none() {
            zlog!(
                flags,
                "[pool {}] cannot get uid for user '{}'",
                self.pools[wp].config.name(),
                name
            );
        }
        r
    }

    fn unix_get_group(&self, wp: usize, name: &str, flags: i32) -> Option<u32> {
        let r = getgrnam(name);
        if r.is_none() {
            zlog!(
                flags,
                "[pool {}] cannot get gid for group '{}'",
                self.pools[wp].config.name(),
                name
            );
        }
        r
    }

    /// fpm_unix_test_config()
    pub fn unix_test_config(&self, wp: usize) -> bool {
        let c = &self.pools[wp].config;
        let check_passwd = |n: &Option<String>, flags: i32| match n {
            None => true,
            Some(n) => is_id(n) || self.unix_get_passwd(wp, n, flags).is_some(),
        };
        let check_group = |n: &Option<String>, flags: i32| match n {
            None => true,
            Some(n) => is_id(n) || self.unix_get_group(wp, n, flags).is_some(),
        };
        let check_listen = || {
            if self.pools[wp].listen_address_domain != Domain::Unix {
                return true;
            }
            let address = c.listen_address.clone().unwrap_or_default();
            let socket_length = 108usize;
            if address.len() < socket_length {
                return true;
            }
            zlog!(
                SYSERROR,
                "[pool {}] cannot bind to UNIX socket '{}' as path is too long (found length: {}, maximal length: {})",
                c.name(),
                address,
                address.len(),
                socket_length
            );
            false
        };
        check_passwd(&c.user, ERROR)
            && check_group(&c.group, ERROR)
            && check_listen()
            && check_passwd(&c.listen_owner, SYSERROR)
            && check_group(&c.listen_group, SYSERROR)
    }

    /// fpm_unix_resolve_socket_permissions()
    pub fn unix_resolve_socket_permissions(&mut self, wp: usize) -> i32 {
        self.pools[wp].socket_uid = -1;
        self.pools[wp].socket_gid = -1;
        self.pools[wp].socket_mode = 0o660;
        let c = self.pools[wp].config.clone();
        if let Some(m) = c.listen_mode.as_deref().filter(|m| !m.is_empty()) {
            self.pools[wp].socket_mode = strtoul(m, 8) as u32;
        }
        self.pools[wp].socket_acl = None;
        let users = c.listen_acl_users.clone().filter(|s| !s.is_empty());
        let groups = c.listen_acl_groups.clone().filter(|s| !s.is_empty());
        if users.is_some() || groups.is_some() {
            let name = c.name().to_string();
            let mut acl: Vec<(u16, u32)> = Vec::new();
            if let Some(users) = users {
                for u in users.split(',') {
                    let Some(pw) = self.unix_get_passwd(wp, u, SYSERROR) else {
                        return -1;
                    };
                    zlog!(DEBUG, "[pool {}] user '{}' have uid={}", name, u, pw.0);
                    acl.push((ACL_USER, pw.0));
                }
            }
            if let Some(groups) = groups {
                for g in groups.split(',') {
                    let Some(gid) = self.unix_get_group(wp, g, SYSERROR) else {
                        return -1;
                    };
                    zlog!(DEBUG, "[pool {}] group '{}' have gid={}", name, g, gid);
                    acl.push((ACL_GROUP, gid));
                }
            }
            if let Some(o) = c.listen_owner.as_deref().filter(|s| !s.is_empty()) {
                zlog!(
                    WARNING,
                    "[pool {}] ACL set, listen.owner = '{}' is ignored",
                    name,
                    o
                );
            }
            if let Some(g) = c.listen_group.as_deref().filter(|s| !s.is_empty()) {
                zlog!(
                    WARNING,
                    "[pool {}] ACL set, listen.group = '{}' is ignored",
                    name,
                    g
                );
            }
            self.pools[wp].socket_acl = Some(acl);
            return 0;
        }
        if let Some(o) = c.listen_owner.as_deref().filter(|m| !m.is_empty()) {
            if is_id(o) {
                self.pools[wp].socket_uid = strtoul(o, 10) as i32;
            } else {
                let Some(pw) = self.unix_get_passwd(wp, o, SYSERROR) else {
                    return -1;
                };
                self.pools[wp].socket_uid = pw.0 as i32;
                self.pools[wp].socket_gid = pw.1 as i32;
            }
        }
        if let Some(g) = c.listen_group.as_deref().filter(|m| !m.is_empty()) {
            if is_id(g) {
                self.pools[wp].socket_gid = strtoul(g, 10) as i32;
            } else {
                let Some(gid) = self.unix_get_group(wp, g, SYSERROR) else {
                    return -1;
                };
                self.pools[wp].socket_gid = gid as i32;
            }
        }
        0
    }

    /// fpm_unix_set_socket_permissions()
    pub fn unix_set_socket_permissions(&self, wp: usize, path: &[u8]) -> i32 {
        let p = &self.pools[wp];
        if let Some(acl) = &p.socket_acl {
            let name = p.config.name();
            let Some(mut entries) = acl_get_file(path) else {
                zlog!(
                    SYSERROR,
                    "[pool {}] failed to read the ACL of the socket '{}'",
                    name,
                    String::from_utf8_lossy(path)
                );
                return -1;
            };
            for &(tag, id) in acl {
                entries.retain(|e| !(e.0 == tag && e.2 == id));
                entries.push((tag, ACL_READ | ACL_WRITE, id));
            }
            // acl_calc_mask()
            let mask = entries
                .iter()
                .filter(|e| matches!(e.0, ACL_USER | ACL_GROUP_OBJ | ACL_GROUP))
                .fold(0u16, |m, e| m | e.1);
            entries.retain(|e| e.0 != ACL_MASK);
            entries.push((ACL_MASK, mask, ACL_UNDEFINED_ID));
            entries.sort_by_key(|e| (e.0, e.2));
            if !acl_set_file(path, &entries) {
                zlog!(
                    SYSERROR,
                    "[pool {}] failed to write the ACL of the socket '{}'",
                    name,
                    String::from_utf8_lossy(path)
                );
                return -1;
            }
            zlog!(
                DEBUG,
                "[pool {}] ACL of the socket '{}' is set",
                name,
                String::from_utf8_lossy(path)
            );
            return 0;
        }
        if p.socket_uid != -1 || p.socket_gid != -1 {
            let cp = std::ffi::CString::new(path.to_vec()).unwrap_or_default();
            if unsafe { libc::chown(cp.as_ptr(), p.socket_uid as u32, p.socket_gid as u32) } < 0 {
                zlog!(
                    SYSERROR,
                    "[pool {}] failed to chown() the socket '{}'",
                    p.config.name(),
                    p.config.listen_address.as_deref().unwrap_or("")
                );
                return -1;
            }
        }
        0
    }

    /// fpm_unix_conf_wp()
    fn unix_conf_wp(&mut self, wp: usize) -> i32 {
        let is_root = unsafe { libc::geteuid() } == 0;
        let c = self.pools[wp].config.clone();
        let name = c.name().to_string();
        if is_root {
            if let Some(u) = c.user.as_deref().filter(|u| !u.is_empty()) {
                if is_id(u) {
                    self.pools[wp].set_uid = strtoul(u, 10) as u32;
                    if let Some(pw) = getpwuid(self.pools[wp].set_uid) {
                        self.pools[wp].set_gid = pw.1;
                        self.pools[wp].set_user = Some(pw.2);
                    }
                } else {
                    let Some(pw) = self.unix_get_passwd(wp, u, ERROR) else {
                        return -1;
                    };
                    self.pools[wp].set_uid = pw.0;
                    self.pools[wp].set_gid = pw.1;
                    self.pools[wp].user = Some(pw.2);
                    self.pools[wp].home = Some(pw.3);
                }
            }
            if let Some(g) = c.group.as_deref().filter(|g| !g.is_empty()) {
                if is_id(g) {
                    self.pools[wp].set_gid = strtoul(g, 10) as u32;
                } else {
                    let Some(gid) = self.unix_get_group(wp, g, ERROR) else {
                        return -1;
                    };
                    self.pools[wp].set_gid = gid;
                }
            }
            if !self.run_as_root && (self.pools[wp].set_uid == 0 || self.pools[wp].set_gid == 0) {
                zlog!(
                    ERROR,
                    "[pool {}] please specify user and group other than root",
                    name
                );
                return -1;
            }
        } else {
            if c.user.as_deref().is_some_and(|u| !u.is_empty()) {
                zlog!(
                    NOTICE,
                    "[pool {}] 'user' directive is ignored when FPM is not running as root",
                    name
                );
            }
            if c.group.as_deref().is_some_and(|u| !u.is_empty()) {
                zlog!(
                    NOTICE,
                    "[pool {}] 'group' directive is ignored when FPM is not running as root",
                    name
                );
            }
            if c.chroot.as_deref().is_some_and(|u| !u.is_empty()) {
                zlog!(
                    NOTICE,
                    "[pool {}] 'chroot' directive is ignored when FPM is not running as root",
                    name
                );
            }
            if c.process_priority != 64 {
                zlog!(
                    NOTICE,
                    "[pool {}] 'process.priority' directive is ignored when FPM is not running as root",
                    name
                );
            }
            if let Some(pw) = getpwuid(unsafe { libc::getuid() }) {
                self.pools[wp].user = Some(pw.2);
                self.pools[wp].home = Some(pw.3);
            }
        }
        0
    }

    /// fpm_unix_init_main()
    pub fn unix_init_main(&mut self) -> i32 {
        let is_root = unsafe { libc::geteuid() } == 0;
        if self.gc.rlimit_files != 0 {
            let r = libc::rlimit {
                rlim_cur: self.gc.rlimit_files as libc::rlim_t,
                rlim_max: self.gc.rlimit_files as libc::rlim_t,
            };
            if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &r) } < 0 {
                zlog!(
                    SYSERROR,
                    "failed to set rlimit_files for this pool. Please check your system limits or decrease rlimit_files. setrlimit(RLIMIT_NOFILE, {})",
                    self.gc.rlimit_files
                );
                return -1;
            }
        }
        if self.gc.rlimit_core != 0 {
            let v = if self.gc.rlimit_core == -1 {
                libc::RLIM_INFINITY
            } else {
                self.gc.rlimit_core as libc::rlim_t
            };
            let r = libc::rlimit {
                rlim_cur: v,
                rlim_max: v,
            };
            if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &r) } < 0 {
                zlog!(
                    SYSERROR,
                    "failed to set rlimit_core for this pool. Please check your system limits or decrease rlimit_core. setrlimit(RLIMIT_CORE, {})",
                    self.gc.rlimit_core
                );
                return -1;
            }
        }
        if self.gc.daemonize != 0 {
            let mut p = [0i32; 2];
            if unsafe { libc::pipe(p.as_mut_ptr()) } == -1 {
                zlog!(SYSERROR, "failed to create pipe");
                return -1;
            }
            self.send_config_pipe = p;
            let pid = unsafe { libc::fork() };
            match pid {
                -1 => {
                    zlog!(SYSERROR, "failed to daemonize");
                    return -1;
                }
                0 => {
                    unsafe { libc::close(p[0]) };
                }
                _ => {
                    unsafe { libc::close(p[1]) };
                    zlog!(
                        DEBUG,
                        "The calling process is waiting for the master process to ping via fd={}",
                        p[0]
                    );
                    let mut fds = libc::pollfd {
                        fd: p[0],
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let ret = unsafe { libc::poll(&mut fds, 1, 10000) };
                    if ret == -1 {
                        zlog!(SYSERROR, "failed to select");
                        std::process::exit(70);
                    }
                    if ret > 0 {
                        let mut v = [0u8; 4];
                        let r = unsafe { libc::read(p[0], v.as_mut_ptr() as *mut libc::c_void, 4) };
                        if r == -1 {
                            zlog!(SYSERROR, "failed to read from pipe");
                            std::process::exit(70);
                        }
                        if r == 0 {
                            zlog!(ERROR, "no data have been read from pipe");
                            std::process::exit(70);
                        }
                        if i32::from_ne_bytes(v) == 1 {
                            zlog!(
                                DEBUG,
                                "I received a valid acknowledge from the master process, I can exit without error"
                            );
                            std::process::exit(0);
                        }
                        zlog!(DEBUG, "The master process returned an error !");
                        std::process::exit(70);
                    }
                    zlog!(
                        ERROR,
                        "the master process didn't send back its status (via the pipe to the calling process)"
                    );
                    std::process::exit(70);
                }
            }
        }
        unsafe { libc::setsid() };
        if self.gc.process_priority != 64 {
            if is_root {
                if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, self.gc.process_priority) } < 0
                {
                    zlog!(SYSERROR, "Unable to set priority for the master process");
                    return -1;
                }
            } else {
                zlog!(
                    NOTICE,
                    "'process.priority' directive is ignored when FPM is not running as root"
                );
            }
        }
        self.parent_pid = unsafe { libc::getpid() };
        super::events::PARENT_PID.store(self.parent_pid, std::sync::atomic::Ordering::Relaxed);
        for i in 0..self.pools.len() {
            if self.unix_conf_wp(i) < 0 {
                return -1;
            }
        }
        0
    }

    /// fpm_unix_init_child()
    pub fn unix_init_child(&mut self, wp: usize) -> i32 {
        let is_root = unsafe { libc::geteuid() } == 0;
        let c = self.pools[wp].config.clone();
        let name = c.name().to_string();
        let mut made_chroot = false;
        if c.rlimit_files != 0 {
            let r = libc::rlimit {
                rlim_cur: c.rlimit_files as libc::rlim_t,
                rlim_max: c.rlimit_files as libc::rlim_t,
            };
            if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &r) } < 0 {
                zlog!(
                    SYSERROR,
                    "[pool {}] failed to set rlimit_files for this pool. Please check your system limits or decrease rlimit_files. setrlimit(RLIMIT_NOFILE, {})",
                    name,
                    c.rlimit_files
                );
            }
        }
        if c.rlimit_core != 0 {
            let v = if c.rlimit_core == -1 {
                libc::RLIM_INFINITY
            } else {
                c.rlimit_core as libc::rlim_t
            };
            let r = libc::rlimit {
                rlim_cur: v,
                rlim_max: v,
            };
            if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &r) } < 0 {
                zlog!(
                    SYSERROR,
                    "[pool {}] failed to set rlimit_core for this pool. Please check your system limits or decrease rlimit_core. setrlimit(RLIMIT_CORE, {})",
                    name,
                    c.rlimit_core
                );
            }
        }
        if is_root {
            if let Some(ch) = c.chroot.as_deref().filter(|s| !s.is_empty()) {
                let cs = cstring(ch);
                if unsafe { libc::chroot(cs.as_ptr()) } < 0 {
                    zlog!(SYSERROR, "[pool {}] failed to chroot({})", name, ch);
                    return -1;
                }
                made_chroot = true;
            }
        }
        if let Some(d) = c.chdir.as_deref().filter(|s| !s.is_empty()) {
            if std::env::set_current_dir(d).is_err() {
                zlog!(SYSERROR, "[pool {}] failed to chdir({})", name, d);
                return -1;
            }
        } else if made_chroot && std::env::set_current_dir("/").is_err() {
            zlog!(WARNING, "[pool {}] failed to chdir(/)", name);
        }
        if is_root {
            if c.process_priority != 64
                && unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, c.process_priority) } < 0
            {
                zlog!(
                    SYSERROR,
                    "[pool {}] Unable to set priority for this new process",
                    name
                );
                return -1;
            }
            let p = &self.pools[wp];
            if p.set_gid != 0 && unsafe { libc::setgid(p.set_gid) } < 0 {
                zlog!(SYSERROR, "[pool {}] failed to setgid({})", name, p.set_gid);
                return -1;
            }
            if p.set_uid != 0 {
                let user = p.set_user.clone().or(c.user.clone()).unwrap_or_default();
                let cu = cstring(&user);
                if unsafe { libc::initgroups(cu.as_ptr(), p.set_gid) } < 0 {
                    zlog!(
                        SYSERROR,
                        "[pool {}] failed to initgroups({}, {})",
                        name,
                        c.user.as_deref().unwrap_or(""),
                        p.set_gid
                    );
                    return -1;
                }
                if unsafe { libc::setuid(p.set_uid) } < 0 {
                    zlog!(SYSERROR, "[pool {}] failed to setuid({})", name, p.set_uid);
                    return -1;
                }
            }
        }
        if c.process_dumpable != 0 && unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0) } < 0
        {
            zlog!(SYSERROR, "[pool {}] failed to prctl(PR_SET_DUMPABLE)", name);
        }
        0
    }

    // ---- fpm_env.c ----

    /// fpm_env_init_main(): resolve env[] values of the form `$NAME`.
    pub fn env_init_main(&mut self) -> i32 {
        for wp in self.pools.iter_mut() {
            for kv in wp.config.env.iter_mut() {
                if let Some(name) = kv.1.strip_prefix('$') {
                    kv.1 = std::env::var(name).unwrap_or_default();
                }
                if kv.0 == "USER" {
                    wp.user = None;
                }
                if kv.0 == "HOME" {
                    wp.home = None;
                }
            }
        }
        0
    }

    /// fpm_env_init_child()
    pub fn env_init_child(&mut self, wp: usize) -> i32 {
        let p = &self.pools[wp];
        if p.config.clear_env != 0 {
            let keys: Vec<std::ffi::OsString> = std::env::vars_os().map(|(k, _)| k).collect();
            for k in keys {
                unsafe { std::env::remove_var(k) };
            }
        }
        for (k, v) in &p.config.env {
            unsafe { std::env::set_var(k, v) };
        }
        if let Some(u) = &p.user {
            unsafe { std::env::set_var("USER", u) };
        }
        if let Some(h) = &p.home {
            unsafe { std::env::set_var("HOME", h) };
        }
        0
    }

    // ---- fpm_stdio.c ----

    /// fpm_stdio_init_main()
    pub fn stdio_init_main(&mut self) -> i32 {
        let fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            zlog!(SYSERROR, "failed to init stdio: open(\"/dev/null\")");
            return -1;
        }
        if unsafe { libc::dup2(fd, 0) } < 0 || unsafe { libc::dup2(fd, 1) } < 0 {
            zlog!(SYSERROR, "failed to init stdio: dup2()");
            unsafe { libc::close(fd) };
            return -1;
        }
        unsafe { libc::close(fd) };
        0
    }

    /// fpm_use_error_log()
    fn use_error_log(&self) -> bool {
        self.gc.daemonize != 0 || (unsafe { libc::isatty(2) } == 0 && !self.force_stderr)
    }

    /// fpm_stdio_init_final()
    pub fn stdio_init_final(&mut self) -> i32 {
        if self.stdio_redirect_stderr_to_error_log() < 0 {
            return -1;
        }
        if self.use_error_log() && self.error_log_fd == zlog::SYSLOG_FD {
            unsafe { libc::dup2(1, 2) };
        }
        zlog::set_launched();
        0
    }

    /// fpm_stdio_save_original_stderr()
    pub fn stdio_save_original_stderr(&mut self) -> i32 {
        zlog!(DEBUG, "saving original STDERR fd: dup()");
        self.fd_stderr_original = unsafe { libc::dup(2) };
        if self.fd_stderr_original < 0 {
            zlog!(
                SYSERROR,
                "failed to save original STDERR fd, access.log records may appear in error_log: dup()"
            );
            return -1;
        }
        unsafe {
            libc::fcntl(self.fd_stderr_original, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        0
    }

    /// fpm_stdio_restore_original_stderr()
    pub fn stdio_restore_original_stderr(&mut self, close_after_restore: bool) -> i32 {
        if self.fd_stderr_original != -1 {
            zlog!(DEBUG, "restoring original STDERR fd: dup2()");
            if unsafe { libc::dup2(self.fd_stderr_original, 2) } < 0 {
                zlog!(
                    SYSERROR,
                    "failed to restore original STDERR fd, access.log records may appear in error_log: dup2()"
                );
                return -1;
            } else if close_after_restore {
                unsafe { libc::close(self.fd_stderr_original) };
            }
        }
        0
    }

    /// fpm_stdio_redirect_stderr_to_error_log()
    pub fn stdio_redirect_stderr_to_error_log(&mut self) -> i32 {
        if self.use_error_log()
            && self.error_log_fd > 0
            && self.error_log_fd != 2
            && unsafe { libc::dup2(self.error_log_fd, 2) } < 0
        {
            zlog!(
                SYSERROR,
                "failed to tie stderr fd with error_log fd: dup2()"
            );
            return -1;
        }
        0
    }

    /// fpm_stdio_open_error_log()
    pub fn stdio_open_error_log(&mut self, reopen: bool) -> i32 {
        let el = self.gc.error_log.clone().unwrap_or_default();
        if el.eq_ignore_ascii_case("syslog") {
            let ident = cstring(self.gc.syslog_ident.as_deref().unwrap_or("php-fpm"));
            // openlog() keeps the pointer
            let ident = Box::leak(Box::new(ident));
            unsafe {
                libc::openlog(
                    ident.as_ptr(),
                    libc::LOG_PID | libc::LOG_CONS,
                    self.gc.syslog_facility,
                );
            }
            self.error_log_fd = zlog::SYSLOG_FD;
            if self.use_error_log() {
                zlog::set_fd(self.error_log_fd, false);
            }
            return 0;
        }
        let ce = cstring(&el);
        let mut fd = unsafe {
            libc::open(
                ce.as_ptr(),
                libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT,
                (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint,
            )
        };
        if fd < 0 {
            zlog!(SYSERROR, "failed to open error_log ({})", el);
            return -1;
        }
        if reopen {
            unsafe {
                libc::dup2(fd, self.error_log_fd);
                libc::close(fd);
            }
            fd = self.error_log_fd;
        } else {
            self.error_log_fd = fd;
            if self.use_error_log() {
                let is_stderr = el == "/dev/stderr" || el == "/proc/self/fd/2";
                zlog::set_fd(fd, is_stderr);
            }
        }
        unsafe {
            let f = libc::fcntl(fd, libc::F_GETFD);
            if libc::fcntl(fd, libc::F_SETFD, f | libc::FD_CLOEXEC) < 0 {
                zlog!(WARNING, "failed to change attribute of error_log");
            }
        }
        0
    }

    /// fpm_stdio_child_said()
    pub fn stdio_child_said(&mut self, pid: i32, is_stdout: bool) {
        let Some(ci) = self.children.iter().position(|c| c.pid == pid) else {
            return;
        };
        let fd = if is_stdout {
            self.children[ci].fd_stdout
        } else {
            self.children[ci].fd_stderr
        };
        if fd < 0 {
            return;
        }
        let pool = self.children[ci].pool;
        let pname = self.pools[pool].config.name().to_string();
        let decorate = self.pools[pool].config.decorate_workers_output != 0;
        let which = if is_stdout { "stdout" } else { "stderr" };
        let prefix = format!("[pool {}] child {} said into {}: ", pname, pid, which);
        let create = self.children[ci].log_stream.is_none();
        let mut stream = match self.children[ci].log_stream.take() {
            None => {
                let mut s = ZlogStream::init_ex(WARNING, 2);
                s.set_decorating(decorate);
                s.set_wrapping(true);
                s.set_msg_prefix(&prefix);
                s.set_msg_quoting(true);
                s.is_stdout = is_stdout;
                s.child_pid = pid;
                s
            }
            Some(mut s) => {
                if s.is_stdout != is_stdout || s.child_pid != pid {
                    s.finish();
                    s.set_msg_prefix(&prefix);
                    s.is_stdout = is_stdout;
                    s.child_pid = pid;
                }
                s
            }
        };
        let mut read_fail = 0isize;
        let mut cmd_pos = 0usize;
        let mut buf = [0u8; 1024];
        'outer: loop {
            let in_buf =
                unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len() - 1) };
            if in_buf <= 0 {
                let e = zlog::errno();
                if in_buf == 0 || (e != libc::EAGAIN && e != libc::EWOULDBLOCK && e != libc::EINTR)
                {
                    read_fail = if in_buf < 0 { in_buf } else { 1 };
                }
                break;
            }
            let in_buf = in_buf as usize;
            let mut start = 0usize;
            if cmd_pos > 0 {
                let rest = FPM_STDIO_CMD_FLUSH.len() - cmd_pos;
                if rest <= in_buf && buf[..rest] == FPM_STDIO_CMD_FLUSH[cmd_pos..] {
                    stream.finish();
                    start = rest;
                } else {
                    stream.str(&FPM_STDIO_CMD_FLUSH[..cmd_pos]);
                }
                cmd_pos = 0;
            }
            let mut pos = start;
            while pos < in_buf {
                match buf[pos] {
                    b'\n' => {
                        stream.str(&buf[start..pos]);
                        stream.finish();
                        start = pos + 1;
                    }
                    0 => {
                        if pos + FPM_STDIO_CMD_FLUSH.len() <= in_buf {
                            if buf[pos..pos + FPM_STDIO_CMD_FLUSH.len()] == *FPM_STDIO_CMD_FLUSH {
                                stream.str(&buf[start..pos]);
                                stream.finish();
                                start = pos + FPM_STDIO_CMD_FLUSH.len();
                                pos = start;
                                continue;
                            }
                        } else if buf[pos..in_buf] == FPM_STDIO_CMD_FLUSH[..in_buf - pos] {
                            cmd_pos = in_buf - pos;
                            stream.str(&buf[start..pos]);
                            continue 'outer;
                        }
                    }
                    _ => {}
                }
                pos += 1;
            }
            if start < pos {
                stream.str(&buf[start..pos]);
            }
        }
        if read_fail != 0 {
            if create {
                stream.set_msg_suffix(None, Some(", pipe is closed"));
                stream.finish();
            }
            if read_fail < 0 {
                zlog!(
                    SYSERROR,
                    "unable to read what child {} said into {}",
                    pid,
                    which
                );
            }
            let kind = if is_stdout {
                FdKind::ChildStdout(pid)
            } else {
                FdKind::ChildStderr(pid)
            };
            self.event_del_fd(kind);
            let c = &mut self.children[ci];
            c.postponed_free = true;
            unsafe { libc::close(fd) };
            if is_stdout {
                c.fd_stdout = -1;
            } else {
                c.fd_stderr = -1;
            }
        }
        self.children[ci].log_stream = Some(stream);
    }

    // ---- fpm_children.c ----

    /// fpm_child_free()
    fn child_free(&mut self, ci: usize) {
        let mut c = self.children.remove(ci);
        if let Some(mut s) = c.log_stream.take() {
            s.finish();
        }
    }

    /// fpm_postponed_child_free()
    pub fn postponed_child_free(&mut self, pid: i32) {
        let Some(ci) = self.children.iter().position(|c| c.pid == pid && !c.linked) else {
            return;
        };
        if self.children[ci].fd_stdout != -1 {
            self.event_del_fd(FdKind::ChildStdout(pid));
            unsafe { libc::close(self.children[ci].fd_stdout) };
        }
        if self.children[ci].fd_stderr != -1 {
            self.event_del_fd(FdKind::ChildStderr(pid));
            unsafe { libc::close(self.children[ci].fd_stderr) };
        }
        self.child_free(ci);
    }

    /// fpm_child_close()
    fn child_close(&mut self, pid: i32, in_event_loop: bool) {
        let Some(ci) = self.children.iter().position(|c| c.pid == pid) else {
            return;
        };
        for stdout in [true, false] {
            let fd = if stdout {
                self.children[ci].fd_stdout
            } else {
                self.children[ci].fd_stderr
            };
            if fd != -1 {
                if in_event_loop {
                    self.children[ci].postponed_free = true;
                    self.fire_child_stdio(pid, stdout);
                }
                let fd = if stdout {
                    self.children[ci].fd_stdout
                } else {
                    self.children[ci].fd_stderr
                };
                if fd != -1 {
                    let kind = if stdout {
                        FdKind::ChildStdout(pid)
                    } else {
                        FdKind::ChildStderr(pid)
                    };
                    self.event_del_fd(kind);
                    unsafe { libc::close(fd) };
                    if stdout {
                        self.children[ci].fd_stdout = -1;
                    } else {
                        self.children[ci].fd_stderr = -1;
                    }
                }
            }
        }
        if in_event_loop && self.children[ci].postponed_free {
            self.event_add_timer(TimerKind::ChildFree(pid), 1000, false);
        } else {
            self.child_free(ci);
        }
    }

    /// fpm_children_bury()
    pub fn children_bury(&mut self) {
        loop {
            let mut status = 0;
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG | libc::WUNTRACED) };
            if pid <= 0 {
                break;
            }
            let mut severity = NOTICE;
            let mut restart_child = true;
            let ci = self.children.iter().position(|c| c.pid == pid && c.linked);
            let buf;
            let mut termsig = 0;
            if libc::WIFEXITED(status) {
                buf = format!("with code {}", libc::WEXITSTATUS(status));
                if ci.is_some_and(|i| self.children[i].idle_kill) {
                    restart_child = false;
                }
                if libc::WEXITSTATUS(status) != 0 {
                    severity = WARNING;
                }
            } else if libc::WIFSIGNALED(status) {
                termsig = libc::WTERMSIG(status);
                let have_core = if libc::WCOREDUMP(status) {
                    " - core dumped"
                } else {
                    ""
                };
                buf = format!(
                    "on signal {} ({}{})",
                    termsig,
                    super::events::signal_name(termsig),
                    have_core
                );
                if ci.is_some_and(|i| self.children[i].idle_kill) && termsig == libc::SIGQUIT {
                    restart_child = false;
                }
                if termsig != libc::SIGQUIT {
                    severity = WARNING;
                }
            } else if libc::WIFSTOPPED(status) {
                zlog!(NOTICE, "child {} stopped for tracing", pid);
                continue;
            } else {
                buf = String::new();
            }
            if let Some(ci) = ci {
                let wp = self.children[ci].pool;
                // fpm_child_unlink()
                self.children[ci].linked = false;
                self.pools[wp].running_children -= 1;
                self.running_children -= 1;
                let sbi = self.children[ci].scoreboard_i;
                scoreboard::proc_free(
                    self.pools[wp].scoreboard,
                    self.pools[wp].config.pm_max_children,
                    sbi,
                );
                let tv1 = scoreboard::clock_get();
                let tv2 = tv1.sub(&self.children[ci].started);
                let name = self.pools[wp].config.name().to_string();
                if restart_child {
                    if !self.pctl_can_spawn_children() {
                        severity = DEBUG;
                    }
                    zlog!(
                        severity,
                        "[pool {}] child {} exited {} after {}.{:06} seconds from start",
                        name,
                        pid,
                        buf,
                        tv2.sec,
                        tv2.usec
                    );
                } else {
                    zlog!(
                        DEBUG,
                        "[pool {}] child {} has been killed by the process management after {}.{:06} seconds from start",
                        name,
                        pid,
                        tv2.sec,
                        tv2.usec
                    );
                }
                self.child_close(pid, true);
                self.pctl_child_exited();
                if !self.last_faults.is_empty()
                    && (termsig == libc::SIGSEGV || termsig == libc::SIGBUS)
                {
                    let now = tv1.sec;
                    let thr = self.gc.emergency_restart_threshold as usize;
                    self.last_faults[self.fault] = now;
                    self.fault += 1;
                    if self.fault == thr {
                        self.fault = 0;
                    }
                    let restart_condition = self
                        .last_faults
                        .iter()
                        .all(|&t| now - t <= self.gc.emergency_restart_interval as i64);
                    if restart_condition {
                        zlog!(
                            WARNING,
                            "failed processes threshold ({} in {} sec) is reached, initiating reload",
                            self.gc.emergency_restart_threshold,
                            self.gc.emergency_restart_interval
                        );
                        self.pctl(
                            super::events::PCTL_STATE_RELOADING,
                            super::events::Action::Set,
                        );
                    }
                }
                if restart_child {
                    self.children_make(wp, true, 1, false);
                }
            } else if self.parent_pid == 1 {
                zlog!(
                    DEBUG,
                    "unknown child ({}) exited {} - most likely an orphan process (master process is the init process)",
                    pid,
                    buf
                );
            } else {
                zlog!(
                    WARNING,
                    "unknown child ({}) exited {} - potentially a bug or pre exec child (e.g. s6-notifyoncheck)",
                    pid,
                    buf
                );
            }
        }
    }

    /// fpm_children_make(): 0 never returns (the child runs the worker),
    /// 1 = done, 2 = failure.
    pub fn children_make(
        &mut self,
        wp: usize,
        in_event_loop: bool,
        nb_to_spawn: i32,
        is_debug: bool,
    ) -> i32 {
        let c = &self.pools[wp].config;
        let max = match c.pm {
            super::conf::PM_STYLE_DYNAMIC => {
                if !in_event_loop {
                    c.pm_start_servers
                } else {
                    self.pools[wp].running_children + nb_to_spawn
                }
            }
            super::conf::PM_STYLE_ONDEMAND => {
                if !in_event_loop {
                    0
                } else {
                    self.pools[wp].running_children + nb_to_spawn
                }
            }
            _ => c.pm_max_children,
        };
        while self.pctl_can_spawn_children()
            && self.pools[wp].running_children < max
            && (self.gc.process_max < 1 || self.running_children < self.gc.process_max)
        {
            self.children_warned = false;
            // fpm_resources_prepare()
            let mut pipes: Option<([i32; 2], [i32; 2])> = None;
            if self.pools[wp].config.catch_workers_output != 0 {
                let mut o = [0i32; 2];
                let mut e = [0i32; 2];
                if unsafe { libc::pipe(o.as_mut_ptr()) } < 0 {
                    zlog!(SYSERROR, "failed to prepare the stdout pipe");
                    return 2;
                }
                if unsafe { libc::pipe(e.as_mut_ptr()) } < 0 {
                    zlog!(SYSERROR, "failed to prepare the stderr pipe");
                    unsafe {
                        libc::close(o[0]);
                        libc::close(o[1]);
                    }
                    return 2;
                }
                if super::events::set_blocked(o[0], false) < 0
                    || super::events::set_blocked(e[0], false) < 0
                {
                    zlog!(SYSERROR, "failed to unblock pipes");
                    unsafe {
                        libc::close(o[0]);
                        libc::close(o[1]);
                        libc::close(e[0]);
                        libc::close(e[1]);
                    }
                    return 2;
                }
                pipes = Some((o, e));
            }
            let sb = self.pools[wp].scoreboard;
            let Some(sbi) = scoreboard::proc_alloc(sb, self.pools[wp].config.pm_max_children)
            else {
                if let Some((o, e)) = pipes {
                    unsafe {
                        libc::close(o[0]);
                        libc::close(o[1]);
                        libc::close(e[0]);
                        libc::close(e[1]);
                    }
                }
                return 2;
            };
            zlog!(DEBUG, "blocking signals before child birth");
            if super::events::signals_child_block() < 0 {
                zlog!(WARNING, "child may miss signals");
            }
            let pid = unsafe { libc::fork() };
            match pid {
                0 => {
                    // fpm_child_resources_use()
                    for (i, p) in self.pools.iter().enumerate() {
                        if i == wp || Some(i) == self.pools[wp].shared {
                            continue;
                        }
                        scoreboard::free(p.scoreboard, p.config.pm_max_children);
                    }
                    scoreboard::child_use(sb, sbi, unsafe { libc::getpid() });
                    if let Some((o, e)) = pipes {
                        unsafe {
                            libc::dup2(o[1], 1);
                            libc::dup2(e[1], 2);
                            libc::close(o[0]);
                            libc::close(o[1]);
                            libc::close(e[0]);
                            libc::close(e[1]);
                        }
                    } else {
                        unsafe { libc::dup2(1, 2) };
                    }
                    zlog::set_is_child(true);
                    super::worker::child_main(self, wp);
                }
                -1 => {
                    zlog!(DEBUG, "unblocking signals");
                    super::events::signals_unblock();
                    zlog!(SYSERROR, "fork() failed");
                    scoreboard::proc_free(sb, self.pools[wp].config.pm_max_children, sbi);
                    if let Some((o, e)) = pipes {
                        unsafe {
                            libc::close(o[0]);
                            libc::close(o[1]);
                            libc::close(e[0]);
                            libc::close(e[1]);
                        }
                    }
                    return 2;
                }
                _ => {
                    zlog!(DEBUG, "unblocking signals, child born");
                    super::events::signals_unblock();
                    let mut child = Child {
                        pid,
                        pool: wp,
                        started: scoreboard::clock_get(),
                        fd_stdout: -1,
                        fd_stderr: -1,
                        scoreboard_i: sbi,
                        idle_kill: false,
                        postponed_free: false,
                        linked: true,
                        slow_logged: Timeval::default(),
                        log_stream: None,
                    };
                    // fpm_parent_resources_use()
                    if let Some((o, e)) = pipes {
                        unsafe {
                            libc::close(o[1]);
                            libc::close(e[1]);
                        }
                        child.fd_stdout = o[0];
                        child.fd_stderr = e[0];
                    }
                    self.children.insert(0, child);
                    if let Some((o, e)) = pipes {
                        self.event_add_fd(o[0], FdKind::ChildStdout(pid), false);
                        self.event_add_fd(e[0], FdKind::ChildStderr(pid), false);
                    }
                    self.pools[wp].running_children += 1;
                    self.running_children += 1;
                    zlog!(
                        if is_debug { DEBUG } else { NOTICE },
                        "[pool {}] child {} started",
                        self.pools[wp].config.name(),
                        pid
                    );
                }
            }
        }
        if !self.children_warned
            && self.gc.process_max > 0
            && self.running_children >= self.gc.process_max
            && self.pools[wp].running_children < max
        {
            self.children_warned = true;
            zlog!(
                WARNING,
                "The maximum number of processes has been reached. Please review your configuration and consider raising 'process.max'"
            );
        }
        1
    }

    /// fpm_children_create_initial()
    pub fn children_create_initial(&mut self, wp: usize) -> i32 {
        if self.pools[wp].config.pm == super::conf::PM_STYLE_ONDEMAND {
            let fd = self.pools[wp].listening_socket;
            self.event_add_fd(fd, FdKind::Ondemand(wp), true);
            self.pools[wp].socket_event_set = true;
            return 1;
        }
        self.children_make(wp, false, 0, true)
    }

    /// fpm_children_init_main()
    pub fn children_init_main(&mut self) -> i32 {
        if self.gc.emergency_restart_threshold != 0 && self.gc.emergency_restart_interval != 0 {
            self.last_faults = vec![0; self.gc.emergency_restart_threshold as usize];
        }
        0
    }

    // ---- fpm_request.c (master side) ----

    fn child_proc(&self, c: &Child) -> Option<*mut scoreboard::Proc> {
        let wp = &self.pools[c.pool];
        if c.scoreboard_i < 0 || c.scoreboard_i >= wp.config.pm_max_children {
            return None;
        }
        scoreboard::proc_get(wp.scoreboard, c.scoreboard_i)
    }

    /// fpm_request_is_idle()
    pub fn request_is_idle(&self, c: &Child) -> bool {
        match self.child_proc(c) {
            Some(p) => unsafe { (*p).request_stage == scoreboard::REQUEST_ACCEPTING },
            None => false,
        }
    }

    /// fpm_request_last_activity()
    pub fn request_last_activity(&self, pid: i32) -> Option<Timeval> {
        let c = self.children.iter().find(|c| c.pid == pid && c.linked)?;
        let p = self.child_proc(c)?;
        Some(unsafe { (*p).tv })
    }

    /// fpm_request_check_timed_out()
    pub fn request_check_timed_out(
        &mut self,
        pid: i32,
        now: Timeval,
        terminate_timeout: i32,
        slowlog_timeout: i32,
        track_finished: bool,
    ) {
        let Some(ci) = self.children.iter().position(|c| c.pid == pid && c.linked) else {
            return;
        };
        let wp = self.children[ci].pool;
        let sbi = self.children[ci].scoreboard_i;
        let Some(pp) = scoreboard::proc_acquire(self.pools[wp].scoreboard, sbi, true) else {
            zlog!(NOTICE, "failed to acquire scoreboard");
            return;
        };
        let proc = unsafe { *pp };
        scoreboard::proc_release(pp);
        let slow = &mut self.children[ci].slow_logged;
        if slow.sec != 0 && (slow.sec != proc.accepted.sec || slow.usec != proc.accepted.usec) {
            *slow = Timeval::default();
        }
        if proc.request_stage > scoreboard::REQUEST_ACCEPTING
            && (proc.request_stage < scoreboard::REQUEST_END || track_finished)
        {
            let tv = now.sub(&proc.accepted);
            let name = self.pools[wp].config.name().to_string();
            let sf = str_purify_filename(scoreboard::cstr(&proc.script_filename));
            let method =
                String::from_utf8_lossy(scoreboard::cstr(&proc.request_method)).into_owned();
            let uri = String::from_utf8_lossy(scoreboard::cstr(&proc.request_uri)).into_owned();
            let qs = String::from_utf8_lossy(scoreboard::cstr(&proc.query_string)).into_owned();
            if self.children[ci].slow_logged.sec == 0
                && slowlog_timeout != 0
                && proc.request_stage == scoreboard::REQUEST_EXECUTING
                && tv.sec >= slowlog_timeout as i64
            {
                self.children[ci].slow_logged = proc.accepted;
                zlog!(
                    WARNING,
                    "[pool {}] child {}, script '{}' (request: \"{} {}{}{}\") executing too slow ({}.{:06} sec), logging",
                    name,
                    pid,
                    sf,
                    method,
                    uri,
                    if qs.is_empty() { "" } else { "?" },
                    qs,
                    tv.sec,
                    tv.usec
                );
                self.php_trace(ci);
            } else if terminate_timeout != 0 && tv.sec >= terminate_timeout as i64 {
                unsafe { libc::kill(pid, libc::SIGTERM) };
                zlog!(
                    WARNING,
                    "[pool {}] child {}, script '{}' (request: \"{} {}{}{}\") execution timed out ({}.{:06} sec), terminating",
                    name,
                    pid,
                    sf,
                    method,
                    uri,
                    if qs.is_empty() { "" } else { "?" },
                    qs,
                    tv.sec,
                    tv.usec
                );
            }
        }
    }

    /// fpm_php_trace(): goro cannot ptrace() the worker; it asks the worker
    /// to write its own backtrace into the slowlog (SIGURG, handled between
    /// two VM instructions).
    fn php_trace(&mut self, ci: usize) {
        let wp = self.children[ci].pool;
        let pid = self.children[ci].pid;
        scoreboard::update(
            0,
            0,
            0,
            0,
            0,
            0,
            1,
            0,
            scoreboard::ACTION_INC,
            self.pools[wp].scoreboard,
        );
        zlog!(NOTICE, "about to trace {}", pid);
        unsafe { libc::kill(pid, libc::SIGURG) };
        zlog!(NOTICE, "finished trace of {}", pid);
    }
}

const ACL_USER_OBJ: u16 = 0x01;
const ACL_USER: u16 = 0x02;
const ACL_GROUP_OBJ: u16 = 0x04;
const ACL_GROUP: u16 = 0x08;
const ACL_MASK: u16 = 0x10;
const ACL_OTHER: u16 = 0x20;
const ACL_READ: u16 = 0x04;
const ACL_WRITE: u16 = 0x02;
const ACL_UNDEFINED_ID: u32 = u32::MAX;
const ACL_EA_VERSION: u32 = 2;

/// acl_get_file(ACL_TYPE_ACCESS): the entries of the file's access ACL
/// (from its mode when it has no extended ACL), as (tag, perm, id).
fn acl_get_file(path: &[u8]) -> Option<Vec<(u16, u16, u32)>> {
    let cp = std::ffi::CString::new(path.to_vec()).ok()?;
    let name = c"system.posix_acl_access";
    let mut buf = vec![0u8; 4096];
    let n = unsafe {
        libc::getxattr(
            cp.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
        )
    };
    if n >= 4 {
        let b = &buf[..n as usize];
        let mut out = Vec::new();
        let mut i = 4;
        while i + 8 <= b.len() {
            out.push((
                u16::from_le_bytes([b[i], b[i + 1]]),
                u16::from_le_bytes([b[i + 2], b[i + 3]]),
                u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]),
            ));
            i += 8;
        }
        return Some(out);
    }
    let md = std::fs::metadata(std::ffi::OsStr::from_bytes(path)).ok()?;
    use std::os::unix::fs::PermissionsExt;
    let mode = md.permissions().mode();
    Some(vec![
        (ACL_USER_OBJ, ((mode >> 6) & 7) as u16, ACL_UNDEFINED_ID),
        (ACL_GROUP_OBJ, ((mode >> 3) & 7) as u16, ACL_UNDEFINED_ID),
        (ACL_OTHER, (mode & 7) as u16, ACL_UNDEFINED_ID),
    ])
}

/// acl_set_file(ACL_TYPE_ACCESS)
fn acl_set_file(path: &[u8], entries: &[(u16, u16, u32)]) -> bool {
    let Ok(cp) = std::ffi::CString::new(path.to_vec()) else {
        return false;
    };
    let mut b = ACL_EA_VERSION.to_le_bytes().to_vec();
    for &(tag, perm, id) in entries {
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&perm.to_le_bytes());
        b.extend_from_slice(&id.to_le_bytes());
    }
    let name = c"system.posix_acl_access";
    unsafe {
        libc::setxattr(
            cp.as_ptr(),
            name.as_ptr(),
            b.as_ptr() as *const libc::c_void,
            b.len(),
            0,
        ) == 0
    }
}

/// strtoul()
fn strtoul(s: &str, radix: u32) -> u64 {
    let t: String = s
        .trim_start()
        .chars()
        .take_while(|c| c.is_digit(radix))
        .collect();
    u64::from_str_radix(&t, radix).unwrap_or(0)
}
