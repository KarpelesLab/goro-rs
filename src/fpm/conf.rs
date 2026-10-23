//! sapi/fpm/fpm/fpm_conf.c: php-fpm.conf parsing, validation and dump.

use super::zlog::{self, ALERT, ERROR, NOTICE, SYSERROR, WARNING};
use super::{Domain, Fpm, Pool};
use crate::zlog;

pub const PM_STYLE_STATIC: i32 = 1;
pub const PM_STYLE_DYNAMIC: i32 = 2;
pub const PM_STYLE_ONDEMAND: i32 = 3;

/// FPM_BACKLOG_DEFAULT (Linux: negative values are truncated to SOMAXCONN)
pub const BACKLOG_DEFAULT: i32 = -1;

/// PHP_PREFIX / PHP_SYSCONFDIR / PHP_LOCALSTATEDIR
pub const PHP_PREFIX: &str = "/usr";
pub const PHP_SYSCONFDIR: &str = "/etc";
pub const PHP_LOCALSTATEDIR: &str = "/var";

pub fn pm2str(pm: i32) -> &'static str {
    match pm {
        PM_STYLE_STATIC => "static",
        PM_STYLE_DYNAMIC => "dynamic",
        _ => "ondemand",
    }
}

/// FPM_PHP_INI_TO_EXPAND
const PHP_INI_TO_EXPAND: &[&str] = &[
    "error_log",
    "extension_dir",
    "sendmail_path",
    "session.cookie_path",
    "session_pgsql.sem_file_name",
    "soap.wsdl_cache_dir",
    "uploadprogress.file.filename_template",
    "xdebug.output_dir",
    "xdebug.profiler_output_dir",
    "xdebug.trace_output_dir",
    "xmms.path",
    "axis2.client_home",
    "blenc.key_file",
    "coin_acceptor.device",
];

/// struct fpm_global_config_s
#[derive(Clone)]
pub struct GlobalConfig {
    pub pid_file: Option<String>,
    pub error_log: Option<String>,
    pub syslog_ident: Option<String>,
    pub syslog_facility: i32,
    pub log_level: i32,
    pub log_limit: i32,
    pub log_buffering: i32,
    pub emergency_restart_threshold: i32,
    pub emergency_restart_interval: i32,
    pub process_control_timeout: i32,
    pub process_max: i32,
    pub process_priority: i32,
    pub daemonize: i32,
    pub rlimit_files: i32,
    pub rlimit_core: i32,
    pub events_mechanism: Option<String>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        GlobalConfig {
            pid_file: None,
            error_log: None,
            syslog_ident: None,
            syslog_facility: -1,
            log_level: 0,
            log_limit: zlog::DEFAULT_LIMIT,
            log_buffering: 1,
            emergency_restart_threshold: 0,
            emergency_restart_interval: 0,
            process_control_timeout: 0,
            process_max: 0,
            process_priority: 64,
            daemonize: 1,
            rlimit_files: 0,
            rlimit_core: 0,
            events_mechanism: None,
        }
    }
}

/// struct fpm_worker_pool_config_s. The key/value lists are in the order
/// fpm_conf_set_array() links them (the last one set first).
#[derive(Clone)]
pub struct PoolConfig {
    pub name: Option<String>,
    pub prefix: Option<String>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub listen_address: Option<String>,
    pub listen_backlog: i32,
    pub listen_acl_users: Option<String>,
    pub listen_acl_groups: Option<String>,
    pub listen_owner: Option<String>,
    pub listen_group: Option<String>,
    pub listen_mode: Option<String>,
    pub listen_allowed_clients: Option<String>,
    pub process_priority: i32,
    pub process_dumpable: i32,
    pub pm: i32,
    pub pm_max_children: i32,
    pub pm_start_servers: i32,
    pub pm_min_spare_servers: i32,
    pub pm_max_spare_servers: i32,
    pub pm_max_spawn_rate: i32,
    pub pm_process_idle_timeout: i32,
    pub pm_max_requests: i32,
    pub pm_status_path: Option<String>,
    pub pm_status_listen: Option<String>,
    pub ping_path: Option<String>,
    pub ping_response: Option<String>,
    pub access_log: Option<String>,
    pub access_format: Option<String>,
    pub access_suppress_paths: Vec<String>,
    pub slowlog: Option<String>,
    pub request_slowlog_timeout: i32,
    pub request_slowlog_trace_depth: i32,
    pub request_terminate_timeout: i32,
    pub request_terminate_timeout_track_finished: i32,
    pub rlimit_files: i32,
    pub rlimit_core: i32,
    pub chroot: Option<String>,
    pub chdir: Option<String>,
    pub catch_workers_output: i32,
    pub decorate_workers_output: i32,
    pub clear_env: i32,
    pub security_limit_extensions: Option<String>,
    pub env: Vec<(String, String)>,
    pub php_admin_values: Vec<(String, String)>,
    pub php_values: Vec<(String, String)>,
}

impl PoolConfig {
    /// fpm_worker_pool_config_alloc()
    pub fn new() -> PoolConfig {
        PoolConfig {
            name: None,
            prefix: None,
            user: None,
            group: None,
            listen_address: None,
            listen_backlog: BACKLOG_DEFAULT,
            listen_acl_users: None,
            listen_acl_groups: None,
            listen_owner: None,
            listen_group: None,
            listen_mode: None,
            listen_allowed_clients: None,
            process_priority: 64,
            process_dumpable: 0,
            pm: 0,
            pm_max_children: 0,
            pm_start_servers: 0,
            pm_min_spare_servers: 0,
            pm_max_spare_servers: 0,
            pm_max_spawn_rate: 32,
            pm_process_idle_timeout: 10,
            pm_max_requests: 0,
            pm_status_path: None,
            pm_status_listen: None,
            ping_path: None,
            ping_response: None,
            access_log: None,
            access_format: None,
            access_suppress_paths: Vec::new(),
            slowlog: None,
            request_slowlog_timeout: 0,
            request_slowlog_trace_depth: 0,
            request_terminate_timeout: 0,
            request_terminate_timeout_track_finished: 0,
            rlimit_files: 0,
            rlimit_core: 0,
            chroot: None,
            chdir: None,
            catch_workers_output: 0,
            decorate_workers_output: 1,
            clear_env: 1,
            security_limit_extensions: None,
            env: Vec::new(),
            php_admin_values: Vec::new(),
            php_values: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
}

/// fpm_conf_set_boolean()
fn parse_boolean(val: &str) -> Result<i32, &'static str> {
    let y = matches!(val, "1" | "yes" | "true" | "on");
    let n = y || val.is_empty() || matches!(val, "0" | "no" | "none" | "false" | "off");
    if !y && !n {
        return Err("invalid boolean value");
    }
    Ok(y as i32)
}

/// atoi()
pub fn atoi(s: &str) -> i64 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut n: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n.wrapping_mul(10).wrapping_add((b[i] - b'0') as i64);
        i += 1;
    }
    if neg { -n } else { n }
}

/// fpm_conf_set_integer()
fn parse_integer(val: &str) -> Result<i32, &'static str> {
    for (i, c) in val.bytes().enumerate() {
        if i == 0 && c == b'-' {
            continue;
        }
        if !c.is_ascii_digit() {
            return Err("is not a valid number (greater or equal than zero)");
        }
    }
    Ok(atoi(val) as i32)
}

