//! The ftp:// and ftps:// wrapper (ext/standard/ftp_fopen_wrapper.c).
//!
//! The control and data connections are ordinary socket streams of the
//! tcp transport (TLS through the crypto hooks for ftps://); the stream
//! handed to the script is served by the native stream layer and closes
//! the control connection after the data one, as
//! php_stream_ftp_stream_close() does.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use super::userstream::{NativeOpened, NativeStream, NativeWrapperOps, StatBuf, wrapper_log_error};
use super::xp_socket::{enable_client_crypto, xport_connect};

const REPORT_ERRORS: i64 = 0x8;
const PHP_STREAM_MKDIR_RECURSIVE: i64 = 1;

fn func_name(vm: &Vm) -> String {
    vm.current_builtin_name().unwrap_or_default()
}

/// Call a registered builtin on stream values.
fn call(vm: &mut Vm, name: &[u8], args: &[Value]) -> Value {
    match vm.functions.get(name).copied() {
        Some(f) => f(vm, args).unwrap_or(Value::False),
        None => Value::False,
    }
}

fn s(b: &[u8]) -> Value {
    Value::String(PhpString::from_bytes(b))
}

fn write_string(vm: &mut Vm, fid: i64, data: &[u8]) {
    call(vm, b"fwrite", &[Value::resource(fid), s(data)]);
}

fn printf(vm: &mut Vm, fid: i64, cmd: &str, arg: &[u8]) {
    let mut line = cmd.as_bytes().to_vec();
    line.push(b' ');
    line.extend_from_slice(arg);
    line.extend_from_slice(b"\r\n");
    write_string(vm, fid, &line);
}

fn close(vm: &mut Vm, fid: i64) {
    call(vm, b"fclose", &[Value::resource(fid)]);
}

/// php_stream_gets(stream, buf, maxlen - 1)
fn gets(vm: &mut Vm, fid: i64, maxlen: usize) -> Option<Vec<u8>> {
    match call(
        vm,
        b"fgets",
        &[Value::resource(fid), Value::Long(maxlen as i64)],
    ) {
        Value::String(v) => Some(v.as_bytes().to_vec()),
        _ => None,
    }
}

fn feof(vm: &mut Vm, fid: i64) -> bool {
    call(vm, b"feof", &[Value::resource(fid)]).is_truthy()
}

/// strtol(buf, NULL, 10)
fn strtol(b: &[u8]) -> i64 {
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let neg = i < b.len() && b[i] == b'-';
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let mut n: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add((b[i] - b'0') as i64);
        i += 1;
    }
    if neg { -n } else { n }
}

/// strtoul(s, &end, 10): (value, digits consumed)
fn strtoul(b: &[u8]) -> (u64, usize) {
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    let mut n: u64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        n = n.wrapping_mul(10).wrapping_add((b[i] - b'0') as u64);
        i += 1;
    }
    if i == start { (0, 0) } else { (n, i) }
}

/// The control connection and its last reply line (`tmp_line`).
struct Ctl {
    fid: i64,
    line: Vec<u8>,
}

impl Ctl {
    /// get_ftp_result()
    fn result(&mut self, vm: &mut Vm) -> i64 {
        self.line.clear();
        while let Some(l) = gets(vm, self.fid, 511) {
            self.line = l;
            let b = &self.line;
            if b.len() >= 4
                && b[0].is_ascii_digit()
                && b[1].is_ascii_digit()
                && b[2].is_ascii_digit()
                && b[3] == b' '
            {
                break;
            }
        }
        strtol(&self.line)
    }

    fn line_str(&self) -> String {
        String::from_utf8_lossy(&self.line).into_owned()
    }
}

/// The parsed URL (php_uri with the default parser).
struct Resource {
    scheme: Vec<u8>,
    host: Vec<u8>,
    port: u16,
    user: Option<Vec<u8>>,
    pass: Option<Vec<u8>>,
    path: Option<Vec<u8>>,
}

