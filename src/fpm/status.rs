//! sapi/fpm/fpm/fpm_status.c: the status and ping pages, fpm_get_status().

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::string::PhpString;
use goro_core::value::Value;

use super::conf::pm2str;
use super::scoreboard::{self, Copy, Proc};
use super::zlog;

fn cs(b: &[u8]) -> String {
    String::from_utf8_lossy(scoreboard::cstr(b)).into_owned()
}

fn proc_cpu(p: &Proc) -> f64 {
    if p.cpu_duration.sec == 0 && p.cpu_duration.usec == 0 {
        return 0.0;
    }
    let t = &p.last_request_cpu;
    (t.utime + t.stime + t.cutime + t.cstime) as f64
        / scoreboard::tick()
        / (p.cpu_duration.sec as f64 + p.cpu_duration.usec as f64 / 1_000_000.0)
        * 100.0
}

fn proc_duration(p: &Proc, now: scoreboard::Timeval) -> u64 {
    let d = if p.request_stage == scoreboard::REQUEST_ACCEPTING {
        p.duration
    } else {
        now.sub(&p.accepted)
    };
    (d.sec as u64)
        .wrapping_mul(1_000_000)
        .wrapping_add(d.usec as u64)
}

fn set(a: &mut PhpArray, k: &str, v: Value) {
    a.set(ArrayKey::String(PhpString::from_bytes(k.as_bytes())), v);
}

fn s(v: &str) -> Value {
    Value::String(PhpString::from_bytes(v.as_bytes()))
}

/// fpm_status_export_to_zval()
pub fn export_to_value() -> Option<Value> {
    let c = scoreboard::copy(std::ptr::null_mut(), true)?;
    let sb = &c.sb;
    let now_epoch = scoreboard::epoch();
    let now = scoreboard::clock_get();
    let mut st = PhpArray::new();
    set(&mut st, "pool", s(&cs(&sb.pool)));
    set(&mut st, "process-manager", s(pm2str(sb.pm)));
    set(&mut st, "start-time", Value::Long(sb.start_epoch));
    set(
        &mut st,
        "start-since",
        Value::Long(now_epoch - sb.start_epoch),
    );
    set(&mut st, "accepted-conn", Value::Long(sb.requests as i64));
    set(&mut st, "listen-queue", Value::Long(sb.lq as i64));
    set(&mut st, "max-listen-queue", Value::Long(sb.lq_max as i64));
    set(&mut st, "listen-queue-len", Value::Long(sb.lq_len as i64));
    set(&mut st, "idle-processes", Value::Long(sb.idle as i64));
    set(&mut st, "active-processes", Value::Long(sb.active as i64));
    set(
        &mut st,
        "total-processes",
        Value::Long((sb.idle + sb.active) as i64),
    );
    set(
        &mut st,
        "max-active-processes",
        Value::Long(sb.active_max as i64),
    );
    set(
        &mut st,
        "max-children-reached",
        Value::Long(sb.max_children_reached as i64),
    );
    set(&mut st, "slow-requests", Value::Long(sb.slow_rq as i64));
    set(&mut st, "memory-peak", Value::Long(sb.memory_peak as i64));
    let mut procs = PhpArray::new();
    for p in c.procs.iter().filter(|p| p.used != 0) {
        let cpu = proc_cpu(p);
        let idle = p.request_stage == scoreboard::REQUEST_ACCEPTING;
        let mut a = PhpArray::new();
        set(&mut a, "pid", Value::Long(p.pid as i64));
        set(&mut a, "state", s(scoreboard::stage_name(p.request_stage)));
        set(&mut a, "start-time", Value::Long(p.start_epoch));
        set(
            &mut a,
            "start-since",
            Value::Long(now_epoch - p.start_epoch),
        );
        set(&mut a, "requests", Value::Long(p.requests as i64));
        set(
            &mut a,
            "request-duration",
            Value::Long(proc_duration(p, now) as i64),
        );
        let m = cs(&p.request_method);
        set(
            &mut a,
            "request-method",
            s(if m.is_empty() { "-" } else { &m }),
        );
        set(&mut a, "request-uri", s(&cs(&p.request_uri)));
        set(&mut a, "query-string", s(&cs(&p.query_string)));
        set(
            &mut a,
            "request-length",
            Value::Long(p.content_length as i64),
        );
        let u = cs(&p.auth_user);
        set(&mut a, "user", s(if u.is_empty() { "-" } else { &u }));
        let sf = cs(&p.script_filename);
        set(&mut a, "script", s(if sf.is_empty() { "-" } else { &sf }));
        set(
            &mut a,
            "last-request-cpu",
            Value::Double(if idle { cpu } else { 0.0 }),
        );
        set(
            &mut a,
            "last-request-memory",
            Value::Long(if idle { p.memory as i64 } else { 0 }),
        );
        procs.push(Value::Array(Rc::new(RefCell::new(a))));
    }
    set(&mut st, "procs", Value::Array(Rc::new(RefCell::new(procs))));
    Some(Value::Array(Rc::new(RefCell::new(st))))
}

