//! The FPM configuration file (PHP manual: FastCGI Process Manager,
//! Configuration): php.ini syntax, a `[global]` section and one section
//! per pool, `include` globs, `$pool` in pool values, `${VAR}`
//! environment references, and the array directives `env[]`,
//! `php_value[]`, `php_flag[]`, `php_admin_value[]`, `php_admin_flag[]`
//! and `access.suppress_path[]`.
//!
//! The file is read line by line so that errors can name the file and
//! line ("[file:line] message"); each line's value is evaluated as php.ini
//! does it (constants, booleans, quoting) by the INI parser of
//! parse_ini_string().

use std::path::{Path, PathBuf};

use goro_ext_standard::ini::{self, Event, IniValue, Mode};

use super::log::{self, Level};

/// Process manager kinds (PHP manual: pm).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pm {
    Static,
    Dynamic,
    Ondemand,
}

impl Pm {
    pub fn name(self) -> &'static str {
        match self {
            Pm::Static => "static",
            Pm::Dynamic => "dynamic",
            Pm::Ondemand => "ondemand",
        }
    }
}

/// The `[global]` section.
#[derive(Clone, Debug)]
pub struct Global {
    pub pid: Option<String>,
    pub error_log: String,
    pub syslog_ident: String,
    pub syslog_facility: String,
    pub log_level: Level,
    pub log_limit: usize,
    pub log_buffering: bool,
    pub emergency_restart_threshold: i64,
    pub emergency_restart_interval: i64,
    pub process_control_timeout: i64,
    pub process_max: i64,
    pub process_priority: Option<i64>,
    pub daemonize: bool,
    pub rlimit_files: i64,
    pub rlimit_core: i64,
    pub events_mechanism: String,
    pub systemd_interval: i64,
}

impl Default for Global {
    fn default() -> Global {
        Global {
            pid: None,
            error_log: "log/php-fpm.log".into(),
            syslog_ident: "php-fpm".into(),
            syslog_facility: "daemon".into(),
            log_level: Level::Notice,
            log_limit: 1024,
            log_buffering: true,
            emergency_restart_threshold: 0,
            emergency_restart_interval: 0,
            process_control_timeout: 0,
            process_max: 0,
            process_priority: None,
            daemonize: true,
            rlimit_files: 0,
            rlimit_core: 0,
            events_mechanism: "epoll".into(),
            systemd_interval: 10,
        }
    }
}

/// A pool section.
#[derive(Clone, Debug)]
pub struct Pool {
    pub name: String,
    pub prefix: Option<String>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub listen: Option<String>,
    pub listen_backlog: i64,
    pub listen_allowed_clients: Option<String>,
    pub listen_owner: Option<String>,
    pub listen_group: Option<String>,
    pub listen_mode: Option<String>,
    pub listen_acl_users: Option<String>,
    pub listen_acl_groups: Option<String>,
    pub pm: Option<Pm>,
    pub max_children: i64,
    pub start_servers: i64,
    pub min_spare_servers: i64,
    pub max_spare_servers: i64,
    pub max_spawn_rate: i64,
    pub process_idle_timeout: i64,
    pub max_requests: i64,
    pub status_listen: Option<String>,
    pub status_path: Option<String>,
    pub ping_path: Option<String>,
    pub ping_response: String,
    pub process_priority: Option<i64>,
    pub process_dumpable: bool,
    pub request_terminate_timeout: i64,
    pub request_terminate_timeout_track_finished: bool,
    pub request_slowlog_timeout: i64,
    pub request_slowlog_trace_depth: i64,
    pub slowlog: Option<String>,
    pub rlimit_files: i64,
    pub rlimit_core: i64,
    pub chroot: Option<String>,
    pub chdir: Option<String>,
    pub catch_workers_output: bool,
    pub decorate_workers_output: bool,
    pub clear_env: bool,
    pub security_limit_extensions: String,
    pub access_log: Option<String>,
    pub access_format: Option<String>,
    pub access_suppress_path: Vec<String>,
    pub env: Vec<(String, String)>,
    pub php_values: Vec<(String, String)>,
    pub php_admin_values: Vec<(String, String)>,
    /// The hidden pool serving pm.status_listen (inherits the settings
    /// of its pool).
    pub status_only: bool,
}