fn parse(url: &[u8]) -> Option<Resource> {
    let u = super::uri_parse_url(url)?;
    Some(Resource {
        scheme: u.scheme.unwrap_or_default(),
        host: u.host.unwrap_or_default(),
        port: u.port,
        user: u.user,
        pass: u.pass,
        path: u.path,
    })
}

/// php_raw_url_decode()
fn raw_url_decode(b: &[u8]) -> Vec<u8> {
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

struct Connected {
    ctl: Ctl,
    resource: Resource,
    use_ssl_on_data: bool,
}

/// php_ftp_fopen_connect(): None after a failure.
fn fopen_connect(
    vm: &mut Vm,
    path: &[u8],
    options: i64,
    context: Option<i64>,
) -> Option<Connected> {
    let func = func_name(vm);
    let mut resource = parse(path)?;
    resource.path.as_ref()?;
    let use_ssl = resource.scheme.len() > 3 && resource.scheme[3] == b's';
    if resource.port == 0 {
        resource.port = 21;
    }
    let mut transport = b"tcp://".to_vec();
    transport.extend_from_slice(&resource.host);
    transport.extend_from_slice(format!(":{}", resource.port).as_bytes());
    let Ok(fid) = xport_connect(vm, &func, &transport, context, false) else {
        return None;
    };
    let mut ctl = Ctl {
        fid,
        line: Vec::new(),
    };
    let fail = |vm: &mut Vm, ctl: &Ctl| {
        close(vm, ctl.fid);
        None
    };

    let result = ctl.result(vm);
    if !(200..=299).contains(&result) {
        return fail(vm, &ctl);
    }
    let mut reuseid = false;
    let mut use_ssl_on_data = false;
    if use_ssl {
        write_string(vm, fid, b"AUTH TLS\r\n");
        if ctl.result(vm) != 234 {
            write_string(vm, fid, b"AUTH SSL\r\n");
            if ctl.result(vm) != 334 {
                wrapper_log_error(vm, options, "Server doesn't support FTPS.".to_string());
                return fail(vm, &ctl);
            }
            reuseid = true;
        }
        if !enable_client_crypto(vm, &func, fid) {
            wrapper_log_error(vm, options, "Unable to activate SSL mode".to_string());
            return fail(vm, &ctl);
        }
        write_string(vm, fid, b"PBSZ 0\r\n");
        ctl.result(vm);
        write_string(vm, fid, b"PROT P\r\n");
        let result = ctl.result(vm);
        use_ssl_on_data = (200..=299).contains(&result) || reuseid;
    }

    // PHP_FTP_CNTRL_CHK
    let has_cntrl = |v: &[u8]| v.iter().any(|&c| c < 0x20 || c == 0x7f);
    match &resource.user {
        Some(u) => {
            let user = raw_url_decode(u);
            if has_cntrl(&user) {
                wrapper_log_error(
                    vm,
                    options,
                    format!("Invalid login {}", String::from_utf8_lossy(&user)),
                );
                return fail(vm, &ctl);
            }
            printf(vm, fid, "USER", &user);
        }
        None => write_string(vm, fid, b"USER anonymous\r\n"),
    }
    let mut result = ctl.result(vm);
    if (300..=399).contains(&result) {
        match &resource.pass {
            Some(p) => {
                let pass = raw_url_decode(p);
                if has_cntrl(&pass) {
                    wrapper_log_error(
                        vm,
                        options,
                        format!("Invalid password {}", String::from_utf8_lossy(&pass)),
                    );
                    return fail(vm, &ctl);
                }
                printf(vm, fid, "PASS", &pass);
            }
            None => {
                let from = vm
                    .constants
                    .get(b"from".as_ref())
                    .map(|v| v.to_php_string().as_bytes().to_vec())
                    .filter(|f| !f.is_empty());
                match from {
                    Some(f) => printf(vm, fid, "PASS", &f),
                    None => write_string(vm, fid, b"PASS anonymous\r\n"),
                }
            }
        }
        result = ctl.result(vm);
    }
    if !(200..=299).contains(&result) {
        return fail(vm, &ctl);
    }
    Some(Connected {
        ctl,
        resource,
        use_ssl_on_data,
    })
}

/// php_fopen_do_pasv(): (port, host from the PASV reply)
fn do_pasv(vm: &mut Vm, ctl: &mut Ctl) -> Option<(u16, Option<Vec<u8>>)> {
    write_string(vm, ctl.fid, b"EPSV\r\n");
    if ctl.result(vm) != 229 {
        write_string(vm, ctl.fid, b"PASV\r\n");
        if ctl.result(vm) != 227 {
            return None;
        }
        // parse pasv command (129, 80, 95, 25, 13, 221)
        let line = ctl.line.clone();
        let mut t = 4.min(line.len());
        while t < line.len() && !line[t].is_ascii_digit() {
            t += 1;
        }
        if t >= line.len() {
            return None;
        }
        let start = t;
        let mut ip = Vec::new();
        for i in 0..4 {
            while t < line.len() && line[t].is_ascii_digit() {
                t += 1;
            }
            if t >= line.len() || line[t] != b',' {
                return None;
            }
            ip.extend_from_slice(&line[start + ip.len()..t]);
            if i < 3 {
                ip.push(b'.');
            }
            t += 1;
        }
        ip.truncate(15);
        let (msb, n) = strtoul(&line[t..]);
        t += n;
        if t >= line.len() || line[t] != b',' {
            return None;
        }
        t += 1;
        let (lsb, _) = strtoul(&line[t..]);
        let port = ((msb as u16).wrapping_mul(256)).wrapping_add(lsb as u16);
        if port == 0 {
            return None;
        }
        return Some((port, Some(ip)));
    }
    // parse epsv command (|||6446|)
    let line = &ctl.line;
    let mut bars = 0;
    let mut t = 4.min(line.len());
    while t < line.len() {
        if line[t] == b'|' {
            bars += 1;
            if bars == 3 {
                break;
            }
        }
        t += 1;
    }
    if bars < 3 {
        return None;
    }
    let (port, _) = strtoul(&line[t + 1..]);
    let port = port as u16;
    if port == 0 { None } else { Some((port, None)) }
}

fn data_transport(host: &[u8], port: u16) -> Vec<u8> {
    let mut t = b"tcp://".to_vec();
    t.extend_from_slice(host);
    t.extend_from_slice(format!(":{}", port).as_bytes());
    t
}

/// The data stream of an ftp:// file (its control stream is
/// `stream->wrapperthis`).
struct FtpStream {
    data: Option<i64>,
    control: Option<i64>,
    write_mode: bool,
}

impl NativeStream for FtpStream {
    fn read(&mut self, vm: &mut Vm, count: usize, eof: &mut bool) -> Option<Vec<u8>> {
        let fid = self.data?;
        match call(
            vm,
            b"fread",
            &[Value::resource(fid), Value::Long(count.max(1) as i64)],
        ) {
            Value::String(v) => {
                if v.is_empty() && feof(vm, fid) {
                    *eof = true;
                }
                Some(v.as_bytes().to_vec())
            }
            _ => {
                *eof = true;
                None
            }
        }
    }

    fn write(&mut self, vm: &mut Vm, buf: &[u8]) -> i64 {
        let Some(fid) = self.data else {
            return -1;
        };
        match call(vm, b"fwrite", &[Value::resource(fid), s(buf)]) {
            Value::Long(n) => n,
            _ => -1,
        }
    }

    /// ops->close of the socket, then php_stream_ftp_stream_close()
    fn close(&mut self, vm: &mut Vm) {
        if let Some(fid) = self.data.take() {
            close(vm, fid);
        }
        if let Some(cfid) = self.control.take() {
            if self.write_mode {
                let mut ctl = Ctl {
                    fid: cfid,
                    line: Vec::new(),
                };
                let result = ctl.result(vm);
                if result != 226 && result != 250 {
                    let f = func_name(vm);
                    let msg = format!("FTP server error {}:{}", result, ctl.line_str());
                    if f.is_empty() {
                        vm.emit_warning(&msg);
                    } else {
                        vm.emit_warning(&format!("{}(): {}", f, msg));
                    }
                }
            }
            write_string(vm, cfid, b"QUIT\r\n");
            close(vm, cfid);
        }
    }

    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        0
    }

    fn seek(&mut self, _vm: &mut Vm, _offset: i64, _whence: i64) -> Option<i64> {
        None
    }

    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }

    fn label(&self) -> &'static str {
        "tcp_socket/ssl"
    }

    fn seekable(&self) -> bool {
        false
    }
}

