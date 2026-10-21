//! Faithful port of php-src's `sapi/cli/php_http_parser.c` (the request
//! parser used by the built-in web server, `php -S`).
//!
//! Original: Copyright 2009,2010 Ryan Dahl <ry@tinyclouds.org>, modified by
//! Moriyoshi Koizumi for the PHP source tree. MIT licensed.
//!
//! Notes on fidelity:
//! * php-src tests `#ifdef HTTP_PARSER_STRICT`, a macro that is never defined
//!   (the header only defines `PHP_HTTP_PARSER_STRICT`), so the parser is
//!   always compiled in non-strict mode: every `STRICT_CHECK()` is a no-op and
//!   `NEW_MESSAGE()` is always the start state.  Those checks are therefore
//!   left out here (they are marked with comments at their original places).
//! * The `tokens`, `unhex` and `normal_url_char` tables are declared with 256
//!   entries in C but only the first 128 are initialised; the rest are zero.
//!   This matters for `unhex`, where bytes >= 0x80 decode as hex digit `0`.
//! * On a callback error the C code returns the current offset *without*
//!   saving the state machine back into the parser (and without moving to
//!   `s_dead`); the same is done here.  A callback error raised while
//!   flushing the pending marks at the end of the buffer therefore returns
//!   `data.len()`, exactly like C.
//! * Arithmetic on `content_length` wraps (C signed overflow, two's complement
//!   in practice).

/// Maximum header size allowed.
pub const PHP_HTTP_MAX_HEADER_SIZE: u32 = 80 * 1024;

/// Request methods (`enum php_http_method`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    Delete,
    Get,
    Head,
    Post,
    Put,
    Patch,
    // pathological
    Connect,
    Options,
    Trace,
    // webdav
    Copy,
    Lock,
    Mkcol,
    Move,
    Mkcalendar,
    Propfind,
    Proppatch,
    Search,
    Unlock,
    // subversion
    Report,
    Mkactivity,
    Checkout,
    Merge,
    // upnp
    MSearch,
    Notify,
    Subscribe,
    Unsubscribe,
    // unknown, not implemented
    NotImplemented,
}

impl Method {
    /// `php_http_method_str()`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Delete => "DELETE",
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Connect => "CONNECT",
            Method::Options => "OPTIONS",
            Method::Trace => "TRACE",
            Method::Copy => "COPY",
            Method::Lock => "LOCK",
            Method::Mkcol => "MKCOL",
            Method::Move => "MOVE",
            Method::Mkcalendar => "MKCALENDAR",
            Method::Propfind => "PROPFIND",
            Method::Proppatch => "PROPPATCH",
            Method::Search => "SEARCH",
            Method::Unlock => "UNLOCK",
            Method::Report => "REPORT",
            Method::Mkactivity => "MKACTIVITY",
            Method::Checkout => "CHECKOUT",
            Method::Merge => "MERGE",
            Method::MSearch => "M-SEARCH",
            Method::Notify => "NOTIFY",
            Method::Subscribe => "SUBSCRIBE",
            Method::Unsubscribe => "UNSUBSCRIBE",
            Method::NotImplemented => "NOTIMPLEMENTED",
        }
    }

    /// Numeric value of the C enum (`PHP_HTTP_DELETE == 0`, ...).
    pub fn as_c_enum(&self) -> u8 {
        *self as u8
    }
}

/// `enum php_http_parser_type`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParserType {
    Request,
    Response,
    Both,
}

/// `enum flags`.
pub const F_CHUNKED: u8 = 1 << 0;
pub const F_CONNECTION_KEEP_ALIVE: u8 = 1 << 1;
pub const F_CONNECTION_CLOSE: u8 = 1 << 2;
pub const F_TRAILING: u8 = 1 << 3;
pub const F_UPGRADE: u8 = 1 << 4;
pub const F_SKIPBODY: u8 = 1 << 5;

/// Read-only view of the parser fields available to callbacks.
#[derive(Clone, Copy, Debug)]
pub struct ParserInfo {
    pub parser_type: ParserType,
    pub method: Method,
    pub http_major: u16,
    pub http_minor: u16,
    pub status_code: u16,
    pub content_length: i64,
    pub upgrade: bool,
    pub flags: u8,
}

impl ParserInfo {
    /// `php_http_should_keep_alive()`.
    pub fn should_keep_alive(&self) -> bool {
        should_keep_alive(self.http_major, self.http_minor, self.flags)
    }
}

fn should_keep_alive(http_major: u16, http_minor: u16, flags: u8) -> bool {
    if http_major > 0 && http_minor > 0 {
        // HTTP/1.1
        flags & F_CONNECTION_CLOSE == 0
    } else {
        // HTTP/1.0 or earlier
        flags & F_CONNECTION_KEEP_ALIVE != 0
    }
}