/// fpm_conf_set_time()
fn parse_time(val: &str) -> Result<i32, &'static str> {
    if val.is_empty() {
        return Err("invalid time value");
    }
    let suffix = *val.as_bytes().last().unwrap();
    let head = &val[..val.len() - 1];
    Ok(match suffix {
        b'm' => 60 * atoi(head) as i32,
        b'h' => 3600 * atoi(head) as i32,
        b'd' => 86400 * atoi(head) as i32,
        b's' => atoi(head) as i32,
        c => {
            if !c.is_ascii_digit() {
                return Err("unknown suffix used in time value");
            }
            atoi(val) as i32
        }
    })
}

/// fpm_conf_set_log_level()
fn parse_log_level(val: &str) -> Result<i32, &'static str> {
    Ok(match val.to_ascii_lowercase().as_str() {
        "debug" => zlog::DEBUG,
        "notice" => NOTICE,
        "warning" | "warn" => WARNING,
        "error" => ERROR,
        "alert" => ALERT,
        _ => return Err("invalid value for 'log_level'"),
    })
}

/// fpm_conf_set_syslog_facility()
fn parse_syslog_facility(val: &str) -> Result<i32, &'static str> {
    Ok(match val.to_ascii_uppercase().as_str() {
        "AUTH" => libc::LOG_AUTH,
        "AUTHPRIV" => libc::LOG_AUTHPRIV,
        "CRON" => libc::LOG_CRON,
        "DAEMON" => libc::LOG_DAEMON,
        "FTP" => libc::LOG_FTP,
        "KERN" => libc::LOG_KERN,
        "LPR" => libc::LOG_LPR,
        "MAIL" => libc::LOG_MAIL,
        "NEWS" => libc::LOG_NEWS,
        "SYSLOG" => libc::LOG_SYSLOG,
        "USER" => libc::LOG_USER,
        "UUCP" => libc::LOG_UUCP,
        "LOCAL0" => libc::LOG_LOCAL0,
        "LOCAL1" => libc::LOG_LOCAL1,
        "LOCAL2" => libc::LOG_LOCAL2,
        "LOCAL3" => libc::LOG_LOCAL3,
        "LOCAL4" => libc::LOG_LOCAL4,
        "LOCAL5" => libc::LOG_LOCAL5,
        "LOCAL6" => libc::LOG_LOCAL6,
        "LOCAL7" => libc::LOG_LOCAL7,
        _ => return Err("invalid value"),
    })
}

/// fpm_conf_set_rlimit_core()
fn parse_rlimit_core(val: &str) -> Result<i32, &'static str> {
    if val.eq_ignore_ascii_case("unlimited") {
        return Ok(-1);
    }
    let v = parse_integer(val)?;
    if v < 0 {
        return Err("must be greater than zero or 'unlimited'");
    }
    Ok(v)
}

/// fpm_conf_set_pm()
fn parse_pm(val: &str) -> Result<i32, &'static str> {
    Ok(match val.to_ascii_lowercase().as_str() {
        "static" => PM_STYLE_STATIC,
        "dynamic" => PM_STYLE_DYNAMIC,
        "ondemand" => PM_STYLE_ONDEMAND,
        _ => return Err("invalid process manager (static, dynamic or ondemand)"),
    })
}

/// fpm_conf_is_dir()
fn is_dir(p: &str) -> bool {
    std::fs::metadata(p).map(|m| m.is_dir()).unwrap_or(false)
}

#[derive(Clone, Copy)]
enum Kind {
    Str,
    Int,
    Time,
    Bool,
    LogLevel,
    Facility,
    RlimitCore,
    Pm,
}

/// ini_fpm_global_options[]
const GLOBAL_OPTIONS: &[(&str, Kind)] = &[
    ("pid", Kind::Str),
    ("error_log", Kind::Str),
    ("syslog.ident", Kind::Str),
    ("syslog.facility", Kind::Facility),
    ("log_buffering", Kind::Bool),
    ("log_level", Kind::LogLevel),
    ("log_limit", Kind::Int),
    ("emergency_restart_threshold", Kind::Int),
    ("emergency_restart_interval", Kind::Time),
    ("process_control_timeout", Kind::Time),
    ("process.max", Kind::Int),
    ("process.priority", Kind::Int),
    ("daemonize", Kind::Bool),
    ("rlimit_files", Kind::Int),
    ("rlimit_core", Kind::RlimitCore),
    ("events.mechanism", Kind::Str),
];

/// ini_fpm_pool_options[]
const POOL_OPTIONS: &[(&str, Kind)] = &[
    ("prefix", Kind::Str),
    ("user", Kind::Str),
    ("group", Kind::Str),
    ("listen", Kind::Str),
    ("listen.backlog", Kind::Int),
    ("listen.acl_users", Kind::Str),
    ("listen.acl_groups", Kind::Str),
    ("listen.owner", Kind::Str),
    ("listen.group", Kind::Str),
    ("listen.mode", Kind::Str),
    ("listen.allowed_clients", Kind::Str),
    ("process.priority", Kind::Int),
    ("process.dumpable", Kind::Bool),
    ("pm", Kind::Pm),
    ("pm.max_children", Kind::Int),
    ("pm.start_servers", Kind::Int),
    ("pm.min_spare_servers", Kind::Int),
    ("pm.max_spare_servers", Kind::Int),
    ("pm.max_spawn_rate", Kind::Int),
    ("pm.process_idle_timeout", Kind::Time),
    ("pm.max_requests", Kind::Int),
    ("pm.status_path", Kind::Str),
    ("pm.status_listen", Kind::Str),
    ("ping.path", Kind::Str),
    ("ping.response", Kind::Str),
    ("access.log", Kind::Str),
    ("access.format", Kind::Str),
    ("slowlog", Kind::Str),
    ("request_slowlog_timeout", Kind::Time),
    ("request_slowlog_trace_depth", Kind::Int),
    ("request_terminate_timeout", Kind::Time),
    ("request_terminate_timeout_track_finished", Kind::Bool),
    ("rlimit_files", Kind::Int),
    ("rlimit_core", Kind::RlimitCore),
    ("chroot", Kind::Str),
    ("chdir", Kind::Str),
    ("catch_workers_output", Kind::Bool),
    ("decorate_workers_output", Kind::Bool),
    ("clear_env", Kind::Bool),
    ("security.limit_extensions", Kind::Str),
];

enum Val {
    S(String),
    I(i32),
}

fn parse_value(kind: Kind, val: &str) -> Result<Val, &'static str> {
    Ok(match kind {
        Kind::Str => Val::S(val.to_string()),
        Kind::Int => Val::I(parse_integer(val)?),
        Kind::Time => Val::I(parse_time(val)?),
        Kind::Bool => Val::I(parse_boolean(val)?),
        Kind::LogLevel => Val::I(parse_log_level(val)?),
        Kind::Facility => Val::I(parse_syslog_facility(val)?),
        Kind::RlimitCore => Val::I(parse_rlimit_core(val)?),
        Kind::Pm => Val::I(parse_pm(val)?),
    })
}

fn set_global(gc: &mut GlobalConfig, name: &str, v: Val) {
    let (s, i) = match v {
        Val::S(s) => (Some(s), 0),
        Val::I(i) => (None, i),
    };
    match name {
        "pid" => gc.pid_file = s,
        "error_log" => gc.error_log = s,
        "syslog.ident" => gc.syslog_ident = s,
        "syslog.facility" => gc.syslog_facility = i,
        "log_buffering" => gc.log_buffering = i,
        "log_level" => gc.log_level = i,
        "log_limit" => gc.log_limit = i,
        "emergency_restart_threshold" => gc.emergency_restart_threshold = i,
        "emergency_restart_interval" => gc.emergency_restart_interval = i,
        "process_control_timeout" => gc.process_control_timeout = i,
        "process.max" => gc.process_max = i,
        "process.priority" => gc.process_priority = i,
        "daemonize" => gc.daemonize = i,
        "rlimit_files" => gc.rlimit_files = i,
        "rlimit_core" => gc.rlimit_core = i,
        "events.mechanism" => gc.events_mechanism = s,
        _ => {}
    }
}

