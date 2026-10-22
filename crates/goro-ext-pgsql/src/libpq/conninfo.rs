//! Connection option parsing (fe-connect.c: conninfo_parse,
//! conninfo_uri_parse, conninfo_add_defaults, passwordFromFile).

/// PQconninfoOptions: (keyword, environment variable, compiled default)
pub const OPTIONS: &[(&str, Option<&str>, Option<&str>)] = &[
    ("service", Some("PGSERVICE"), None),
    ("user", Some("PGUSER"), None),
    ("password", Some("PGPASSWORD"), None),
    ("passfile", Some("PGPASSFILE"), None),
    ("channel_binding", Some("PGCHANNELBINDING"), Some("prefer")),
    ("connect_timeout", Some("PGCONNECT_TIMEOUT"), None),
    ("dbname", Some("PGDATABASE"), None),
    ("host", Some("PGHOST"), None),
    ("hostaddr", Some("PGHOSTADDR"), None),
    ("port", Some("PGPORT"), Some("5432")),
    ("client_encoding", Some("PGCLIENTENCODING"), None),
    ("options", Some("PGOPTIONS"), Some("")),
    ("application_name", Some("PGAPPNAME"), None),
    ("fallback_application_name", None, None),
    ("keepalives", None, None),
    ("keepalives_idle", None, None),
    ("keepalives_interval", None, None),
    ("keepalives_count", None, None),
    ("tcp_user_timeout", None, None),
    ("sslmode", Some("PGSSLMODE"), Some("prefer")),
    ("sslnegotiation", Some("PGSSLNEGOTIATION"), Some("postgres")),
    ("sslcompression", Some("PGSSLCOMPRESSION"), Some("0")),
    ("sslcert", Some("PGSSLCERT"), None),
    ("sslkey", Some("PGSSLKEY"), None),
    ("sslcertmode", Some("PGSSLCERTMODE"), Some("allow")),
    ("sslpassword", None, None),
    ("sslrootcert", Some("PGSSLROOTCERT"), None),
    ("sslcrl", Some("PGSSLCRL"), None),
    ("sslcrldir", Some("PGSSLCRLDIR"), None),
    ("sslsni", Some("PGSSLSNI"), Some("1")),
    ("requirepeer", Some("PGREQUIREPEER"), None),
    ("require_auth", Some("PGREQUIREAUTH"), None),
    (
        "ssl_min_protocol_version",
        Some("PGSSLMINPROTOCOLVERSION"),
        Some("TLSv1.2"),
    ),
    (
        "ssl_max_protocol_version",
        Some("PGSSLMAXPROTOCOLVERSION"),
        None,
    ),
    ("gssencmode", Some("PGGSSENCMODE"), Some("disable")),
    ("krbsrvname", Some("PGKRBSRVNAME"), Some("postgres")),
    ("gsslib", Some("PGGSSLIB"), None),
    ("gssdelegation", Some("PGGSSDELEGATION"), Some("0")),
    ("replication", None, None),
    (
        "target_session_attrs",
        Some("PGTARGETSESSIONATTRS"),
        Some("any"),
    ),
    (
        "load_balance_hosts",
        Some("PGLOADBALANCEHOSTS"),
        Some("disable"),
    ),
];

/// A parsed option set: one optional value per entry of OPTIONS.
#[derive(Clone, Debug, Default)]
pub struct ConnInfo {
    pub vals: Vec<Option<String>>,
}

impl ConnInfo {
    pub fn new() -> ConnInfo {
        ConnInfo {
            vals: vec![None; OPTIONS.len()],
        }
    }

    fn index(name: &str) -> Option<usize> {
        OPTIONS.iter().position(|o| o.0 == name)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        Self::index(name).and_then(|i| self.vals[i].as_deref())
    }

    pub fn set(&mut self, name: &str, v: Option<String>) {
        if let Some(i) = Self::index(name) {
            self.vals[i] = v;
        }
    }

