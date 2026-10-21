//! Engine tests against tiny in-process HTTP servers. Expected bytes were
//! recorded from a real libcurl 8.14.1.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use super::*;

/// A server answering each connection's requests with the given canned
/// responses (one per request, keep-alive), reporting the raw requests.
fn server(responses: Vec<Vec<u8>>) -> (u16, mpsc::Receiver<Vec<u8>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut responses = responses.into_iter();
        if let Ok((mut s, _)) = l.accept() {
            loop {
                // read one request: headers + Content-Length body
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                let mut need: Option<usize> = None;
                loop {
                    if let Some(n) = need
                        && req.len() >= n
                    {
                        break;
                    }
                    let n = match s.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    req.extend_from_slice(&buf[..n]);
                    if need.is_none()
                        && let Some(p) = req.windows(4).position(|w| w == b"\r\n\r\n")
                    {
                        let head = String::from_utf8_lossy(&req[..p]).to_ascii_lowercase();
                        let cl = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .map(|v| v.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        need = Some(p + 4 + cl);
                    }
                }
                tx.send(req).unwrap();
                match responses.next() {
                    Some(r) => {
                        s.write_all(&r).unwrap();
                    }
                    None => return,
                }
            }
        }
    });
    (port, rx)
}

#[derive(Default)]
struct Rec {
    body: Vec<u8>,
    headers: Vec<Vec<u8>>,
    debug: Vec<(u32, Vec<u8>)>,
    upload: Vec<u8>,
    upos: usize,
}

impl Callbacks for Rec {
    fn write(&mut self, data: &[u8]) -> usize {
        self.body.extend_from_slice(data);
        data.len()
    }
    fn header(&mut self, data: &[u8]) -> usize {
        self.headers.push(data.to_vec());
        data.len()
    }
    fn read(&mut self, max: usize) -> ReadResult {
        let n = (self.upload.len() - self.upos).min(max);
        let v = self.upload[self.upos..self.upos + n].to_vec();
        self.upos += n;
        ReadResult::Data(v)
    }
    fn debug(&mut self, kind: u32, data: &[u8]) {
        self.debug.push((kind, data.to_vec()));
    }
}

fn easy(url: &str) -> Easy {
    let mut e = Easy::new();
    // keep the environment's proxies out of the tests
    e.setopt(opt::PROXY, OptVal::Str(Some(b"")));
    e.setopt(opt::NOPROXY, OptVal::Str(Some(b"")));
    e.setopt(opt::URL, OptVal::Str(Some(url.as_bytes())));
    e
}

fn long(e: &Easy, i: u32) -> i64 {
    match e.getinfo(i) {
        Some(InfoVal::Long(v)) | Some(InfoVal::OffT(v)) => v,
        o => panic!("{o:?}"),
    }
}

fn string(e: &Easy, i: u32) -> Option<Vec<u8>> {
    match e.getinfo(i) {
        Some(InfoVal::Str(v)) => v,
        o => panic!("{o:?}"),
    }
}