/// php_ftp_dirstream_data
struct FtpDir {
    data: Option<i64>,
    control: Option<i64>,
}

impl NativeStream for FtpDir {
    fn read(&mut self, _vm: &mut Vm, _count: usize, _eof: &mut bool) -> Option<Vec<u8>> {
        None
    }
    fn write(&mut self, _vm: &mut Vm, _buf: &[u8]) -> i64 {
        -1
    }
    /// php_ftp_dirstream_close()
    fn close(&mut self, vm: &mut Vm) {
        if let Some(c) = self.control.take() {
            close(vm, c);
        }
        if let Some(d) = self.data.take() {
            close(vm, d);
        }
    }
    fn flush(&mut self, _vm: &mut Vm) -> i32 {
        0
    }
    fn seek(&mut self, _vm: &mut Vm, _offset: i64, _whence: i64) -> Option<i64> {
        None
    }
    fn lock(&mut self, _vm: &mut Vm, _value: i64) -> i32 {
        -1
    }
    fn label(&self) -> &'static str {
        "ftpdir"
    }
    fn seekable(&self) -> bool {
        false
    }
    /// php_ftp_dirstream_read()
    fn readdir(&mut self, vm: &mut Vm) -> Option<Vec<u8>> {
        let fid = self.data?;
        if feof(vm, fid) {
            return None;
        }
        let line = gets(vm, fid, 4096)?;
        let base = super::php_basename_bytes(&line);
        // tmp_len = MIN(sizeof(d_name), len(basename) - 1); the name ends
        // one byte before tmp_len (dropping the line ending).
        let tmp_len = base.len().saturating_sub(1).min(4096);
        let mut name = base[..tmp_len.saturating_sub(1)].to_vec();
        while matches!(name.last(), Some(b'\n' | b'\r' | b'\t' | b' ')) {
            name.pop();
        }
        Some(name)
    }
}

