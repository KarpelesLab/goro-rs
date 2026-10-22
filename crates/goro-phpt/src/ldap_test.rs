//! In-process LDAP server fixture for the ext/ldap tests.
//!
//! Like the embedded MySQL/PostgreSQL fixtures, but implemented in Rust: at
//! runner startup (when ext/ldap tests are selected) a background thread
//! listens on an ephemeral TCP port and speaks LDAPv3 against an in-memory
//! DIT with the suffix `dc=my-domain,dc=com` and rootdn `cn=Manager,...`
//! (password "secret"), the semantics the tests rely on (schema basics,
//! filters, scopes, size limits, the controls they use, the rootDSE, simple
//! bind, StartTLS with a test cert, and the whoami / password-modify
//! extended operations). It never special-cases test names — it implements
//! the operations and returns the same result codes slapd would.
//!
//! LDAP_TEST_URI / LDAP_TEST_HOST / LDAP_TEST_PORT are exported so
//! connect.inc finds it. ldaps (port 636) is not provided (privileged port),
//! so those few tests skip.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

use goro_ldap::ber::{self, Reader, TAG_OCTETSTRING, TAG_SET, Writer};
use goro_ldap::consts::*;

const SUFFIX: &str = "dc=my-domain,dc=com";
const ROOTDN: &str = "cn=Manager,dc=my-domain,dc=com";
const ROOTPW: &[u8] = b"secret";

static TEST_KEY_DER: &[u8] = include_bytes!("ldap_test_key.der");
static TEST_CERT_DER: &[u8] = include_bytes!("ldap_test_cert.der");

/// Known attribute types (lowercased). Values: single-valued?
fn known_attr(name: &[u8]) -> Option<bool> {
    let n = name.to_ascii_lowercase();
    let single = matches!(n.as_slice(), b"dc");
    let known = matches!(
        n.as_slice(),
        b"objectclass"
            | b"dc"
            | b"o"
            | b"ou"
            | b"cn"
            | b"sn"
            | b"c"
            | b"l"
            | b"userpassword"
            | b"telephonenumber"
            | b"description"
            | b"ref"
            | b"member"
            | b"uid"
            | b"mail"
            | b"givenname"
            | b"st"
            | b"street"
    );
    known.then_some(single)
}

/// A DIT entry: original DN and ordered attributes.
#[derive(Clone)]
struct Entry {
    dn: String,
    attrs: Vec<(Vec<u8>, Vec<Vec<u8>>)>,
}

impl Entry {
    fn get(&self, name: &[u8]) -> Option<&Vec<Vec<u8>>> {
        self.attrs
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }
    fn get_mut(&mut self, name: &[u8]) -> Option<&mut Vec<Vec<u8>>> {
        self.attrs
            .iter_mut()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }
    fn is_referral(&self) -> bool {
        self.get(b"objectclass")
            .is_some_and(|v| v.iter().any(|x| x.eq_ignore_ascii_case(b"referral")))
    }
}

struct Dit {
    /// normalized-DN -> entry
    entries: BTreeMap<String, Entry>,
}

/// Normalize a DN string for lookup (lowercase attr & value, trim spaces).
fn norm_dn(dn: &[u8]) -> Option<String> {
    let parsed = goro_ldap::dn::str2dn(dn)?;
    let mut parts = Vec::new();
    for rdn in &parsed {
        let mut avas: Vec<String> = rdn
            .iter()
            .map(|ava| {
                format!(
                    "{}={}",
                    String::from_utf8_lossy(&ava.attr).to_ascii_lowercase(),
                    String::from_utf8_lossy(&ava.value)
                        .trim()
                        .to_ascii_lowercase()
                )
            })
            .collect();
        avas.sort();
        parts.push(avas.join("+"));
    }
    Some(parts.join(","))
}

/// Validate DN: parse and every RDN attribute type must be known.
fn dn_valid(dn: &[u8]) -> bool {
    match goro_ldap::dn::str2dn(dn) {
        Some(parsed) => parsed
            .iter()
            .all(|rdn| rdn.iter().all(|ava| known_attr(&ava.attr).is_some())),
        None => false,
    }
}

/// The parent DN (normalized) of a DN, or None at the top.
fn parent_norm(dn: &[u8]) -> Option<String> {
    let parsed = goro_ldap::dn::str2dn(dn)?;
    if parsed.len() <= 1 {
        return Some(String::new());
    }
    let child = goro_ldap::dn::dn2str_v3(&parsed[1..].to_vec());
    norm_dn(&child)
}

impl Dit {
    fn new() -> Dit {
        Dit {
            entries: BTreeMap::new(),
        }
    }
}

// ───────────────────────── fixture lifecycle ─────────────────────────

struct Fixture {
    port: u16,
    stop: Arc<AtomicBool>,
}

static FIXTURE: Mutex<Option<Fixture>> = Mutex::new(None);
static INIT: std::sync::Once = std::sync::Once::new();
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static PORT: AtomicU16 = AtomicU16::new(0);

pub fn is_ldap_test(path: &str) -> bool {
    path.contains("/ext/ldap/tests/")
}

pub fn ensure_started() -> bool {
    INIT.call_once(|| {
        if std::env::var_os("LDAP_TEST_URI").is_some()
            && std::env::var_os("GORO_LDAP_FIXTURE").is_none()
        {
            AVAILABLE.store(true, Ordering::SeqCst);
            return;
        }
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(_) => return,
        };
        let port = match listener.local_addr() {
            Ok(a) => a.port(),
            Err(_) => return,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut dit0 = Dit::new();
        // The directory is provisioned with the suffix entry (as the test
        // slapd is), so insert_dummy_data() finds it and only adds children.
        dit0.entries.insert(
            norm_dn(SUFFIX.as_bytes()).unwrap(),
            Entry {
                dn: SUFFIX.to_string(),
                attrs: vec![
                    (
                        b"objectClass".to_vec(),
                        vec![
                            b"top".to_vec(),
                            b"dcObject".to_vec(),
                            b"organization".to_vec(),
                        ],
                    ),
                    (b"o".to_vec(), vec![b"php ldap tests".to_vec()]),
                    (b"dc".to_vec(), vec![b"my-domain".to_vec()]),
                ],
            },
        );
        let dit = Arc::new(Mutex::new(dit0));
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            listener.set_nonblocking(true).ok();
            loop {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((sock, _)) => {
                        let dit = dit.clone();
                        std::thread::spawn(move || {
                            let _ = handle_conn(sock, dit);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        unsafe {
            std::env::set_var("LDAP_TEST_HOST", "127.0.0.1");
            std::env::set_var("LDAP_TEST_PORT", port.to_string());
            std::env::set_var("LDAP_TEST_URI", format!("ldap://127.0.0.1:{}", port));
            std::env::set_var("LDAP_TEST_BASE", SUFFIX);
            std::env::set_var("LDAP_TEST_USER", ROOTDN);
            std::env::set_var("LDAP_TEST_PASSWD", "secret");
            std::env::set_var("LDAP_TEST_SKIP_BIND_FAILURE", "1");
        }
        PORT.store(port, Ordering::SeqCst);
        AVAILABLE.store(true, Ordering::SeqCst);
        *FIXTURE.lock().unwrap() = Some(Fixture { port, stop });
    });
    AVAILABLE.load(Ordering::SeqCst)
}

pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::SeqCst)
}

pub fn shutdown() {
    if let Some(fx) = FIXTURE.lock().unwrap().take() {
        fx.stop.store(true, Ordering::SeqCst);
        // nudge the accept loop
        let _ = TcpStream::connect(("127.0.0.1", fx.port));
    }
}

// ───────────────────────── connection handling ─────────────────────────

enum Transport {
    Plain(TcpStream),
    Tls(Box<goro_ldap::tls::TlsStream>),
}

impl Transport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Transport::Plain(s) => s.read(buf),
            Transport::Tls(t) => t.read(buf),
        }
    }
    fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Transport::Plain(s) => s.write_all(data),
            Transport::Tls(t) => t.write_all(data),
        }
    }
}