impl Pool {
    fn new(name: &str) -> Pool {
        Pool {
            name: name.to_string(),
            prefix: None,
            user: None,
            group: None,
            listen: None,
            listen_backlog: 511,
            listen_allowed_clients: None,
            listen_owner: None,
            listen_group: None,
            listen_mode: None,
            listen_acl_users: None,
            listen_acl_groups: None,
            pm: None,
            max_children: 0,
            start_servers: 0,
            min_spare_servers: 0,
            max_spare_servers: 0,
            max_spawn_rate: 32,
            process_idle_timeout: 10,
            max_requests: 0,
            status_listen: None,
            status_path: None,
            ping_path: None,
            ping_response: "pong".into(),
            process_priority: None,
            process_dumpable: false,
            request_terminate_timeout: 0,
            request_terminate_timeout_track_finished: false,
            request_slowlog_timeout: 0,
            request_slowlog_trace_depth: 20,
            slowlog: None,
            rlimit_files: 0,
            rlimit_core: 0,
            chroot: None,
            chdir: None,
            catch_workers_output: false,
            decorate_workers_output: true,
            clear_env: true,
            security_limit_extensions: ".php .phar".into(),
            access_log: None,
            access_format: None,
            access_suppress_path: Vec::new(),
            env: Vec::new(),
            php_values: Vec::new(),
            php_admin_values: Vec::new(),
            status_only: false,
        }
    }

    pub fn pm(&self) -> Pm {
        self.pm.unwrap_or(Pm::Static)
    }

    /// The path of a pool file setting: relative paths are under the
    /// pool prefix, else the global one.
    pub fn path(&self, p: &str, global_prefix: &Path) -> PathBuf {
        let p = Path::new(p);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        match &self.prefix {
            Some(pre) => Path::new(pre).join(p),
            None => global_prefix.join(p),
        }
    }
}

/// The whole configuration.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub file: PathBuf,
    pub global: Global,
    pub pools: Vec<Pool>,
}

/// Constants for php.ini expressions (E_ALL & ~E_NOTICE ...).
pub type ConstLookup<'a> = &'a dyn Fn(&[u8]) -> Option<Vec<u8>>;

struct LineEnv<'a> {
    consts: ConstLookup<'a>,
}

impl ini::Env for LineEnv<'_> {
    fn constant(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        (self.consts)(name)
    }
    fn var(&mut self, name: &[u8]) -> Option<Vec<u8>> {
        use std::os::unix::ffi::OsStrExt;
        let n = std::str::from_utf8(name).ok()?;
        Some(
            std::env::var_os(n)
                .map(|v| v.as_bytes().to_vec())
                .unwrap_or_default(),
        )
    }
}

fn value_text(v: IniValue) -> String {
    match v {
        IniValue::Str(s) => String::from_utf8_lossy(&s).into_owned(),
        IniValue::Int(i) => i.to_string(),
        IniValue::Float(f) => f.to_string(),
        IniValue::Bool(true) => "1".into(),
        IniValue::Bool(false) | IniValue::Null => String::new(),
    }
}

/// A boolean setting (php.ini's words, and what `${VAR}` gives; test
/// sapi/fpm/tests/gh13563-conf-bool-env.phpt).
fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "yes" | "on" | "true" => Some(true),
        "0" | "no" | "off" | "false" | "none" | "" => Some(false),
        _ => None,
    }
}

fn parse_int(v: &str) -> Option<i64> {
    v.trim().parse().ok()
}

