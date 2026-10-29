//! The status page (pm.status_path) and fpm_get_status() (PHP manual:
//! FPM status page: the fields, the `full`, `json`, `xml`, `html` and
//! `openmetrics` query options). Output formats as the tests read them
//! (sapi/fpm/tests/status.inc, fpm_get_status_basic.phpt).

use std::os::fd::RawFd;
use std::sync::atomic::Ordering::Relaxed;

use goro_runtime::{Key, PhpArray, PhpStr, Value};

use super::log::{self, now_secs, now_us};
use super::scoreboard::{self, Board};

pub struct Proc {
    pub pid: i64,
    pub state: &'static str,
    pub start_time: i64,
    pub start_since: i64,
    pub requests: i64,
    pub duration: i64,
    pub method: String,
    pub uri: String,
    pub query: String,
    pub content_length: i64,
    pub user: String,
    pub script: String,
    pub last_cpu: f64,
    pub last_memory: i64,
}

pub struct Status {
    pub pool: String,
    pub pm: &'static str,
    pub start_time: i64,
    pub start_since: i64,
    pub accepted: i64,
    pub listen_queue: i64,
    pub max_listen_queue: i64,
    pub listen_queue_len: i64,
    pub idle: i64,
    pub active: i64,
    pub total: i64,
    pub max_active: i64,
    pub max_children_reached: i64,
    pub slow_requests: i64,
    pub memory_peak: i64,
    pub procs: Vec<Proc>,
}

/// The accept queue of a TCP listening socket (Linux TCP_INFO: the
/// current length and the backlog).
fn listen_queue(fd: RawFd) -> Option<(i64, i64)> {
    // SAFETY: tcp_info is plain data filled by getsockopt.
    let mut info: libc::tcp_info = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::tcp_info>() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            fd,
            libc::IPPROTO_TCP,
            libc::TCP_INFO,
            &mut info as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    (r == 0).then_some((info.tcpi_unacked as i64, info.tcpi_sacked as i64))
}

impl Status {
    pub fn read(a: &super::request::Active) -> Status {
        // SAFETY: the board's mapping outlives the request process.
        let b = unsafe { &*a.board };
        Status::of(b, &a.pool, a.pm, a.listen_backlog, a.listen_fd)
    }

    pub fn of(b: &Board, pool: &str, pm: &'static str, backlog: i64, listen_fd: RawFd) -> Status {
        let h = b.head();
        let now = now_secs();
        let now_u = now_us();
        let mut procs = Vec::new();
        let mut idle = 0;
        let mut active = 0;
        let mut peak = 0;
        for (_, s) in b.used() {
            let st = s.state.load(Relaxed);
            if st == scoreboard::STARTING {
                continue;
            }
            if st == scoreboard::IDLE {
                idle += 1;
            } else {
                active += 1;
            }
            let rs = s.req_start.load(Relaxed);
            let re = s.req_end.load(Relaxed);
            let duration = if rs == 0 {
                0
            } else if st == scoreboard::IDLE {
                (re - rs).max(0)
            } else {
                (now_u - rs).max(0)
            };
            let mem = s.last_memory.load(Relaxed) as i64;
            peak = peak.max(mem);
            let user = String::from_utf8_lossy(&s.user.get()).into_owned();
            procs.push(Proc {
                pid: s.pid.load(Relaxed) as i64,
                state: scoreboard::state_name(st),
                start_time: s.started.load(Relaxed) / 1_000_000,
                start_since: now - s.started.load(Relaxed) / 1_000_000,
                requests: s.requests.load(Relaxed) as i64,
                duration,
                method: String::from_utf8_lossy(&s.method.get()).into_owned(),
                uri: String::from_utf8_lossy(&s.uri.get()).into_owned(),
                query: String::from_utf8_lossy(&s.query.get()).into_owned(),
                content_length: s.content_length.load(Relaxed) as i64,
                user: if user.is_empty() { "-".into() } else { user },
                script: String::from_utf8_lossy(&s.script.get()).into_owned(),
                last_cpu: if st == scoreboard::IDLE {
                    f64::from_bits(s.last_cpu.load(Relaxed))
                } else {
                    0.0
                },
                last_memory: if st == scoreboard::IDLE { mem } else { 0 },
            });
        }
        let (queue, qlen) = listen_queue(listen_fd).unwrap_or((0, backlog));
        let max_active = (h.max_active.load(Relaxed) as i64).max(active);
        Status {
            pool: pool.to_string(),
            pm,
            start_time: h.start_time.load(Relaxed),
            start_since: now - h.start_time.load(Relaxed),
            accepted: h.accepted.load(Relaxed) as i64,
            listen_queue: queue,
            max_listen_queue: queue,
            listen_queue_len: qlen,
            idle,
            active,
            total: idle + active,
            max_active,
            max_children_reached: h.max_children_reached.load(Relaxed) as i64,
            slow_requests: h.slow_requests.load(Relaxed) as i64,
            memory_peak: peak,
            procs,
        }
    }