fn set_pool(c: &mut PoolConfig, name: &str, v: Val) {
    let (s, i) = match v {
        Val::S(s) => (Some(s), 0),
        Val::I(i) => (None, i),
    };
    match name {
        "prefix" => c.prefix = s,
        "user" => c.user = s,
        "group" => c.group = s,
        "listen" => c.listen_address = s,
        "listen.backlog" => c.listen_backlog = i,
        "listen.acl_users" => c.listen_acl_users = s,
        "listen.acl_groups" => c.listen_acl_groups = s,
        "listen.owner" => c.listen_owner = s,
        "listen.group" => c.listen_group = s,
        "listen.mode" => c.listen_mode = s,
        "listen.allowed_clients" => c.listen_allowed_clients = s,
        "process.priority" => c.process_priority = i,
        "process.dumpable" => c.process_dumpable = i,
        "pm" => c.pm = i,
        "pm.max_children" => c.pm_max_children = i,
        "pm.start_servers" => c.pm_start_servers = i,
        "pm.min_spare_servers" => c.pm_min_spare_servers = i,
        "pm.max_spare_servers" => c.pm_max_spare_servers = i,
        "pm.max_spawn_rate" => c.pm_max_spawn_rate = i,
        "pm.process_idle_timeout" => c.pm_process_idle_timeout = i,
        "pm.max_requests" => c.pm_max_requests = i,
        "pm.status_path" => c.pm_status_path = s,
        "pm.status_listen" => c.pm_status_listen = s,
        "ping.path" => c.ping_path = s,
        "ping.response" => c.ping_response = s,
        "access.log" => c.access_log = s,
        "access.format" => c.access_format = s,
        "slowlog" => c.slowlog = s,
        "request_slowlog_timeout" => c.request_slowlog_timeout = i,
        "request_slowlog_trace_depth" => c.request_slowlog_trace_depth = i,
        "request_terminate_timeout" => c.request_terminate_timeout = i,
        "request_terminate_timeout_track_finished" => {
            c.request_terminate_timeout_track_finished = i
        }
        "rlimit_files" => c.rlimit_files = i,
        "rlimit_core" => c.rlimit_core = i,
        "chroot" => c.chroot = s,
        "chdir" => c.chdir = s,
        "catch_workers_output" => c.catch_workers_output = i,
        "decorate_workers_output" => c.decorate_workers_output = i,
        "clear_env" => c.clear_env = i,
        "security.limit_extensions" => c.security_limit_extensions = s,
        _ => {}
    }
}

fn str2str(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("undefined")
}

fn bool2str(b: i32) -> &'static str {
    if b != 0 { "yes" } else { "no" }
}

impl Fpm {
    /// fpm_conf_expand_pool_name()
    fn expand_pool_name(&self, value: &str) -> Result<String, ()> {
        let mut v = value.to_string();
        while let Some(p) = v.find("$pool") {
            let name = match self
                .current_wp
                .and_then(|i| self.pools[i].config.name.clone())
            {
                Some(n) => n,
                None => return Err(()),
            };
            v = format!("{}{}{}", &v[..p], name, &v[p + 5..]);
        }
        Ok(v)
    }