    /// conninfo_storeval
    fn store(&mut self, name: &str, value: String, err: &mut String) -> bool {
        // "requiressl" is an old name of sslmode=require
        let (name, value) = if name == "requiressl" {
            (
                "sslmode",
                if value.starts_with('1') {
                    "require".to_string()
                } else {
                    "prefer".to_string()
                },
            )
        } else {
            (name, value)
        };
        match Self::index(name) {
            Some(i) => {
                self.vals[i] = Some(value);
                true
            }
            None => {
                err.push_str(&format!("invalid connection option \"{}\"\n", name));
                false
            }
        }
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Whether a dbname / conninfo string is a URI
pub fn uri_prefix_length(s: &[u8]) -> usize {
    if s.starts_with(b"postgresql://") {
        13
    } else if s.starts_with(b"postgres://") {
        11
    } else {
        0
    }
}

/// recognized_connection_string
pub fn recognized_connection_string(s: &[u8]) -> bool {
    uri_prefix_length(s) != 0 || s.contains(&b'=')
}

/// parse_connection_string: key=value pairs or a URI.
pub fn parse(conninfo: &[u8], err: &mut String) -> Option<ConnInfo> {
    if uri_prefix_length(conninfo) != 0 {
        parse_uri(conninfo, err)
    } else {
        parse_kv(conninfo, err)
    }
}

/// conninfo_parse
pub fn parse_kv(conninfo: &[u8], err: &mut String) -> Option<ConnInfo> {
    let mut opts = ConnInfo::new();
    let s = conninfo;
    let mut i = 0;
    let n = s.len();
    while i < n {
        if is_space(s[i]) {
            i += 1;
            continue;
        }
        // the keyword
        let kstart = i;
        let mut kend;
        loop {
            if i >= n {
                kend = i;
                break;
            }
            if s[i] == b'=' {
                kend = i;
                break;
            }
            if is_space(s[i]) {
                kend = i;
                i += 1;
                while i < n && is_space(s[i]) {
                    i += 1;
                }
                break;
            }
            i += 1;
        }
        if kend > n {
            kend = n;
        }
        let pname = String::from_utf8_lossy(&s[kstart..kend]).into_owned();
        if i >= n || s[i] != b'=' {
            err.push_str(&format!(
                "missing \"=\" after \"{}\" in connection info string\n",
                pname
            ));
            return None;
        }
        i += 1;
        while i < n && is_space(s[i]) {
            i += 1;
        }
        let mut val = Vec::new();
        if i < n && s[i] == b'\'' {
            i += 1;
            loop {
                if i >= n {
                    err.push_str("unterminated quoted string in connection info string\n");
                    return None;
                }
                if s[i] == b'\\' {
                    i += 1;
                    if i < n {
                        val.push(s[i]);
                        i += 1;
                    }
                    continue;
                }
                if s[i] == b'\'' {
                    i += 1;
                    break;
                }
                val.push(s[i]);
                i += 1;
            }
        } else {
            while i < n {
                if is_space(s[i]) {
                    i += 1;
                    break;
                }
                if s[i] == b'\\' {
                    i += 1;
                    if i < n {
                        val.push(s[i]);
                        i += 1;
                    }
                } else {
                    val.push(s[i]);
                    i += 1;
                }
            }
        }
        let val = String::from_utf8_lossy(&val).into_owned();
        if !opts.store(&pname, val, err) {
            return None;
        }
    }
    Some(opts)
}

fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// conninfo_uri_decode
fn uri_decode(s: &[u8], err: &mut String) -> Option<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            let (h, l) = (
                s.get(i + 1).copied().and_then(hexval),
                s.get(i + 2).copied().and_then(hexval),
            );
            match (h, l) {
                (Some(h), Some(l)) => {
                    let c = (h << 4) | l;
                    if c == 0 {
                        err.push_str(&format!(
                            "forbidden value %00 in percent-encoded value: \"{}\"\n",
                            String::from_utf8_lossy(s)
                        ));
                        return None;
                    }
                    out.push(c);
                    i += 3;
                }
                _ => {
                    err.push_str(&format!(
                        "invalid percent-encoded token: \"{}\"\n",
                        String::from_utf8_lossy(s)
                    ));
                    return None;
                }
            }
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// conninfo_uri_parse
pub fn parse_uri(uri: &[u8], err: &mut String) -> Option<ConnInfo> {
    let mut opts = ConnInfo::new();
    let start = uri_prefix_length(uri);
    let full = String::from_utf8_lossy(uri).into_owned();
    let rest = &uri[start..];
    // split off ?params
    let (main, query) = match rest.iter().position(|&c| c == b'?') {
        Some(p) => (&rest[..p], Some(&rest[p + 1..])),
        None => (rest, None),
    };
    let (netloc, path) = match main.iter().position(|&c| c == b'/') {
        Some(p) => (&main[..p], Some(&main[p + 1..])),
        None => (main, None),
    };
    // user info
    let (userinfo, hostlist) = match netloc.iter().rposition(|&c| c == b'@') {
        Some(p) => (Some(&netloc[..p]), &netloc[p + 1..]),
        None => (None, netloc),
    };
    if let Some(ui) = userinfo {
        let (u, p) = match ui.iter().position(|&c| c == b':') {
            Some(p) => (&ui[..p], Some(&ui[p + 1..])),
            None => (ui, None),
        };
        if !u.is_empty() {
            let v = uri_decode(u, err)?;
            opts.set("user", Some(v));
        }
        if let Some(p) = p {
            let v = uri_decode(p, err)?;
            opts.set("password", Some(v));
        }
    }
    // hosts and ports
    let mut hosts = Vec::new();
    let mut ports = Vec::new();
    let mut any_port = false;
    for spec in hostlist.split(|&c| c == b',') {
        let (h, p) = if spec.first() == Some(&b'[') {
            match spec.iter().position(|&c| c == b']') {
                Some(e) => {
                    let h = &spec[1..e];
                    let after = &spec[e + 1..];
                    if !after.is_empty() && after[0] != b':' {
                        err.push_str(&format!(
                            "unexpected character \"{}\" at position {} in URI (expected \":\" or \"/\"): \"{}\"\n",
                            after[0] as char,
                            start + e + 2,
                            full
                        ));
                        return None;
                    }
                    (h, after.strip_prefix(b":"))
                }
                None => {
                    err.push_str(&format!(
                        "end of string reached when looking for matching \"]\" in IPv6 host address in URI: \"{}\"\n",
                        full
                    ));
                    return None;
                }
            }
        } else {
            match spec.iter().position(|&c| c == b':') {
                Some(p) => (&spec[..p], Some(&spec[p + 1..])),
                None => (spec, None),
            }
        };
        hosts.push(uri_decode(h, err)?);
        if let Some(p) = p {
            any_port = true;
            ports.push(uri_decode(p, err)?);
        } else {
            ports.push(String::new());
        }
    }
    if hosts.iter().any(|h| !h.is_empty()) || hosts.len() > 1 {
        opts.set("host", Some(hosts.join(",")));
    }
    if any_port {
        opts.set("port", Some(ports.join(",")));
    }
    if let Some(path) = path {
        if !path.is_empty() {
            let v = uri_decode(path, err)?;
            opts.set("dbname", Some(v));
        }
    }
    if let Some(q) = query {
        for kv in q.split(|&c| c == b'&') {
            if kv.is_empty() {
                continue;
            }
            let Some(eq) = kv.iter().position(|&c| c == b'=') else {
                err.push_str(&format!(
                    "missing key/value separator \"=\" in URI query parameter: \"{}\"\n",
                    String::from_utf8_lossy(kv)
                ));
                return None;
            };
            let k = uri_decode(&kv[..eq], err)?;
            let v = uri_decode(&kv[eq + 1..], err)?;
            let k = if k == "ssl" && v == "true" {
                opts.set("sslmode", Some("require".into()));
                continue;
            } else {
                k
            };
            if ConnInfo::index(&k).is_none() {
                err.push_str(&format!("invalid URI query parameter: \"{}\"\n", k));
                return None;
            }
            opts.set(&k, Some(v));
        }
    }
    Some(opts)
}

/// pg_fe_getauthname: the effective OS user name
pub fn os_user_name() -> Option<String> {
    unsafe {
        let uid = libc::geteuid();
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 4096];
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) == 0
            && !out.is_null()
        {
            let n = std::ffi::CStr::from_ptr(pw.pw_name);
            return Some(n.to_string_lossy().into_owned());
        }
    }
    std::env::var("USER").ok()
}