struct Session {
    bound_dn: Option<String>, // None = anonymous
}

fn handle_conn(sock: TcpStream, dit: Arc<Mutex<Dit>>) -> std::io::Result<()> {
    sock.set_nodelay(true).ok();
    let mut transport = Transport::Plain(sock);
    let mut session = Session { bound_dn: None };
    let mut inbuf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 16384];
    loop {
        // find a complete message
        let msg_len = loop {
            match ber::element_len(&inbuf) {
                Some(Ok(n)) => break Some(n),
                Some(Err(_)) => return Ok(()),
                None => {
                    let n = transport.read(&mut tmp)?;
                    if n == 0 {
                        return Ok(());
                    }
                    inbuf.extend_from_slice(&tmp[..n]);
                }
            }
        };
        let Some(total) = msg_len else { return Ok(()) };
        let msg = inbuf[..total].to_vec();
        inbuf.drain(..total);

        match dispatch(&msg, &mut session, &dit) {
            DispatchResult::Reply(bytes) => transport.write_all(&bytes)?,
            DispatchResult::StartTls(bytes) => {
                transport.write_all(&bytes)?;
                // upgrade to TLS (server handshake)
                let Transport::Plain(s) = transport else {
                    return Ok(());
                };
                let fd = std::os::fd::OwnedFd::from(s);
                let cfg =
                    match goro_ldap::tls::server_config(vec![TEST_CERT_DER.to_vec()], TEST_KEY_DER)
                    {
                        Ok(c) => c,
                        Err(_) => return Ok(()),
                    };
                match goro_ldap::tls::TlsStream::accept(fd, &cfg) {
                    Ok(t) => transport = Transport::Tls(Box::new(t)),
                    Err(_) => return Ok(()),
                }
            }
            DispatchResult::Close => return Ok(()),
        }
    }
}

enum DispatchResult {
    Reply(Vec<u8>),
    StartTls(Vec<u8>),
    Close,
}

fn dispatch(msg: &[u8], session: &mut Session, dit: &Arc<Mutex<Dit>>) -> DispatchResult {
    let mut r = Reader::new(msg);
    let Ok(mut seq) = r.seq() else {
        return DispatchResult::Close;
    };
    let Ok(msgid) = seq.int() else {
        return DispatchResult::Close;
    };
    let Some(op_tag) = seq.peek_tag() else {
        return DispatchResult::Close;
    };
    let Ok(op) = seq.read_raw() else {
        return DispatchResult::Close;
    };
    // controls (optional [0])
    let controls = match seq.optional(LDAP_TAG_CONTROLS) {
        Ok(Some(c)) => parse_req_controls(c),
        _ => Vec::new(),
    };

    match op_tag {
        LDAP_REQ_BIND => DispatchResult::Reply(do_bind(msgid, op, session, dit)),
        LDAP_REQ_UNBIND => DispatchResult::Close,
        LDAP_REQ_SEARCH => DispatchResult::Reply(do_search(msgid, op, session, dit, &controls)),
        LDAP_REQ_ADD => DispatchResult::Reply(do_add(msgid, op, dit, &controls)),
        LDAP_REQ_DELETE => DispatchResult::Reply(do_delete(msgid, op, dit, &controls)),
        LDAP_REQ_MODIFY => DispatchResult::Reply(do_modify(msgid, op, dit, &controls)),
        LDAP_REQ_MODDN => DispatchResult::Reply(do_moddn(msgid, op, dit, &controls)),
        LDAP_REQ_COMPARE => DispatchResult::Reply(do_compare(msgid, op, dit, &controls)),
        LDAP_REQ_EXTENDED => do_extended(msgid, op, session, dit, &controls),
        _ => DispatchResult::Reply(ldap_result_msg(
            msgid,
            LDAP_RES_EXTENDED,
            LDAP_PROTOCOL_ERROR,
            "",
            "",
        )),
    }
}

// ───────────────────────── request controls ─────────────────────────

struct ReqControl {
    oid: Vec<u8>,
    value: Vec<u8>,
}

fn parse_req_controls(bytes: &[u8]) -> Vec<ReqControl> {
    let mut out = Vec::new();
    let mut r = Reader::new(bytes);
    while !r.is_empty() {
        let Ok(mut c) = r.seq() else { break };
        let Ok(oid) = c.octets() else { break };
        let mut value = Vec::new();
        while !c.is_empty() {
            match c.peek_tag() {
                Some(ber::TAG_BOOLEAN) => {
                    let _ = c.boolean();
                }
                Some(TAG_OCTETSTRING) => value = c.octets().map(|v| v.to_vec()).unwrap_or_default(),
                _ => {
                    let _ = c.read_raw();
                }
            }
        }
        out.push(ReqControl {
            oid: oid.to_vec(),
            value,
        });
    }
    out
}

fn find_ctrl<'a>(ctrls: &'a [ReqControl], oid: &str) -> Option<&'a ReqControl> {
    ctrls.iter().find(|c| c.oid == oid.as_bytes())
}

/// If an assertion control is present, whether it holds for `entry`.
fn assertion_holds(controls: &[ReqControl], entry: &Entry) -> bool {
    match find_ctrl(controls, LDAP_CONTROL_ASSERT) {
        Some(c) => eval_filter(&c.value, entry),
        None => true,
    }
}

// ───────────────────────── response builders ─────────────────────────

/// SearchResultDone with resultCode=referral(10), a matchedDN and a
/// referral [3] field.
fn search_referral_done(msgid: i64, matched: &str, refs: &[Vec<u8>]) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(msgid);
    w.start(LDAP_RES_SEARCH_RESULT);
    w.enumerated(LDAP_REFERRAL as i64);
    w.octets(matched.as_bytes());
    w.octets(b"");
    w.start(LDAP_TAG_REFERRAL);
    for r in refs {
        w.octets(r);
    }
    w.end();
    w.end();
    w.end();
    w.finish()
}

fn ldap_result_msg(msgid: i64, res_tag: u32, code: i32, matched: &str, errmsg: &str) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(msgid);
    w.start(res_tag);
    w.enumerated(code as i64);
    w.octets(matched.as_bytes());
    w.octets(errmsg.as_bytes());
    w.end();
    w.end();
    w.finish()
}

/// A result message with response controls.
fn ldap_result_msg_ctrls(
    msgid: i64,
    res_tag: u32,
    code: i32,
    matched: &str,
    errmsg: &str,
    ctrls: &[(Vec<u8>, Vec<u8>)],
) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(msgid);
    w.start(res_tag);
    w.enumerated(code as i64);
    w.octets(matched.as_bytes());
    w.octets(errmsg.as_bytes());
    w.end();
    if !ctrls.is_empty() {
        w.start(LDAP_TAG_CONTROLS);
        for (oid, val) in ctrls {
            w.start_seq();
            w.octets(oid);
            w.octets(val);
            w.end();
        }
        w.end();
    }
    w.end();
    w.finish()
}

// ───────────────────────── bind ─────────────────────────

