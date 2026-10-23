//! The FastCGI Process Manager SAPI (sapi/fpm): the php-fpm master process
//! (configuration, sockets, the process manager, logging) and its workers
//! running the FastCGI requests. goro runs as php-fpm when invoked under a
//! name containing "fpm" (e.g. a `php-fpm` symlink to the goro binary).

pub mod children;
pub mod conf;
pub mod events;
pub mod fastcgi;
pub mod log;
pub mod scoreboard;
pub mod sockets;
pub mod status;
pub mod worker;
pub mod zlog;

use std::os::unix::ffi::OsStrExt;

use goro::engine::{self, IniSettings};
use goro_core::vm::Vm;

use crate::getopt::{GETOPT_INVALID_ARG, GetOpt, Opt};
use crate::zlog;
use conf::{GlobalConfig, PoolConfig};
use events::{Cleanup, Events};
use scoreboard::Timeval;
use zlog::{DEBUG, ERROR, NOTICE, WARNING};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    Unix,
    Inet,
}

/// struct fpm_worker_pool_s
pub struct Pool {
    pub config: PoolConfig,
    /// The pool whose status this (pm.status_listen) pool serves.
    pub shared: Option<usize>,
    pub user: Option<String>,
    pub home: Option<String>,
    pub listen_address_domain: Domain,
    pub listening_socket: i32,
    pub set_uid: u32,
    pub set_gid: u32,
    pub set_user: Option<String>,
    pub socket_uid: i32,
    pub socket_gid: i32,
    pub socket_mode: u32,
    /// listen.acl_users / listen.acl_groups: the (tag, uid or gid) entries.
    pub socket_acl: Option<Vec<(u16, u32)>>,
    pub running_children: i32,
    pub idle_spawn_rate: i32,
    pub warn_max_children: bool,
    pub scoreboard: *mut scoreboard::Scoreboard,
    pub log_fd: i32,
    pub limit_extensions: Option<Vec<String>>,
    pub socket_event_set: bool,
}

impl Pool {
    pub fn new(config: PoolConfig) -> Pool {
        Pool {
            config,
            shared: None,
            user: None,
            home: None,
            listen_address_domain: Domain::Unix,
            listening_socket: -1,
            set_uid: 0,
            set_gid: 0,
            set_user: None,
            socket_uid: -1,
            socket_gid: -1,
            socket_mode: 0o660,
            socket_acl: None,
            running_children: 0,
            idle_spawn_rate: 1,
            warn_max_children: false,
            scoreboard: std::ptr::null_mut(),
            log_fd: -1,
            limit_extensions: None,
            socket_event_set: false,
        }
    }
}

/// struct fpm_child_s
pub struct Child {
    pub pid: i32,
    pub pool: usize,
    pub started: Timeval,
    pub fd_stdout: i32,
    pub fd_stderr: i32,
    pub scoreboard_i: i32,
    pub idle_kill: bool,
    pub postponed_free: bool,
    /// Still in its pool's children list (not exited yet).
    pub linked: bool,
    pub slow_logged: Timeval,
    pub log_stream: Option<zlog::ZlogStream>,
}

/// fpm_globals plus the state of the master's modules.
pub struct Fpm {
    pub parent_pid: i32,
    pub argv: Vec<Vec<u8>>,
    pub config: Option<String>,
    pub prefix: Option<String>,
    pub pid: Option<String>,
    pub running_children: i32,
    pub error_log_fd: i32,
    pub log_level: i32,
    pub test_successful: bool,
    pub heartbeat: i32,
    pub run_as_root: bool,
    pub force_stderr: bool,
    pub send_config_pipe: [i32; 2],
    pub gc: GlobalConfig,
    pub pools: Vec<Pool>,
    pub children: Vec<Child>,
    // fpm_conf.c
    pub current_wp: Option<usize>,
    pub ini_recursion: i32,
    pub ini_filename: String,
    pub ini_lineno: i32,
    pub ini_include: Option<String>,
    // fpm_events.c / fpm_signals.c / fpm_process_ctl.c
    pub ev: Events,
    pub sp: [i32; 2],
    pub pctl_state: i32,
    pub pctl_signal_sent: i32,
    // fpm_stdio.c
    pub fd_stderr_original: i32,
    // fpm_sockets.c
    pub sockets: Vec<sockets::ListeningSocket>,
    // fpm_children.c
    pub last_faults: Vec<i64>,
    pub fault: usize,
    pub children_warned: bool,
    /// The PHP INI settings of the module (php.ini and -d)
    pub php_ini: IniSettings,
}

