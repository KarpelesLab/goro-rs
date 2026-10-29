//! The FastCGI Process Manager SAPI: goro run as `php-fpm` (or
//! `goro --fpm`) (PHP manual: FastCGI Process Manager (FPM), its
//! installation and configuration pages; FastCGI Specification 1.0).
//!
//! - [`config`]: the configuration file.
//! - [`master`]: the master process, process managers and signals.
//! - [`worker`]: worker processes, connections, ping and status pages,
//!   the access log.
//! - [`request`]: one request run by the engine.
//! - [`fcgi`]: the FastCGI protocol.
//! - [`capture`]: workers' output in the log.
//! - [`scoreboard`], [`status`]: shared process state and the status
//!   page.

pub mod capture;
pub mod config;
pub mod fcgi;
pub mod log;
pub mod master;
pub mod request;
pub mod scoreboard;
pub mod status;
pub mod worker;

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// What `php-fpm` was given.
#[derive(Clone, Default)]
pub struct Options {
    /// The whole command line, for re-executing on reload.
    pub argv: Vec<OsString>,
    pub config: Option<OsString>,
    pub prefix: Option<OsString>,
    pub pid: Option<OsString>,
    /// 1 for -t, 2 for -tt (with the configuration dump).
    pub test: u8,
    /// -D / -F: whether to run in the background.
    pub daemonize: Option<bool>,
    /// -O: the log on stderr in the foreground.
    pub force_stderr: bool,
    pub allow_root: bool,
    /// php.ini (-c) and -d settings, in order.
    pub ini: Vec<(Vec<u8>, Vec<u8>)>,
}

/// What the program has to do besides the options.
pub struct Hooks {
    /// The `-v` text.
    pub version: String,
    /// Runs the CLI with arguments (for -i and -m).
    pub cli: fn(Vec<OsString>) -> i32,
}

const USAGE: &str = "Usage: php-fpm [-n] [-e] [-h] [-i] [-m] [-v] [-t] [-p <prefix>] [-g <pid>] [-c <file>] [-d foo[=bar]] [-y <file>] [-D] [-R] [-F [-O]]
  -c <path>|<file> Look for php.ini file in this directory
  -n               No php.ini file will be used
  -d foo[=bar]     Define INI entry foo with value 'bar'
  -e               Generate extended information for debugger/profiler
  -h               This help
  -i               PHP information
  -m               Show compiled in modules
  -v               Version number
  -p, --prefix <dir>
                   Specify alternative prefix path (relative paths of the
                   configuration are under it; default: the current
                   directory)
  -g, --pid <file>
                   Specify the PID file location.
  -y, --fpm-config <file>
                   Specify alternative path to FastCGI process manager config file.
  -t, --test       Test FPM configuration and exit
  -D, --daemonize  force to run in background, and ignore daemonize option from config file
  -F, --nodaemonize
                   force to stay in foreground, and ignore daemonize option from config file
  -O, --force-stderr
                   force output to stderr in nodaemonize even if stderr is not a TTY
  -R, --allow-to-run-as-root
                   Allow pool to run as root (disabled by default)
";

/// `name=value` of -d (a bare name is "1"; quotes are removed; `;`
/// starts a comment, as in php.ini).
fn split_d(v: &[u8]) -> (Vec<u8>, Vec<u8>) {
    match v.iter().position(|&c| c == b'=') {
        Some(p) => {
            let mut val = v[p + 1..].to_vec();
            if val.len() >= 2 && val[0] == b'"' && val[val.len() - 1] == b'"' {
                val = val[1..val.len() - 1].to_vec();
            } else {
                if let Some(c) = val.iter().position(|&c| c == b';') {
                    val.truncate(c);
                }
                while val.last().is_some_and(|c| c.is_ascii_whitespace()) {
                    val.pop();
                }
                match val.to_ascii_lowercase().as_slice() {
                    b"on" | b"yes" | b"true" => val = b"1".to_vec(),
                    b"off" | b"no" | b"false" | b"none" | b"null" => val.clear(),
                    _ => {}
                }
            }
            (v[..p].trim_ascii().to_vec(), val)
        }
        None => (v.trim_ascii().to_vec(), b"1".to_vec()),
    }
}