/// php_escape_html_entities() (ENT_COMPAT, UTF-8, double encode).
fn html_escape(b: &[u8]) -> String {
    let mut out = String::new();
    for ch in String::from_utf8_lossy(b).chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// php_json_encode_string() without the quotes.
fn json_escape(b: &[u8]) -> String {
    let mut out = String::new();
    for ch in String::from_utf8_lossy(b).chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) > 0x7f => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => out.push(c),
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Plain,
    Html,
    Xml,
    Json,
    OpenMetrics,
}

pub struct StatusPage {
    pub content_type: &'static str,
    pub body: String,
}

/// The body of the status page (fpm_status_handle_request()).
pub fn render(c: &Copy, full: bool, fmt: Format) -> StatusPage {
    let sb = &c.sb;
    let now_epoch = scoreboard::epoch();
    let pool = cs(&sb.pool);
    let pm = pm2str(sb.pm);
    let since = (now_epoch - sb.start_epoch) as u64;
    let total = sb.idle + sb.active;
    let (content_type, time_format) = match fmt {
        Format::Html => ("text/html", "%d/%b/%Y:%H:%M:%S %z"),
        Format::Xml => ("text/xml", "%s"),
        Format::Json => ("application/json", "%s"),
        Format::OpenMetrics => (
            "application/openmetrics-text; version=1.0.0; charset=utf-8",
            "%s",
        ),
        Format::Plain => ("text/plain", "%d/%b/%Y:%H:%M:%S %z"),
    };
    let start_time = zlog::strftime(time_format, sb.start_epoch);
    let mut body = match fmt {
        Format::Html => format!(
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\">\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"en\" lang=\"en\">\n\
<head><title>PHP-FPM Status Page</title></head>\n\
<body>\n\
<table>\n\
<tr><th>pool</th><td>{}</td></tr>\n\
<tr><th>process manager</th><td>{}</td></tr>\n\
<tr><th>start time</th><td>{}</td></tr>\n\
<tr><th>start since</th><td>{}</td></tr>\n\
<tr><th>accepted conn</th><td>{}</td></tr>\n\
<tr><th>listen queue</th><td>{}</td></tr>\n\
<tr><th>max listen queue</th><td>{}</td></tr>\n\
<tr><th>listen queue len</th><td>{}</td></tr>\n\
<tr><th>idle processes</th><td>{}</td></tr>\n\
<tr><th>active processes</th><td>{}</td></tr>\n\
<tr><th>total processes</th><td>{}</td></tr>\n\
<tr><th>max active processes</th><td>{}</td></tr>\n\
<tr><th>max children reached</th><td>{}</td></tr>\n\
<tr><th>slow requests</th><td>{}</td></tr>\n\
<tr><th>memory peak</th><td>{}</td></tr>\n\
</table>\n",
            pool,
            pm,
            start_time,
            since,
            sb.requests,
            sb.lq,
            sb.lq_max,
            sb.lq_len,
            sb.idle,
            sb.active,
            total,
            sb.active_max,
            sb.max_children_reached,
            sb.slow_rq,
            sb.memory_peak
        ),
        Format::Xml => format!(
            "<?xml version=\"1.0\" ?>\n\
<status>\n\
<pool>{}</pool>\n\
<process-manager>{}</process-manager>\n\
<start-time>{}</start-time>\n\
<start-since>{}</start-since>\n\
<accepted-conn>{}</accepted-conn>\n\
<listen-queue>{}</listen-queue>\n\
<max-listen-queue>{}</max-listen-queue>\n\
<listen-queue-len>{}</listen-queue-len>\n\
<idle-processes>{}</idle-processes>\n\
<active-processes>{}</active-processes>\n\
<total-processes>{}</total-processes>\n\
<max-active-processes>{}</max-active-processes>\n\
<max-children-reached>{}</max-children-reached>\n\
<slow-requests>{}</slow-requests>\n\
<memory-peak>{}</memory-peak>\n",
            pool,
            pm,
            start_time,
            since,
            sb.requests,
            sb.lq,
            sb.lq_max,
            sb.lq_len,
            sb.idle,
            sb.active,
            total,
            sb.active_max,
            sb.max_children_reached,
            sb.slow_rq,
            sb.memory_peak
        ),
        Format::Json => format!(
            "{{\"pool\":\"{}\",\"process manager\":\"{}\",\"start time\":{},\"start since\":{},\"accepted conn\":{},\"listen queue\":{},\"max listen queue\":{},\"listen queue len\":{},\"idle processes\":{},\"active processes\":{},\"total processes\":{},\"max active processes\":{},\"max children reached\":{},\"slow requests\":{},\"memory peak\":{}",
            pool,
            pm,
            start_time,
            since,
            sb.requests,
            sb.lq,
            sb.lq_max,
            sb.lq_len,
            sb.idle,
            sb.active,
            total,
            sb.active_max,
            sb.max_children_reached,
            sb.slow_rq,
            sb.memory_peak
        ),
        Format::OpenMetrics => format!(
            "# HELP phpfpm_up Could pool {} using a {} PM on PHP-FPM be reached?\n\
# TYPE phpfpm_up gauge\n\
phpfpm_up 1\n\
# HELP phpfpm_start_since The number of seconds since FPM has started.\n\
# TYPE phpfpm_start_since counter\n\
phpfpm_start_since {}\n\
# HELP phpfpm_accepted_connections The number of requests accepted by the pool.\n\
# TYPE phpfpm_accepted_connections counter\n\
phpfpm_accepted_connections {}\n\
# HELP phpfpm_listen_queue The number of requests in the queue of pending connections.\n\
# TYPE phpfpm_listen_queue gauge\n\
phpfpm_listen_queue {}\n\
# HELP phpfpm_max_listen_queue The maximum number of requests in the queue of pending connections since FPM has started.\n\
# TYPE phpfpm_max_listen_queue counter\n\
phpfpm_max_listen_queue {}\n\
# TYPE phpfpm_listen_queue_length gauge\n\
# HELP phpfpm_listen_queue_length The size of the socket queue of pending connections.\n\
phpfpm_listen_queue_length {}\n\
# HELP phpfpm_idle_processes The number of idle processes.\n\
# TYPE phpfpm_idle_processes gauge\n\
phpfpm_idle_processes {}\n\
# HELP phpfpm_active_processes The number of active processes.\n\
# TYPE phpfpm_active_processes gauge\n\
phpfpm_active_processes {}\n\
# HELP phpfpm_total_processes The number of idle + active processes.\n\
# TYPE phpfpm_total_processes gauge\n\
phpfpm_total_processes {}\n\
# HELP phpfpm_max_active_processes The maximum number of active processes since FPM has started.\n\
# TYPE phpfpm_max_active_processes counter\n\
phpfpm_max_active_processes {}\n\
# HELP phpfpm_max_children_reached The number of times, the process limit has been reached, when pm tries to start more children (works only for pm 'dynamic' and 'ondemand').\n\
# TYPE phpfpm_max_children_reached counter\n\
phpfpm_max_children_reached {}\n\
# HELP phpfpm_slow_requests The number of requests that exceeded your 'request_slowlog_timeout' value.\n\
# TYPE phpfpm_slow_requests counter\n\
phpfpm_slow_requests {}\n\
# HELP phpfpm_memory_peak The memory usage peak since FPM has started.\n\
# TYPE phpfpm_memory_peak gauge\n\
phpfpm_memory_peak {}\n\
# EOF\n",
            pool,
            pm,
            since,
            sb.requests,
            sb.lq,
            sb.lq_max,
            sb.lq_len,
            sb.idle,
            sb.active,
            total,
            sb.active_max,
            sb.max_children_reached,
            sb.slow_rq,
            sb.memory_peak
        ),
        Format::Plain => format!(
            "pool:                 {}\n\
process manager:      {}\n\
start time:           {}\n\
start since:          {}\n\
accepted conn:        {}\n\
listen queue:         {}\n\
max listen queue:     {}\n\
listen queue len:     {}\n\
idle processes:       {}\n\
active processes:     {}\n\
total processes:      {}\n\
max active processes: {}\n\
max children reached: {}\n\
slow requests:        {}\n\
memory peak:          {}\n",
            pool,
            pm,
            start_time,
            since,
            sb.requests,
            sb.lq,
            sb.lq_max,
            sb.lq_len,
            sb.idle,
            sb.active,
            total,
            sb.active_max,
            sb.max_children_reached,
            sb.slow_rq,
            sb.memory_peak
        ),
    };
    if !full {
        body.push_str(match fmt {
            Format::Html => "</body></html>",
            Format::Xml => "</status>",
            Format::Json => "}",
            _ => "",
        });
        return StatusPage { content_type, body };
    }
    if fmt == Format::OpenMetrics {
        return StatusPage { content_type, body };
    }
    let (pre, sep, post) = match fmt {
        Format::Html => (
            "<table border=\"1\">\n<tr><th>pid</th><th>state</th><th>start time</th><th>start since</th><th>requests</th><th>request duration</th><th>request method</th><th>request uri</th><th>content length</th><th>user</th><th>script</th><th>last request cpu</th><th>last request memory</th></tr>\n",
            "",
            "</table></body></html>",
        ),
        Format::Xml => ("<processes>\n", "", "</processes>\n</status>"),
        Format::Json => (", \"processes\":[", ",", "]}"),
        _ => ("", "", ""),
    };
    let now = scoreboard::clock_get();
    body.push_str(pre);
    let mut first = true;
    for p in c.procs.iter().filter(|p| p.used != 0) {
        if !first {
            body.push_str(sep);
        }
        first = false;
        let qs_raw = scoreboard::cstr(&p.query_string);
        let qs: Option<String> = if qs_raw.is_empty() {
            None
        } else {
            Some(match fmt {
                Format::Html | Format::Xml => html_escape(qs_raw),
                Format::Json => json_escape(qs_raw),
                _ => String::from_utf8_lossy(qs_raw).into_owned(),
            })
        };
        let cpu = proc_cpu(p);
        let idle = p.request_stage == scoreboard::REQUEST_ACCEPTING;
        let duration = proc_duration(p, now);
        let ptime = zlog::strftime(time_format, p.start_epoch);
        let m = cs(&p.request_method);
        let m = if m.is_empty() { "-".to_string() } else { m };
        let uri = cs(&p.request_uri);
        let uri = if uri.is_empty() { "-".to_string() } else { uri };
        let q1 = if qs.is_some() { "?" } else { "" };
        let q2 = qs.unwrap_or_default();
        let u = cs(&p.auth_user);
        let u = if u.is_empty() { "-".to_string() } else { u };
        let sf = cs(&p.script_filename);
        let sf = if sf.is_empty() { "-".to_string() } else { sf };
        let lcpu = if idle { cpu } else { 0.0 };
        let lmem = if idle { p.memory } else { 0 };
        let since = (now_epoch - p.start_epoch) as u64;
        let row = match fmt {
            Format::Html => format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}{}{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.2}</td><td>{}</td></tr>\n",
                p.pid,
                scoreboard::stage_name(p.request_stage),
                ptime,
                since,
                p.requests,
                duration,
                m,
                uri,
                q1,
                q2,
                p.content_length,
                u,
                sf,
                lcpu,
                lmem
            ),
            Format::Xml => format!(
                "<process><pid>{}</pid><state>{}</state><start-time>{}</start-time><start-since>{}</start-since><requests>{}</requests><request-duration>{}</request-duration><request-method>{}</request-method><request-uri>{}{}{}</request-uri><content-length>{}</content-length><user>{}</user><script>{}</script><last-request-cpu>{:.2}</last-request-cpu><last-request-memory>{}</last-request-memory></process>\n",
                p.pid,
                scoreboard::stage_name(p.request_stage),
                ptime,
                since,
                p.requests,
                duration,
                m,
                uri,
                q1,
                q2,
                p.content_length,
                u,
                sf,
                lcpu,
                lmem
            ),
            Format::Json => format!(
                "{{\"pid\":{},\"state\":\"{}\",\"start time\":{},\"start since\":{},\"requests\":{},\"request duration\":{},\"request method\":\"{}\",\"request uri\":\"{}{}{}\",\"content length\":{},\"user\":\"{}\",\"script\":\"{}\",\"last request cpu\":{:.2},\"last request memory\":{}}}",
                p.pid,
                scoreboard::stage_name(p.request_stage),
                ptime,
                since,
                p.requests,
                duration,
                m,
                uri,
                q1,
                q2,
                p.content_length,
                u,
                sf,
                lcpu,
                lmem
            ),
            _ => format!(
                "\n************************\n\
pid:                  {}\n\
state:                {}\n\
start time:           {}\n\
start since:          {}\n\
requests:             {}\n\
request duration:     {}\n\
request method:       {}\n\
request URI:          {}{}{}\n\
content length:       {}\n\
user:                 {}\n\
script:               {}\n\
last request cpu:     {:.2}\n\
last request memory:  {}\n",
                p.pid,
                scoreboard::stage_name(p.request_stage),
                ptime,
                since,
                p.requests,
                duration,
                m,
                uri,
                q1,
                q2,
                p.content_length,
                u,
                sf,
                lcpu,
                lmem
            ),
        };
        body.push_str(&row);
    }
    body.push_str(post);
    StatusPage { content_type, body }
}