/// A scratch engine for the INI parser (constants in values).
pub fn scratch_vm() -> Vm {
    let mut vm = Vm::new();
    goro_ext_standard::register_standard_functions(&mut vm);
    vm
}

/// php-fpm's OPTIONS[]
const fn o(opt_char: i32, need_param: u8, opt_name: &'static str) -> Opt {
    Opt {
        opt_char,
        need_param,
        opt_name,
    }
}

const FPM_OPTIONS: &[Opt] = &[
    o('c' as i32, 1, "php-ini"),
    o('d' as i32, 1, "define"),
    o('e' as i32, 0, "profile-info"),
    o('h' as i32, 0, "help"),
    o('i' as i32, 0, "info"),
    o('m' as i32, 0, "modules"),
    o('n' as i32, 0, "no-php-ini"),
    o('?' as i32, 0, "usage"),
    o('v' as i32, 0, "version"),
    o('y' as i32, 1, "fpm-config"),
    o('t' as i32, 0, "test"),
    o('p' as i32, 1, "prefix"),
    o('g' as i32, 1, "pid"),
    o('R' as i32, 0, "allow-to-run-as-root"),
    o('D' as i32, 0, "daemonize"),
    o('F' as i32, 0, "nodaemonize"),
    o('O' as i32, 0, "force-stderr"),
];

/// php_cgi_usage()
fn usage(argv0: &[u8]) -> String {
    let a = String::from_utf8_lossy(argv0);
    let prog = match a.rfind('/') {
        Some(p) => &a[p + 1..],
        None => "php",
    };
    format!(
        "Usage: {p} [-n] [-e] [-h] [-i] [-m] [-v] [-t] [-p <prefix>] [-g <pid>] [-c <file>] [-d foo[=bar]] [-y <file>] [-D] [-F [-O]]\n\
\x20 -c <path>|<file> Look for php.ini file in this directory\n\
\x20 -n               No php.ini file will be used\n\
\x20 -d foo[=bar]     Define INI entry foo with value 'bar'\n\
\x20 -e               Generate extended information for debugger/profiler\n\
\x20 -h               This help\n\
\x20 -i               PHP information\n\
\x20 -m               Show compiled in modules\n\
\x20 -v               Version number\n\
\x20 -p, --prefix <dir>\n\
\x20                  Specify alternative prefix path to FastCGI process manager (default: {pre}).\n\
\x20 -g, --pid <file>\n\
\x20                  Specify the PID file location.\n\
\x20 -y, --fpm-config <file>\n\
\x20                  Specify alternative path to FastCGI process manager config file.\n\
\x20 -t, --test       Test FPM configuration and exit\n\
\x20 -D, --daemonize  force to run in background, and ignore daemonize option from config file\n\
\x20 -F, --nodaemonize\n\
\x20                  force to stay in foreground, and ignore daemonize option from config file\n\
\x20 -O, --force-stderr\n\
\x20                  force output to stderr in nodaemonize even if stderr is not a TTY\n\
\x20 -R, --allow-to-run-as-root\n\
\x20                  Allow pool to run as root (disabled by default)\n",
        p = prog,
        pre = conf::PHP_PREFIX
    )
}

fn stdout_write(b: &[u8]) {
    use std::io::Write;
    let mut so = std::io::stdout().lock();
    let _ = so.write_all(b);
    let _ = so.flush();
}

/// Whether `argv0` names the FPM SAPI.
pub fn is_fpm_name(argv0: &[u8]) -> bool {
    let base = match argv0.iter().rposition(|&c| c == b'/') {
        Some(p) => &argv0[p + 1..],
        None => argv0,
    };
    base.windows(3).any(|w| w == b"fpm")
}

impl Fpm {
    fn new(argv: Vec<Vec<u8>>) -> Fpm {
        Fpm {
            parent_pid: 0,
            argv,
            config: None,
            prefix: None,
            pid: None,
            running_children: 0,
            error_log_fd: 0,
            log_level: 0,
            test_successful: false,
            heartbeat: 0,
            run_as_root: false,
            force_stderr: false,
            send_config_pipe: [0, 0],
            gc: GlobalConfig::default(),
            pools: Vec::new(),
            children: Vec::new(),
            current_wp: None,
            ini_recursion: 0,
            ini_filename: String::new(),
            ini_lineno: 0,
            ini_include: None,
            ev: Events::default(),
            sp: [-1, -1],
            pctl_state: events::PCTL_STATE_NORMAL,
            pctl_signal_sent: 0,
            fd_stderr_original: -1,
            sockets: Vec::new(),
            last_faults: Vec::new(),
            fault: 0,
            children_warned: false,
            php_ini: Vec::new(),
        }
    }