/// Callbacks, mirroring `php_http_parser_settings`.  Returning `true` means
/// "error" (non-zero in C) and stops parsing.
#[allow(unused_variables)]
pub trait ParserCallbacks {
    fn on_message_begin(&mut self, p: &ParserInfo) -> bool {
        false
    }
    fn on_path(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    fn on_query_string(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    fn on_url(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    fn on_fragment(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    fn on_header_field(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    fn on_header_value(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    /// Returning `true` corresponds to the C return value `1`: "this message
    /// has no body" (`F_SKIPBODY`).  To signal an error (any other non-zero
    /// C return value) override [`ParserCallbacks::on_headers_complete_raw`].
    fn on_headers_complete(&mut self, p: &ParserInfo) -> bool {
        false
    }
    /// Raw C-style result of `on_headers_complete`: 0 = continue, 1 = skip
    /// body, anything else = error.  Defaults to `on_headers_complete`.
    fn on_headers_complete_raw(&mut self, p: &ParserInfo) -> i32 {
        i32::from(self.on_headers_complete(p))
    }
    /// The return value is ignored by the parser (as in C).
    fn on_body(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
        false
    }
    fn on_message_complete(&mut self, p: &ParserInfo) -> bool {
        false
    }
}

/// `enum state`.  The numeric order matters (see `parsing_header`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum State {
    Dead = 1,

    StartReqOrRes,
    ResOrRespH,
    StartRes,
    ResH,
    ResHT,
    ResHTT,
    ResHTTP,
    ResFirstHttpMajor,
    ResHttpMajor,
    ResFirstHttpMinor,
    ResHttpMinor,
    ResFirstStatusCode,
    ResStatusCode,
    ResStatus,
    ResLineAlmostDone,

    StartReq,

    ReqMethod,
    ReqSpacesBeforeUrl,
    ReqSchema,
    ReqSchemaSlash,
    ReqSchemaSlashSlash,
    ReqHost,
    ReqPort,
    ReqPath,
    ReqQueryStringStart,
    ReqQueryString,
    ReqFragmentStart,
    ReqFragment,
    ReqHttpStart,
    ReqHttpH,
    ReqHttpHT,
    ReqHttpHTT,
    ReqHttpHTTP,
    ReqFirstHttpMajor,
    ReqHttpMajor,
    ReqFirstHttpMinor,
    ReqHttpMinor,
    ReqLineAlmostDone,

    HeaderFieldStart,
    HeaderField,
    HeaderValueStart,
    HeaderValue,

    HeaderAlmostDone,

    /// Must be the last 'header' state.
    HeadersAlmostDone,

    ChunkSizeStart,
    ChunkSize,
    ChunkSizeAlmostDone,
    ChunkParameters,
    ChunkData,
    ChunkDataAlmostDone,
    ChunkDataDone,

    BodyIdentity,
    BodyIdentityEof,
}

/// `enum header_states`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HeaderState {
    General,
    C,
    Co,
    Con,

    MatchingConnection,
    MatchingProxyConnection,
    MatchingContentLength,
    MatchingTransferEncoding,
    MatchingUpgrade,

    Connection,
    ContentLength,
    TransferEncoding,
    Upgrade,

    MatchingTransferEncodingChunked,
    MatchingConnectionKeepAlive,
    MatchingConnectionClose,

    TransferEncodingChunked,
    ConnectionKeepAlive,
    ConnectionClose,
}

const PROXY_CONNECTION: &[u8] = b"proxy-connection";
const CONNECTION: &[u8] = b"connection";
const CONTENT_LENGTH: &[u8] = b"content-length";
const TRANSFER_ENCODING: &[u8] = b"transfer-encoding";
const UPGRADE: &[u8] = b"upgrade";
const CHUNKED: &[u8] = b"chunked";
const KEEP_ALIVE: &[u8] = b"keep-alive";
const CLOSE: &[u8] = b"close";

const CR: u8 = b'\r';
const LF: u8 = b'\n';

/// Tokens as defined by rfc 2616. Also lowercases them.  Only the first 128
/// entries are initialised in C, the rest are 0.
const TOKENS: [u8; 256] = build_tokens();

const fn build_tokens() -> [u8; 256] {
    let first: [u8; 128] = [
        0, 0, 0, 0, 0, 0, 0, 0, //
        0, 0, 0, 0, 0, 0, 0, 0, //
        0, 0, 0, 0, 0, 0, 0, 0, //
        0, 0, 0, 0, 0, 0, 0, 0, //
        b' ', b'!', b'"', b'#', b'$', b'%', b'&', b'\'', //
        0, 0, b'*', b'+', 0, b'-', b'.', b'/', //
        b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', //
        b'8', b'9', 0, 0, 0, 0, 0, 0, //
        0, b'a', b'b', b'c', b'd', b'e', b'f', b'g', //
        b'h', b'i', b'j', b'k', b'l', b'm', b'n', b'o', //
        b'p', b'q', b'r', b's', b't', b'u', b'v', b'w', //
        b'x', b'y', b'z', 0, 0, 0, b'^', b'_', //
        b'`', b'a', b'b', b'c', b'd', b'e', b'f', b'g', //
        b'h', b'i', b'j', b'k', b'l', b'm', b'n', b'o', //
        b'p', b'q', b'r', b's', b't', b'u', b'v', b'w', //
        b'x', b'y', b'z', 0, b'|', b'}', b'~', 0, //
    ];
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 128 {
        t[i] = first[i];
        i += 1;
    }
    t
}

/// Hex digit values; -1 for non-hex.  Only the first 128 entries are
/// initialised in C, so bytes >= 0x80 map to 0 (not -1).
const UNHEX: [i8; 256] = build_unhex();

const fn build_unhex() -> [i8; 256] {
    let mut t = [0i8; 256];
    let mut i = 0;
    while i < 128 {
        let b = i as u8;
        t[i] = match b {
            b'0'..=b'9' => (b - b'0') as i8,
            b'a'..=b'f' => (b - b'a' + 10) as i8,
            b'A'..=b'F' => (b - b'A' + 10) as i8,
            _ => -1,
        };
        i += 1;
    }
    t
}

/// `normal_url_char`: everything printable from 0x21 to 0x7e except '#' and
/// '?'.  Bytes >= 0x80 are 0 (uninitialised part of the C table).
const NORMAL_URL_CHAR: [bool; 256] = build_normal_url_char();

const fn build_normal_url_char() -> [bool; 256] {
    let mut t = [false; 256];
    let mut i = 33;
    while i < 127 {
        t[i] = i != b'#' as usize && i != b'?' as usize;
        i += 1;
    }
    t
}

/// The shared body of the `h_matching_*` cases:
/// `index++; if (index > sizeof(S)-1 || c != S[index]) general; else if (index == sizeof(S)-2) done;`
fn matching(index: &mut u32, header_state: &mut HeaderState, c: u8, s: &[u8], done: HeaderState) {
    *index = index.wrapping_add(1);
    let i = *index as usize;
    if i >= s.len() || c != s[i] {
        *header_state = HeaderState::General;
    } else if i == s.len() - 1 {
        *header_state = done;
    }
}

#[inline]
fn lower(c: u8) -> u8 {
    c | 0x20
}

#[inline]
fn token(c: u8) -> u8 {
    TOKENS[c as usize]
}

#[inline]
fn normal_url_char(c: u8) -> bool {
    NORMAL_URL_CHAR[c as usize]
}

/// `struct php_http_parser`.
#[derive(Clone, Debug)]
pub struct HttpParser {
    parser_type: ParserType,
    flags: u8,
    state: State,
    header_state: HeaderState,
    /// `unsigned char` in C: truncated when stored between calls.
    index: u8,
    nread: u32,
    pub content_length: i64,
    pub http_major: u16,
    pub http_minor: u16,
    /// Responses only.
    pub status_code: u16,
    /// Requests only.
    pub method: Method,
    /// Set when an Upgrade header was present or the method is CONNECT.
    pub upgrade: bool,
}

impl HttpParser {
    /// `php_http_parser_init(parser, t)`.
    pub fn new(t: ParserType) -> Self {
        HttpParser {
            parser_type: t,
            state: match t {
                ParserType::Request => State::StartReq,
                ParserType::Response => State::StartRes,
                ParserType::Both => State::StartReqOrRes,
            },
            nread: 0,
            upgrade: false,
            flags: 0,
            method: Method::Delete,
            // Fields left uninitialised by php_http_parser_init().
            header_state: HeaderState::General,
            index: 0,
            content_length: 0,
            http_major: 0,
            http_minor: 0,
            status_code: 0,
        }
    }

    /// `php_http_parser_init(parser, PHP_HTTP_REQUEST)`.
    pub fn new_request() -> Self {
        Self::new(ParserType::Request)
    }

    /// `parser->state == s_start_req`.
    pub fn is_start_state(&self) -> bool {
        self.state == State::StartReq
    }

    /// `parser->state == s_dead` (set after a parse error).
    pub fn is_dead(&self) -> bool {
        self.state == State::Dead
    }

    pub fn method(&self) -> Method {
        self.method
    }
    pub fn http_major(&self) -> u16 {
        self.http_major
    }
    pub fn http_minor(&self) -> u16 {
        self.http_minor
    }
    pub fn content_length(&self) -> i64 {
        self.content_length
    }
    pub fn status_code(&self) -> u16 {
        self.status_code
    }
    pub fn upgrade(&self) -> bool {
        self.upgrade
    }
    pub fn flags(&self) -> u8 {
        self.flags
    }
    pub fn parser_type(&self) -> ParserType {
        self.parser_type
    }

    /// `php_http_should_keep_alive()`.
    pub fn should_keep_alive(&self) -> bool {
        should_keep_alive(self.http_major, self.http_minor, self.flags)
    }

    /// Snapshot handed to callbacks.
    pub fn info(&self) -> ParserInfo {
        ParserInfo {
            parser_type: self.parser_type,
            method: self.method,
            http_major: self.http_major,
            http_minor: self.http_minor,
            status_code: self.status_code,
            content_length: self.content_length,
            upgrade: self.upgrade,
            flags: self.flags,
        }
    }

    fn start_state(&self) -> State {
        if self.parser_type == ParserType::Request {
            State::StartReq
        } else {
            State::StartRes
        }
    }

    /// `NEW_MESSAGE()` in non-strict mode.
    fn new_message(&self) -> State {
        self.start_state()
    }

    /// The `start_req_method_assign:` label.
    fn start_req_method_assign(&mut self, ch: u8) {
        self.method = match ch {
            b'C' => Method::Connect, // or COPY, CHECKOUT
            b'D' => Method::Delete,
            b'G' => Method::Get,
            b'H' => Method::Head,
            b'L' => Method::Lock,
            b'M' => Method::Mkcol, // or MOVE, MKCALENDAR, MKACTIVITY, MERGE, M-SEARCH
            b'N' => Method::Notify,
            b'O' => Method::Options,
            b'P' => Method::Post, // or PROPFIND or PROPPATCH or PUT
            b'R' => Method::Report,
            b'S' => Method::Subscribe, // or SEARCH
            b'T' => Method::Trace,
            b'U' => Method::Unlock, // or UNSUBSCRIBE
            _ => Method::NotImplemented,
        };
    }

    /// `php_http_parser_execute()`.  Returns the number of bytes consumed:
    /// `data.len()` on success, less on error.
    pub fn execute<C: ParserCallbacks>(&mut self, cb: &mut C, data: &[u8]) -> usize {
        let len = data.len();

        let mut state = self.state;
        let mut header_state = self.header_state;
        let mut index: u32 = u32::from(self.index);
        let mut nread: u32 = self.nread;

        let mut header_field_mark: Option<usize> = None;
        let mut header_value_mark: Option<usize> = None;
        let mut fragment_mark: Option<usize> = None;
        let mut query_string_mark: Option<usize> = None;
        let mut path_mark: Option<usize> = None;
        let mut url_mark: Option<usize> = None;

        // CALLBACK2(FOR)
        macro_rules! callback2 {
            ($method:ident, $p:expr) => {
                if cb.$method(&self.info()) {
                    return $p;
                }
            };
        }
        // CALLBACK_NOCLEAR(FOR)
        macro_rules! callback_noclear {
            ($mark:ident, $method:ident, $p:expr) => {
                if let Some(m) = $mark {
                    if cb.$method(&self.info(), &data[m..$p]) {
                        return $p;
                    }
                }
            };
        }
        // CALLBACK(FOR)
        macro_rules! callback {
            ($mark:ident, $method:ident, $p:expr) => {
                callback_noclear!($mark, $method, $p);
                $mark = None;
            };
        }
        // goto error
        macro_rules! error {
            ($p:expr) => {{
                self.state = State::Dead;
                return $p;
            }};
        }

        if len == 0 {
            if state == State::BodyIdentityEof {
                callback2!(on_message_complete, 0);
            }
            return 0;
        }

        if state == State::HeaderField {
            header_field_mark = Some(0);
        }
        if state == State::HeaderValue {
            header_value_mark = Some(0);
        }
        if state == State::ReqFragment {
            fragment_mark = Some(0);
        }
        if state == State::ReqQueryString {
            query_string_mark = Some(0);
        }
        if state == State::ReqPath {
            path_mark = Some(0);
        }
        if matches!(
            state,
            State::ReqPath
                | State::ReqSchema
                | State::ReqSchemaSlash
                | State::ReqSchemaSlashSlash
                | State::ReqPort
                | State::ReqQueryStringStart
                | State::ReqQueryString
                | State::ReqHost
                | State::ReqFragmentStart
                | State::ReqFragment
        ) {
            url_mark = Some(0);
        }

        let mut p: usize = 0;
        while p != len {
            let ch = data[p];

            // PARSING_HEADER(state)
            if (state as u8) <= (State::HeadersAlmostDone as u8) && self.flags & F_TRAILING == 0 {
                nread = nread.wrapping_add(1);
                // Buffer overflow attack
                if nread > PHP_HTTP_MAX_HEADER_SIZE {
                    error!(p);
                }
            }

            // The inner loop implements the `goto header_almost_done` /
            // `goto headers_almost_done` jumps: set `state` and `continue`.
            loop {
                match state {
                    State::Dead => {
                        // this state is used after a 'Connection: close'
                        // message; the parser will error out if it reads
                        // another message
                        error!(p);
                    }

                    State::StartReqOrRes => {
                        if ch == CR || ch == LF {
                            break;
                        }
                        self.flags = 0;
                        self.content_length = -1;

                        callback2!(on_message_begin, p);

                        if ch == b'H' {
                            state = State::ResOrRespH;
                        } else {
                            self.parser_type = ParserType::Request;
                            // goto start_req_method_assign
                            self.start_req_method_assign(ch);
                            index = 1;
                            state = State::ReqMethod;
                        }
                    }

                    State::ResOrRespH => {
                        if ch == b'T' {
                            self.parser_type = ParserType::Response;
                            state = State::ResHT;
                        } else {
                            if ch != b'E' {
                                error!(p);
                            }
                            self.parser_type = ParserType::Request;
                            self.method = Method::Head;
                            index = 2;
                            state = State::ReqMethod;
                        }
                    }

                    State::StartRes => {
                        self.flags = 0;
                        self.content_length = -1;

                        callback2!(on_message_begin, p);

                        match ch {
                            b'H' => state = State::ResH,
                            CR | LF => {}
                            _ => error!(p),
                        }
                    }

                    // STRICT_CHECK(ch != 'T') etc. are no-ops.
                    State::ResH => state = State::ResHT,
                    State::ResHT => state = State::ResHTT,
                    State::ResHTT => state = State::ResHTTP,
                    State::ResHTTP => state = State::ResFirstHttpMajor,

                    State::ResFirstHttpMajor => {
                        if !(b'1'..=b'9').contains(&ch) {
                            error!(p);
                        }
                        self.http_major = u16::from(ch - b'0');
                        state = State::ResHttpMajor;
                    }

                    // major HTTP version or dot
                    State::ResHttpMajor => {
                        if ch == b'.' {
                            state = State::ResFirstHttpMinor;
                            break;
                        }
                        if !ch.is_ascii_digit() {
                            error!(p);
                        }
                        self.http_major = self.http_major.wrapping_mul(10);
                        self.http_major = self.http_major.wrapping_add(u16::from(ch - b'0'));
                        if self.http_major > 999 {
                            error!(p);
                        }
                    }

                    // first digit of minor HTTP version
                    State::ResFirstHttpMinor => {
                        if !ch.is_ascii_digit() {
                            error!(p);
                        }
                        self.http_minor = u16::from(ch - b'0');
                        state = State::ResHttpMinor;
                    }

                    // minor HTTP version or end of request line
                    State::ResHttpMinor => {
                        if ch == b' ' {
                            state = State::ResFirstStatusCode;
                            break;
                        }
                        if !ch.is_ascii_digit() {
                            error!(p);
                        }
                        self.http_minor = self.http_minor.wrapping_mul(10);
                        self.http_minor = self.http_minor.wrapping_add(u16::from(ch - b'0'));
                        if self.http_minor > 999 {
                            error!(p);
                        }
                    }

                    State::ResFirstStatusCode => {
                        if !ch.is_ascii_digit() {
                            if ch == b' ' {
                                break;
                            }
                            error!(p);
                        }
                        self.status_code = u16::from(ch - b'0');
                        state = State::ResStatusCode;
                    }

                    State::ResStatusCode => {
                        if !ch.is_ascii_digit() {
                            match ch {
                                b' ' => state = State::ResStatus,
                                CR => state = State::ResLineAlmostDone,
                                LF => state = State::HeaderFieldStart,
                                _ => error!(p),
                            }
                            break;
                        }
                        self.status_code = self.status_code.wrapping_mul(10);
                        self.status_code = self.status_code.wrapping_add(u16::from(ch - b'0'));
                        if self.status_code > 999 {
                            error!(p);
                        }
                    }

                    State::ResStatus => {
                        // the human readable status. e.g. "NOT FOUND"
                        if ch == CR {
                            state = State::ResLineAlmostDone;
                        } else if ch == LF {
                            state = State::HeaderFieldStart;
                        }
                    }

                    State::ResLineAlmostDone => {
                        // STRICT_CHECK(ch != LF);
                        state = State::HeaderFieldStart;
                    }

                    State::StartReq => {
                        if ch == CR || ch == LF {
                            break;
                        }
                        self.flags = 0;
                        self.content_length = -1;

                        callback2!(on_message_begin, p);

                        if !ch.is_ascii_uppercase() {
                            error!(p);
                        }

                        // start_req_method_assign:
                        self.start_req_method_assign(ch);
                        index = 1;
                        state = State::ReqMethod;
                    }

                    State::ReqMethod => {
                        if ch == 0 {
                            error!(p);
                        }

                        let matcher = self.method.as_str().as_bytes();
                        // matcher[index], with the C string's NUL terminator
                        let mch = matcher.get(index as usize).copied().unwrap_or(0);
                        let m = self.method;
                        if ch == b' ' {
                            if m != Method::NotImplemented && mch != 0 {
                                self.method = Method::NotImplemented;
                            }
                            state = State::ReqSpacesBeforeUrl;
                        } else if m == Method::NotImplemented || ch == mch {
                            // nada
                        } else if m == Method::Connect {
                            if index == 1 && ch == b'H' {
                                self.method = Method::Checkout;
                            } else if index == 2 && ch == b'P' {
                                self.method = Method::Copy;
                            } else {
                                self.method = Method::NotImplemented;
                            }
                        } else if m == Method::Mkcol {
                            if index == 1 && ch == b'O' {
                                self.method = Method::Move;
                            } else if index == 3 && ch == b'A' {
                                self.method = Method::Mkcalendar;
                            } else if index == 1 && ch == b'E' {
                                self.method = Method::Merge;
                            } else if index == 1 && ch == b'-' {
                                self.method = Method::MSearch;
                            } else if index == 2 && ch == b'A' {
                                self.method = Method::Mkactivity;
                            } else {
                                self.method = Method::NotImplemented;
                            }
                        } else if index == 1 && m == Method::Post && ch == b'R' {
                            self.method = Method::Propfind; // or HTTP_PROPPATCH
                        } else if index == 1 && m == Method::Post && ch == b'U' {
                            self.method = Method::Put;
                        } else if index == 1 && m == Method::Post && ch == b'A' {
                            self.method = Method::Patch;
                        } else if index == 1 && m == Method::Subscribe && ch == b'E' {
                            self.method = Method::Search;
                        } else if index == 2 && m == Method::Unlock && ch == b'S' {
                            self.method = Method::Unsubscribe;
                        } else if index == 4 && m == Method::Propfind && ch == b'P' {
                            self.method = Method::Proppatch;
                        } else {
                            self.method = Method::NotImplemented;
                        }

                        index = index.wrapping_add(1);
                    }

                    State::ReqSpacesBeforeUrl => {
                        if ch == b' ' {
                            break;
                        }

                        if ch == b'/' || ch == b'*' {
                            url_mark = Some(p);
                            path_mark = Some(p);
                            state = State::ReqPath;
                            break;
                        }

                        if lower(ch).is_ascii_lowercase() {
                            url_mark = Some(p);
                            state = State::ReqSchema;
                            break;
                        }

                        error!(p);
                    }

                    State::ReqSchema => {
                        if lower(ch).is_ascii_lowercase() {
                            break;
                        }

                        if ch == b':' {
                            state = State::ReqSchemaSlash;
                            break;
                        } else if ch == b'.' || ch.is_ascii_digit() {
                            state = State::ReqHost;
                            break;
                        }

                        error!(p);
                    }

                    State::ReqSchemaSlash => {
                        // STRICT_CHECK(ch != '/');
                        state = State::ReqSchemaSlashSlash;
                    }

                    State::ReqSchemaSlashSlash => {
                        // STRICT_CHECK(ch != '/');
                        state = State::ReqHost;
                    }

                    State::ReqHost => {
                        if lower(ch).is_ascii_lowercase() {
                            break;
                        }
                        if ch.is_ascii_digit() || ch == b'.' || ch == b'-' {
                            break;
                        }
                        match ch {
                            b':' => state = State::ReqPort,
                            b'/' => {
                                path_mark = Some(p);
                                state = State::ReqPath;
                            }
                            b' ' => {
                                // "GET http://foo.bar.com HTTP/1.1": no path.
                                callback!(url_mark, on_url, p);
                                state = State::ReqHttpStart;
                            }
                            _ => error!(p),
                        }
                    }

                    State::ReqPort => {
                        if ch.is_ascii_digit() {
                            break;
                        }
                        match ch {
                            b'/' => {
                                path_mark = Some(p);
                                state = State::ReqPath;
                            }
                            b' ' => {
                                // "GET http://foo.bar.com:1234 HTTP/1.1"
                                callback!(url_mark, on_url, p);
                                state = State::ReqHttpStart;
                            }
                            _ => error!(p),
                        }
                    }

                    State::ReqPath => {
                        if normal_url_char(ch) {
                            break;
                        }

                        match ch {
                            b' ' => {
                                callback!(url_mark, on_url, p);
                                callback!(path_mark, on_path, p);
                                state = State::ReqHttpStart;
                            }
                            CR => {
                                callback!(url_mark, on_url, p);
                                callback!(path_mark, on_path, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::ReqLineAlmostDone;
                            }
                            LF => {
                                callback!(url_mark, on_url, p);
                                callback!(path_mark, on_path, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::HeaderFieldStart;
                            }
                            b'?' => {
                                callback!(path_mark, on_path, p);
                                state = State::ReqQueryStringStart;
                            }
                            b'#' => {
                                callback!(path_mark, on_path, p);
                                state = State::ReqFragmentStart;
                            }
                            _ => error!(p),
                        }
                    }

                    State::ReqQueryStringStart => {
                        if normal_url_char(ch) {
                            query_string_mark = Some(p);
                            state = State::ReqQueryString;
                            break;
                        }

                        match ch {
                            b'?' => {} // XXX ignore extra '?' ... is this right?
                            b' ' => {
                                callback!(url_mark, on_url, p);
                                state = State::ReqHttpStart;
                            }
                            CR => {
                                callback!(url_mark, on_url, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::ReqLineAlmostDone;
                            }
                            LF => {
                                callback!(url_mark, on_url, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::HeaderFieldStart;
                            }
                            b'#' => state = State::ReqFragmentStart,
                            _ => error!(p),
                        }
                    }

                    State::ReqQueryString => {
                        if normal_url_char(ch) {
                            break;
                        }

                        match ch {
                            b'?' => {} // allow extra '?' in query string
                            b' ' => {
                                callback!(url_mark, on_url, p);
                                callback!(query_string_mark, on_query_string, p);
                                state = State::ReqHttpStart;
                            }
                            CR => {
                                callback!(url_mark, on_url, p);
                                callback!(query_string_mark, on_query_string, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::ReqLineAlmostDone;
                            }
                            LF => {
                                callback!(url_mark, on_url, p);
                                callback!(query_string_mark, on_query_string, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::HeaderFieldStart;
                            }
                            b'#' => {
                                callback!(query_string_mark, on_query_string, p);
                                state = State::ReqFragmentStart;
                            }
                            _ => error!(p),
                        }
                    }

                    State::ReqFragmentStart => {
                        if normal_url_char(ch) {
                            fragment_mark = Some(p);
                            state = State::ReqFragment;
                            break;
                        }

                        match ch {
                            b' ' => {
                                callback!(url_mark, on_url, p);
                                state = State::ReqHttpStart;
                            }
                            CR => {
                                callback!(url_mark, on_url, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::ReqLineAlmostDone;
                            }
                            LF => {
                                callback!(url_mark, on_url, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::HeaderFieldStart;
                            }
                            b'?' => {
                                fragment_mark = Some(p);
                                state = State::ReqFragment;
                            }
                            b'#' => {}
                            _ => error!(p),
                        }
                    }

                    State::ReqFragment => {
                        if normal_url_char(ch) {
                            break;
                        }

                        match ch {
                            b' ' => {
                                callback!(url_mark, on_url, p);
                                callback!(fragment_mark, on_fragment, p);
                                state = State::ReqHttpStart;
                            }
                            CR => {
                                callback!(url_mark, on_url, p);
                                callback!(fragment_mark, on_fragment, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::ReqLineAlmostDone;
                            }
                            LF => {
                                callback!(url_mark, on_url, p);
                                callback!(fragment_mark, on_fragment, p);
                                self.http_major = 0;
                                self.http_minor = 9;
                                state = State::HeaderFieldStart;
                            }
                            b'?' | b'#' => {}
                            _ => error!(p),
                        }
                    }

                    State::ReqHttpStart => match ch {
                        b'H' => state = State::ReqHttpH,
                        b' ' => {}
                        _ => error!(p),
                    },

                    // STRICT_CHECK()s are no-ops.
                    State::ReqHttpH => state = State::ReqHttpHT,
                    State::ReqHttpHT => state = State::ReqHttpHTT,
                    State::ReqHttpHTT => state = State::ReqHttpHTTP,
                    State::ReqHttpHTTP => state = State::ReqFirstHttpMajor,

                    // first digit of major HTTP version
                    State::ReqFirstHttpMajor => {
                        if !(b'1'..=b'9').contains(&ch) {
                            error!(p);
                        }
                        self.http_major = u16::from(ch - b'0');
                        state = State::ReqHttpMajor;
                    }

                    // major HTTP version or dot
                    State::ReqHttpMajor => {
                        if ch == b'.' {
                            state = State::ReqFirstHttpMinor;
                            break;
                        }
                        if !ch.is_ascii_digit() {
                            error!(p);
                        }
                        self.http_major = self.http_major.wrapping_mul(10);
                        self.http_major = self.http_major.wrapping_add(u16::from(ch - b'0'));
                        if self.http_major > 999 {
                            error!(p);
                        }
                    }

                    // first digit of minor HTTP version
                    State::ReqFirstHttpMinor => {
                        if !ch.is_ascii_digit() {
                            error!(p);
                        }
                        self.http_minor = u16::from(ch - b'0');
                        state = State::ReqHttpMinor;
                    }

                    // minor HTTP version or end of request line
                    State::ReqHttpMinor => {
                        if ch == CR {
                            state = State::ReqLineAlmostDone;
                            break;
                        }
                        if ch == LF {
                            state = State::HeaderFieldStart;
                            break;
                        }
                        // XXX allow spaces after digit?
                        if !ch.is_ascii_digit() {
                            error!(p);
                        }
                        self.http_minor = self.http_minor.wrapping_mul(10);
                        self.http_minor = self.http_minor.wrapping_add(u16::from(ch - b'0'));
                        if self.http_minor > 999 {
                            error!(p);
                        }
                    }

                    // end of request line
                    State::ReqLineAlmostDone => {
                        if ch != LF {
                            error!(p);
                        }
                        state = State::HeaderFieldStart;
                    }

                    State::HeaderFieldStart => {
                        if ch == CR {
                            state = State::HeadersAlmostDone;
                            break;
                        }

                        if ch == LF {
                            // they might be just sending \n instead of \r\n
                            // so this would be the second \n to denote the
                            // end of headers
                            state = State::HeadersAlmostDone;
                            continue; // goto headers_almost_done
                        }

                        let c = token(ch);
                        if c == 0 {
                            error!(p);
                        }

                        header_field_mark = Some(p);

                        index = 0;
                        state = State::HeaderField;

                        header_state = match c {
                            b'c' => HeaderState::C,
                            b'p' => HeaderState::MatchingProxyConnection,
                            b't' => HeaderState::MatchingTransferEncoding,
                            b'u' => HeaderState::MatchingUpgrade,
                            _ => HeaderState::General,
                        };
                    }

                    State::HeaderField => {
                        let c = token(ch);

                        if c != 0 {
                            match header_state {
                                HeaderState::General => {}

                                HeaderState::C => {
                                    index = index.wrapping_add(1);
                                    header_state = if c == b'o' {
                                        HeaderState::Co
                                    } else {
                                        HeaderState::General
                                    };
                                }

                                HeaderState::Co => {
                                    index = index.wrapping_add(1);
                                    header_state = if c == b'n' {
                                        HeaderState::Con
                                    } else {
                                        HeaderState::General
                                    };
                                }

                                HeaderState::Con => {
                                    index = index.wrapping_add(1);
                                    header_state = match c {
                                        b'n' => HeaderState::MatchingConnection,
                                        b't' => HeaderState::MatchingContentLength,
                                        _ => HeaderState::General,
                                    };
                                }

                                HeaderState::MatchingConnection => matching(
                                    &mut index,
                                    &mut header_state,
                                    c,
                                    CONNECTION,
                                    HeaderState::Connection,
                                ),
                                HeaderState::MatchingProxyConnection => matching(
                                    &mut index,
                                    &mut header_state,
                                    c,
                                    PROXY_CONNECTION,
                                    HeaderState::Connection,
                                ),
                                HeaderState::MatchingContentLength => matching(
                                    &mut index,
                                    &mut header_state,
                                    c,
                                    CONTENT_LENGTH,
                                    HeaderState::ContentLength,
                                ),
                                HeaderState::MatchingTransferEncoding => matching(
                                    &mut index,
                                    &mut header_state,
                                    c,
                                    TRANSFER_ENCODING,
                                    HeaderState::TransferEncoding,
                                ),
                                HeaderState::MatchingUpgrade => matching(
                                    &mut index,
                                    &mut header_state,
                                    c,
                                    UPGRADE,
                                    HeaderState::Upgrade,
                                ),

                                HeaderState::Connection
                                | HeaderState::ContentLength
                                | HeaderState::TransferEncoding
                                | HeaderState::Upgrade
                                    if ch != b' ' =>
                                {
                                    header_state = HeaderState::General;
                                }

                                // assert(0 && "Unknown header_state")
                                _ => {}
                            }
                            break;
                        }

                        if ch == b':' {
                            callback!(header_field_mark, on_header_field, p);
                            state = State::HeaderValueStart;
                            break;
                        }

                        if ch == CR {
                            state = State::HeaderAlmostDone;
                            callback!(header_field_mark, on_header_field, p);
                            break;
                        }

                        if ch == LF {
                            callback!(header_field_mark, on_header_field, p);
                            state = State::HeaderFieldStart;
                            break;
                        }

                        error!(p);
                    }

                    State::HeaderValueStart => {
                        if ch == b' ' {
                            break;
                        }

                        header_value_mark = Some(p);

                        state = State::HeaderValue;
                        index = 0;

                        let c = lower(ch);

                        if ch == CR {
                            callback!(header_value_mark, on_header_value, p);
                            header_state = HeaderState::General;
                            state = State::HeaderAlmostDone;
                            break;
                        }

                        if ch == LF {
                            callback!(header_value_mark, on_header_value, p);
                            state = State::HeaderFieldStart;
                            break;
                        }

                        match header_state {
                            HeaderState::Upgrade => {
                                self.flags |= F_UPGRADE;
                                header_state = HeaderState::General;
                            }

                            HeaderState::TransferEncoding => {
                                // looking for 'Transfer-Encoding: chunked'
                                header_state = if c == b'c' {
                                    HeaderState::MatchingTransferEncodingChunked
                                } else {
                                    HeaderState::General
                                };
                            }

                            HeaderState::ContentLength => {
                                if !ch.is_ascii_digit() {
                                    error!(p);
                                }
                                self.content_length = i64::from(ch - b'0');
                            }

                            HeaderState::Connection => {
                                header_state = if c == b'k' {
                                    // looking for 'Connection: keep-alive'
                                    HeaderState::MatchingConnectionKeepAlive
                                } else if c == b'c' {
                                    // looking for 'Connection: close'
                                    HeaderState::MatchingConnectionClose
                                } else {
                                    HeaderState::General
                                };
                            }

                            _ => header_state = HeaderState::General,
                        }
                    }

                    State::HeaderValue => {
                        let c = lower(ch);

                        if ch == CR {
                            callback!(header_value_mark, on_header_value, p);
                            state = State::HeaderAlmostDone;
                            break;
                        }

                        if ch == LF {
                            callback!(header_value_mark, on_header_value, p);
                            state = State::HeaderAlmostDone;
                            continue; // goto header_almost_done
                        }

                        match header_state {
                            HeaderState::General => {}

                            // assert(0 && "Shouldn't get here.")
                            HeaderState::Connection | HeaderState::TransferEncoding => {}

                            HeaderState::ContentLength => {
                                if ch == b' ' {
                                    break;
                                }
                                if !ch.is_ascii_digit() {
                                    error!(p);
                                }
                                self.content_length = self.content_length.wrapping_mul(10);
                                self.content_length =
                                    self.content_length.wrapping_add(i64::from(ch - b'0'));
                            }

                            // Transfer-Encoding: chunked
                            HeaderState::MatchingTransferEncodingChunked => matching(
                                &mut index,
                                &mut header_state,
                                c,
                                CHUNKED,
                                HeaderState::TransferEncodingChunked,
                            ),

                            // looking for 'Connection: keep-alive'
                            HeaderState::MatchingConnectionKeepAlive => matching(
                                &mut index,
                                &mut header_state,
                                c,
                                KEEP_ALIVE,
                                HeaderState::ConnectionKeepAlive,
                            ),

                            // looking for 'Connection: close'
                            HeaderState::MatchingConnectionClose => matching(
                                &mut index,
                                &mut header_state,
                                c,
                                CLOSE,
                                HeaderState::ConnectionClose,
                            ),

                            HeaderState::TransferEncodingChunked
                            | HeaderState::ConnectionKeepAlive
                            | HeaderState::ConnectionClose => {
                                if ch != b' ' {
                                    header_state = HeaderState::General;
                                }
                            }

                            _ => {
                                state = State::HeaderValue;
                                header_state = HeaderState::General;
                            }
                        }
                    }

                    // header_almost_done:
                    State::HeaderAlmostDone => {
                        // STRICT_CHECK(ch != LF);
                        state = State::HeaderFieldStart;

                        match header_state {
                            HeaderState::ConnectionKeepAlive => {
                                self.flags |= F_CONNECTION_KEEP_ALIVE
                            }
                            HeaderState::ConnectionClose => self.flags |= F_CONNECTION_CLOSE,
                            HeaderState::TransferEncodingChunked => self.flags |= F_CHUNKED,
                            _ => {}
                        }
                    }

                    // headers_almost_done:
                    State::HeadersAlmostDone => {
                        // STRICT_CHECK(ch != LF);

                        if self.flags & F_TRAILING != 0 {
                            // End of a chunked request
                            callback2!(on_message_complete, p);
                            state = self.new_message();
                            break;
                        }

                        nread = 0;

                        if self.flags & F_UPGRADE != 0 || self.method == Method::Connect {
                            self.upgrade = true;
                        }

                        // If the user returns 1, interpret that as saying
                        // that this message has no body.
                        match cb.on_headers_complete_raw(&self.info()) {
                            0 => {}
                            1 => self.flags |= F_SKIPBODY,
                            _ => return p, // Error
                        }

                        // (The upgrade early exit is `#if 0`'d out in C.)

                        if self.flags & F_SKIPBODY != 0 {
                            callback2!(on_message_complete, p);
                            state = self.new_message();
                        } else if self.flags & F_CHUNKED != 0 {
                            // chunked encoding - ignore Content-Length header
                            state = State::ChunkSizeStart;
                        } else if self.content_length == 0 {
                            // Content-Length header given but zero
                            callback2!(on_message_complete, p);
                            state = self.new_message();
                        } else if self.content_length > 0 {
                            // Content-Length header given and non-zero
                            state = State::BodyIdentity;
                        } else if self.parser_type == ParserType::Request
                            || self.should_keep_alive()
                        {
                            // Assume content-length 0 - read the next
                            callback2!(on_message_complete, p);
                            state = self.new_message();
                        } else {
                            // Read body until EOF
                            state = State::BodyIdentityEof;
                        }
                    }

                    State::BodyIdentity => {
                        let to_read = ((len - p) as u64).min(self.content_length as u64) as usize;
                        if to_read > 0 {
                            // return value ignored, as in C
                            cb.on_body(&self.info(), &data[p..p + to_read]);
                            p += to_read - 1;
                            self.content_length = self.content_length.wrapping_sub(to_read as i64);
                            if self.content_length == 0 {
                                callback2!(on_message_complete, p);
                                state = self.new_message();
                            }
                        }
                    }

                    // read until EOF
                    State::BodyIdentityEof => {
                        let to_read = len - p;
                        if to_read > 0 {
                            cb.on_body(&self.info(), &data[p..p + to_read]);
                            p += to_read - 1;
                        }
                    }

                    State::ChunkSizeStart => {
                        let c = UNHEX[ch as usize];
                        if c == -1 {
                            error!(p);
                        }
                        self.content_length = i64::from(c);
                        state = State::ChunkSize;
                    }

                    State::ChunkSize => {
                        if ch == CR {
                            state = State::ChunkSizeAlmostDone;
                            break;
                        }

                        let c = UNHEX[ch as usize];

                        if c == -1 {
                            if ch == b';' || ch == b' ' {
                                state = State::ChunkParameters;
                                break;
                            }
                            error!(p);
                        }

                        self.content_length = self.content_length.wrapping_mul(16);
                        self.content_length = self.content_length.wrapping_add(i64::from(c));
                    }

                    State::ChunkParameters => {
                        // just ignore this. TODO check for overflow
                        if ch == CR {
                            state = State::ChunkSizeAlmostDone;
                        }
                    }

                    State::ChunkSizeAlmostDone => {
                        // STRICT_CHECK(ch != LF);
                        if self.content_length == 0 {
                            self.flags |= F_TRAILING;
                            state = State::HeaderFieldStart;
                        } else {
                            state = State::ChunkData;
                        }
                    }

                    State::ChunkData => {
                        let to_read = ((len - p) as u64).min(self.content_length as u64) as usize;

                        if to_read > 0 {
                            cb.on_body(&self.info(), &data[p..p + to_read]);
                            p += to_read - 1;
                        }

                        if to_read as u64 == self.content_length as u64 {
                            state = State::ChunkDataAlmostDone;
                        }

                        self.content_length = self.content_length.wrapping_sub(to_read as i64);
                    }

                    State::ChunkDataAlmostDone => {
                        // STRICT_CHECK(ch != CR);
                        state = State::ChunkDataDone;
                    }

                    State::ChunkDataDone => {
                        // STRICT_CHECK(ch != LF);
                        state = State::ChunkSizeStart;
                    }
                }
                break;
            }
            p += 1;
        }

        callback_noclear!(header_field_mark, on_header_field, p);
        callback_noclear!(header_value_mark, on_header_value, p);
        callback_noclear!(fragment_mark, on_fragment, p);
        callback_noclear!(query_string_mark, on_query_string, p);
        callback_noclear!(path_mark, on_path, p);
        callback_noclear!(url_mark, on_url, p);

        self.state = state;
        self.header_state = header_state;
        self.index = index as u8; // unsigned char in C
        self.nread = nread;

        len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Rec {
        begins: usize,
        path: Vec<u8>,
        query: Vec<u8>,
        url: Vec<u8>,
        fragment: Vec<u8>,
        /// (field, value) with partial pieces concatenated like php_cli_server.c
        headers: Vec<(Vec<u8>, Vec<u8>)>,
        last_was_value: bool,
        field_pieces: usize,
        body: Vec<u8>,
        headers_complete: usize,
        complete: usize,
        method: Option<Method>,
    }

    impl ParserCallbacks for Rec {
        fn on_message_begin(&mut self, _p: &ParserInfo) -> bool {
            self.begins += 1;
            false
        }
        fn on_path(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
            self.path.extend_from_slice(at);
            false
        }
        fn on_query_string(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
            self.query.extend_from_slice(at);
            false
        }
        fn on_url(&mut self, p: &ParserInfo, at: &[u8]) -> bool {
            self.method = Some(p.method);
            self.url.extend_from_slice(at);
            false
        }
        fn on_fragment(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
            self.fragment.extend_from_slice(at);
            false
        }
        fn on_header_field(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
            self.field_pieces += 1;
            if self.last_was_value || self.headers.is_empty() {
                self.headers.push((at.to_vec(), Vec::new()));
            } else {
                self.headers.last_mut().unwrap().0.extend_from_slice(at);
            }
            self.last_was_value = false;
            false
        }
        fn on_header_value(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
            self.headers.last_mut().unwrap().1.extend_from_slice(at);
            self.last_was_value = true;
            false
        }
        fn on_headers_complete(&mut self, _p: &ParserInfo) -> bool {
            self.headers_complete += 1;
            self.last_was_value = true;
            false
        }
        fn on_body(&mut self, _p: &ParserInfo, at: &[u8]) -> bool {
            self.body.extend_from_slice(at);
            false
        }
        fn on_message_complete(&mut self, _p: &ParserInfo) -> bool {
            self.complete += 1;
            false
        }
    }

    #[test]
    fn simple_get_with_query() {
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"GET /foo/bar.php?a=1&b=2#frag HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(p.execute(&mut r, req), req.len());
        assert_eq!(r.method, Some(Method::Get));
        assert_eq!(r.path, b"/foo/bar.php");
        assert_eq!(r.query, b"a=1&b=2");
        assert_eq!(r.fragment, b"frag");
        assert_eq!(r.url, b"/foo/bar.php?a=1&b=2#frag");
        assert_eq!(r.headers, vec![(b"Host".to_vec(), b"localhost".to_vec())]);
        assert_eq!((p.http_major(), p.http_minor()), (1, 1));
        assert_eq!(r.complete, 1);
        assert!(p.is_start_state());
        assert!(p.should_keep_alive());
    }

    #[test]
    fn post_body_split() {
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let a = b"POST /x HTTP/1.0\r\nContent-Length: 11\r\nConnection: keep-alive\r\n\r\nhello";
        let b = b" world";
        assert_eq!(p.execute(&mut r, a), a.len());
        assert_eq!(p.method(), Method::Post);
        assert_eq!(r.complete, 0);
        assert_eq!(p.content_length(), 6);
        assert_eq!(p.execute(&mut r, b), b.len());
        assert_eq!(r.body, b"hello world");
        assert_eq!(r.complete, 1);
        assert!(p.should_keep_alive());
        assert!(p.is_start_state());
    }

    #[test]
    fn header_split_across_calls() {
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let parts: [&[u8]; 4] = [
            b"GET /a?q",
            b"s=1 HTTP/1.1\r\nX-Lo",
            b"ng-Name: some va",
            b"lue\r\n\r\n",
        ];
        for part in parts {
            assert_eq!(p.execute(&mut r, part), part.len());
        }
        assert_eq!(r.url, b"/a?qs=1");
        assert_eq!(r.path, b"/a");
        assert_eq!(r.query, b"qs=1");
        assert_eq!(r.field_pieces, 2);
        assert_eq!(
            r.headers,
            vec![(b"X-Long-Name".to_vec(), b"some value".to_vec())]
        );
        assert_eq!(r.complete, 1);
    }

    #[test]
    fn chunked_body() {
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nX-Trailer: t\r\n\r\n";
        assert_eq!(p.execute(&mut r, req), req.len());
        assert_eq!(r.body, b"hello world");
        assert_eq!(r.complete, 1);
        assert_eq!(r.headers_complete, 1);
        assert!(p.flags() & F_CHUNKED != 0);
        assert!(p.is_start_state());
    }

    #[test]
    fn methods() {
        for (line, m) in [
            (&b"FOO / HTTP/1.1\r\n\r\n"[..], Method::NotImplemented),
            (b"GETX / HTTP/1.1\r\n\r\n", Method::NotImplemented),
            (b"GE / HTTP/1.1\r\n\r\n", Method::NotImplemented),
            (b"PROPPATCH / HTTP/1.1\r\n\r\n", Method::Proppatch),
            (b"M-SEARCH * HTTP/1.1\r\n\r\n", Method::MSearch),
            (b"MKCALENDAR / HTTP/1.1\r\n\r\n", Method::Mkcalendar),
            (b"PATCH / HTTP/1.1\r\n\r\n", Method::Patch),
            (b"COPY / HTTP/1.1\r\n\r\n", Method::Copy),
            (b"UNSUBSCRIBE / HTTP/1.1\r\n\r\n", Method::Unsubscribe),
        ] {
            let mut p = HttpParser::new_request();
            let mut r = Rec::default();
            assert_eq!(p.execute(&mut r, line), line.len());
            assert_eq!(p.method(), m);
            assert_eq!(r.complete, 1);
        }
        assert_eq!(Method::NotImplemented.as_str(), "NOTIMPLEMENTED");
        assert_eq!(Method::MSearch.as_str(), "M-SEARCH");
    }

    #[test]
    fn malformed() {
        // lowercase method
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        assert_eq!(p.execute(&mut r, b"get / HTTP/1.1\r\n\r\n"), 0);
        assert!(p.is_dead());

        // bad version
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"GET / HTTP/x.1\r\n\r\n";
        assert_eq!(p.execute(&mut r, req), 11);

        // control char in path
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"GET /a\x01b HTTP/1.1\r\n\r\n";
        assert_eq!(p.execute(&mut r, req), 6);

        // TLS hello
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        assert_eq!(p.execute(&mut r, b"\x16\x03\x01\x02\x00"), 0);

        // bad content-length
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n";
        assert!(p.execute(&mut r, req) < req.len());
    }

    #[test]
    fn http09_and_absolute_url() {
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"GET /index.html\r\n\r\n";
        assert_eq!(p.execute(&mut r, req), req.len());
        assert_eq!((p.http_major(), p.http_minor()), (0, 9));
        assert_eq!(r.complete, 1);

        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let req = b"GET http://example.com:8080/p?x HTTP/1.1\r\n\r\n";
        assert_eq!(p.execute(&mut r, req), req.len());
        assert_eq!(r.url, b"http://example.com:8080/p?x");
        assert_eq!(r.path, b"/p");
        assert_eq!(r.query, b"x");
    }

    #[test]
    fn byte_by_byte() {
        let req: &[u8] = b"POST /a/b?c=d HTTP/1.1\r\nHost: h\r\nContent-Length: 3\r\n\r\nabc";
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        for b in req.chunks(1) {
            assert_eq!(p.execute(&mut r, b), 1);
        }
        assert_eq!(r.url, b"/a/b?c=d");
        assert_eq!(r.path, b"/a/b");
        assert_eq!(r.query, b"c=d");
        assert_eq!(
            r.headers,
            vec![
                (b"Host".to_vec(), b"h".to_vec()),
                (b"Content-Length".to_vec(), b"3".to_vec())
            ]
        );
        assert_eq!(r.body, b"abc");
        assert_eq!(r.complete, 1);
    }

    #[test]
    fn header_too_large() {
        let mut p = HttpParser::new_request();
        let mut r = Rec::default();
        let mut req = b"GET / HTTP/1.1\r\nX: ".to_vec();
        req.resize(PHP_HTTP_MAX_HEADER_SIZE as usize + 10, b'a');
        assert_eq!(p.execute(&mut r, &req), PHP_HTTP_MAX_HEADER_SIZE as usize);
    }

    #[test]
    fn callback_error_stops() {
        struct Fail;
        impl ParserCallbacks for Fail {
            fn on_path(&mut self, _p: &ParserInfo, _at: &[u8]) -> bool {
                true
            }
        }
        let mut p = HttpParser::new_request();
        let req = b"GET /abc HTTP/1.1\r\n\r\n";
        assert_eq!(p.execute(&mut Fail, req), 8);
        // flush at end of buffer also fails, and returns len
        let mut p = HttpParser::new_request();
        assert_eq!(p.execute(&mut Fail, b"GET /ab"), 7);
    }
}