/// A time with an optional unit: s (default), m, h or d (PHP manual:
/// request_terminate_timeout and the other time settings).
fn parse_time(v: &str) -> Option<i64> {
    let v = v.trim();
    let (num, mul) = match v.as_bytes().last() {
        Some(b's') => (&v[..v.len() - 1], 1),
        Some(b'm') => (&v[..v.len() - 1], 60),
        Some(b'h') => (&v[..v.len() - 1], 3600),
        Some(b'd') => (&v[..v.len() - 1], 86400),
        _ => (v, 1),
    };
    num.trim().parse::<i64>().ok().map(|n| n * mul)
}

/// Matches `name` against a shell pattern with `*`, `?` and `[...]`.
fn glob_match(p: &[u8], n: &[u8]) -> bool {
    if p.is_empty() {
        return n.is_empty();
    }
    match p[0] {
        b'*' => (0..=n.len()).any(|i| glob_match(&p[1..], &n[i..])),
        b'?' => !n.is_empty() && glob_match(&p[1..], &n[1..]),
        b'[' => {
            let Some(close) = p.iter().skip(1).position(|&c| c == b']').map(|x| x + 1) else {
                return !n.is_empty() && n[0] == b'[' && glob_match(&p[1..], &n[1..]);
            };
            if n.is_empty() {
                return false;
            }
            let set = &p[1..close];
            let (neg, set) = match set.first() {
                Some(b'!') | Some(b'^') => (true, &set[1..]),
                _ => (false, set),
            };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == b'-' {
                    if set[i] <= n[0] && n[0] <= set[i + 2] {
                        hit = true;
                    }
                    i += 3;
                } else {
                    if set[i] == n[0] {
                        hit = true;
                    }
                    i += 1;
                }
            }
            hit != neg && glob_match(&p[close + 1..], &n[1..])
        }
        c => !n.is_empty() && n[0] == c && glob_match(&p[1..], &n[1..]),
    }
}

/// The files an `include` pattern names, in alphabetical order (test
/// sapi/fpm/tests/bug68391-conf-include-order.phpt).
fn glob_files(pattern: &Path) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let s = pattern.as_os_str().as_bytes();
    if !s.iter().any(|c| matches!(c, b'*' | b'?' | b'[')) {
        return if pattern.exists() {
            vec![pattern.to_path_buf()]
        } else {
            Vec::new()
        };
    }
    let dir = pattern.parent().unwrap_or(Path::new("/"));
    let Some(fpat) = pattern.file_name() else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name();
            let n = n.as_bytes();
            !n.starts_with(b".") && glob_match(fpat.as_bytes(), n)
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Parses the configuration; errors are logged ("[file:line] ...") and
/// make the result None.
pub fn load(file: &Path, prefix: &Path, consts: ConstLookup) -> Option<Config> {
    let mut cfg = Config {
        file: file.to_path_buf(),
        ..Default::default()
    };
    let mut p = Parser {
        cfg: &mut cfg,
        prefix: prefix.to_path_buf(),
        consts,
        current: None,
        ok: true,
        depth: 0,
    };
    p.file(file);
    let ok = p.ok;
    ok.then_some(cfg)
}

struct Parser<'a, 'c> {
    cfg: &'a mut Config,
    prefix: PathBuf,
    consts: ConstLookup<'c>,
    /// The section being read: None for [global] (or before any).
    current: Option<usize>,
    ok: bool,
    depth: usize,
}

impl Parser<'_, '_> {
    fn error(&mut self, file: &Path, line: usize, msg: &str) {
        log::zlog(
            Level::Error,
            &format!("[{}:{}] {}", file.display(), line, msg),
        );
        self.ok = false;
    }