    /// fpm_scoreboard_init_main()
    fn scoreboard_init_main(&mut self) -> i32 {
        for i in 0..self.pools.len() {
            let c = &self.pools[i].config;
            if c.pm_max_children < 1 {
                zlog!(
                    ERROR,
                    "[pool {}] Unable to create scoreboard SHM because max_client is not set",
                    c.name()
                );
                return -1;
            }
            let sb = scoreboard::alloc(c.name(), c.pm, c.pm_max_children);
            if sb.is_null() {
                return -1;
            }
            self.pools[i].scoreboard = sb;
            if let Some(sh) = self.pools[i].shared {
                unsafe { (*sb).shared = self.pools[sh].scoreboard };
            }
        }
        0
    }

    /// fpm_init()
    #[allow(clippy::too_many_arguments)]
    fn init(&mut self, test_conf: i32, force_daemon: i32) -> Result<bool, ()> {
        let failed = self.stdio_init_main() < 0
            || self.conf_init_main(test_conf, force_daemon).is_err()
            || self.unix_init_main() < 0
            || self.scoreboard_init_main() < 0
            || self.env_init_main() < 0
            || self.signals_init_main() < 0
            || self.children_init_main() < 0
            || self.sockets_init_main() < 0
            || self.event_init_main() < 0;
        if failed {
            if self.test_successful {
                return Ok(false);
            }
            zlog!(ERROR, "FPM initialization failed");
            return Err(());
        }
        if self.conf_write_pid() < 0 {
            zlog!(ERROR, "FPM initialization failed");
            return Err(());
        }
        self.stdio_init_final();
        zlog!(NOTICE, "fpm is running, pid {}", self.parent_pid);
        Ok(true)
    }

    /// fpm_run(): the master never returns (children run their worker).
    fn run(&mut self) -> ! {
        for i in 0..self.pools.len() {
            let r = self.children_create_initial(i);
            if r == 2 {
                self.pctl(events::PCTL_STATE_TERMINATING, events::Action::Set);
                self.event_loop(true);
            }
        }
        self.event_loop(false);
    }

    /// fpm_cleanups_run(FPM_CLEANUP_CHILD) in a new child.
    pub fn child_cleanup(&mut self) {
        // fpm_children_free(): the siblings' pipes
        for c in &self.children {
            if c.fd_stdout != -1 {
                unsafe { libc::close(c.fd_stdout) };
            }
            if c.fd_stderr != -1 {
                unsafe { libc::close(c.fd_stderr) };
            }
        }
        self.children.clear();
        self.sockets_cleanup(Cleanup::Child);
        if self.ev.epollfd_raw() > 0 {
            unsafe { libc::close(self.ev.epollfd_raw()) };
        }
    }
}