fn home_dir() -> Option<String> {
    if let Ok(h) = std::env::var("HOME") {
        return Some(h);
    }
    unsafe {
        let uid = libc::geteuid();
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut buf = vec![0 as libc::c_char; 4096];
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) == 0
            && !out.is_null()
        {
            let n = std::ffi::CStr::from_ptr(pw.pw_dir);
            return Some(n.to_string_lossy().into_owned());
        }
    }
    None
}

/// DEFAULT_PGSOCKET_DIR
pub fn default_socket_dir() -> &'static str {
    for d in ["/run/postgresql", "/var/run/postgresql"] {
        if std::path::Path::new(d).is_dir() {
            return d;
        }
    }
    "/tmp"
}

/// conninfo_add_defaults: environment variables and compiled defaults
pub fn add_defaults(opts: &mut ConnInfo) {
    for (i, (name, env, def)) in OPTIONS.iter().enumerate() {
        if opts.vals[i].is_some() {
            continue;
        }
        if let Some(e) = env {
            if let Ok(v) = std::env::var(e) {
                // PGSSLMODE etc. are taken verbatim
                opts.vals[i] = Some(v);
                continue;
            }
        }
        if *name == "sslmode" {
            if let Ok(v) = std::env::var("PGREQUIRESSL") {
                if v.starts_with('1') {
                    opts.vals[i] = Some("require".into());
                    continue;
                }
            }
        }
        if let Some(d) = def {
            opts.vals[i] = Some(d.to_string());
            continue;
        }
        if *name == "user" {
            opts.vals[i] = os_user_name();
        }
    }
}