#[test]
fn get_request_bytes_and_response() {
    let (port, rx) = server(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nhello".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/a?b=c"));
    e.setopt(opt::VERBOSE, OptVal::Long(1));
    e.setopt(opt::DEBUGFUNCTION, OptVal::Callback(true));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    let req = rx.recv().unwrap();
    let expect = format!("GET /a?b=c HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: */*\r\n\r\n");
    assert_eq!(String::from_utf8_lossy(&req), expect);
    assert_eq!(cb.body, b"hello");
    assert_eq!(
        cb.headers,
        vec![
            b"HTTP/1.1 200 OK\r\n".to_vec(),
            b"Content-Length: 5\r\n".to_vec(),
            b"Content-Type: text/plain\r\n".to_vec(),
            b"\r\n".to_vec()
        ]
    );
    // the HEADER_OUT trace is the whole header block in one piece
    let hout: Vec<_> = cb
        .debug
        .iter()
        .filter(|(k, _)| *k == info::HEADER_OUT)
        .collect();
    assert_eq!(hout.len(), 1);
    assert_eq!(hout[0].1, expect.as_bytes());
    let texts: Vec<String> = cb
        .debug
        .iter()
        .filter(|(k, _)| *k == info::TEXT)
        .map(|(_, d)| String::from_utf8_lossy(d).into_owned())
        .collect();
    assert_eq!(texts[0], format!("  Trying 127.0.0.1:{port}...\n"));
    assert_eq!(
        texts[1],
        format!("Connected to 127.0.0.1 (127.0.0.1) port {port}\n")
    );
    assert_eq!(texts[2], "using HTTP/1.x\n");
    assert_eq!(texts[3], "Request completely sent off\n");
    assert_eq!(texts[4], "Connection #0 to host 127.0.0.1 left intact\n");
    assert_eq!(long(&e, info::RESPONSE_CODE), 200);
    assert_eq!(long(&e, info::HEADER_SIZE), 64);
    assert_eq!(long(&e, info::REQUEST_SIZE), expect.len() as i64);
    assert_eq!(long(&e, info::HTTP_VERSION), httpver::_1_1);
    assert_eq!(long(&e, info::CONN_ID), 0);
    assert_eq!(string(&e, info::CONTENT_TYPE), Some(b"text/plain".to_vec()));
    assert_eq!(
        string(&e, info::EFFECTIVE_URL),
        Some(format!("http://127.0.0.1:{port}/a?b=c").into_bytes())
    );
}

#[test]
fn post_fields_and_keepalive_reuse() {
    let (port, rx) = server(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/p"));
    e.setopt(opt::POSTFIELDSIZE, OptVal::Long(3));
    e.setopt(opt::COPYPOSTFIELDS, OptVal::Str(Some(b"a=b")));
    e.setopt(opt::USERAGENT, OptVal::Str(Some(b"ua/1")));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    let req = rx.recv().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&req),
        format!(
            "POST /p HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUser-Agent: ua/1\r\nAccept: */*\r\nContent-Length: 3\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\na=b"
        )
    );
    assert_eq!(long(&e, info::SIZE_UPLOAD_T), 3);
    // second perform reuses the connection
    assert_eq!(e.perform(&mut cb), code::OK);
    rx.recv().unwrap();
    assert_eq!(long(&e, info::CONN_ID), 0);
    assert_eq!(long(&e, info::NUM_CONNECTS), 0);
    assert_eq!(long(&e, info::XFER_ID), 1);
}

#[test]
fn custom_headers_and_upload_chunked() {
    let (port, rx) = server(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/u"));
    e.setopt(
        opt::HTTPHEADER,
        OptVal::Slist(Some(vec![
            b"X-A: 1".to_vec(),
            b"Accept:".to_vec(),
            b"X-Empty;".to_vec(),
            b"X-None:   ".to_vec(),
        ])),
    );
    e.setopt(opt::UPLOAD, OptVal::Long(1));
    e.setopt(opt::INFILESIZE, OptVal::Long(5));
    let mut cb = Rec {
        upload: b"hello".to_vec(),
        ..Default::default()
    };
    assert_eq!(e.perform(&mut cb), code::OK);
    let req = rx.recv().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&req),
        format!(
            "PUT /u HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-A: 1\r\nX-Empty:\r\nContent-Length: 5\r\n\r\nhello"
        )
    );
}