    /// fpm_get_status()'s array.
    pub fn php_array(&self) -> PhpArray {
        let s = |v: &str| Value::Str(PhpStr::from(v));
        let mut a = PhpArray::new();
        a.insert(Key::from("pool"), s(&self.pool));
        a.insert(Key::from("process-manager"), s(self.pm));
        for (k, v) in self.numbers() {
            a.insert(Key::from(k.replace(' ', "-").as_str()), Value::Int(v));
        }
        let mut procs = PhpArray::new();
        for p in &self.procs {
            let mut e = PhpArray::new();
            e.insert(Key::from("pid"), Value::Int(p.pid));
            e.insert(Key::from("state"), s(p.state));
            e.insert(Key::from("start-time"), Value::Int(p.start_time));
            e.insert(Key::from("start-since"), Value::Int(p.start_since));
            e.insert(Key::from("requests"), Value::Int(p.requests));
            e.insert(Key::from("request-duration"), Value::Int(p.duration));
            e.insert(Key::from("request-method"), s(&p.method));
            e.insert(Key::from("request-uri"), s(&p.uri));
            e.insert(Key::from("query-string"), s(&p.query));
            e.insert(Key::from("request-length"), Value::Int(p.content_length));
            e.insert(Key::from("user"), s(&p.user));
            e.insert(Key::from("script"), s(&p.script));
            e.insert(Key::from("last-request-cpu"), Value::Float(p.last_cpu));
            e.insert(Key::from("last-request-memory"), Value::Int(p.last_memory));
            let _ = procs.push(Value::Array(e));
        }
        a.insert(Key::from("procs"), Value::Array(procs));
        a
    }

    /// The numeric fields after "process manager", in page order; "start
    /// time" is a Unix time here.
    fn numbers(&self) -> Vec<(&'static str, i64)> {
        vec![
            ("start time", self.start_time),
            ("start since", self.start_since),
            ("accepted conn", self.accepted),
            ("listen queue", self.listen_queue),
            ("max listen queue", self.max_listen_queue),
            ("listen queue len", self.listen_queue_len),
            ("idle processes", self.idle),
            ("active processes", self.active),
            ("total processes", self.total),
            ("max active processes", self.max_active),
            ("max children reached", self.max_children_reached),
            ("slow requests", self.slow_requests),
            ("memory peak", self.memory_peak),
        ]
    }

    /// The page for a query string: (content type, body).
    pub fn page(&self, query: &str) -> (&'static str, String) {
        let full = query.contains("full");
        if query.contains("json") {
            ("application/json", self.json(full))
        } else if query.contains("xml") {
            ("text/xml", self.xml(full))
        } else if query.contains("html") {
            ("text/html", self.html(full))
        } else if query.contains("openmetrics") {
            (
                "application/openmetrics-text; version=1.0.0; charset=utf-8",
                self.openmetrics(),
            )
        } else {
            ("text/plain", self.plain(full))
        }
    }