    fn file(&mut self, file: &Path) {
        let data = match std::fs::read(file) {
            Ok(d) => d,
            Err(e) => {
                log::zlog(
                    Level::Error,
                    &format!(
                        "failed to open configuration file '{}': {} ({})",
                        file.display(),
                        crate::builtins::exec::strerror(e.raw_os_error().unwrap_or(0)),
                        e.raw_os_error().unwrap_or(0)
                    ),
                );
                self.ok = false;
                return;
            }
        };
        self.depth += 1;
        for (i, raw) in data.split(|&c| c == b'\n').enumerate() {
            let line = i + 1;
            let t = raw.trim_ascii();
            if t.is_empty() || t[0] == b';' || t[0] == b'#' {
                continue;
            }
            if t[0] == b'[' {
                let Some(end) = t.iter().position(|&c| c == b']') else {
                    self.error(file, line, "syntax error");
                    continue;
                };
                let name = String::from_utf8_lossy(t[1..end].trim_ascii()).into_owned();
                self.section(&name);
                continue;
            }
            self.entry(file, line, t);
            if !self.ok && self.depth > 0 {
                // keep reading to report further errors
            }
        }
        self.depth -= 1;
    }

    fn section(&mut self, name: &str) {
        if name.eq_ignore_ascii_case("global") {
            self.current = None;
            return;
        }
        match self.cfg.pools.iter().position(|p| p.name == name) {
            Some(i) => self.current = Some(i),
            None => {
                self.cfg.pools.push(Pool::new(name));
                self.current = Some(self.cfg.pools.len() - 1);
            }
        }
    }

    fn entry(&mut self, file: &Path, line: usize, t: &[u8]) {
        let mut src = t.to_vec();
        src.push(b'\n');
        let mut env = LineEnv {
            consts: self.consts,
        };
        let ev = match ini::parse(&src, Mode::Normal, &mut env) {
            Ok(ev) => ev,
            Err(_) => {
                // A value the INI grammar rejects (as "[::]:9000"): taken
                // as written, without its quotes.
                match t.iter().position(|&c| c == b'=') {
                    Some(p) => {
                        let k = String::from_utf8_lossy(t[..p].trim_ascii()).into_owned();
                        let mut v = t[p + 1..].trim_ascii();
                        if v.len() >= 2 && (v[0] == b'"' || v[0] == b'\'') && v[v.len() - 1] == v[0]
                        {
                            v = &v[1..v.len() - 1];
                        }
                        let (key, offset) = match (k.find('['), k.ends_with(']')) {
                            (Some(b), true) => {
                                let o = &k[b + 1..k.len() - 1];
                                (
                                    k[..b].to_string(),
                                    Some((!o.is_empty()).then(|| o.as_bytes().to_vec())),
                                )
                            }
                            _ => (k, None),
                        };
                        vec![Event::Entry {
                            key: key.into_bytes(),
                            offset,
                            value: IniValue::Str(v.to_vec()),
                        }]
                    }
                    None => {
                        self.error(file, line, "syntax error");
                        return;
                    }
                }
            }
        };
        for e in ev {
            if let Event::Entry { key, offset, value } = e {
                let key = String::from_utf8_lossy(&key).into_owned();
                let mut v = value_text(value);
                if let Some(i) = self.current {
                    let name = self.cfg.pools[i].name.clone();
                    v = v.replace("$pool", &name);
                }
                let offset = offset.map(|o| o.map(|o| String::from_utf8_lossy(&o).into_owned()));
                self.set(file, line, &key, offset, v);
            }
        }
    }