pub(crate) struct FtpWrapper;

impl NativeWrapperOps for FtpWrapper {
    fn label(&self) -> &'static str {
        "ftp"
    }

    /// php_stream_url_wrap_ftp()
    fn open(
        &self,
        vm: &mut Vm,
        path: &[u8],
        mode: &str,
        options: i64,
        context: Option<i64>,
    ) -> Result<Option<NativeOpened>, VmError> {
        let func = func_name(vm);
        let mut read_write = 0;
        if mode.contains(['r', '+']) {
            read_write = 1;
        }
        if mode.contains(['w', 'a', '+']) {
            if read_write != 0 {
                wrapper_log_error(
                    vm,
                    options,
                    "FTP does not support simultaneous read/write connections".to_string(),
                );
                return Ok(None);
            }
            read_write = if mode.contains('a') { 3 } else { 2 };
        }
        if read_write == 0 {
            wrapper_log_error(vm, options, "Unknown file open mode".to_string());
            return Ok(None);
        }
        if super::userstream::context_option(context, b"ftp", b"proxy").is_some() {
            if read_write == 1 {
                wrapper_log_error(
                    vm,
                    options,
                    "FTP proxy is not supported by this build".to_string(),
                );
            } else {
                wrapper_log_error(
                    vm,
                    options,
                    "FTP proxy may only be used in read mode".to_string(),
                );
            }
            return Ok(None);
        }
        let Some(Connected {
            mut ctl,
            resource,
            use_ssl_on_data,
        }) = fopen_connect(vm, path, options, context)
        else {
            return Ok(None);
        };
        let rpath = resource.path.clone().unwrap_or_else(|| b"/".to_vec());
        let mut error_message: Option<String> = None;
        let ok = (|| -> Option<i64> {
            write_string(vm, ctl.fid, b"TYPE I\r\n");
            let result = ctl.result(vm);
            if !(200..=299).contains(&result) {
                return None;
            }
            printf(vm, ctl.fid, "SIZE", &rpath);
            let result = ctl.result(vm);
            if read_write == 1 {
                if !(200..=299).contains(&result) {
                    return None;
                }
            } else if read_write == 2 && (200..=299).contains(&result) {
                let overwrite = super::userstream::context_option(context, b"ftp", b"overwrite")
                    .is_some_and(|v| v.deref().is_truthy());
                if overwrite {
                    printf(vm, ctl.fid, "DELE", &rpath);
                    let result = ctl.result(vm);
                    if !(200..=299).contains(&result) {
                        return None;
                    }
                } else {
                    wrapper_log_error(
                        vm,
                        options,
                        "Remote file already exists and overwrite context option not specified"
                            .to_string(),
                    );
                    return None;
                }
            }
            let (portno, hoststart) = do_pasv(vm, &mut ctl)?;
            let cmd = if read_write == 1 {
                if let Some(Value::Long(pos)) =
                    super::userstream::context_option(context, b"ftp", b"resume_pos")
                        .map(|v| v.deref())
                {
                    if pos > 0 {
                        printf(vm, ctl.fid, "REST", pos.to_string().as_bytes());
                        let result = ctl.result(vm);
                        if !(300..=399).contains(&result) {
                            wrapper_log_error(
                                vm,
                                options,
                                format!("Unable to resume from offset {}", pos),
                            );
                            return None;
                        }
                    }
                }
                "RETR"
            } else if read_write == 2 {
                "STOR"
            } else {
                "APPE"
            };
            ctl.line = cmd.as_bytes().to_vec();
            printf(vm, ctl.fid, cmd, &rpath);
            let host = hoststart.unwrap_or_else(|| resource.host.clone());
            let data = match xport_connect(vm, &func, &data_transport(&host, portno), context, true)
            {
                Ok(d) => d,
                Err(e) => {
                    ctl.line.clear();
                    error_message = Some(e);
                    return None;
                }
            };
            let result = ctl.result(vm);
            if result != 150 && result != 125 {
                close(vm, data);
                return None;
            }
            if use_ssl_on_data && !enable_client_crypto(vm, &func, data) {
                wrapper_log_error(vm, options, "Unable to activate SSL mode".to_string());
                close(vm, data);
                ctl.line.clear();
                return None;
            }
            Some(data)
        })();
        match ok {
            Some(data) => Ok(Some(NativeOpened {
                stream: Rc::new(RefCell::new(FtpStream {
                    data: Some(data),
                    control: Some(ctl.fid),
                    write_mode: mode.contains(['w', 'a', '+']),
                })),
                opened_path: None,
            })),
            None => {
                close(vm, ctl.fid);
                if !ctl.line.is_empty() {
                    let l = ctl.line_str();
                    wrapper_log_error(vm, options, format!("FTP server reports {}", l));
                }
                if let Some(e) = error_message {
                    wrapper_log_error(vm, options, format!("Failed to set up data channel: {}", e));
                }
                Ok(None)
            }
        }
    }

    /// php_stream_ftp_opendir()
    fn opendir(
        &self,
        vm: &mut Vm,
        path: &[u8],
        options: i64,
        context: Option<i64>,
    ) -> Result<Option<Rc<RefCell<dyn NativeStream>>>, VmError> {
        let func = func_name(vm);
        let Some(Connected {
            mut ctl,
            resource,
            use_ssl_on_data,
        }) = fopen_connect(vm, path, options, context)
        else {
            return Ok(None);
        };
        let r = (|| -> Option<i64> {
            write_string(vm, ctl.fid, b"TYPE A\r\n");
            let result = ctl.result(vm);
            if !(200..=299).contains(&result) {
                return None;
            }
            let pasv = do_pasv(vm, &mut ctl);
            // tmp_line isn't relevant after the php_fopen_do_pasv().
            ctl.line.clear();
            let (portno, hoststart) = pasv?;
            let host = hoststart.unwrap_or_else(|| resource.host.clone());
            // php_stream_sock_open_host() reports nothing.
            let data =
                xport_connect(vm, &func, &data_transport(&host, portno), context, true).ok()?;
            let rpath = resource.path.clone().unwrap_or_else(|| b"/".to_vec());
            printf(vm, ctl.fid, "NLST", &rpath);
            let result = ctl.result(vm);
            if result != 150 && result != 125 {
                close(vm, data);
                return None;
            }
            if use_ssl_on_data && !enable_client_crypto(vm, &func, data) {
                wrapper_log_error(vm, options, "Unable to activate SSL mode".to_string());
                close(vm, data);
                return None;
            }
            Some(data)
        })();
        match r {
            Some(data) => Ok(Some(Rc::new(RefCell::new(FtpDir {
                data: Some(data),
                control: Some(ctl.fid),
            })))),
            None => {
                close(vm, ctl.fid);
                if !ctl.line.is_empty() {
                    let l = ctl.line_str();
                    wrapper_log_error(vm, options, format!("FTP server reports {}", l));
                }
                Ok(None)
            }
        }
    }

    /// php_stream_ftp_url_stat()
    fn url_stat(&self, vm: &mut Vm, url: &[u8], _flags: i64) -> Result<Option<StatBuf>, VmError> {
        let Some(Connected {
            mut ctl, resource, ..
        }) = fopen_connect(vm, url, 0, None)
        else {
            return Ok(None);
        };
        let rpath = resource.path.clone().unwrap_or_else(|| b"/".to_vec());
        let r = (|| -> Option<StatBuf> {
            let mut sb = StatBuf {
                dev: 0,
                ino: 0,
                mode: 0o644,
                nlink: 1,
                uid: 0,
                gid: 0,
                rdev: -1,
                size: 0,
                atime: -1,
                mtime: -1,
                ctime: -1,
                blksize: 4096,
                blocks: 0,
            };
            printf(vm, ctl.fid, "CWD", &rpath);
            let result = ctl.result(vm);
            let is_dir = (200..=299).contains(&result);
            if is_dir {
                sb.mode |= (libc::S_IFDIR | libc::S_IXUSR | libc::S_IXGRP | libc::S_IXOTH) as i64;
            } else {
                sb.mode |= libc::S_IFREG as i64;
            }
            write_string(vm, ctl.fid, b"TYPE I\r\n");
            let result = ctl.result(vm);
            if !(200..=299).contains(&result) {
                return None;
            }
            printf(vm, ctl.fid, "SIZE", &rpath);
            let result = ctl.result(vm);
            if !(200..=299).contains(&result) {
                if !is_dir {
                    return None;
                }
                sb.size = 0;
            } else {
                sb.size = strtol(ctl.line.get(4..).unwrap_or_default());
            }
            printf(vm, ctl.fid, "MDTM", &rpath);
            if ctl.result(vm) == 213 {
                let line = &ctl.line;
                let mut p = 4.min(line.len());
                while p < line.len() && !line[p].is_ascii_digit() {
                    p += 1;
                }
                sb.mtime = parse_mdtm(&line[p..]).unwrap_or(-1);
            }
            sb.blocks = (4095 + sb.size) / sb.blksize;
            Some(sb)
        })();
        close(vm, ctl.fid);
        Ok(r)
    }

    /// php_stream_ftp_unlink()
    fn unlink(
        &self,
        vm: &mut Vm,
        url: &[u8],
        options: i64,
        context: Option<i64>,
    ) -> Result<bool, VmError> {
        let Some(Connected {
            mut ctl, resource, ..
        }) = fopen_connect(vm, url, 0, context)
        else {
            if options & REPORT_ERRORS != 0 {
                warn(
                    vm,
                    &format!("Unable to connect to {}", String::from_utf8_lossy(url)),
                );
            }
            return Ok(false);
        };
        let rpath = resource.path.clone().unwrap_or_default();
        printf(vm, ctl.fid, "DELE", &rpath);
        let result = ctl.result(vm);
        let ok = (200..=299).contains(&result);
        if !ok && options & REPORT_ERRORS != 0 {
            let l = ctl.line_str();
            warn(vm, &format!("Error Deleting file: {}", l));
        }
        close(vm, ctl.fid);
        Ok(ok)
    }

    /// php_stream_ftp_rename()
    fn rename(
        &self,
        vm: &mut Vm,
        from: &[u8],
        to: &[u8],
        options: i64,
        context: Option<i64>,
    ) -> Result<bool, VmError> {
        let (Some(rf), Some(rt)) = (parse(from), parse(to)) else {
            return Ok(false);
        };
        if rf.scheme.is_empty()
            || rt.scheme.is_empty()
            || rf.scheme != rt.scheme
            || rf.host.is_empty()
            || rt.host.is_empty()
            || rf.host != rt.host
            || (rf.port != rt.port
                && (rf.port as u32) * (rt.port as u32) != 0
                && rf.port as u32 + rt.port as u32 != 21)
            || rf.path.is_none()
            || rt.path.is_none()
        {
            return Ok(false);
        }
        let Some(Connected { mut ctl, .. }) = fopen_connect(vm, from, 0, context) else {
            if options & REPORT_ERRORS != 0 {
                warn(
                    vm,
                    &format!("Unable to connect to {}", String::from_utf8_lossy(&rf.host)),
                );
            }
            return Ok(false);
        };
        let mut ok = false;
        printf(vm, ctl.fid, "RNFR", rf.path.as_deref().unwrap_or_default());
        let result = ctl.result(vm);
        if !(300..=399).contains(&result) {
            if options & REPORT_ERRORS != 0 {
                let l = ctl.line_str();
                warn(vm, &format!("Error Renaming file: {}", l));
            }
        } else {
            printf(vm, ctl.fid, "RNTO", rt.path.as_deref().unwrap_or_default());
            let result = ctl.result(vm);
            if !(200..=299).contains(&result) {
                if options & REPORT_ERRORS != 0 {
                    let l = ctl.line_str();
                    warn(vm, &format!("Error Renaming file: {}", l));
                }
            } else {
                ok = true;
            }
        }
        close(vm, ctl.fid);
        Ok(ok)
    }

    /// php_stream_ftp_mkdir()
    fn mkdir(
        &self,
        vm: &mut Vm,
        url: &[u8],
        _mode: i64,
        options: i64,
        context: Option<i64>,
    ) -> Result<bool, VmError> {
        let Some(Connected {
            mut ctl, resource, ..
        }) = fopen_connect(vm, url, 0, context)
        else {
            if options & REPORT_ERRORS != 0 {
                warn(
                    vm,
                    &format!("Unable to connect to {}", String::from_utf8_lossy(url)),
                );
            }
            return Ok(false);
        };
        let path = resource.path.clone().unwrap_or_default();
        let result = if options & PHP_STREAM_MKDIR_RECURSIVE == 0 {
            printf(vm, ctl.fid, "MKD", &path);
            ctl.result(vm)
        } else {
            // buf with NUL-terminated components, as the C code edits it
            let mut buf = path.clone();
            let e = buf.len();
            let cur = |buf: &[u8]| -> Vec<u8> {
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                buf[..end].to_vec()
            };
            // find a top level directory we need to create
            let mut p: Option<usize> = None;
            while let Some(pos) = cur(&buf).iter().rposition(|&ch| ch == b'/') {
                buf[pos] = 0;
                let c = cur(&buf);
                printf(vm, ctl.fid, "CWD", if c.is_empty() { b"/" } else { &c });
                let r = ctl.result(vm);
                if (200..=299).contains(&r) {
                    buf[pos] = b'/';
                    p = Some(pos);
                    break;
                }
            }
            let c = cur(&buf);
            printf(vm, ctl.fid, "MKD", if c.is_empty() { b"/" } else { &c });
            let mut r = ctl.result(vm);
            if (200..=299).contains(&r) {
                let mut i = p.unwrap_or(0);
                while i != e {
                    if buf[i] == 0 && i + 1 < e && buf[i + 1] != 0 {
                        buf[i] = b'/';
                        let c = cur(&buf);
                        printf(vm, ctl.fid, "MKD", &c);
                        r = ctl.result(vm);
                        if !(200..=299).contains(&r) {
                            if options & REPORT_ERRORS != 0 {
                                let l = ctl.line_str();
                                warn(vm, &l);
                            }
                            break;
                        }
                    }
                    i += 1;
                }
            }
            r
        };
        close(vm, ctl.fid);
        Ok((200..=299).contains(&result))
    }

    /// php_stream_ftp_rmdir()
    fn rmdir(
        &self,
        vm: &mut Vm,
        url: &[u8],
        options: i64,
        context: Option<i64>,
    ) -> Result<bool, VmError> {
        let Some(Connected {
            mut ctl, resource, ..
        }) = fopen_connect(vm, url, 0, context)
        else {
            if options & REPORT_ERRORS != 0 {
                warn(
                    vm,
                    &format!("Unable to connect to {}", String::from_utf8_lossy(url)),
                );
            }
            return Ok(false);
        };
        printf(
            vm,
            ctl.fid,
            "RMD",
            resource.path.as_deref().unwrap_or_default(),
        );
        let result = ctl.result(vm);
        let ok = (200..=299).contains(&result);
        if !ok && options & REPORT_ERRORS != 0 {
            let l = ctl.line_str();
            warn(vm, &l);
        }
        close(vm, ctl.fid);
        Ok(ok)
    }
}

/// php_error_docref(NULL, E_WARNING, ...)
fn warn(vm: &mut Vm, msg: &str) {
    let f = func_name(vm);
    if f.is_empty() {
        vm.emit_warning(msg);
    } else {
        vm.emit_warning(&format!("{}(): {}", f, msg));
    }
}

/// sscanf("%4d%2d%2d%2d%2d%2d") of an MDTM reply, as a UTC timestamp.
fn parse_mdtm(p: &[u8]) -> Option<i64> {
    let digits: Vec<u8> = p
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .copied()
        .collect();
    if digits.len() < 14 {
        return None;
    }
    let n = |a: usize, b: usize| -> i64 { strtol(&digits[a..b]) };
    let (y, mo, d, h, mi, s) = (n(0, 4), n(4, 6), n(6, 8), n(8, 10), n(10, 12), n(12, 14));
    // days_from_civil
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

pub(crate) fn register() {
    let w: Rc<dyn NativeWrapperOps> = Rc::new(FtpWrapper);
    super::userstream::register_native_wrapper(b"ftp", w.clone());
    super::userstream::register_native_wrapper(b"ftps", w);
}