fn do_bind(msgid: i64, op: &[u8], session: &mut Session, dit: &Arc<Mutex<Dit>>) -> Vec<u8> {
    let mut r = Reader::new(op);
    let Ok(mut body) = r.enter(LDAP_REQ_BIND) else {
        return ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_PROTOCOL_ERROR, "", "");
    };
    let _version = body.int().unwrap_or(3);
    let Ok(name) = body.octets() else {
        return ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_PROTOCOL_ERROR, "", "");
    };
    let name = name.to_vec();
    // authentication: [0] simple OCTET STRING, or [3] SASL
    let auth_tag = body.peek_tag();
    if auth_tag == Some(LDAP_AUTH_SASL) {
        // SASL not supported by the fixture.
        return ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_AUTH_METHOD_NOT_SUPPORTED, "", "");
    }
    let cred = body
        .expect(LDAP_AUTH_SIMPLE)
        .map(|v| v.to_vec())
        .unwrap_or_default();

    // anonymous
    if name.is_empty() {
        session.bound_dn = None;
        return ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_SUCCESS, "", "");
    }
    if !dn_valid(&name) {
        return ldap_result_msg(
            msgid,
            LDAP_RES_BIND,
            LDAP_INVALID_DN_SYNTAX,
            "",
            "invalid DN",
        );
    }
    let nn = norm_dn(&name).unwrap_or_default();
    // rootdn
    if nn == norm_dn(ROOTDN.as_bytes()).unwrap() {
        if cred == ROOTPW {
            session.bound_dn = Some(nn);
            return ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_SUCCESS, "", "");
        }
        return ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_INVALID_CREDENTIALS, "", "");
    }
    let dit = dit.lock().unwrap();
    match dit.entries.get(&nn) {
        Some(e) => {
            let ok = e
                .get(b"userpassword")
                .is_some_and(|v| v.iter().any(|p| p == &cred));
            if ok {
                drop(dit);
                session.bound_dn = Some(nn);
                ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_SUCCESS, "", "")
            } else {
                ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_INVALID_CREDENTIALS, "", "")
            }
        }
        None => ldap_result_msg(msgid, LDAP_RES_BIND, LDAP_INVALID_CREDENTIALS, "", ""),
    }
}

// ───────────────────────── search ─────────────────────────

#[allow(clippy::too_many_lines)]
fn do_search(
    msgid: i64,
    op: &[u8],
    _session: &Session,
    dit: &Arc<Mutex<Dit>>,
    controls: &[ReqControl],
) -> Vec<u8> {
    let mut r = Reader::new(op);
    let Ok(mut body) = r.enter(LDAP_REQ_SEARCH) else {
        return ldap_result_msg(msgid, LDAP_RES_SEARCH_RESULT, LDAP_PROTOCOL_ERROR, "", "");
    };
    let base = body.octets().unwrap_or(b"").to_vec();
    let scope = body.enumerated().unwrap_or(0) as i32;
    let _deref = body.enumerated().unwrap_or(0);
    let sizelimit = body.int().unwrap_or(0);
    let _timelimit = body.int().unwrap_or(0);
    let typesonly = body.boolean().unwrap_or(false);
    let filter = body.read_raw().unwrap_or(&[]).to_vec();
    // requested attributes
    let mut want_attrs: Vec<Vec<u8>> = Vec::new();
    if let Ok(mut al) = body.seq() {
        while !al.is_empty() {
            if let Ok(a) = al.octets() {
                want_attrs.push(a.to_vec());
            } else {
                break;
            }
        }
    }

    let managedsait = find_ctrl(controls, LDAP_CONTROL_MANAGEDSAIT).is_some();

    // rootDSE: an empty base is the root; a non-base scope does not descend
    // into the naming contexts, so only the rootDSE itself is in scope.
    if base.is_empty() {
        let entry = root_dse();
        let mut w = Writer::new();
        if eval_filter(&filter, &entry) {
            push_search_entry(&mut w, msgid, &entry, &want_attrs, typesonly);
        }
        let mut done = w.finish();
        done.extend_from_slice(&ldap_result_msg(
            msgid,
            LDAP_RES_SEARCH_RESULT,
            LDAP_SUCCESS,
            "",
            "",
        ));
        return done;
    }

    let dit = dit.lock().unwrap();
    let base_norm = match norm_dn(&base) {
        Some(n) => n,
        None => {
            return ldap_result_msg(
                msgid,
                LDAP_RES_SEARCH_RESULT,
                LDAP_INVALID_DN_SYNTAX,
                "",
                "",
            );
        }
    };
    // base must exist (except rootDSE handled above)
    if !dit.entries.contains_key(&base_norm) {
        return ldap_result_msg(msgid, LDAP_RES_SEARCH_RESULT, LDAP_NO_SUCH_OBJECT, "", "");
    }

    // assertion control
    if let Some(c) = find_ctrl(controls, LDAP_CONTROL_ASSERT) {
        if let Some(base_entry) = dit.entries.get(&base_norm) {
            if !eval_filter(&c.value, base_entry) {
                return ldap_result_msg(
                    msgid,
                    LDAP_RES_SEARCH_RESULT,
                    LDAP_ASSERTION_FAILED,
                    "",
                    "",
                );
            }
        }
    }

    // A search whose base is itself a referral returns a referral result
    // (resultCode referral, matchedDN = base, the ref URLs as referrals).
    if let Some(base_entry) = dit.entries.get(&base_norm) {
        if base_entry.is_referral() && !managedsait {
            let refs = base_entry.get(b"ref").cloned().unwrap_or_default();
            return search_referral_done(msgid, &base_entry.dn, &refs);
        }
    }

    // collect matching entries
    let mut matches: Vec<&Entry> = Vec::new();
    for (ndn, e) in dit.entries.iter() {
        if !in_scope(&base_norm, ndn, scope) {
            continue;
        }
        if eval_filter(&filter, e) {
            matches.push(e);
        }
    }
    // stable order by DN depth then insertion-ish (BTreeMap gives sorted DN);
    // present them in the order the tests expect (userA, userB, userC...).
    matches.sort_by(|a, b| entry_order(a).cmp(&entry_order(b)));

    // sort control
    let mut sorted_resp = false;
    if let Some(c) = find_ctrl(controls, LDAP_CONTROL_SORTREQUEST) {
        if let Some((attr, reverse)) = parse_sort_req(&c.value) {
            matches.sort_by(|a, b| {
                let av = a
                    .get(&attr)
                    .and_then(|v| v.first())
                    .cloned()
                    .unwrap_or_default();
                let bv = b
                    .get(&attr)
                    .and_then(|v| v.first())
                    .cloned()
                    .unwrap_or_default();
                let o = av.to_ascii_lowercase().cmp(&bv.to_ascii_lowercase());
                if reverse { o.reverse() } else { o }
            });
            sorted_resp = true;
        }
    }

    // VLV control (requires a sort)
    let mut vlv_resp: Option<(i64, i64)> = None; // (target, count)
    if let Some(c) = find_ctrl(controls, LDAP_CONTROL_VLVREQUEST) {
        if let Some((before, after, offset, _count)) = parse_vlv_req(&c.value) {
            let total = matches.len() as i64;
            let target = offset.clamp(1, total.max(1));
            let start = (target - 1 - before).max(0);
            let end = (target - 1 + after + 1).min(total);
            let slice: Vec<&Entry> = matches[start as usize..end as usize].to_vec();
            matches = slice;
            vlv_resp = Some((target, total));
        }
    }

    // paged results control
    let mut paged_cookie_out: Option<Vec<u8>> = None;
    if let Some(c) = find_ctrl(controls, LDAP_CONTROL_PAGEDRESULTS) {
        if let Some((pagesize, cookie)) = parse_paged_req(&c.value) {
            let offset: usize = if cookie.is_empty() {
                0
            } else {
                std::str::from_utf8(&cookie)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0)
            };
            let endp = (offset + pagesize.max(0) as usize).min(matches.len());
            let page: Vec<&Entry> = matches[offset.min(matches.len())..endp].to_vec();
            let next = if endp < matches.len() {
                endp.to_string().into_bytes()
            } else {
                Vec::new()
            };
            matches = page;
            paged_cookie_out = Some(next);
        }
    }

    // size limit
    let mut size_exceeded = false;
    if sizelimit > 0 && matches.len() as i64 > sizelimit {
        matches.truncate(sizelimit as usize);
        size_exceeded = true;
    }

    // value-return filter control
    let vrf = find_ctrl(controls, LDAP_CONTROL_VALUESRETURNFILTER).map(|c| c.value.clone());

    let mut w = Writer::new();
    for e in &matches {
        if e.is_referral() && !managedsait {
            push_search_reference(&mut w, msgid, e);
        } else {
            push_search_entry_vrf(&mut w, msgid, e, &want_attrs, typesonly, vrf.as_deref());
        }
    }
    let mut out = w.finish();

    // build response controls
    let mut resp_ctrls: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if let Some(cookie) = paged_cookie_out {
        resp_ctrls.push((
            LDAP_CONTROL_PAGEDRESULTS.as_bytes().to_vec(),
            build_paged_resp(0, &cookie),
        ));
    }
    if sorted_resp {
        resp_ctrls.push((
            LDAP_CONTROL_SORTRESPONSE.as_bytes().to_vec(),
            build_sort_resp(0),
        ));
    }
    if let Some((target, count)) = vlv_resp {
        resp_ctrls.push((
            LDAP_CONTROL_VLVRESPONSE.as_bytes().to_vec(),
            build_vlv_resp(target, count),
        ));
    }

    let code = if size_exceeded {
        LDAP_SIZELIMIT_EXCEEDED
    } else {
        LDAP_SUCCESS
    };
    out.extend_from_slice(&ldap_result_msg_ctrls(
        msgid,
        LDAP_RES_SEARCH_RESULT,
        code,
        "",
        "",
        &resp_ctrls,
    ));
    out
}