/// The settings of a php.ini given with -c (a file, or a directory
/// holding php.ini); sections are left out.
fn ini_file(f: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let p = std::path::Path::new(std::ffi::OsStr::from_bytes(f));
    let p = if p.is_dir() {
        p.join("php.ini")
    } else {
        p.to_path_buf()
    };
    let Ok(text) = std::fs::read(&p) else {
        return Vec::new();
    };
    struct E;
    impl goro_ext_standard::ini::Env for E {
        fn constant(&mut self, _n: &[u8]) -> Option<Vec<u8>> {
            None
        }
        fn var(&mut self, n: &[u8]) -> Option<Vec<u8>> {
            let n = std::str::from_utf8(n).ok()?;
            std::env::var_os(n).map(|v| v.into_vec())
        }
    }
    let Ok(ev) = goro_ext_standard::ini::parse(&text, goro_ext_standard::ini::Mode::Normal, &mut E)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in ev {
        match e {
            goro_ext_standard::ini::Event::Section(_) => break,
            goro_ext_standard::ini::Event::Entry {
                key,
                offset: None,
                value,
            } => {
                let v = match value {
                    goro_ext_standard::ini::IniValue::Str(s) => s,
                    goro_ext_standard::ini::IniValue::Int(i) => i.to_string().into_bytes(),
                    goro_ext_standard::ini::IniValue::Float(f) => f.to_string().into_bytes(),
                    goro_ext_standard::ini::IniValue::Bool(true) => b"1".to_vec(),
                    _ => Vec::new(),
                };
                out.push((key, v));
            }
            _ => {}
        }
    }
    out
}

/// The FPM defaults for the engine, as for the other web SAPIs: errors
/// shown (as HTML), no implicit flush, a 30 s time limit.
const DEFAULT_INI: &[(&str, &str)] = &[
    ("display_errors", "1"),
    ("html_errors", "1"),
    ("implicit_flush", "0"),
    ("max_execution_time", "30"),
];

/// `php-fpm ARGS`: `argv` is the whole command line.
pub fn main(argv: Vec<OsString>, args: Vec<OsString>, hooks: Hooks) -> i32 {
    let mut o = Options {
        argv,
        ..Default::default()
    };
    let mut ini: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut ini_path: Option<Vec<u8>> = None;
    let mut cli_mode: Option<&str> = None;
    let args: Vec<Vec<u8>> = args.into_iter().map(|a| a.into_vec()).collect();
    let mut i = 0;
    let usage_err = || {
        eprint!("{}", USAGE);
        64
    };
    while i < args.len() {
        let a = args[i].clone();
        i += 1;
        let (name, attached): (String, Option<Vec<u8>>) = if let Some(l) = a.strip_prefix(b"--") {
            match l.iter().position(|&c| c == b'=') {
                Some(p) => (
                    String::from_utf8_lossy(&l[..p]).into_owned(),
                    Some(l[p + 1..].to_vec()),
                ),
                None => (String::from_utf8_lossy(l).into_owned(), None),
            }
        } else if a.len() >= 2 && a[0] == b'-' {
            let c = (a[1] as char).to_string();
            let rest = (a.len() > 2).then(|| a[2..].to_vec());
            // -tt
            if a == b"-tt" {
                o.test = 2;
                continue;
            }
            (c, rest)
        } else {
            return usage_err();
        };
        let short = match name.as_str() {
            "prefix" => "p",
            "pid" => "g",
            "fpm-config" => "y",
            "test" => "t",
            "daemonize" => "D",
            "nodaemonize" => "F",
            "force-stderr" => "O",
            "allow-to-run-as-root" => "R",
            "help" => "h",
            "version" => "v",
            "php-ini" => "c",
            "no-php-ini" => "n",
            "define" => "d",
            "info" => "i",
            "modules" => "m",
            n if n.len() == 1 => n,
            _ => return usage_err(),
        };
        let needs = matches!(short, "p" | "g" | "y" | "c" | "d");
        let val = if needs {
            match attached {
                Some(v) => Some(v),
                None => {
                    let v = args.get(i).cloned();
                    i += 1;
                    match v {
                        Some(v) => Some(v),
                        None => return usage_err(),
                    }
                }
            }
        } else {
            if attached.is_some() && a.starts_with(b"--") {
                return usage_err();
            }
            None
        };
        match short {
            "p" => o.prefix = val.map(OsString::from_vec),
            "g" => o.pid = val.map(OsString::from_vec),
            "y" => o.config = val.map(OsString::from_vec),
            "c" => ini_path = val,
            "d" => ini.push(split_d(&val.unwrap_or_default())),
            "t" => o.test = o.test.max(1),
            "D" => o.daemonize = Some(true),
            "F" => o.daemonize = Some(false),
            "O" => o.force_stderr = true,
            "R" => o.allow_root = true,
            "n" | "e" => {}
            "h" | "?" => {
                print!("{}", USAGE);
                return 0;
            }
            "v" => {
                println!("{}", hooks.version);
                return 0;
            }
            "i" => cli_mode = Some("-i"),
            "m" => cli_mode = Some("-m"),
            _ => return usage_err(),
        }
    }
    if let Some(m) = cli_mode {
        let mut a: Vec<OsString> = vec!["-n".into()];
        for (k, v) in &ini {
            let mut d = k.clone();
            d.push(b'=');
            d.extend_from_slice(v);
            a.push("-d".into());
            a.push(OsString::from_vec(d));
        }
        a.push(m.into());
        return (hooks.cli)(a);
    }
    let mut all: Vec<(Vec<u8>, Vec<u8>)> = DEFAULT_INI
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect();
    if let Some(f) = &ini_path {
        all.extend(ini_file(f));
    }
    all.extend(ini);
    o.ini = all;
    master::main(o)
}