/// main() of sapi/fpm/fpm/fpm_main.c
pub fn fpm_main(argv: Vec<Vec<u8>>) -> i32 {
    // signal(SIGPIPE, SIG_IGN) is done by main(); block the early reload
    // signals (fpm_signals_init_mask() / fpm_signals_block()).
    if events::signals_block() < 0 {
        zlog!(WARNING, "Could die in the case of too early reload signal");
    }
    zlog!(DEBUG, "Blocked some signals");
    let argv0 = argv.first().cloned().unwrap_or_else(|| b"php-fpm".to_vec());
    let mut fpm = Fpm::new(argv.clone());
    let mut ini_path_override: Option<Vec<u8>> = None;
    let mut ini_ignore = false;
    let mut defines: Vec<u8> = Vec::new();
    let mut test_conf = 0;
    let mut force_daemon = -1;
    let mut php_information = false;
    let mut fpm_config: Option<String> = None;

    let mut g = GetOpt::with_options(&argv, false, FPM_OPTIONS);
    let mut optarg = None;
    loop {
        let c = g.next(&mut optarg);
        if c == -1 {
            break;
        }
        let arg = optarg.take();
        let ch = if (0..128).contains(&c) {
            c as u8 as char
        } else {
            '\0'
        };
        let ini_now = |defines: &[u8], ini_path_override: &Option<Vec<u8>>, ini_ignore: bool| {
            let mut ini: IniSettings = worker::sapi_ini_defaults();
            crate::load_php_ini(
                ini_path_override.as_deref(),
                ini_ignore,
                &mut ini,
                "php-fpm-fcgi.ini",
            );
            engine::parse_ini_text(defines, &mut ini);
            ini
        };
        match ch {
            'c' => ini_path_override = arg,
            'n' => ini_ignore = true,
            'd' => {
                if let Some(a) = arg {
                    crate::ini_define_line(&a, &mut defines);
                }
            }
            'y' => fpm_config = arg.map(|a| String::from_utf8_lossy(&a).into_owned()),
            'p' => fpm.prefix = arg.map(|a| String::from_utf8_lossy(&a).into_owned()),
            'g' => fpm.pid = arg.map(|a| String::from_utf8_lossy(&a).into_owned()),
            'e' => {}
            't' => test_conf += 1,
            'm' => {
                let ini = ini_now(&defines, &ini_path_override, ini_ignore);
                let mut vm = engine::new_vm(&ini, worker::SAPI_NAME);
                let info = goro_ext_standard::request::RequestInfo::default();
                engine::request_startup(&mut vm, &info, &ini);
                crate::run_snippet(
                    &mut vm,
                    "$m = get_loaded_extensions(); usort($m, 'strcasecmp'); \
                     echo \"[PHP Modules]\\n\"; foreach ($m as $n) echo $n, \"\\n\"; \
                     echo \"\\n[Zend Modules]\\n\\n\";",
                );
                stdout_write(&vm.take_output());
                return 0;
            }
            'i' => php_information = true,
            'R' => fpm.run_as_root = true,
            'D' => force_daemon = 1,
            'F' => force_daemon = 0,
            'O' => fpm.force_stderr = true,
            'v' => {
                let ini = ini_now(&defines, &ini_path_override, ini_ignore);
                let mut vm = engine::new_vm(&ini, worker::SAPI_NAME);
                stdout_write(&vm.take_output());
                stdout_write(crate::version_string(worker::SAPI_NAME).as_bytes());
                return 0;
            }
            _ => {
                stdout_write(usage(&argv0).as_bytes());
                return if c == GETOPT_INVALID_ARG { 64 } else { 0 };
            }
        }
    }
    let optind = g.optind;
    let mut ini: IniSettings = Vec::new();
    crate::load_php_ini(
        ini_path_override.as_deref(),
        ini_ignore,
        &mut ini,
        "php-fpm-fcgi.ini",
    );
    engine::parse_ini_text(&defines, &mut ini);
    fpm.php_ini = ini.clone();

    if php_information {
        let mut full = worker::sapi_ini_defaults();
        full.extend(ini.iter().cloned());
        let mut vm = engine::new_vm(&full, worker::SAPI_NAME);
        vm.cli_direct_output = true;
        let info = goro_ext_standard::request::RequestInfo::default();
        engine::request_startup(&mut vm, &info, &full);
        crate::run_snippet(&mut vm, "phpinfo();");
        stdout_write(&vm.take_output());
        return 0;
    }
    // No other args are permitted here as there is no interactive mode
    if argv.len() != optind {
        stdout_write(usage(&argv0).as_bytes());
        return 64;
    }

    // fpm.config INI entry
    let cfg = fpm_config.or_else(|| {
        ini.iter()
            .rev()
            .find(|(k, _)| k == "fpm.config")
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    });
    if let Some(c) = cfg.filter(|c| !c.is_empty()) {
        fpm.config = Some(c);
    }
    let _ = std::ffi::OsStr::from_bytes(b"");

    match fpm.init(test_conf, force_daemon) {
        Err(()) => {
            if fpm.send_config_pipe[1] != 0 {
                let v: i32 = 0;
                zlog!(
                    DEBUG,
                    "Sending \"0\" (error) to parent via fd={}",
                    fpm.send_config_pipe[1]
                );
                unsafe {
                    libc::write(
                        fpm.send_config_pipe[1],
                        &v as *const i32 as *const libc::c_void,
                        4,
                    );
                    libc::close(fpm.send_config_pipe[1]);
                }
            }
            return 78;
        }
        Ok(false) => return 0,
        Ok(true) => {}
    }
    if fpm.send_config_pipe[1] != 0 {
        let v: i32 = 1;
        zlog!(
            DEBUG,
            "Sending \"1\" (OK) to parent via fd={}",
            fpm.send_config_pipe[1]
        );
        unsafe {
            libc::write(
                fpm.send_config_pipe[1],
                &v as *const i32 as *const libc::c_void,
                4,
            );
            libc::close(fpm.send_config_pipe[1]);
        }
    }
    fpm.run();
}