#[test]
fn chunked_gzip_response() {
    // "hello gzip" gzip-compressed, sent chunked
    let gz = {
        let (st, out) = goro_zlib::deflate_once(b"hello gzip", 6, 31, 8, 0, 256);
        assert_eq!(st, goro_zlib::Z_STREAM_END);
        out
    };
    let mut resp =
        b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    resp.extend_from_slice(format!("{:x}\r\n", gz.len()).as_bytes());
    resp.extend_from_slice(&gz);
    resp.extend_from_slice(b"\r\n0\r\n\r\n");
    let (port, rx) = server(vec![resp]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/"));
    e.setopt(opt::ACCEPT_ENCODING, OptVal::Str(Some(b"")));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    let req = rx.recv().unwrap();
    assert!(String::from_utf8_lossy(&req).contains("\r\nAccept-Encoding: deflate, gzip\r\n"));
    assert_eq!(cb.body, b"hello gzip");
    assert_eq!(long(&e, info::SIZE_DOWNLOAD_T), gz.len() as i64);
}

#[test]
fn redirects_and_failonerror() {
    let (port, rx) = server(vec![
        b"HTTP/1.1 302 Found\r\nLocation: /b\r\nContent-Length: 0\r\n\r\n".to_vec(),
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 1\r\n\r\nx".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/a"));
    e.setopt(opt::FOLLOWLOCATION, OptVal::Long(1));
    e.setopt(opt::FAILONERROR, OptVal::Long(1));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::HTTP_RETURNED_ERROR);
    assert_eq!(e.error_buffer(), b"The requested URL returned error: 404");
    rx.recv().unwrap();
    let r2 = rx.recv().unwrap();
    assert!(r2.starts_with(b"GET /b HTTP/1.1\r\n"));
    assert_eq!(long(&e, info::REDIRECT_COUNT), 1);
    assert_eq!(long(&e, info::RESPONSE_CODE), 404);
    // without following: CURLINFO_REDIRECT_URL
    let (port, _rx) = server(vec![
        b"HTTP/1.1 301 Moved\r\nLocation: x/y\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/a/b"));
    assert_eq!(e.perform(&mut cb), code::OK);
    assert_eq!(
        string(&e, info::REDIRECT_URL),
        Some(format!("http://127.0.0.1:{port}/a/x/y").into_bytes())
    );
}

#[test]
fn error_codes_and_texts() {
    let mut cb = Rec::default();
    let mut e = Easy::new();
    assert_eq!(e.perform(&mut cb), code::URL_MALFORMAT);
    assert_eq!(e.error_buffer(), b"No URL set");
    let mut e = easy("foo://bar/");
    assert_eq!(e.perform(&mut cb), code::UNSUPPORTED_PROTOCOL);
    assert_eq!(e.error_buffer(), b"Protocol \"foo\" not supported");
    let mut e = easy("http://");
    assert_eq!(e.perform(&mut cb), code::URL_MALFORMAT);
    assert_eq!(e.error_buffer(), b"URL rejected: No host part in the URL");
    // a port nobody listens on
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut e = easy(&format!("http://127.0.0.1:{port}/"));
    assert_eq!(e.perform(&mut cb), code::COULDNT_CONNECT);
    let msg = String::from_utf8_lossy(e.error_buffer()).into_owned();
    assert!(msg.starts_with(&format!(
        "Failed to connect to 127.0.0.1 port {port} after "
    )));
    assert!(msg.ends_with(" ms: Could not connect to server"));
    assert_eq!(long(&e, info::OS_ERRNO), libc::ECONNREFUSED as i64);
    let mut e = easy("file:///nonexistent/file");
    assert_eq!(e.perform(&mut cb), code::FILE_COULDNT_READ_FILE);
    assert_eq!(e.error_buffer(), b"Couldn't open file /nonexistent/file");
    assert_eq!(
        easy_strerror(code::WRITE_ERROR),
        "Failed writing received data to disk/application"
    );
    assert_eq!(multi_strerror(mcode::BAD_HANDLE), "Invalid multi handle");
    assert_eq!(share_strerror(99), "CURLSHcode unknown");
}