    fn rows(&self, fmt_time: bool) -> Vec<(String, String)> {
        let mut r = vec![
            ("pool".to_string(), self.pool.clone()),
            ("process manager".to_string(), self.pm.to_string()),
        ];
        for (k, v) in self.numbers() {
            let val = if k == "start time" && fmt_time {
                clf_time(v)
            } else {
                v.to_string()
            };
            r.push((k.to_string(), val));
        }
        r
    }

    fn proc_rows(p: &Proc, fmt_time: bool) -> Vec<(&'static str, String)> {
        vec![
            ("pid", p.pid.to_string()),
            ("state", p.state.to_string()),
            (
                "start time",
                if fmt_time {
                    clf_time(p.start_time)
                } else {
                    p.start_time.to_string()
                },
            ),
            ("start since", p.start_since.to_string()),
            ("requests", p.requests.to_string()),
            ("request duration", p.duration.to_string()),
            ("request method", p.method.clone()),
            ("request URI", p.full_uri()),
            ("content length", p.content_length.to_string()),
            ("user", p.user.clone()),
            ("script", p.script.clone()),
            ("last request cpu", format!("{:.2}", p.last_cpu)),
            ("last request memory", p.last_memory.to_string()),
        ]
    }

    fn plain(&self, full: bool) -> String {
        let mut o = String::new();
        for (k, v) in self.rows(true) {
            o.push_str(&format!("{:<22}{}\n", format!("{}:", k), v));
        }
        if full {
            for p in &self.procs {
                o.push_str("\n************************\n");
                for (k, v) in Status::proc_rows(p, true) {
                    o.push_str(&format!("{:<22}{}\n", format!("{}:", k), v));
                }
            }
        }
        o
    }

    fn html(&self, full: bool) -> String {
        let mut o = String::from(
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\">\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"en\" lang=\"en\">\n<head><title>PHP-FPM Status Page</title></head>\n<body>\n<table>\n",
        );
        for (k, v) in self.rows(true) {
            o.push_str(&format!("<tr><th>{}</th><td>{}</td></tr>\n", k, html_escape(&v)));
        }
        o.push_str("</table>\n");
        if full {
            o.push_str("<table border=\"1\">\n<tr>");
            for (k, _) in Status::proc_rows(&Proc::empty(), true) {
                o.push_str(&format!("<th>{}</th>", k));
            }
            o.push_str("</tr>\n");
            for p in &self.procs {
                o.push_str("<tr>");
                for (_, v) in Status::proc_rows(p, true) {
                    o.push_str(&format!("<td>{}</td>", html_escape(&v)));
                }
                o.push_str("</tr>\n");
            }
            o.push_str("</table>\n");
        }
        o.push_str("</body></html>");
        o
    }

    fn xml(&self, full: bool) -> String {
        let mut o = String::from("<?xml version=\"1.0\" ?>\n<status>\n");
        for (k, v) in self.rows(false) {
            let t = k.replace(' ', "-");
            o.push_str(&format!("<{}>{}</{}>\n", t, html_escape(&v), t));
        }
        if full {
            o.push_str("<processes>\n");
            for p in &self.procs {
                o.push_str("<process>");
                for (k, v) in Status::proc_rows(p, false) {
                    let t = k.replace(' ', "-").to_ascii_lowercase();
                    o.push_str(&format!("<{}>{}</{}>", t, html_escape(&v), t));
                }
                o.push_str("</process>\n");
            }
            o.push_str("</processes>\n");
        }
        o.push_str("</status>");
        o
    }