/// A sort key for entries matching the tests' default ordering: shallower
/// DNs first, then by the full normalized DN but with a twist so that
/// userA < userB < userC and the leaf userC (under userB) comes last.
fn entry_order(e: &Entry) -> (usize, String) {
    let depth = e.dn.matches(',').count();
    (depth, e.dn.to_ascii_lowercase())
}

fn in_scope(base: &str, dn: &str, scope: i32) -> bool {
    match scope {
        LDAP_SCOPE_BASE => dn == base,
        LDAP_SCOPE_ONELEVEL => parent_of(dn).as_deref() == Some(base),
        LDAP_SCOPE_SUBTREE => dn == base || dn.ends_with(&format!(",{}", base)),
        _ => false,
    }
}

fn parent_of(dn: &str) -> Option<String> {
    dn.find(',').map(|i| dn[i + 1..].to_string())
}

fn push_search_entry(w: &mut Writer, msgid: i64, e: &Entry, want: &[Vec<u8>], typesonly: bool) {
    push_search_entry_vrf(w, msgid, e, want, typesonly, None);
}

fn push_search_entry_vrf(
    w: &mut Writer,
    msgid: i64,
    e: &Entry,
    want: &[Vec<u8>],
    typesonly: bool,
    vrf: Option<&[u8]>,
) {
    w.start_seq();
    w.int(msgid);
    w.start(LDAP_RES_SEARCH_ENTRY);
    w.octets(e.dn.as_bytes());
    w.start_seq();
    let all = want.is_empty() || want.iter().any(|a| a == b"*");
    for (name, vals) in &e.attrs {
        let selected = all || want.iter().any(|a| a.eq_ignore_ascii_case(name));
        if !selected {
            continue;
        }
        w.start_seq();
        w.octets(name);
        w.start_set();
        if !typesonly {
            for v in vals {
                if let Some(vrf) = vrf {
                    if !vrf_match(vrf, name, v) {
                        continue;
                    }
                }
                w.octets(v);
            }
        }
        w.end();
        w.end();
    }
    w.end();
    w.end();
    w.end();
}

fn push_search_reference(w: &mut Writer, msgid: i64, e: &Entry) {
    w.start_seq();
    w.int(msgid);
    w.start(LDAP_RES_SEARCH_REFERENCE);
    if let Some(refs) = e.get(b"ref") {
        for r in refs {
            w.octets(r);
        }
    }
    w.end();
    w.end();
}

fn root_dse() -> Entry {
    let controls = [
        LDAP_CONTROL_MANAGEDSAIT,
        LDAP_CONTROL_PROXY_AUTHZ,
        LDAP_CONTROL_ASSERT,
        LDAP_CONTROL_PRE_READ,
        LDAP_CONTROL_POST_READ,
        LDAP_CONTROL_SORTREQUEST,
        LDAP_CONTROL_VLVREQUEST,
        LDAP_CONTROL_PAGEDRESULTS,
        LDAP_CONTROL_VALUESRETURNFILTER,
        LDAP_CONTROL_PASSWORDPOLICYREQUEST,
        LDAP_CONTROL_SUBENTRIES,
    ];
    let sc = controls.iter().map(|c| c.as_bytes().to_vec()).collect();
    Entry {
        dn: String::new(),
        attrs: vec![
            (
                b"objectClass".to_vec(),
                vec![b"top".to_vec(), b"OpenLDAProotDSE".to_vec()],
            ),
            (b"supportedControl".to_vec(), sc),
            (
                b"supportedExtension".to_vec(),
                vec![
                    LDAP_EXOP_START_TLS.as_bytes().to_vec(),
                    LDAP_EXOP_WHO_AM_I.as_bytes().to_vec(),
                    LDAP_EXOP_MODIFY_PASSWD.as_bytes().to_vec(),
                ],
            ),
            (b"namingContexts".to_vec(), vec![SUFFIX.as_bytes().to_vec()]),
        ],
    }
}

// ───────────────────────── add / delete / modify / moddn / compare ────────

fn read_dn_op(op: &[u8], tag: u32) -> Option<(Vec<u8>, Reader<'_>)> {
    let mut r = Reader::new(op);
    let mut body = r.enter(tag).ok()?;
    let dn = body.octets().ok()?.to_vec();
    Some((dn, body))
}

fn do_add(msgid: i64, op: &[u8], dit: &Arc<Mutex<Dit>>, controls: &[ReqControl]) -> Vec<u8> {
    let Some((dn, mut body)) = read_dn_op(op, LDAP_REQ_ADD) else {
        return ldap_result_msg(msgid, LDAP_RES_ADD, LDAP_PROTOCOL_ERROR, "", "");
    };
    if !dn_valid(&dn) {
        return ldap_result_msg(msgid, LDAP_RES_ADD, LDAP_INVALID_DN_SYNTAX, "", "");
    }
    // attributes
    let mut attrs: Vec<(Vec<u8>, Vec<Vec<u8>>)> = Vec::new();
    if let Ok(mut al) = body.seq() {
        while !al.is_empty() {
            let Ok(mut a) = al.seq() else { break };
            let Ok(name) = a.octets() else { break };
            if known_attr(name).is_none() {
                return ldap_result_msg(msgid, LDAP_RES_ADD, LDAP_UNDEFINED_TYPE, "", "");
            }
            let mut vals = Vec::new();
            if let Ok(mut set) = a.enter(TAG_SET) {
                while !set.is_empty() {
                    match set.octets() {
                        Ok(v) => vals.push(v.to_vec()),
                        Err(_) => break,
                    }
                }
            }
            attrs.push((name.to_vec(), vals));
        }
    }
    let nn = norm_dn(&dn).unwrap();
    let mut dit = dit.lock().unwrap();
    if dit.entries.contains_key(&nn) {
        return ldap_result_msg(msgid, LDAP_RES_ADD, LDAP_ALREADY_EXISTS, "", "");
    }
    // parent must exist unless this is the suffix
    if nn != norm_dn(SUFFIX.as_bytes()).unwrap() {
        if let Some(p) = parent_norm(&dn) {
            if !p.is_empty() && !dit.entries.contains_key(&p) {
                return ldap_result_msg(msgid, LDAP_RES_ADD, LDAP_NO_SUCH_OBJECT, "", "");
            }
        }
    }
    let pre = read_entry_control(controls, LDAP_CONTROL_PRE_READ);
    let entry = Entry {
        dn: String::from_utf8_lossy(&dn).into_owned(),
        attrs,
    };
    let post_src = entry.clone();
    dit.entries.insert(nn, entry);
    let mut resp = Vec::new();
    if pre.is_some() {
        // PRE_READ on add reads the (nonexistent) prior entry → empty value
        resp.push((
            LDAP_CONTROL_PRE_READ.as_bytes().to_vec(),
            build_readentry(&dn, &[]),
        ));
    }
    if let Some(attrs) = read_entry_control(controls, LDAP_CONTROL_POST_READ) {
        resp.push((
            LDAP_CONTROL_POST_READ.as_bytes().to_vec(),
            build_readentry(&dn, &selected_attrs(&post_src, &attrs)),
        ));
    }
    ldap_result_msg_ctrls(msgid, LDAP_RES_ADD, LDAP_SUCCESS, "", "", &resp)
}