#[test]
fn write_callback_short_return() {
    struct Short;
    impl Callbacks for Short {
        fn write(&mut self, _d: &[u8]) -> usize {
            1
        }
        fn header(&mut self, d: &[u8]) -> usize {
            d.len()
        }
        fn read(&mut self, _m: usize) -> ReadResult {
            ReadResult::Data(Vec::new())
        }
    }
    let (port, _rx) = server(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/"));
    assert_eq!(e.perform(&mut Short), code::WRITE_ERROR);
    assert_eq!(
        e.error_buffer(),
        b"Failure writing output to destination, passed 5 returned 1"
    );
}

#[test]
fn file_read_and_nobody_headers() {
    let dir = std::env::temp_dir().join(format!("goro-curl-engine-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("f.txt");
    std::fs::write(&f, b"file data").unwrap();
    let url = format!("file://{}", f.display());
    let mut e = easy(&url);
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    assert_eq!(cb.body, b"file data");
    assert_eq!(cb.headers[0], b"Content-Length: 9\r\n");
    assert_eq!(cb.headers[1], b"Accept-ranges: bytes\r\n");
    assert!(cb.headers[2].starts_with(b"Last-Modified: "));
    assert_eq!(long(&e, info::PROTOCOL), proto::FILE);
    let mut e = easy(&url);
    e.setopt(opt::NOBODY, OptVal::Long(1));
    e.setopt(opt::HEADER, OptVal::Long(1));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    assert!(
        cb.body
            .starts_with(b"Content-Length: 9\r\nAccept-ranges: bytes\r\nLast-Modified: ")
    );
    // upload to a file
    let up = dir.join("up.txt");
    let mut e = easy(&format!("file://{}", up.display()));
    e.setopt(opt::UPLOAD, OptVal::Long(1));
    let mut cb = Rec {
        upload: b"uploaded".to_vec(),
        ..Default::default()
    };
    assert_eq!(e.perform(&mut cb), code::OK);
    assert_eq!(std::fs::read(&up).unwrap(), b"uploaded");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn setopt_validation() {
    let mut e = Easy::new();
    assert_eq!(
        e.setopt(opt::FOLLOWLOCATION, OptVal::Long(4)),
        code::BAD_FUNCTION_ARGUMENT
    );
    assert_eq!(e.error_buffer(), b"setopt 0x34 got bad argument");
    assert_eq!(
        e.setopt(opt::TIMEOUT, OptVal::Long(-1)),
        code::BAD_FUNCTION_ARGUMENT
    );
    assert_eq!(
        e.setopt(opt::SSLVERSION, OptVal::Long(2)),
        code::BAD_FUNCTION_ARGUMENT
    );
    assert_eq!(
        e.setopt(opt::HTTP_VERSION, OptVal::Long(-1)),
        code::BAD_FUNCTION_ARGUMENT
    );
    assert_eq!(
        e.setopt(opt::HTTP_VERSION, OptVal::Long(3)),
        code::UNSUPPORTED_PROTOCOL
    );
    assert_eq!(
        e.setopt(opt::PROTOCOLS_STR, OptVal::Str(Some(b"foo"))),
        code::UNSUPPORTED_PROTOCOL
    );
    assert_eq!(
        e.setopt(opt::PROTOCOLS_STR, OptVal::Str(Some(b","))),
        code::BAD_FUNCTION_ARGUMENT
    );
    assert_eq!(
        e.setopt(opt::PROTOCOLS_STR, OptVal::Str(Some(b"http,https"))),
        code::OK
    );
    assert_eq!(e.setopt(99999, OptVal::Long(1)), code::UNKNOWN_OPTION);
    assert_eq!(
        e.setopt(opt::SSH_AUTH_TYPES, OptVal::Long(1)),
        code::UNKNOWN_OPTION
    );
    assert_eq!(
        e.setopt(opt::SSL_FALSESTART, OptVal::Long(1)),
        code::NOT_BUILT_IN
    );
    assert_eq!(
        e.setopt(opt::HTTPAUTH, OptVal::Long(auth::NEGOTIATE as i64)),
        code::NOT_BUILT_IN
    );
}

#[test]
fn getinfo_before_transfer() {
    let e = Easy::new();
    assert_eq!(long(&e, info::CONN_ID), -1);
    assert_eq!(long(&e, info::XFER_ID), -1);
    assert_eq!(long(&e, info::FILETIME), -1);
    assert_eq!(long(&e, info::PRIMARY_PORT), -1);
    assert_eq!(long(&e, info::CONTENT_LENGTH_DOWNLOAD_T), -1);
    assert_eq!(string(&e, info::EFFECTIVE_URL), Some(Vec::new()));
    assert_eq!(string(&e, info::PRIMARY_IP), Some(Vec::new()));
    assert_eq!(string(&e, info::SCHEME), None);
    assert_eq!(string(&e, info::EFFECTIVE_METHOD), Some(b"GET".to_vec()));
    assert_eq!(
        string(&e, info::CAINFO),
        Some(b"/etc/ssl/certs/ca-certificates.crt".to_vec())
    );
    assert!(e.getinfo(0x200000 + 999).is_none());
}

#[test]
fn cookies_engine() {
    let (port, rx) = server(vec![
        b"HTTP/1.1 200 OK\r\nSet-Cookie: a=b; path=/\r\nContent-Length: 0\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/"));
    e.setopt(opt::COOKIEFILE, OptVal::Str(Some(b"")));
    e.setopt(opt::COOKIE, OptVal::Str(Some(b"x=y")));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    let r1 = rx.recv().unwrap();
    assert!(String::from_utf8_lossy(&r1).contains("\r\nCookie: x=y\r\n"));
    assert_eq!(e.perform(&mut cb), code::OK);
    let r2 = rx.recv().unwrap();
    assert!(String::from_utf8_lossy(&r2).contains("\r\nCookie: a=b; x=y\r\n"));
    match e.getinfo(info::COOKIELIST) {
        Some(InfoVal::Slist(l)) => {
            assert_eq!(l, vec![b"127.0.0.1\tFALSE\t/\tFALSE\t0\ta\tb".to_vec()])
        }
        o => panic!("{o:?}"),
    }
    e.setopt(opt::COOKIELIST, OptVal::Str(Some(b"ALL")));
    match e.getinfo(info::COOKIELIST) {
        Some(InfoVal::Slist(l)) => assert!(l.is_empty()),
        o => panic!("{o:?}"),
    }
}

#[test]
fn multipart_body_layout() {
    let (port, rx) = server(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let mut e = easy(&format!("http://127.0.0.1:{port}/m"));
    let mime = Mime {
        parts: vec![
            MimePart {
                name: b"a".to_vec(),
                data: MimeData::Bytes(b"1".to_vec()),
                filename: None,
                content_type: None,
            },
            MimePart {
                name: b"f".to_vec(),
                data: MimeData::Bytes(b"xyz".to_vec()),
                filename: Some(b"n\"x.txt".to_vec()),
                content_type: Some(b"application/octet-stream".to_vec()),
            },
        ],
    };
    e.setopt(opt::MIMEPOST, OptVal::Mime(Some(mime)));
    let mut cb = Rec::default();
    assert_eq!(e.perform(&mut cb), code::OK);
    let req = String::from_utf8_lossy(&rx.recv().unwrap()).into_owned();
    let i = req.find("boundary=").unwrap() + 9;
    let b = &req[i..i + 46];
    assert!(b.starts_with("------------------------"));
    let body = &req[req.find("\r\n\r\n").unwrap() + 4..];
    assert_eq!(
        body,
        format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n1\r\n--{b}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"n%22x.txt\"\r\nContent-Type: application/octet-stream\r\n\r\nxyz\r\n--{b}--\r\n"
        )
    );
    assert!(req.contains(&format!("Content-Length: {}\r\n", body.len())));
}

#[test]
fn escape_and_version() {
    assert_eq!(easy_escape(b"a b/\xc3\xa4"), b"a%20b%2F%C3%A4");
    assert_eq!(easy_unescape(b"a%20b%2f"), b"a b/");
    let v = version_info();
    assert_eq!(v.version, "8.14.1");
    assert_eq!(v.version_num, 0x080e01);
    assert!(v.protocols.contains(&"https".to_string()));
    assert!(version_string().starts_with("libcurl/8.14.1 "));
}

#[test]
fn share_options() {
    let mut s = Share::new();
    assert_eq!(s.setopt(shopt::SHARE, lock::COOKIE as i64), shcode::OK);
    assert_eq!(
        s.setopt(shopt::SHARE, lock::PSL as i64),
        shcode::NOT_BUILT_IN
    );
    assert_eq!(s.setopt(shopt::SHARE, 99), shcode::BAD_OPTION);
    assert_eq!(s.setopt(99, 1), shcode::BAD_OPTION);
    let s = std::rc::Rc::new(std::cell::RefCell::new(s));
    let mut e = Easy::new();
    e.setopt(opt::SHARE, OptVal::Share(Some(s.clone())));
    assert_eq!(
        s.borrow_mut().setopt(shopt::SHARE, lock::DNS as i64),
        shcode::IN_USE
    );
    drop(e);
    assert_eq!(
        s.borrow_mut().setopt(shopt::SHARE, lock::DNS as i64),
        shcode::OK
    );
}