    fn set(
        &mut self,
        file: &Path,
        line: usize,
        key: &str,
        offset: Option<Option<String>>,
        v: String,
    ) {
        if key == "include" {
            let pat = if Path::new(&v).is_absolute() {
                PathBuf::from(&v)
            } else {
                self.prefix.join(&v)
            };
            if self.depth > 16 {
                self.error(file, line, "too many nested includes");
                return;
            }
            let files = glob_files(&pat);
            if files.is_empty() && !v.contains(['*', '?', '[']) {
                self.error(
                    file,
                    line,
                    &format!(
                        "Unable to include {} from {} at line {}",
                        v,
                        file.display(),
                        line
                    ),
                );
                return;
            }
            let saved = self.current;
            for f in files {
                self.file(&f);
            }
            self.current = saved;
            return;
        }
        let bad = |p: &mut Self, what: &str| {
            p.error(
                file,
                line,
                &format!("Invalid value '{}' for '{}'{}", v, key, what),
            );
        };
        match self.current {
            None => {
                if offset.is_some() {
                    self.error(file, line, &format!("unknown entry '{}'", key));
                    return;
                }
                let g = &mut self.cfg.global;
                match key {
                    "pid" => g.pid = Some(v.clone()),
                    "error_log" => g.error_log = v.clone(),
                    "syslog.ident" => g.syslog_ident = v.clone(),
                    "syslog.facility" => g.syslog_facility = v.clone(),
                    "log_level" => match Level::parse(&v) {
                        Some(l) => g.log_level = l,
                        None => bad(self, ""),
                    },
                    "log_limit" => match parse_int(&v) {
                        Some(n) if n >= 0 => g.log_limit = n as usize,
                        _ => bad(self, ""),
                    },
                    "log_buffering" => match parse_bool(&v) {
                        Some(b) => g.log_buffering = b,
                        None => bad(self, ""),
                    },
                    "emergency_restart_threshold" => match parse_int(&v) {
                        Some(n) => g.emergency_restart_threshold = n,
                        None => bad(self, ""),
                    },
                    "emergency_restart_interval" => match parse_time(&v) {
                        Some(n) => g.emergency_restart_interval = n,
                        None => bad(self, ""),
                    },
                    "process_control_timeout" => match parse_time(&v) {
                        Some(n) => g.process_control_timeout = n,
                        None => bad(self, ""),
                    },
                    "process.max" => match parse_int(&v) {
                        Some(n) => g.process_max = n,
                        None => bad(self, ""),
                    },
                    "process.priority" => match parse_int(&v) {
                        Some(n) => g.process_priority = Some(n),
                        None => bad(self, ""),
                    },
                    "daemonize" => match parse_bool(&v) {
                        Some(b) => g.daemonize = b,
                        None => bad(self, ""),
                    },
                    "rlimit_files" => match parse_int(&v) {
                        Some(n) => g.rlimit_files = n,
                        None => bad(self, ""),
                    },
                    "rlimit_core" => match parse_rlimit(&v) {
                        Some(n) => g.rlimit_core = n,
                        None => bad(self, ""),
                    },
                    "events.mechanism" => g.events_mechanism = v.clone(),
                    "systemd_interval" => match parse_time(&v) {
                        Some(n) => g.systemd_interval = n,
                        None => bad(self, ""),
                    },
                    _ => self.error(file, line, &format!("unknown entry '{}'", key)),
                }
            }
            Some(i) => self.pool_set(i, file, line, key, offset, v),
        }
    }