/// The value of a service file entry, applied to unset options
/// (parseServiceInfo / parseServiceFile).
pub fn apply_service(opts: &mut ConnInfo, err: &mut String) -> bool {
    let Some(service) = opts.get("service").map(|s| s.to_string()) else {
        return true;
    };
    let mut files = Vec::new();
    if let Ok(f) = std::env::var("PGSERVICEFILE") {
        files.push(f);
    } else if let Some(h) = home_dir() {
        files.push(format!("{}/.pg_service.conf", h));
    }
    let sysconf = std::env::var("PGSYSCONFDIR").unwrap_or_else(|_| "/etc/postgresql-common".into());
    files.push(format!("{}/pg_service.conf", sysconf));
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let mut in_group = false;
        let mut found = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if in_group {
                    break;
                }
                in_group = line.trim_start_matches('[').trim_end_matches(']') == service;
                if in_group {
                    found = true;
                }
                continue;
            }
            if in_group {
                if let Some((k, v)) = line.split_once('=') {
                    let (k, v) = (k.trim(), v.trim());
                    if ConnInfo::index(k).is_none() {
                        err.push_str(&format!(
                            "syntax error in service file \"{}\", line {}\n",
                            f, 0
                        ));
                        return false;
                    }
                    if opts.get(k).is_none() {
                        opts.set(k, Some(v.to_string()));
                    }
                }
            }
        }
        if found {
            return true;
        }
    }
    err.push_str(&format!(
        "definition of service \"{}\" not found\n",
        service
    ));
    false
}

/// passwordFromFile: the password of ~/.pgpass for this host/port/db/user
pub fn password_from_file(
    passfile: Option<&str>,
    host: &str,
    port: &str,
    dbname: &str,
    user: &str,
) -> Option<String> {
    let path = match passfile {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => format!("{}/.pgpass", home_dir()?),
    };
    let text = std::fs::read_to_string(&path).ok()?;
    let host = if host.is_empty() || host.starts_with('/') {
        "localhost"
    } else {
        host
    };
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        // fields separated by ':', with backslash escapes
        let mut fields = Vec::new();
        let mut cur = String::new();
        let mut it = line.chars();
        while let Some(c) = it.next() {
            match c {
                '\\' => {
                    if let Some(n) = it.next() {
                        cur.push(n);
                    }
                }
                ':' if fields.len() < 4 => fields.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            }
        }
        fields.push(cur);
        if fields.len() != 5 {
            continue;
        }
        let m = |f: &str, v: &str| f == "*" || f == v;
        if m(&fields[0], host)
            && m(&fields[1], port)
            && m(&fields[2], dbname)
            && m(&fields[3], user)
        {
            return Some(fields[4].clone());
        }
    }
    None
}