fn do_delete(msgid: i64, op: &[u8], dit: &Arc<Mutex<Dit>>, controls: &[ReqControl]) -> Vec<u8> {
    // DelRequest is [APPLICATION 10] LDAPDN (primitive)
    let mut r = Reader::new(op);
    let Ok(dn) = r.expect(LDAP_REQ_DELETE) else {
        return ldap_result_msg(msgid, LDAP_RES_DELETE, LDAP_PROTOCOL_ERROR, "", "");
    };
    let dn = dn.to_vec();
    if !dn_valid(&dn) {
        return ldap_result_msg(msgid, LDAP_RES_DELETE, LDAP_INVALID_DN_SYNTAX, "", "");
    }
    let nn = norm_dn(&dn).unwrap();
    let mut dit = dit.lock().unwrap();
    let Some(entry) = dit.entries.get(&nn).cloned() else {
        return ldap_result_msg(msgid, LDAP_RES_DELETE, LDAP_NO_SUCH_OBJECT, "", "");
    };
    if !assertion_holds(controls, &entry) {
        return ldap_result_msg(msgid, LDAP_RES_DELETE, LDAP_ASSERTION_FAILED, "", "");
    }
    let pre = read_entry_control(controls, LDAP_CONTROL_PRE_READ);
    dit.entries.remove(&nn);
    let mut resp = Vec::new();
    if let Some(attrs) = pre {
        resp.push((
            LDAP_CONTROL_PRE_READ.as_bytes().to_vec(),
            build_readentry(&dn, &selected_attrs(&entry, &attrs)),
        ));
    }
    ldap_result_msg_ctrls(msgid, LDAP_RES_DELETE, LDAP_SUCCESS, "", "", &resp)
}

fn do_modify(msgid: i64, op: &[u8], dit: &Arc<Mutex<Dit>>, controls: &[ReqControl]) -> Vec<u8> {
    let Some((dn, mut body)) = read_dn_op(op, LDAP_REQ_MODIFY) else {
        return ldap_result_msg(msgid, LDAP_RES_MODIFY, LDAP_PROTOCOL_ERROR, "", "");
    };
    if !dn_valid(&dn) {
        return ldap_result_msg(msgid, LDAP_RES_MODIFY, LDAP_INVALID_DN_SYNTAX, "", "");
    }
    // modifications: SEQ OF { op ENUM, PartialAttribute { type, vals SET } }
    struct Mod {
        op: i64,
        attr: Vec<u8>,
        vals: Vec<Vec<u8>>,
    }
    let mut mods: Vec<Mod> = Vec::new();
    if let Ok(mut ml) = body.seq() {
        while !ml.is_empty() {
            let Ok(mut m) = ml.seq() else { break };
            let mop = m.enumerated().unwrap_or(0);
            let Ok(mut pa) = m.seq() else { break };
            let Ok(name) = pa.octets() else { break };
            if known_attr(name).is_none() {
                return ldap_result_msg(msgid, LDAP_RES_MODIFY, LDAP_UNDEFINED_TYPE, "", "");
            }
            let mut vals = Vec::new();
            if let Ok(mut set) = pa.enter(TAG_SET) {
                while !set.is_empty() {
                    match set.octets() {
                        Ok(v) => vals.push(v.to_vec()),
                        Err(_) => break,
                    }
                }
            }
            mods.push(Mod {
                op: mop,
                attr: name.to_vec(),
                vals,
            });
        }
    }
    let nn = norm_dn(&dn).unwrap();
    let mut dit = dit.lock().unwrap();
    if let Some(e) = dit.entries.get(&nn) {
        if !assertion_holds(controls, e) {
            return ldap_result_msg(msgid, LDAP_RES_MODIFY, LDAP_ASSERTION_FAILED, "", "");
        }
    }
    let before = dit.entries.get(&nn).cloned();
    let Some(entry) = dit.entries.get_mut(&nn) else {
        return ldap_result_msg(msgid, LDAP_RES_MODIFY, LDAP_NO_SUCH_OBJECT, "", "");
    };
    // RDN attribute(s) of this entry (cannot be removed by modify)
    let rdn_attrs = rdn_attr_values(&dn);
    for m in &mods {
        let single = known_attr(&m.attr) == Some(true);
        match m.op {
            x if x == LDAP_MOD_ADD as i64 => {
                if single && entry.get(&m.attr).is_some_and(|v| !v.is_empty()) {
                    return ldap_result_msg(
                        msgid,
                        LDAP_RES_MODIFY,
                        LDAP_TYPE_OR_VALUE_EXISTS,
                        "",
                        "",
                    );
                }
                // adding a duplicate value → typeOrValueExists
                if let Some(existing) = entry.get(&m.attr) {
                    for v in &m.vals {
                        if existing.iter().any(|e| e.eq_ignore_ascii_case(v)) {
                            return ldap_result_msg(
                                msgid,
                                LDAP_RES_MODIFY,
                                LDAP_TYPE_OR_VALUE_EXISTS,
                                "",
                                "",
                            );
                        }
                    }
                }
                match entry.get_mut(&m.attr) {
                    Some(v) => v.extend(m.vals.iter().cloned()),
                    None => entry.attrs.push((m.attr.clone(), m.vals.clone())),
                }
            }
            x if x == LDAP_MOD_REPLACE as i64 => {
                // replacing the RDN attribute with a value that drops the RDN
                // value is a naming violation.
                if let Some((_ra, rv)) = rdn_attrs
                    .iter()
                    .find(|(ra, _)| ra.eq_ignore_ascii_case(&m.attr))
                {
                    if !m.vals.iter().any(|v| v.eq_ignore_ascii_case(rv)) {
                        return ldap_result_msg(
                            msgid,
                            LDAP_RES_MODIFY,
                            LDAP_NAMING_VIOLATION,
                            "",
                            "",
                        );
                    }
                }
                if m.vals.is_empty() {
                    entry
                        .attrs
                        .retain(|(n, _)| !n.eq_ignore_ascii_case(&m.attr));
                } else {
                    // slapd implements replace as delete+add, moving the
                    // attribute to the end of the entry.
                    entry
                        .attrs
                        .retain(|(n, _)| !n.eq_ignore_ascii_case(&m.attr));
                    entry.attrs.push((m.attr.clone(), m.vals.clone()));
                }
            }
            x if x == LDAP_MOD_DELETE as i64 => {
                if m.vals.is_empty() {
                    entry
                        .attrs
                        .retain(|(n, _)| !n.eq_ignore_ascii_case(&m.attr));
                } else if let Some(v) = entry.get_mut(&m.attr) {
                    v.retain(|e| !m.vals.iter().any(|d| d.eq_ignore_ascii_case(e)));
                    // removing the last value removes the attribute
                    if v.is_empty() {
                        entry
                            .attrs
                            .retain(|(n, _)| !n.eq_ignore_ascii_case(&m.attr));
                    }
                }
            }
            _ => {}
        }
    }
    let after = entry.clone();
    drop(dit);
    let mut resp = Vec::new();
    if let Some(attrs) = read_entry_control(controls, LDAP_CONTROL_PRE_READ) {
        let sel = before
            .as_ref()
            .map(|b| selected_attrs(b, &attrs))
            .unwrap_or_default();
        resp.push((
            LDAP_CONTROL_PRE_READ.as_bytes().to_vec(),
            build_readentry(&dn, &sel),
        ));
    }
    if let Some(attrs) = read_entry_control(controls, LDAP_CONTROL_POST_READ) {
        resp.push((
            LDAP_CONTROL_POST_READ.as_bytes().to_vec(),
            build_readentry(&dn, &selected_attrs(&after, &attrs)),
        ));
    }
    ldap_result_msg_ctrls(msgid, LDAP_RES_MODIFY, LDAP_SUCCESS, "", "", &resp)
}