    /// fpm_conf_set_array(): returns the stored value.
    fn conf_set_array(&self, value: &str, convert_to_bool: bool) -> Result<String, &'static str> {
        if convert_to_bool {
            let b = parse_boolean(value)?;
            Ok(if b != 0 { "1" } else { "0" }.to_string())
        } else {
            self.expand_pool_name(value)
                .map_err(|_| "Can't use '$pool' when the pool is not defined")
        }
    }

    /// fpm_worker_pool_config_alloc(): a new pool, which becomes the
    /// current one.
    fn pool_alloc(&mut self) -> usize {
        self.pools.push(Pool::new(PoolConfig::new()));
        let i = self.pools.len() - 1;
        self.current_wp = Some(i);
        i
    }

    /// fpm_evaluate_full_path()
    pub fn evaluate_full_path(
        &self,
        path: &mut Option<String>,
        wp: Option<usize>,
        default_prefix: Option<&str>,
        expand: bool,
    ) -> Result<(), ()> {
        let Some(p) = path.clone() else {
            return Ok(());
        };
        if p.is_empty() || p.starts_with('/') {
            return Ok(());
        }
        let mut prefix: Option<String> = wp.and_then(|i| self.pools[i].config.prefix.clone());
        if prefix.is_none() {
            prefix = self.prefix.clone();
        }
        let prefix = prefix.unwrap_or_else(|| default_prefix.unwrap_or(PHP_PREFIX).to_string());
        let mut cur: Option<String> = Some(p);
        if expand {
            let s = cur.clone().unwrap();
            if let Some(pos) = s.find("$prefix") {
                if pos != 0 {
                    zlog!(ERROR, "'$prefix' must be use at the beginning of the value");
                    return Err(());
                }
                cur = if s.len() > 7 {
                    Some(s[7..].to_string())
                } else {
                    None
                };
            }
        }
        let full = match cur {
            Some(c) => format!("{}/{}", prefix, c),
            None => prefix.clone(),
        };
        *path = Some(full.clone());
        if !full.starts_with('/') && wp.is_some() {
            return self.evaluate_full_path(path, None, default_prefix, expand);
        }
        Ok(())
    }

    /// fpm_worker_pool_shared_status_alloc()
    fn shared_status_alloc(&mut self, shared: usize) {
        let idx = self.pool_alloc();
        if self.pools[shared].config.pm_status_path.is_none() {
            self.pools[shared].config.pm_status_path = Some("/".to_string());
        }
        let sc = self.pools[shared].config.clone();
        let c = &mut self.pools[idx].config;
        c.name = Some(format!("{}_status", sc.name()));
        c.listen_address = sc.pm_status_listen.clone();
        c.listen_acl_groups = sc.listen_acl_groups.clone();
        c.listen_acl_users = sc.listen_acl_users.clone();
        c.listen_allowed_clients = sc.listen_allowed_clients.clone();
        c.listen_group = sc.listen_group.clone();
        c.listen_owner = sc.listen_owner.clone();
        c.listen_mode = sc.listen_mode.clone();
        c.user = sc.user.clone();
        c.group = sc.group.clone();
        c.pm_status_path = sc.pm_status_path.clone();
        c.ping_path = sc.ping_path.clone();
        c.ping_response = sc.ping_response.clone();
        // fpm_conf_apply_kv_array_to_kv_array(): prepended one by one
        for kv in &sc.php_values {
            c.php_values.insert(0, kv.clone());
        }
        for kv in &sc.php_admin_values {
            c.php_admin_values.insert(0, kv.clone());
        }
        c.pm = PM_STYLE_ONDEMAND;
        c.pm_max_children = 2;
        self.pools[idx].shared = Some(shared);
    }

    /// fpm_conf_process_all_pools()
    fn process_all_pools(&mut self) -> Result<(), ()> {
        if self.pools.is_empty() {
            zlog!(
                ERROR,
                "No pool defined. at least one pool section must be specified in config file"
            );
            return Err(());
        }
        let mut i = 0;
        while i < self.pools.len() {
            let name = self.pools[i].config.name().to_string();
            // prefix
            if self.pools[i]
                .config
                .prefix
                .as_deref()
                .is_some_and(|p| !p.is_empty())
            {
                let mut p = self.pools[i].config.prefix.clone();
                let _ = self.evaluate_full_path(&mut p, None, None, false);
                self.pools[i].config.prefix = p.clone();
                let p = p.unwrap_or_default();
                if !is_dir(&p) {
                    zlog!(
                        ERROR,
                        "[pool {}] the prefix '{}' does not exist or is not a directory",
                        name,
                        p
                    );
                    return Err(());
                }
            }
            if self.pools[i].config.user.is_none()
                && unsafe { libc::geteuid() } == 0
                && !self.run_as_root
            {
                zlog!(
                    ALERT,
                    "[pool {}] 'user' directive has not been specified when running as a root without --allow-to-run-as-root",
                    name
                );
                return Err(());
            }
            // listen
            match self.pools[i].config.listen_address.clone() {
                Some(a) if !a.is_empty() => {
                    let d = super::sockets::domain_from_address(&a);
                    self.pools[i].listen_address_domain = d;
                    if d == Domain::Unix && !a.starts_with('/') {
                        let mut p = Some(a);
                        let _ = self.evaluate_full_path(&mut p, Some(i), None, false);
                        self.pools[i].config.listen_address = p;
                    }
                }
                _ => {
                    zlog!(
                        ALERT,
                        "[pool {}] no listen address have been defined!",
                        name
                    );
                    return Err(());
                }
            }
            let c = &self.pools[i].config;
            if c.process_priority != 64 && (c.process_priority < -19 || c.process_priority > 20) {
                zlog!(
                    ERROR,
                    "[pool {}] process.priority must be included into [-19,20]",
                    name
                );
                return Err(());
            }
            if !matches!(c.pm, PM_STYLE_STATIC | PM_STYLE_DYNAMIC | PM_STYLE_ONDEMAND) {
                zlog!(
                    ALERT,
                    "[pool {}] the process manager is missing (static, dynamic or ondemand)",
                    name
                );
                return Err(());
            }
            if c.pm_max_children < 1 {
                zlog!(
                    ALERT,
                    "[pool {}] pm.max_children must be a positive value",
                    name
                );
                return Err(());
            }
            if c.pm == PM_STYLE_DYNAMIC {
                let c = &mut self.pools[i].config;
                if c.pm_min_spare_servers <= 0 {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.min_spare_servers({}) must be a positive value",
                        name,
                        c.pm_min_spare_servers
                    );
                    return Err(());
                }
                if c.pm_max_spare_servers <= 0 {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.max_spare_servers({}) must be a positive value",
                        name,
                        c.pm_max_spare_servers
                    );
                    return Err(());
                }
                if c.pm_min_spare_servers > c.pm_max_children
                    || c.pm_max_spare_servers > c.pm_max_children
                {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.min_spare_servers({}) and pm.max_spare_servers({}) cannot be greater than pm.max_children({})",
                        name,
                        c.pm_min_spare_servers,
                        c.pm_max_spare_servers,
                        c.pm_max_children
                    );
                    return Err(());
                }
                if c.pm_max_spare_servers < c.pm_min_spare_servers {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.max_spare_servers({}) must not be less than pm.min_spare_servers({})",
                        name,
                        c.pm_max_spare_servers,
                        c.pm_min_spare_servers
                    );
                    return Err(());
                }
                if c.pm_start_servers <= 0 {
                    c.pm_start_servers = c.pm_min_spare_servers
                        + ((c.pm_max_spare_servers - c.pm_min_spare_servers) / 2);
                    zlog!(
                        NOTICE,
                        "[pool {}] pm.start_servers is not set. It's been set to {}.",
                        name,
                        c.pm_start_servers
                    );
                } else if c.pm_start_servers < c.pm_min_spare_servers
                    || c.pm_start_servers > c.pm_max_spare_servers
                {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.start_servers({}) must not be less than pm.min_spare_servers({}) and not greater than pm.max_spare_servers({})",
                        name,
                        c.pm_start_servers,
                        c.pm_min_spare_servers,
                        c.pm_max_spare_servers
                    );
                    return Err(());
                }
                if c.pm_max_spawn_rate < 1 {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.max_spawn_rate must be a positive value",
                        name
                    );
                    return Err(());
                }
            } else if c.pm == PM_STYLE_ONDEMAND {
                let c = &mut self.pools[i].config;
                if c.pm_process_idle_timeout < 1 {
                    zlog!(
                        ALERT,
                        "[pool {}] pm.process_idle_timeout({}s) must be greater than 0s",
                        name,
                        c.pm_process_idle_timeout
                    );
                    return Err(());
                }
                if c.listen_backlog < BACKLOG_DEFAULT {
                    zlog!(
                        WARNING,
                        "[pool {}] listen.backlog({}) was too low for the ondemand process manager. I updated it for you to {}.",
                        name,
                        c.listen_backlog,
                        BACKLOG_DEFAULT
                    );
                    c.listen_backlog = BACKLOG_DEFAULT;
                }
                c.pm_start_servers = 0;
                c.pm_min_spare_servers = 0;
                c.pm_max_spare_servers = 0;
            }
            // status
            if self.pools[i].config.pm_status_listen.is_some() {
                self.shared_status_alloc(i);
            }
            if let Some(status) = self.pools[i]
                .config
                .pm_status_path
                .clone()
                .filter(|s| !s.is_empty())
            {
                if !status.starts_with('/') {
                    zlog!(
                        ERROR,
                        "[pool {}] the status path '{}' must start with a '/'",
                        name,
                        status
                    );
                    return Err(());
                }
                if self.pools[i].config.pm_status_listen.is_none()
                    && self.pools[i].shared.is_none()
                    && status.len() < 2
                {
                    zlog!(
                        ERROR,
                        "[pool {}] the status path '{}' is not long enough",
                        name,
                        status
                    );
                    return Err(());
                }
                if status
                    .bytes()
                    .any(|c| !c.is_ascii_alphanumeric() && !b"/-_.~".contains(&c))
                {
                    zlog!(
                        ERROR,
                        "[pool {}] the status path '{}' must contain only the following characters '[alphanum]/_-.~'",
                        name,
                        status
                    );
                    return Err(());
                }
            }
            // ping
            if let Some(ping) = self.pools[i]
                .config
                .ping_path
                .clone()
                .filter(|s| !s.is_empty())
            {
                if !ping.starts_with('/') {
                    zlog!(
                        ERROR,
                        "[pool {}] the ping path '{}' must start with a '/'",
                        name,
                        ping
                    );
                    return Err(());
                }
                if ping.len() < 2 {
                    zlog!(
                        ERROR,
                        "[pool {}] the ping path '{}' is not long enough",
                        name,
                        ping
                    );
                    return Err(());
                }
                if ping
                    .bytes()
                    .any(|c| !c.is_ascii_alphanumeric() && !b"/-_.~".contains(&c))
                {
                    zlog!(
                        ERROR,
                        "[pool {}] the ping path '{}' must contain only the following characters '[alphanum]/_-.~'",
                        name,
                        ping
                    );
                    return Err(());
                }
                match &self.pools[i].config.ping_response {
                    None => self.pools[i].config.ping_response = Some("pong".to_string()),
                    Some(r) if r.is_empty() => {
                        zlog!(
                            ERROR,
                            "[pool {}] the ping response page '{}' is not long enough",
                            name,
                            r
                        );
                        return Err(());
                    }
                    _ => {}
                }
            } else {
                self.pools[i].config.ping_response = None;
            }
            // access.log, access.format
            if self.pools[i]
                .config
                .access_log
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                let mut p = self.pools[i].config.access_log.clone();
                let _ = self.evaluate_full_path(&mut p, Some(i), None, false);
                self.pools[i].config.access_log = p;
                if self.pools[i].config.access_format.is_none() {
                    self.pools[i].config.access_format =
                        Some("%R - %u %t \"%m %r\" %s".to_string());
                }
            }
            let rtt = self.pools[i].config.request_terminate_timeout;
            if rtt != 0 {
                let hb = (rtt * 1000) / 3;
                self.heartbeat = if self.heartbeat != 0 {
                    self.heartbeat.min(hb)
                } else {
                    hb
                };
            }
            // slowlog
            if self.pools[i]
                .config
                .slowlog
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                let mut p = self.pools[i].config.slowlog.clone();
                let _ = self.evaluate_full_path(&mut p, Some(i), None, false);
                self.pools[i].config.slowlog = p;
            }
            let c = &self.pools[i].config;
            if c.request_slowlog_timeout != 0 {
                let slowlog = c.slowlog.clone().filter(|s| !s.is_empty());
                let Some(slowlog) = slowlog else {
                    zlog!(
                        ERROR,
                        "[pool {}] 'slowlog' must be specified for use with 'request_slowlog_timeout'",
                        name
                    );
                    return Err(());
                };
                let cs = std::ffi::CString::new(slowlog.clone()).unwrap_or_default();
                let fd = unsafe {
                    libc::open(
                        cs.as_ptr(),
                        libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT,
                        (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint,
                    )
                };
                if fd < 0 {
                    zlog!(SYSERROR, "Unable to create or open slowlog({})", slowlog);
                    return Err(());
                }
                unsafe { libc::close(fd) };
                let hb = (c.request_slowlog_timeout * 1000) / 3;
                self.heartbeat = if self.heartbeat != 0 {
                    self.heartbeat.min(hb)
                } else {
                    hb
                };
                if c.request_terminate_timeout != 0
                    && c.request_slowlog_timeout > c.request_terminate_timeout
                {
                    zlog!(
                        ERROR,
                        "[pool {}] 'request_slowlog_timeout' ({}) can't be greater than 'request_terminate_timeout' ({})",
                        name,
                        c.request_slowlog_timeout,
                        c.request_terminate_timeout
                    );
                    return Err(());
                }
            }
            let c = &mut self.pools[i].config;
            if c.request_slowlog_trace_depth != 0 {
                if !c.slowlog.as_deref().is_some_and(|s| !s.is_empty()) {
                    zlog!(
                        ERROR,
                        "[pool {}] 'slowlog' must be specified for use with 'request_slowlog_trace_depth'",
                        name
                    );
                    return Err(());
                }
                if c.request_slowlog_trace_depth <= 0 {
                    zlog!(
                        ERROR,
                        "[pool {}] 'request_slowlog_trace_depth' ({}) must be a positive value",
                        name,
                        c.request_slowlog_trace_depth
                    );
                    return Err(());
                }
            } else {
                c.request_slowlog_trace_depth = 20;
            }
            // chroot
            if self.pools[i]
                .config
                .chroot
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                let mut p = self.pools[i].config.chroot.clone();
                if self
                    .evaluate_full_path(&mut p, Some(i), None, true)
                    .is_err()
                {
                    return Err(());
                }
                self.pools[i].config.chroot = p.clone();
                let p = p.unwrap_or_default();
                if !p.starts_with('/') {
                    zlog!(
                        ERROR,
                        "[pool {}] the chroot path '{}' must start with a '/'",
                        name,
                        p
                    );
                    return Err(());
                }
                if !is_dir(&p) {
                    zlog!(
                        ERROR,
                        "[pool {}] the chroot path '{}' does not exist or is not a directory",
                        name,
                        p
                    );
                    return Err(());
                }
            }
            // chdir
            if self.pools[i]
                .config
                .chdir
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                let mut p = self.pools[i].config.chdir.clone();
                let _ = self.evaluate_full_path(&mut p, Some(i), None, false);
                self.pools[i].config.chdir = p.clone();
                let p = p.unwrap_or_default();
                if !p.starts_with('/') {
                    zlog!(
                        ERROR,
                        "[pool {}] the chdir path '{}' must start with a '/'",
                        name,
                        p
                    );
                    return Err(());
                }
                if let Some(chroot) = self.pools[i].config.chroot.clone() {
                    let buf = format!("{}/{}", chroot, p);
                    if !is_dir(&buf) {
                        zlog!(
                            ERROR,
                            "[pool {}] the chdir path '{}' within the chroot path '{}' ('{}') does not exist or is not a directory",
                            name,
                            p,
                            chroot,
                            buf
                        );
                        return Err(());
                    }
                } else if !is_dir(&p) {
                    zlog!(
                        ERROR,
                        "[pool {}] the chdir path '{}' does not exist or is not a directory",
                        name,
                        p
                    );
                    return Err(());
                }
            }
            // security.limit_extensions
            if self.pools[i].config.security_limit_extensions.is_none() {
                self.pools[i].config.security_limit_extensions = Some(".php .phar".to_string());
            }
            let sle = self.pools[i]
                .config
                .security_limit_extensions
                .clone()
                .unwrap_or_default();
            let exts: Vec<String> = sle
                .split([' ', '\t'])
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if !exts.is_empty() {
                self.pools[i].limit_extensions = Some(exts);
            }
            // env[], php_value[], php_admin_values[]
            if self.pools[i].config.chroot.is_none() {
                let mut pv = self.pools[i].config.php_values.clone();
                for kv in pv.iter_mut() {
                    if PHP_INI_TO_EXPAND
                        .iter()
                        .any(|o| kv.0.eq_ignore_ascii_case(o))
                    {
                        let mut p = Some(kv.1.clone());
                        let _ = self.evaluate_full_path(&mut p, Some(i), None, false);
                        kv.1 = p.unwrap_or_default();
                    }
                }
                self.pools[i].config.php_values = pv;
                let mut pv = self.pools[i].config.php_admin_values.clone();
                for kv in pv.iter_mut() {
                    if kv.0.eq_ignore_ascii_case("error_log") && kv.1.eq_ignore_ascii_case("syslog")
                    {
                        continue;
                    }
                    if PHP_INI_TO_EXPAND
                        .iter()
                        .any(|o| kv.0.eq_ignore_ascii_case(o))
                    {
                        let mut p = Some(kv.1.clone());
                        let _ = self.evaluate_full_path(&mut p, Some(i), None, false);
                        kv.1 = p.unwrap_or_default();
                    }
                }
                self.pools[i].config.php_admin_values = pv;
            }
            i += 1;
        }
        // ensure 2 pools do not use the same listening address
        for a in 0..self.pools.len() {
            for b in 0..self.pools.len() {
                if a == b {
                    continue;
                }
                let la = self.pools[a]
                    .config
                    .listen_address
                    .clone()
                    .unwrap_or_default();
                let lb = self.pools[b]
                    .config
                    .listen_address
                    .clone()
                    .unwrap_or_default();
                if !la.is_empty() && !lb.is_empty() && la == lb {
                    zlog!(
                        ERROR,
                        "[pool {}] unable to set listen address as it's already used in another pool '{}'",
                        self.pools[b].config.name(),
                        self.pools[a].config.name()
                    );
                    return Err(());
                }
            }
        }
        Ok(())
    }

    /// fpm_conf_unlink_pid()
    pub fn conf_unlink_pid(&self) -> i32 {
        if let Some(p) = &self.gc.pid_file {
            if std::fs::remove_file(p).is_err() {
                zlog!(SYSERROR, "Unable to remove the PID file ({}).", p);
                return -1;
            }
        }
        0
    }

    /// fpm_conf_write_pid()
    pub fn conf_write_pid(&self) -> i32 {
        if let Some(p) = &self.gc.pid_file {
            // Written next to it and renamed over it: a reload (which
            // writes it again) never leaves it missing or empty for a
            // reader.
            let tmp = format!("{}.{}.tmp", p, self.parent_pid);
            let ct = std::ffi::CString::new(tmp.clone()).unwrap_or_default();
            let fd = unsafe {
                libc::creat(
                    ct.as_ptr(),
                    libc::S_IRUSR | libc::S_IWUSR | libc::S_IRGRP | libc::S_IROTH,
                )
            };
            if fd >= 0 {
                let buf = format!("{}", self.parent_pid);
                let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
                unsafe { libc::close(fd) };
                if n == buf.len() as isize && std::fs::rename(&tmp, p).is_ok() {
                    return 0;
                }
                let _ = std::fs::remove_file(&tmp);
            }
            let _ = std::fs::remove_file(p);
            let cs = std::ffi::CString::new(p.clone()).unwrap_or_default();
            let fd = unsafe {
                libc::creat(
                    cs.as_ptr(),
                    libc::S_IRUSR | libc::S_IWUSR | libc::S_IRGRP | libc::S_IROTH,
                )
            };
            if fd < 0 {
                zlog!(SYSERROR, "Unable to create the PID file ({}).", p);
                return -1;
            }
            let buf = format!("{}", self.parent_pid);
            let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
            if n != buf.len() as isize {
                zlog!(SYSERROR, "Unable to write to the PID file.");
                unsafe { libc::close(fd) };
                return -1;
            }
            unsafe { libc::close(fd) };
        }
        0
    }

    /// fpm_conf_post_process()
    fn conf_post_process(&mut self, force_daemon: i32) -> Result<(), ()> {
        if self.gc.pid_file.is_some() {
            let mut p = self.gc.pid_file.clone();
            let _ = self.evaluate_full_path(&mut p, None, Some(PHP_LOCALSTATEDIR), false);
            self.gc.pid_file = p;
        }
        if force_daemon >= 0 {
            self.gc.daemonize = force_daemon;
        }
        self.log_level = self.gc.log_level;
        zlog::set_level(self.log_level);
        if self.gc.log_limit < zlog::MIN_LIMIT {
            zlog!(ERROR, "log_limit must be greater than {}", zlog::MIN_LIMIT);
            return Err(());
        }
        zlog::set_limit(self.gc.log_limit);
        zlog::set_buffering(self.gc.log_buffering != 0);
        if self.gc.process_max < 0 {
            zlog!(ERROR, "process_max can't be negative");
            return Err(());
        }
        if self.gc.process_priority != 64
            && (self.gc.process_priority < -19 || self.gc.process_priority > 20)
        {
            zlog!(ERROR, "process.priority must be included into [-19,20]");
            return Err(());
        }
        if self.gc.error_log.is_none() {
            self.gc.error_log = Some("log/php-fpm.log".to_string());
        }
        if self.gc.syslog_ident.is_none() {
            self.gc.syslog_ident = Some("php-fpm".to_string());
        }
        if self.gc.syslog_facility < 0 {
            self.gc.syslog_facility = libc::LOG_DAEMON;
        }
        if !self
            .gc
            .error_log
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case("syslog")
        {
            let mut p = self.gc.error_log.clone();
            let _ = self.evaluate_full_path(&mut p, None, Some(PHP_LOCALSTATEDIR), false);
            self.gc.error_log = p;
        }
        if self.gc.daemonize == 0 && self.stdio_save_original_stderr() < 0 {
            return Err(());
        }
        if self.stdio_open_error_log(false) < 0 {
            return Err(());
        }
        if self.event_pre_init().is_err() {
            return Err(());
        }
        self.process_all_pools()?;
        if self.log_open(false) < 0 {
            return Err(());
        }
        for i in 0..self.pools.len() {
            if !self.pools[i]
                .config
                .access_log
                .as_deref()
                .is_some_and(|s| !s.is_empty())
            {
                continue;
            }
            let fmt = self.pools[i]
                .config
                .access_format
                .clone()
                .unwrap_or_default();
            if super::log::check_format(&fmt).is_err() {
                zlog!(
                    ERROR,
                    "[pool {}] wrong format for access.format '{}'",
                    self.pools[i].config.name(),
                    fmt
                );
                return Err(());
            }
        }
        Ok(())
    }

    /// fpm_conf_ini_parser_include()
    fn conf_ini_parser_include(&mut self, inc: &str, error: &mut bool) {
        if *error {
            return;
        }
        let filename = self.ini_filename.clone();
        let pattern = std::ffi::CString::new(inc).unwrap_or_default();
        let mut g: libc::glob_t = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::glob(
                pattern.as_ptr(),
                libc::GLOB_ERR | libc::GLOB_MARK,
                None,
                &mut g,
            )
        };
        if r != 0 {
            if r == libc::GLOB_NOMATCH {
                zlog!(
                    WARNING,
                    "Nothing matches the include pattern '{}' from {} at line {}.",
                    inc,
                    filename,
                    self.ini_lineno
                );
                unsafe { libc::globfree(&mut g) };
                return;
            }
            zlog!(
                ERROR,
                "Unable to globalize '{}' (ret={}) from {} at line {}.",
                inc,
                r,
                filename,
                self.ini_lineno
            );
            unsafe { libc::globfree(&mut g) };
            *error = true;
            return;
        }
        let mut paths = Vec::new();
        for k in 0..g.gl_pathc {
            let p = unsafe { *g.gl_pathv.add(k) };
            if p.is_null() {
                continue;
            }
            paths.push(
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        unsafe { libc::globfree(&mut g) };
        for p in paths {
            if p.is_empty() || p.ends_with('/') {
                continue;
            }
            if self.conf_load_ini_file(&p).is_err() {
                zlog!(
                    ERROR,
                    "Unable to include {} from {} at line {}",
                    p,
                    filename,
                    self.ini_lineno
                );
                *error = true;
                return;
            }
        }
    }

    /// fpm_conf_ini_parser_section()
    fn conf_ini_parser_section(&mut self, section: &str) {
        if section.eq_ignore_ascii_case("global") {
            self.current_wp = None;
            return;
        }
        for (i, wp) in self.pools.iter().enumerate() {
            if wp
                .config
                .name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(section))
            {
                self.current_wp = Some(i);
                return;
            }
        }
        let i = self.pool_alloc();
        self.pools[i].config.name = Some(section.to_string());
    }

    /// fpm_conf_ini_parser_entry()
    fn conf_ini_parser_entry(&mut self, name: &str, value: &str, error: &mut bool) {
        if name == "include" {
            if self.ini_include.is_some() {
                zlog!(
                    ERROR,
                    "[{}:{}] two includes at the same time !",
                    self.ini_filename,
                    self.ini_lineno
                );
                *error = true;
                return;
            }
            self.ini_include = Some(value.to_string());
            return;
        }
        let table = if self.current_wp.is_none() {
            GLOBAL_OPTIONS
        } else {
            POOL_OPTIONS
        };
        for (pname, kind) in table {
            if pname.eq_ignore_ascii_case(name) {
                let parsed = match parse_value(*kind, value) {
                    Ok(Val::S(s)) => match self.expand_pool_name(&s) {
                        Ok(s) => Ok(Val::S(s)),
                        Err(_) => Err("Can't use '$pool' when the pool is not defined"),
                    },
                    other => other,
                };
                match parsed {
                    Ok(v) => match self.current_wp {
                        None => set_global(&mut self.gc, pname, v),
                        Some(i) => set_pool(&mut self.pools[i].config, pname, v),
                    },
                    Err(e) => {
                        zlog!(
                            ERROR,
                            "[{}:{}] unable to parse value for entry '{}': {}",
                            self.ini_filename,
                            self.ini_lineno,
                            pname,
                            e
                        );
                        *error = true;
                    }
                }
                return;
            }
        }
        zlog!(
            ERROR,
            "[{}:{}] unknown entry '{}'",
            self.ini_filename,
            self.ini_lineno,
            name
        );
        *error = true;
    }

    /// fpm_conf_ini_parser_array()
    fn conf_ini_parser_array(&mut self, name: &str, key: &str, value: &str, error: &mut bool) {
        let (f, l) = (self.ini_filename.clone(), self.ini_lineno);
        if name == "access.suppress_path" {
            if !key.is_empty() {
                zlog!(
                    ERROR,
                    "[{}:{}] Keys provided to field 'access.suppress_path' are ignored",
                    f,
                    l
                );
                *error = true;
            }
            if !value.starts_with('/') {
                zlog!(
                    ERROR,
                    "[{}:{}] Values provided to field 'access.suppress_path' must begin with '/'",
                    f,
                    l
                );
                *error = true;
            }
            if *error {
                return;
            }
        } else if key.is_empty() {
            zlog!(
                ERROR,
                "[{}:{}] You must provide a key for field '{}'",
                f,
                l,
                name
            );
            *error = true;
            return;
        }
        let Some(wp) = self.current_wp else {
            zlog!(
                ERROR,
                "[{}:{}] Array are not allowed in the global section",
                f,
                l
            );
            *error = true;
            return;
        };
        let r = match name {
            "env" => {
                if value.is_empty() {
                    zlog!(ERROR, "[{}:{}] empty value", f, l);
                    *error = true;
                    return;
                }
                self.conf_set_array(value, false)
                    .map(|v| self.pools[wp].config.env.insert(0, (key.to_string(), v)))
            }
            "php_value" => self.conf_set_array(value, false).map(|v| {
                self.pools[wp]
                    .config
                    .php_values
                    .insert(0, (key.to_string(), v))
            }),
            "php_admin_value" => self.conf_set_array(value, false).map(|v| {
                self.pools[wp]
                    .config
                    .php_admin_values
                    .insert(0, (key.to_string(), v))
            }),
            "php_flag" => self.conf_set_array(value, true).map(|v| {
                self.pools[wp]
                    .config
                    .php_values
                    .insert(0, (key.to_string(), v))
            }),
            "php_admin_flag" => self.conf_set_array(value, true).map(|v| {
                self.pools[wp]
                    .config
                    .php_admin_values
                    .insert(0, (key.to_string(), v))
            }),
            "access.suppress_path" => self
                .conf_set_array(value, false)
                .map(|v| self.pools[wp].config.access_suppress_paths.insert(0, v)),
            _ => {
                zlog!(ERROR, "[{}:{}] unknown directive '{}'", f, l, name);
                *error = true;
                return;
            }
        };
        if let Err(e) = r {
            zlog!(
                ERROR,
                "[{}:{}] error while parsing '{}[{}]' : {}",
                f,
                l,
                name,
                key,
                e
            );
            *error = true;
        }
    }

    /// fpm_conf_load_ini_file()
    pub fn conf_load_ini_file(&mut self, filename: &str) -> Result<(), ()> {
        if filename.is_empty() {
            zlog!(ERROR, "configuration filename is empty");
            return Err(());
        }
        let data = match std::fs::read(filename) {
            Ok(d) => d,
            Err(e) => {
                unsafe { *libc::__errno_location() = e.raw_os_error().unwrap_or(0) };
                zlog!(SYSERROR, "failed to open configuration file '{}'", filename);
                return Err(());
            }
        };
        let rec = self.ini_recursion;
        self.ini_recursion += 1;
        if rec > 4 {
            zlog!(ERROR, "failed to include more than 5 files recursively");
            return Err(());
        }
        let r = self.conf_load_ini_lines(filename, &data);
        self.ini_recursion -= 1;
        r
    }

    fn conf_load_ini_lines(&mut self, filename: &str, data: &[u8]) -> Result<(), ()> {
        self.ini_lineno = 0;
        let mut lines: Vec<&[u8]> = data.split(|&c| c == b'\n').collect();
        // the read loop stops at EOF: a trailing newline gives no extra line
        if data.ends_with(b"\n") {
            lines.pop();
        }
        let mut vm = super::scratch_vm();
        for line in lines {
            self.ini_lineno += 1;
            self.ini_filename = filename.to_string();
            if line.is_empty() {
                continue;
            }
            let mut buf = line.to_vec();
            buf.push(b'\n');
            let mut error = false;
            let parsed = goro_ext_standard::ini_parser::parse_ini_events(
                &mut vm,
                &buf,
                goro_ext_standard::ini_parser::SCANNER_NORMAL,
                "Unknown",
            );
            let _ = vm.take_output();
            let (events, failed) = match parsed {
                Ok(e) => (e, false),
                Err(e) => (e, true),
            };
            for ev in events {
                if error {
                    break;
                }
                use goro_ext_standard::ini_parser::IniEvent;
                match ev {
                    IniEvent::Entry(k, v) => {
                        let (k, v) = (lossy(&k), lossy(&v));
                        self.conf_ini_parser_entry(&k, &v, &mut error)
                    }
                    IniEvent::Section(s) => self.conf_ini_parser_section(&lossy(&s)),
                    IniEvent::PopEntry(n, k, v) => {
                        let (n, k, v) = (lossy(&n), lossy(&k), lossy(&v));
                        self.conf_ini_parser_array(&n, &k, &v, &mut error)
                    }
                }
            }
            self.ini_filename = filename.to_string();
            if error || failed {
                self.ini_include = None;
                return Err(());
            }
            if let Some(inc) = self.ini_include.take() {
                let mut p = Some(inc);
                let _ = self.evaluate_full_path(&mut p, None, None, false);
                let inc = p.unwrap_or_default();
                self.conf_ini_parser_include(&inc, &mut error);
                if error {
                    return Err(());
                }
            }
        }
        Ok(())
    }

    /// fpm_conf_dump()
    fn conf_dump(&self) {
        let gc = &self.gc;
        zlog!(NOTICE, "[global]");
        zlog!(NOTICE, "\tpid = {}", str2str(&gc.pid_file));
        zlog!(NOTICE, "\terror_log = {}", str2str(&gc.error_log));
        zlog!(NOTICE, "\tsyslog.ident = {}", str2str(&gc.syslog_ident));
        zlog!(NOTICE, "\tsyslog.facility = {}", gc.syslog_facility);
        zlog!(NOTICE, "\tlog_buffering = {}", bool2str(gc.log_buffering));
        zlog!(NOTICE, "\tlog_level = {}", zlog::level_name(self.log_level));
        zlog!(NOTICE, "\tlog_limit = {}", gc.log_limit);
        zlog!(
            NOTICE,
            "\temergency_restart_interval = {}s",
            gc.emergency_restart_interval
        );
        zlog!(
            NOTICE,
            "\temergency_restart_threshold = {}",
            gc.emergency_restart_threshold
        );
        zlog!(
            NOTICE,
            "\tprocess_control_timeout = {}s",
            gc.process_control_timeout
        );
        zlog!(NOTICE, "\tprocess.max = {}", gc.process_max);
        if gc.process_priority == 64 {
            zlog!(NOTICE, "\tprocess.priority = undefined");
        } else {
            zlog!(NOTICE, "\tprocess.priority = {}", gc.process_priority);
        }
        zlog!(NOTICE, "\tdaemonize = {}", bool2str(gc.daemonize));
        zlog!(NOTICE, "\trlimit_files = {}", gc.rlimit_files);
        zlog!(NOTICE, "\trlimit_core = {}", gc.rlimit_core);
        zlog!(
            NOTICE,
            "\tevents.mechanism = {}",
            super::events::mechanism_name()
        );
        zlog!(NOTICE, " ");
        for wp in &self.pools {
            if wp.shared.is_some() {
                continue;
            }
            let c = &wp.config;
            zlog!(NOTICE, "[{}]", str2str(&c.name));
            zlog!(NOTICE, "\tprefix = {}", str2str(&c.prefix));
            zlog!(NOTICE, "\tuser = {}", str2str(&c.user));
            zlog!(NOTICE, "\tgroup = {}", str2str(&c.group));
            zlog!(NOTICE, "\tlisten = {}", str2str(&c.listen_address));
            zlog!(NOTICE, "\tlisten.backlog = {}", c.listen_backlog);
            zlog!(
                NOTICE,
                "\tlisten.acl_users = {}",
                str2str(&c.listen_acl_users)
            );
            zlog!(
                NOTICE,
                "\tlisten.acl_groups = {}",
                str2str(&c.listen_acl_groups)
            );
            zlog!(NOTICE, "\tlisten.owner = {}", str2str(&c.listen_owner));
            zlog!(NOTICE, "\tlisten.group = {}", str2str(&c.listen_group));
            zlog!(NOTICE, "\tlisten.mode = {}", str2str(&c.listen_mode));
            zlog!(
                NOTICE,
                "\tlisten.allowed_clients = {}",
                str2str(&c.listen_allowed_clients)
            );
            if c.process_priority == 64 {
                zlog!(NOTICE, "\tprocess.priority = undefined");
            } else {
                zlog!(NOTICE, "\tprocess.priority = {}", c.process_priority);
            }
            zlog!(
                NOTICE,
                "\tprocess.dumpable = {}",
                bool2str(c.process_dumpable)
            );
            zlog!(NOTICE, "\tpm = {}", pm2str(c.pm));
            zlog!(NOTICE, "\tpm.max_children = {}", c.pm_max_children);
            zlog!(NOTICE, "\tpm.start_servers = {}", c.pm_start_servers);
            zlog!(
                NOTICE,
                "\tpm.min_spare_servers = {}",
                c.pm_min_spare_servers
            );
            zlog!(
                NOTICE,
                "\tpm.max_spare_servers = {}",
                c.pm_max_spare_servers
            );
            zlog!(NOTICE, "\tpm.max_spawn_rate = {}", c.pm_max_spawn_rate);
            zlog!(
                NOTICE,
                "\tpm.process_idle_timeout = {}",
                c.pm_process_idle_timeout
            );
            zlog!(NOTICE, "\tpm.max_requests = {}", c.pm_max_requests);
            zlog!(NOTICE, "\tpm.status_path = {}", str2str(&c.pm_status_path));
            zlog!(
                NOTICE,
                "\tpm.status_listen = {}",
                str2str(&c.pm_status_listen)
            );
            zlog!(NOTICE, "\tping.path = {}", str2str(&c.ping_path));
            zlog!(NOTICE, "\tping.response = {}", str2str(&c.ping_response));
            zlog!(NOTICE, "\taccess.log = {}", str2str(&c.access_log));
            zlog!(NOTICE, "\taccess.format = {}", str2str(&c.access_format));
            for v in &c.access_suppress_paths {
                zlog!(NOTICE, "\taccess.suppress_path[] = {}", v);
            }
            zlog!(NOTICE, "\tslowlog = {}", str2str(&c.slowlog));
            zlog!(
                NOTICE,
                "\trequest_slowlog_timeout = {}s",
                c.request_slowlog_timeout
            );
            zlog!(
                NOTICE,
                "\trequest_slowlog_trace_depth = {}",
                c.request_slowlog_trace_depth
            );
            zlog!(
                NOTICE,
                "\trequest_terminate_timeout = {}s",
                c.request_terminate_timeout
            );
            zlog!(
                NOTICE,
                "\trequest_terminate_timeout_track_finished = {}",
                bool2str(c.request_terminate_timeout_track_finished)
            );
            zlog!(NOTICE, "\trlimit_files = {}", c.rlimit_files);
            zlog!(NOTICE, "\trlimit_core = {}", c.rlimit_core);
            zlog!(NOTICE, "\tchroot = {}", str2str(&c.chroot));
            zlog!(NOTICE, "\tchdir = {}", str2str(&c.chdir));
            zlog!(
                NOTICE,
                "\tcatch_workers_output = {}",
                bool2str(c.catch_workers_output)
            );
            zlog!(
                NOTICE,
                "\tdecorate_workers_output = {}",
                bool2str(c.decorate_workers_output)
            );
            zlog!(NOTICE, "\tclear_env = {}", bool2str(c.clear_env));
            zlog!(
                NOTICE,
                "\tsecurity.limit_extensions = {}",
                c.security_limit_extensions.as_deref().unwrap_or("(null)")
            );
            for (k, v) in &c.env {
                zlog!(NOTICE, "\tenv[{}] = {}", k, v);
            }
            for (k, v) in &c.php_values {
                zlog!(NOTICE, "\tphp_value[{}] = {}", k, v);
            }
            for (k, v) in &c.php_admin_values {
                zlog!(NOTICE, "\tphp_admin_value[{}] = {}", k, v);
            }
            zlog!(NOTICE, " ");
        }
    }

    /// fpm_conf_init_main()
    pub fn conf_init_main(&mut self, test_conf: i32, force_daemon: i32) -> Result<(), ()> {
        if let Some(p) = self.prefix.clone().filter(|p| !p.is_empty()) {
            if !is_dir(&p) {
                zlog!(
                    ERROR,
                    "the global prefix '{}' does not exist or is not a directory",
                    p
                );
                return Err(());
            }
        }
        if let Some(p) = self.pid.clone().filter(|p| !p.is_empty()) {
            self.gc.pid_file = Some(p);
        }
        if self.config.is_none() {
            self.config = Some(match &self.prefix {
                None => format!("{}/php-fpm.conf", PHP_SYSCONFDIR),
                Some(p) => format!("{}/etc/php-fpm.conf", p),
            });
        }
        let cfg = self.config.clone().unwrap_or_default();
        if self.conf_load_ini_file(&cfg).is_err() {
            zlog!(ERROR, "failed to load configuration file '{}'", cfg);
            return Err(());
        }
        if self.conf_post_process(force_daemon).is_err() {
            zlog!(ERROR, "failed to post process the configuration");
            return Err(());
        }
        if test_conf != 0 {
            for i in 0..self.pools.len() {
                if !self.unix_test_config(i) {
                    return Err(());
                }
            }
            if test_conf > 1 {
                self.conf_dump();
            }
            zlog!(NOTICE, "configuration file {} test is successful", cfg);
            self.test_successful = true;
            return Err(());
        }
        Ok(())
    }
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