    fn pool_set(
        &mut self,
        i: usize,
        file: &Path,
        line: usize,
        key: &str,
        offset: Option<Option<String>>,
        v: String,
    ) {
        let array_keys = [
            "env",
            "php_value",
            "php_flag",
            "php_admin_value",
            "php_admin_flag",
        ];
        if array_keys.contains(&key) {
            let Some(Some(k)) = offset else {
                self.error(
                    file,
                    line,
                    &format!("You must provide a key for field '{}'", key),
                );
                return;
            };
            let p = &mut self.cfg.pools[i];
            match key {
                "env" => {
                    // `$NAME` takes the value from FPM's own environment.
                    let val = match v.strip_prefix('$') {
                        Some(n) if !n.is_empty() => std::env::var(n).unwrap_or_default(),
                        _ => v,
                    };
                    p.env.retain(|(n, _)| n != &k);
                    p.env.push((k, val));
                }
                "php_value" | "php_admin_value" => {
                    let list = if key == "php_value" {
                        &mut p.php_values
                    } else {
                        &mut p.php_admin_values
                    };
                    list.retain(|(n, _)| n != &k);
                    list.push((k, v));
                }
                _ => {
                    let Some(b) = parse_bool(&v) else {
                        self.error(
                            file,
                            line,
                            &format!("invalid boolean value '{}' for '{}'", v, key),
                        );
                        return;
                    };
                    let list = if key == "php_flag" {
                        &mut p.php_values
                    } else {
                        &mut p.php_admin_values
                    };
                    list.retain(|(n, _)| n != &k);
                    list.push((k, if b { "1".into() } else { "0".into() }));
                }
            }
            return;
        }
        if key == "access.suppress_path" {
            match offset {
                Some(None) => {}
                Some(Some(_)) => {
                    self.error(
                        file,
                        line,
                        "Keys provided to field 'access.suppress_path' are ignored",
                    );
                    return;
                }
                None => {
                    self.error(
                        file,
                        line,
                        "You must provide an array for field 'access.suppress_path'",
                    );
                    return;
                }
            }
            if !v.starts_with('/') {
                self.error(
                    file,
                    line,
                    "Values provided to field 'access.suppress_path' must begin with '/'",
                );
                return;
            }
            self.cfg.pools[i].access_suppress_path.push(v);
            return;
        }
        if offset.is_some() {
            self.error(file, line, &format!("unknown entry '{}'", key));
            return;
        }
        let mut invalid = false;
        let p = &mut self.cfg.pools[i];
        let s = || Some(v.clone()).filter(|x| !x.is_empty());
        match key {
            "prefix" => p.prefix = s(),
            "user" => p.user = s(),
            "group" => p.group = s(),
            "listen" => p.listen = s(),
            "listen.backlog" => match parse_int(&v) {
                Some(n) => p.listen_backlog = n,
                None => invalid = true,
            },
            "listen.allowed_clients" => p.listen_allowed_clients = s(),
            "listen.owner" => p.listen_owner = s(),
            "listen.group" => p.listen_group = s(),
            "listen.mode" => p.listen_mode = s(),
            "listen.acl_users" => p.listen_acl_users = s(),
            "listen.acl_groups" => p.listen_acl_groups = s(),
            "pm" => match v.trim().to_ascii_lowercase().as_str() {
                "static" => p.pm = Some(Pm::Static),
                "dynamic" => p.pm = Some(Pm::Dynamic),
                "ondemand" => p.pm = Some(Pm::Ondemand),
                _ => invalid = true,
            },
            "pm.max_children" => match parse_int(&v) {
                Some(n) => p.max_children = n,
                None => invalid = true,
            },
            "pm.start_servers" => match parse_int(&v) {
                Some(n) => p.start_servers = n,
                None => invalid = true,
            },
            "pm.min_spare_servers" => match parse_int(&v) {
                Some(n) => p.min_spare_servers = n,
                None => invalid = true,
            },
            "pm.max_spare_servers" => match parse_int(&v) {
                Some(n) => p.max_spare_servers = n,
                None => invalid = true,
            },
            "pm.max_spawn_rate" => match parse_int(&v) {
                Some(n) => p.max_spawn_rate = n,
                None => invalid = true,
            },
            "pm.process_idle_timeout" => match parse_time(&v) {
                Some(n) => p.process_idle_timeout = n,
                None => invalid = true,
            },
            "pm.max_requests" => match parse_int(&v) {
                Some(n) => p.max_requests = n,
                None => invalid = true,
            },
            "pm.status_listen" => p.status_listen = s(),
            "pm.status_path" => p.status_path = s(),
            "ping.path" => p.ping_path = s(),
            "ping.response" => p.ping_response = v.clone(),
            "process.priority" => match parse_int(&v) {
                Some(n) => p.process_priority = Some(n),
                None => invalid = true,
            },
            "process.dumpable" => match parse_bool(&v) {
                Some(b) => p.process_dumpable = b,
                None => invalid = true,
            },
            "request_terminate_timeout" => match parse_time(&v) {
                Some(n) => p.request_terminate_timeout = n,
                None => invalid = true,
            },
            "request_terminate_timeout_track_finished" => match parse_bool(&v) {
                Some(b) => p.request_terminate_timeout_track_finished = b,
                None => invalid = true,
            },
            "request_slowlog_timeout" => match parse_time(&v) {
                Some(n) => p.request_slowlog_timeout = n,
                None => invalid = true,
            },
            "request_slowlog_trace_depth" => match parse_int(&v) {
                Some(n) => p.request_slowlog_trace_depth = n,
                None => invalid = true,
            },
            "slowlog" => p.slowlog = s(),
            "rlimit_files" => match parse_int(&v) {
                Some(n) => p.rlimit_files = n,
                None => invalid = true,
            },
            "rlimit_core" => match parse_rlimit(&v) {
                Some(n) => p.rlimit_core = n,
                None => invalid = true,
            },
            "chroot" => p.chroot = s(),
            "chdir" => p.chdir = s(),
            "catch_workers_output" => match parse_bool(&v) {
                Some(b) => p.catch_workers_output = b,
                None => invalid = true,
            },
            "decorate_workers_output" => match parse_bool(&v) {
                Some(b) => p.decorate_workers_output = b,
                None => invalid = true,
            },
            "clear_env" => match parse_bool(&v) {
                Some(b) => p.clear_env = b,
                None => invalid = true,
            },
            "security.limit_extensions" => p.security_limit_extensions = v.clone(),
            "access.log" => p.access_log = s(),
            "access.format" => p.access_format = s(),
            _ => {
                self.error(file, line, &format!("unknown entry '{}'", key));
                return;
            }
        }
        if invalid {
            self.error(file, line, &format!("Invalid value '{}' for '{}'", v, key));
        }
    }
}