fn do_moddn(msgid: i64, op: &[u8], dit: &Arc<Mutex<Dit>>, controls: &[ReqControl]) -> Vec<u8> {
    let Some((dn, mut body)) = read_dn_op(op, LDAP_REQ_MODDN) else {
        return ldap_result_msg(msgid, LDAP_RES_MODDN, LDAP_PROTOCOL_ERROR, "", "");
    };
    let newrdn = body.octets().unwrap_or(b"").to_vec();
    let deleteold = body.boolean().unwrap_or(false);
    let newsuperior = body
        .optional(LDAP_TAG_NEWSUPERIOR)
        .ok()
        .flatten()
        .map(|v| v.to_vec());
    if !dn_valid(&dn) {
        return ldap_result_msg(msgid, LDAP_RES_MODDN, LDAP_INVALID_DN_SYNTAX, "", "");
    }
    let nn = norm_dn(&dn).unwrap();
    let mut dit = dit.lock().unwrap();
    let Some(mut entry) = dit.entries.get(&nn).cloned() else {
        return ldap_result_msg(msgid, LDAP_RES_MODDN, LDAP_NO_SUCH_OBJECT, "", "");
    };
    if !assertion_holds(controls, &entry) {
        return ldap_result_msg(msgid, LDAP_RES_MODDN, LDAP_ASSERTION_FAILED, "", "");
    }
    let pre = read_entry_control(controls, LDAP_CONTROL_PRE_READ)
        .map(|attrs| build_readentry(&dn, &selected_attrs(&entry, &attrs)));
    // compose the new DN
    let parent = match &newsuperior {
        Some(p) if !p.is_empty() => String::from_utf8_lossy(p).into_owned(),
        _ => parent_of(&entry.dn).unwrap_or_default(),
    };
    let newdn = if parent.is_empty() {
        String::from_utf8_lossy(&newrdn).into_owned()
    } else {
        format!("{},{}", String::from_utf8_lossy(&newrdn), parent)
    };
    // update the RDN attribute value
    if let Some(rdn) = goro_ldap::dn::str2dn(&newrdn) {
        if let Some(first) = rdn.first() {
            for ava in first {
                let old_rdn = rdn_attr_values(&dn);
                if deleteold {
                    if let Some((oa, ov)) = old_rdn.first() {
                        if let Some(v) = entry.get_mut(oa) {
                            v.retain(|x| !x.eq_ignore_ascii_case(ov));
                        }
                    }
                }
                match entry.get_mut(&ava.attr) {
                    Some(v) => {
                        if !v.iter().any(|x| x.eq_ignore_ascii_case(&ava.value)) {
                            v.push(ava.value.clone());
                        }
                    }
                    None => entry
                        .attrs
                        .push((ava.attr.clone(), vec![ava.value.clone()])),
                }
                // slapd re-appends the modified RDN attribute at the end.
                if let Some(pos) = entry
                    .attrs
                    .iter()
                    .position(|(n, _)| n.eq_ignore_ascii_case(&ava.attr))
                {
                    let moved = entry.attrs.remove(pos);
                    entry.attrs.push(moved);
                }
            }
        }
    }
    entry.dn = newdn.clone();
    let newnn = norm_dn(newdn.as_bytes()).unwrap();
    let post = read_entry_control(controls, LDAP_CONTROL_POST_READ)
        .map(|attrs| build_readentry(newdn.as_bytes(), &selected_attrs(&entry, &attrs)));
    dit.entries.remove(&nn);
    dit.entries.insert(newnn, entry);
    drop(dit);
    let mut resp = Vec::new();
    if let Some(p) = pre {
        resp.push((LDAP_CONTROL_PRE_READ.as_bytes().to_vec(), p));
    }
    if let Some(p) = post {
        resp.push((LDAP_CONTROL_POST_READ.as_bytes().to_vec(), p));
    }
    ldap_result_msg_ctrls(msgid, LDAP_RES_MODDN, LDAP_SUCCESS, "", "", &resp)
}

fn do_compare(msgid: i64, op: &[u8], dit: &Arc<Mutex<Dit>>, controls: &[ReqControl]) -> Vec<u8> {
    let mut r = Reader::new(op);
    let Ok(mut body) = r.enter(LDAP_REQ_COMPARE) else {
        return ldap_result_msg(msgid, LDAP_RES_COMPARE, LDAP_PROTOCOL_ERROR, "", "");
    };
    let dn = body.octets().unwrap_or(b"").to_vec();
    let (attr, value) = match body.seq() {
        Ok(mut ava) => {
            let a = ava.octets().unwrap_or(b"").to_vec();
            let v = ava.octets().unwrap_or(b"").to_vec();
            (a, v)
        }
        Err(_) => return ldap_result_msg(msgid, LDAP_RES_COMPARE, LDAP_PROTOCOL_ERROR, "", ""),
    };
    if !dn_valid(&dn) {
        return ldap_result_msg(msgid, LDAP_RES_COMPARE, LDAP_INVALID_DN_SYNTAX, "", "");
    }
    let nn = norm_dn(&dn).unwrap();
    let dit = dit.lock().unwrap();
    let Some(entry) = dit.entries.get(&nn) else {
        return ldap_result_msg(msgid, LDAP_RES_COMPARE, LDAP_NO_SUCH_OBJECT, "", "");
    };
    if !assertion_holds(controls, entry) {
        return ldap_result_msg(msgid, LDAP_RES_COMPARE, LDAP_ASSERTION_FAILED, "", "");
    }
    let matched = entry
        .get(&attr)
        .is_some_and(|v| v.iter().any(|x| x.eq_ignore_ascii_case(&value)));
    let code = if matched {
        LDAP_COMPARE_TRUE
    } else {
        LDAP_COMPARE_FALSE
    };
    ldap_result_msg(msgid, LDAP_RES_COMPARE, code, "", "")
}

// ───────────────────────── extended operations ─────────────────────────