    fn json(&self, full: bool) -> String {
        let mut o = String::from("{");
        o.push_str(&format!(
            "\"pool\":{},\"process manager\":{}",
            json_str(&self.pool),
            json_str(self.pm)
        ));
        for (k, v) in self.numbers() {
            o.push_str(&format!(",\"{}\":{}", k, v));
        }
        if full {
            o.push_str(", \"processes\":[");
            for (i, p) in self.procs.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                o.push_str(&format!(
                    "{{\"pid\":{},\"state\":{},\"start time\":{},\"start since\":{},\"requests\":{},\"request duration\":{},\"request method\":{},\"request uri\":{},\"content length\":{},\"user\":{},\"script\":{},\"last request cpu\":{:.2},\"last request memory\":{}}}",
                    p.pid,
                    json_str(p.state),
                    p.start_time,
                    p.start_since,
                    p.requests,
                    p.duration,
                    json_str(&p.method),
                    json_str(&p.full_uri()),
                    p.content_length,
                    json_str(&p.user),
                    json_str(&p.script),
                    p.last_cpu,
                    p.last_memory
                ));
            }
            o.push(']');
        }
        o.push('}');
        o
    }

    fn openmetrics(&self) -> String {
        let mut o = String::new();
        let mut m = |name: &str, help: &str, ty: &str, v: i64, type_first: bool| {
            if type_first {
                o.push_str(&format!("# TYPE phpfpm_{} {}\n# HELP phpfpm_{} {}\n", name, ty, name, help));
            } else {
                o.push_str(&format!("# HELP phpfpm_{} {}\n# TYPE phpfpm_{} {}\n", name, help, name, ty));
            }
            o.push_str(&format!("phpfpm_{} {}\n", name, v));
        };
        m(
            "up",
            &format!(
                "Could pool {} using a {} PM on PHP-FPM be reached?",
                self.pool, self.pm
            ),
            "gauge",
            1,
            false,
        );
        m("start_since", "The number of seconds since FPM has started.", "counter", self.start_since, false);
        m("accepted_connections", "The number of requests accepted by the pool.", "counter", self.accepted, false);
        m("listen_queue", "The number of requests in the queue of pending connections.", "gauge", self.listen_queue, false);
        m("max_listen_queue", "The maximum number of requests in the queue of pending connections since FPM has started.", "counter", self.max_listen_queue, false);
        m("listen_queue_length", "The size of the socket queue of pending connections.", "gauge", self.listen_queue_len, true);
        m("idle_processes", "The number of idle processes.", "gauge", self.idle, false);
        m("active_processes", "The number of active processes.", "gauge", self.active, false);
        m("total_processes", "The number of idle + active processes.", "gauge", self.total, false);
        m("max_active_processes", "The maximum number of active processes since FPM has started.", "counter", self.max_active, false);
        m("max_children_reached", "The number of times, the process limit has been reached, when pm tries to start more children (works only for pm 'dynamic' and 'ondemand').", "counter", self.max_children_reached, false);
        m("slow_requests", "The number of requests that exceeded your 'request_slowlog_timeout' value.", "counter", self.slow_requests, false);
        m("memory_peak", "The memory usage peak since FPM has started.", "gauge", self.memory_peak, false);
        o.push_str("# EOF\n");
        o
    }
}

impl Proc {
    fn empty() -> Proc {
        Proc {
            pid: 0,
            state: "",
            start_time: 0,
            start_since: 0,
            requests: 0,
            duration: 0,
            method: String::new(),
            uri: String::new(),
            query: String::new(),
            content_length: 0,
            user: String::new(),
            script: String::new(),
            last_cpu: 0.0,
            last_memory: 0,
        }
    }

    /// The request URI with its query string.
    fn full_uri(&self) -> String {
        if self.query.is_empty() || self.uri.contains('?') {
            self.uri.clone()
        } else {
            format!("{}?{}", self.uri, self.query)
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '/' => o.push_str("\\/"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// "30/Oct/2026:12:00:00 +0100" (the status page's start time).
pub fn clf_time(t: i64) -> String {
    let tm = log::local_tm(t);
    let off = tm.tm_gmtoff;
    format!(
        "{:02}/{}/{}:{:02}:{:02}:{:02} {}{:02}{:02}",
        tm.tm_mday,
        log::MONTHS[tm.tm_mon.clamp(0, 11) as usize],
        tm.tm_year + 1900,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        if off < 0 { '-' } else { '+' },
        off.abs() / 3600,
        off.abs() % 3600 / 60
    )
}