/// rlimit_core: a number or "unlimited".
fn parse_rlimit(v: &str) -> Option<i64> {
    if v.trim().eq_ignore_ascii_case("unlimited") {
        return Some(-1);
    }
    parse_int(v)
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "undefined".into())
}

/// The configuration as `-tt` shows it, one line per setting.
pub fn dump(cfg: &Config) -> Vec<String> {
    let g = &cfg.global;
    let mut out = vec!["[General]".to_string()];
    let mut kv = |k: &str, v: String| out.push(format!("\t{} = {}", k, v));
    kv("pid", opt(&g.pid));
    kv("error_log", g.error_log.clone());
    kv("syslog.ident", g.syslog_ident.clone());
    kv("syslog.facility", g.syslog_facility.clone());
    kv("log_buffering", yes_no(g.log_buffering).into());
    kv("log_level", g.log_level.name().to_ascii_lowercase());
    kv("log_limit", g.log_limit.to_string());
    kv(
        "emergency_restart_interval",
        format!("{}s", g.emergency_restart_interval),
    );
    kv(
        "emergency_restart_threshold",
        g.emergency_restart_threshold.to_string(),
    );
    kv(
        "process_control_timeout",
        format!("{}s", g.process_control_timeout),
    );
    kv("process.max", g.process_max.to_string());
    kv(
        "process.priority",
        g.process_priority
            .map(|p| p.to_string())
            .unwrap_or_else(|| "undefined".into()),
    );
    kv("daemonize", yes_no(g.daemonize).into());
    kv("rlimit_files", g.rlimit_files.to_string());
    kv("rlimit_core", g.rlimit_core.to_string());
    kv("events.mechanism", g.events_mechanism.clone());
    out.push(String::new());
    for p in cfg.pools.iter().filter(|p| !p.status_only) {
        out.push(format!("[{}]", p.name));
        let mut kv = |k: &str, v: String| out.push(format!("\t{} = {}", k, v));
        kv("prefix", opt(&p.prefix));
        kv("user", opt(&p.user));
        kv("group", opt(&p.group));
        kv("listen", opt(&p.listen));
        kv("listen.backlog", p.listen_backlog.to_string());
        kv("listen.acl_users", opt(&p.listen_acl_users));
        kv("listen.acl_groups", opt(&p.listen_acl_groups));
        kv("listen.owner", opt(&p.listen_owner));
        kv("listen.group", opt(&p.listen_group));
        kv("listen.mode", opt(&p.listen_mode));
        kv("listen.allowed_clients", opt(&p.listen_allowed_clients));
        kv(
            "process.priority",
            p.process_priority
                .map(|p| p.to_string())
                .unwrap_or_else(|| "undefined".into()),
        );
        kv("process.dumpable", yes_no(p.process_dumpable).into());
        kv("pm", p.pm().name().into());
        kv("pm.max_children", p.max_children.to_string());
        kv("pm.start_servers", p.start_servers.to_string());
        kv("pm.min_spare_servers", p.min_spare_servers.to_string());
        kv("pm.max_spare_servers", p.max_spare_servers.to_string());
        kv("pm.max_spawn_rate", p.max_spawn_rate.to_string());
        kv(
            "pm.process_idle_timeout",
            format!("{}s", p.process_idle_timeout),
        );
        kv("pm.max_requests", p.max_requests.to_string());
        kv("pm.status_path", opt(&p.status_path));
        kv("pm.status_listen", opt(&p.status_listen));
        kv("ping.path", opt(&p.ping_path));
        kv("ping.response", p.ping_response.clone());
        kv("access.log", opt(&p.access_log));
        kv("access.format", opt(&p.access_format));
        for s in &p.access_suppress_path {
            out.push(format!("\taccess.suppress_path[] = {}", s));
        }
        let mut kv = |k: &str, v: String| out.push(format!("\t{} = {}", k, v));
        kv("slowlog", opt(&p.slowlog));
        kv(
            "request_slowlog_timeout",
            format!("{}s", p.request_slowlog_timeout),
        );
        kv(
            "request_slowlog_trace_depth",
            p.request_slowlog_trace_depth.to_string(),
        );
        kv(
            "request_terminate_timeout",
            format!("{}s", p.request_terminate_timeout),
        );
        kv(
            "request_terminate_timeout_track_finished",
            yes_no(p.request_terminate_timeout_track_finished).into(),
        );
        kv("rlimit_files", p.rlimit_files.to_string());
        kv("rlimit_core", p.rlimit_core.to_string());
        kv("chroot", opt(&p.chroot));
        kv("chdir", opt(&p.chdir));
        kv(
            "catch_workers_output",
            yes_no(p.catch_workers_output).into(),
        );
        kv(
            "decorate_workers_output",
            yes_no(p.decorate_workers_output).into(),
        );
        kv("clear_env", yes_no(p.clear_env).into());
        kv(
            "security.limit_extensions",
            p.security_limit_extensions.clone(),
        );
        for (k, v) in &p.env {
            out.push(format!("\tenv[{}] = {}", k, v));
        }
        for (k, v) in &p.php_values {
            out.push(format!("\tphp_value[{}] = {}", k, v));
        }
        for (k, v) in &p.php_admin_values {
            out.push(format!("\tphp_admin_value[{}] = {}", k, v));
        }
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match(b"*.conf", b"a.conf"));
        assert!(!glob_match(b"*.conf", b"a.ini"));
        assert!(glob_match(b"a?[0-9].c*", b"ab7.conf"));
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("10"), Some(10));
        assert_eq!(parse_time("2m"), Some(120));
        assert_eq!(parse_bool("${X}"), None);
        assert_eq!(parse_bool("none"), Some(false));
    }
}