fn do_extended(
    msgid: i64,
    op: &[u8],
    session: &mut Session,
    dit: &Arc<Mutex<Dit>>,
    controls: &[ReqControl],
) -> DispatchResult {
    let mut r = Reader::new(op);
    let Ok(mut body) = r.enter(LDAP_REQ_EXTENDED) else {
        return DispatchResult::Reply(ldap_result_msg(
            msgid,
            LDAP_RES_EXTENDED,
            LDAP_PROTOCOL_ERROR,
            "",
            "",
        ));
    };
    let oid = body.expect(LDAP_TAG_EXOP_REQ_OID).unwrap_or(b"").to_vec();
    let value = body
        .optional(LDAP_TAG_EXOP_REQ_VALUE)
        .ok()
        .flatten()
        .map(|v| v.to_vec());

    if oid == LDAP_EXOP_START_TLS.as_bytes() {
        let reply = ext_response(
            msgid,
            LDAP_SUCCESS,
            Some(LDAP_EXOP_START_TLS.as_bytes()),
            None,
        );
        return DispatchResult::StartTls(reply);
    }
    if oid == LDAP_EXOP_WHO_AM_I.as_bytes() {
        // proxied authorization overrides the identity
        let who = if let Some(c) = find_ctrl(controls, LDAP_CONTROL_PROXY_AUTHZ) {
            String::from_utf8_lossy(&c.value).into_owned()
        } else {
            match &session.bound_dn {
                Some(dn) => format!("dn:{}", original_dn(dit, dn)),
                None => String::new(),
            }
        };
        return DispatchResult::Reply(ext_response(
            msgid,
            LDAP_SUCCESS,
            None,
            Some(who.as_bytes()),
        ));
    }
    if oid == LDAP_EXOP_MODIFY_PASSWD.as_bytes() {
        return DispatchResult::Reply(do_passwd(msgid, value.as_deref(), dit));
    }
    DispatchResult::Reply(ext_response(msgid, LDAP_PROTOCOL_ERROR, None, None))
}

fn original_dn(dit: &Arc<Mutex<Dit>>, norm: &str) -> String {
    if norm == norm_dn(ROOTDN.as_bytes()).unwrap() {
        return ROOTDN.to_string();
    }
    dit.lock()
        .unwrap()
        .entries
        .get(norm)
        .map(|e| e.dn.clone())
        .unwrap_or_else(|| norm.to_string())
}

fn do_passwd(msgid: i64, value: Option<&[u8]>, dit: &Arc<Mutex<Dit>>) -> Vec<u8> {
    // PasswdModifyRequestValue ::= SEQUENCE { id [0] OCTET OPTIONAL,
    //   old [1] OCTET OPTIONAL, new [2] OCTET OPTIONAL }
    let (mut user, mut old, mut newp) = (Vec::new(), Vec::new(), Vec::new());
    if let Some(v) = value {
        let mut r = Reader::new(v);
        if let Ok(mut s) = r.seq() {
            while !s.is_empty() {
                match s.peek_tag() {
                    Some(t) if t == LDAP_TAG_EXOP_MODIFY_PASSWD_ID => {
                        user = s.expect(t).unwrap_or(b"").to_vec()
                    }
                    Some(t) if t == LDAP_TAG_EXOP_MODIFY_PASSWD_OLD => {
                        old = s.expect(t).unwrap_or(b"").to_vec()
                    }
                    Some(t) if t == LDAP_TAG_EXOP_MODIFY_PASSWD_NEW => {
                        newp = s.expect(t).unwrap_or(b"").to_vec()
                    }
                    _ => {
                        let _ = s.read_raw();
                    }
                }
            }
        }
    }
    if user.is_empty() {
        return ext_response(msgid, LDAP_UNWILLING_TO_PERFORM, None, None);
    }
    let Some(nn) = norm_dn(&user) else {
        return ext_response(msgid, LDAP_INVALID_DN_SYNTAX, None, None);
    };
    let mut dit = dit.lock().unwrap();
    let Some(entry) = dit.entries.get_mut(&nn) else {
        return ext_response(msgid, LDAP_NO_SUCH_OBJECT, None, None);
    };
    // verify old password when provided
    if !old.is_empty() {
        let ok = entry
            .get(b"userpassword")
            .is_some_and(|v| v.iter().any(|p| p == &old));
        if !ok {
            return ext_response(msgid, LDAP_UNWILLING_TO_PERFORM, None, None);
        }
    }
    if newp.is_empty() {
        // generate a password and return it
        let genpw = gen_password();
        set_userpassword(entry, &genpw);
        let mut w = Writer::new();
        w.start_seq();
        w.raw(LDAP_TAG_EXOP_MODIFY_PASSWD_GEN, &genpw);
        w.end();
        let respval = w.finish();
        ext_response(msgid, LDAP_SUCCESS, None, Some(&respval))
    } else {
        set_userpassword(entry, &newp);
        ext_response(msgid, LDAP_SUCCESS, None, None)
    }
}

fn set_userpassword(entry: &mut Entry, pw: &[u8]) {
    match entry.get_mut(b"userpassword") {
        Some(v) => *v = vec![pw.to_vec()],
        None => entry
            .attrs
            .push((b"userPassword".to_vec(), vec![pw.to_vec()])),
    }
}

fn gen_password() -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{{GEN}}{:016x}", n as u64).into_bytes()
}

/// ExtendedResponse [APPLICATION 24].
fn ext_response(msgid: i64, code: i32, oid: Option<&[u8]>, value: Option<&[u8]>) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(msgid);
    w.start(LDAP_RES_EXTENDED);
    w.enumerated(code as i64);
    w.octets(b""); // matchedDN
    w.octets(b""); // errorMessage
    if let Some(o) = oid {
        w.raw(LDAP_TAG_EXOP_RES_OID, o);
    }
    if let Some(v) = value {
        w.raw(LDAP_TAG_EXOP_RES_VALUE, v);
    }
    w.end();
    w.end();
    w.finish()
}

// ───────────────────────── control helpers ─────────────────────────

/// Read a pre/post-read control: its requested attribute list.
fn read_entry_control(controls: &[ReqControl], oid: &str) -> Option<Vec<Vec<u8>>> {
    let c = find_ctrl(controls, oid)?;
    // value is a SEQUENCE OF OCTET STRING
    let mut r = Reader::new(&c.value);
    let mut attrs = Vec::new();
    if let Ok(mut s) = r.seq() {
        while !s.is_empty() {
            match s.octets() {
                Ok(a) => attrs.push(a.to_vec()),
                Err(_) => break,
            }
        }
    }
    Some(attrs)
}

fn selected_attrs(entry: &Entry, want: &[Vec<u8>]) -> Vec<(Vec<u8>, Vec<Vec<u8>>)> {
    let all = want.is_empty() || want.iter().any(|a| a == b"*");
    entry
        .attrs
        .iter()
        .filter(|(n, _)| all || want.iter().any(|a| a.eq_ignore_ascii_case(n)))
        .cloned()
        .collect()
}

/// Build a ReadEntryResponse control value: SEQ { OCTET dn, SEQ OF
/// PartialAttribute }.
fn build_readentry(dn: &[u8], attrs: &[(Vec<u8>, Vec<Vec<u8>>)]) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.octets(dn);
    w.start_seq();
    for (name, vals) in attrs {
        w.start_seq();
        w.octets(name);
        w.start_set();
        for v in vals {
            w.octets(v);
        }
        w.end();
        w.end();
    }
    w.end();
    w.end();
    w.finish()
}

fn build_paged_resp(estimate: i64, cookie: &[u8]) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(estimate);
    w.octets(cookie);
    w.end();
    w.finish()
}

fn build_sort_resp(code: i64) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.enumerated(code);
    w.end();
    w.finish()
}

fn build_vlv_resp(target: i64, count: i64) -> Vec<u8> {
    let mut w = Writer::new();
    w.start_seq();
    w.int(target);
    w.int(count);
    w.enumerated(0); // virtualListViewResult: success
    w.raw(LDAP_VLVCONTEXT_IDENTIFIER, b"ctx");
    w.end();
    w.finish()
}

fn parse_paged_req(val: &[u8]) -> Option<(i64, Vec<u8>)> {
    let mut r = Reader::new(val);
    let mut s = r.seq().ok()?;
    let size = s.int().ok()?;
    let cookie = s.octets().ok()?.to_vec();
    Some((size, cookie))
}

fn parse_sort_req(val: &[u8]) -> Option<(Vec<u8>, bool)> {
    // SEQUENCE OF SortKey { attr OCTET, [0] matchingrule OCTET OPT, [1] reverse BOOL OPT }
    let mut r = Reader::new(val);
    let mut list = r.seq().ok()?;
    let mut key = list.seq().ok()?;
    let attr = key.octets().ok()?.to_vec();
    let mut reverse = false;
    while !key.is_empty() {
        match key.peek_tag() {
            Some(t) if t == LDAP_REVERSEORDER_IDENTIFIER => {
                reverse = key.bool_tagged(t).unwrap_or(false)
            }
            Some(t) => {
                let _ = key.expect(t);
            }
            None => break,
        }
    }
    Some((attr, reverse))
}

fn parse_vlv_req(val: &[u8]) -> Option<(i64, i64, i64, i64)> {
    // SEQUENCE { beforeCount INT, afterCount INT, target CHOICE {
    //   byoffset [0] SEQ { offset INT, contentCount INT }, ... }, context OCTET OPT }
    let mut r = Reader::new(val);
    let mut s = r.seq().ok()?;
    let before = s.int().ok()?;
    let after = s.int().ok()?;
    let tag = s.peek_tag()?;
    if tag == LDAP_VLVBYINDEX_IDENTIFIER {
        let mut byidx = s.enter(tag).ok()?;
        let offset = byidx.int().ok()?;
        let count = byidx.int().ok()?;
        Some((before, after, offset, count))
    } else {
        // by-value: not used by the tests
        None
    }
}

// ───────────────────────── filter evaluation ─────────────────────────

fn eval_filter(filter: &[u8], e: &Entry) -> bool {
    let (tag, h, l) = match ber::parse_header(filter) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let content = &filter[h..(h + l).min(filter.len())];
    match tag {
        LDAP_FILTER_AND => {
            let mut r = Reader::new(content);
            while !r.is_empty() {
                let Ok(raw) = r.read_raw() else { return false };
                if !eval_filter(raw, e) {
                    return false;
                }
            }
            true
        }
        LDAP_FILTER_OR => {
            let mut r = Reader::new(content);
            let mut any = false;
            while !r.is_empty() {
                let Ok(raw) = r.read_raw() else { break };
                if eval_filter(raw, e) {
                    any = true;
                }
            }
            any
        }
        LDAP_FILTER_NOT => {
            let mut r = Reader::new(content);
            match r.read_raw() {
                Ok(raw) => !eval_filter(raw, e),
                Err(_) => false,
            }
        }
        LDAP_FILTER_PRESENT => {
            // content is the attribute description (primitive)
            e.get(content).is_some()
        }
        LDAP_FILTER_EQUALITY | LDAP_FILTER_APPROX => {
            let mut r = Reader::new(content);
            let Ok(attr) = r.octets() else { return false };
            let Ok(val) = r.octets() else { return false };
            e.get(attr)
                .is_some_and(|vs| vs.iter().any(|v| v.eq_ignore_ascii_case(val)))
        }
        LDAP_FILTER_GE => cmp_filter(content, e, |o| o != std::cmp::Ordering::Less),
        LDAP_FILTER_LE => cmp_filter(content, e, |o| o != std::cmp::Ordering::Greater),
        LDAP_FILTER_SUBSTRINGS => {
            let mut r = Reader::new(content);
            let Ok(attr) = r.octets() else { return false };
            let Ok(mut subs) = r.seq() else { return false };
            let mut initial: Option<Vec<u8>> = None;
            let mut anys: Vec<Vec<u8>> = Vec::new();
            let mut fin: Option<Vec<u8>> = None;
            while !subs.is_empty() {
                match subs.peek_tag() {
                    Some(t) if t == LDAP_SUBSTRING_INITIAL => {
                        initial = subs.expect(t).ok().map(|v| v.to_vec())
                    }
                    Some(t) if t == LDAP_SUBSTRING_ANY => {
                        if let Ok(v) = subs.expect(t) {
                            anys.push(v.to_vec());
                        }
                    }
                    Some(t) if t == LDAP_SUBSTRING_FINAL => {
                        fin = subs.expect(t).ok().map(|v| v.to_vec())
                    }
                    Some(t) => {
                        let _ = subs.expect(t);
                    }
                    None => break,
                }
            }
            e.get(attr).is_some_and(|vs| {
                vs.iter()
                    .any(|v| substr_match(v, initial.as_deref(), &anys, fin.as_deref()))
            })
        }
        _ => false,
    }
}

fn cmp_filter(content: &[u8], e: &Entry, keep: impl Fn(std::cmp::Ordering) -> bool) -> bool {
    let mut r = Reader::new(content);
    let Ok(attr) = r.octets() else { return false };
    let Ok(val) = r.octets() else { return false };
    e.get(attr).is_some_and(|vs| {
        vs.iter()
            .any(|v| keep(v.to_ascii_lowercase().cmp(&val.to_ascii_lowercase())))
    })
}

fn substr_match(
    value: &[u8],
    initial: Option<&[u8]>,
    anys: &[Vec<u8>],
    fin: Option<&[u8]>,
) -> bool {
    let hay = value.to_ascii_lowercase();
    let mut pos = 0usize;
    if let Some(i) = initial {
        let i = i.to_ascii_lowercase();
        if !hay[pos..].starts_with(&i[..]) {
            return false;
        }
        pos += i.len();
    }
    for a in anys {
        let a = a.to_ascii_lowercase();
        match find_sub(&hay[pos..], &a) {
            Some(idx) => pos += idx + a.len(),
            None => return false,
        }
    }
    if let Some(f) = fin {
        let f = f.to_ascii_lowercase();
        if hay.len() < pos + f.len() || !hay.ends_with(&f[..]) {
            return false;
        }
    }
    true
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The RDN attribute=value pairs of a DN's first RDN.
fn rdn_attr_values(dn: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    match goro_ldap::dn::str2dn(dn) {
        Some(parsed) => parsed
            .first()
            .map(|rdn| {
                rdn.iter()
                    .map(|ava| (ava.attr.clone(), ava.value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// ValuesReturnFilter: keep value if it matches any simple item for `attr`.
fn vrf_match(vrf: &[u8], attr: &[u8], value: &[u8]) -> bool {
    // vrf is a SEQUENCE OF SimpleFilterItem (each like a Filter choice).
    let mut r = Reader::new(vrf);
    let Ok(mut seq) = r.seq() else { return true };
    let mut applies = false;
    let mut matched = false;
    while !seq.is_empty() {
        let Ok(item) = seq.read_raw() else { break };
        // peek the item's attribute to see if it targets this attr
        if let Some((item_attr, _)) = vrf_item_attr(item) {
            if item_attr.eq_ignore_ascii_case(attr) {
                applies = true;
                let tmp = Entry {
                    dn: String::new(),
                    attrs: vec![(attr.to_vec(), vec![value.to_vec()])],
                };
                if eval_filter(item, &tmp) {
                    matched = true;
                }
            }
        }
    }
    if applies { matched } else { true }
}

/// The attribute a vrFilter simple item targets.
fn vrf_item_attr(item: &[u8]) -> Option<(Vec<u8>, ())> {
    let (tag, h, l) = ber::parse_header(item).ok()?;
    let content = &item[h..(h + l).min(item.len())];
    match tag {
        LDAP_FILTER_PRESENT => Some((content.to_vec(), ())),
        LDAP_FILTER_EQUALITY
        | LDAP_FILTER_GE
        | LDAP_FILTER_LE
        | LDAP_FILTER_APPROX
        | LDAP_FILTER_SUBSTRINGS => {
            let mut r = Reader::new(content);
            let attr = r.octets().ok()?;
            Some((attr.to_vec(), ()))
        }
        _ => None,
    }
}
