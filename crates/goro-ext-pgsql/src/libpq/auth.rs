//! Client authentication (fe-auth.c, fe-auth-scram.c): cleartext and MD5
//! passwords, SASL SCRAM-SHA-256, and PQencryptPasswordConn.

use purecrypto::hash::{HmacSha256, Sha256};

use super::conn::{PgConn, ScramState};

const AUTH_REQ_KRB4: i32 = 1;
const AUTH_REQ_KRB5: i32 = 2;
const AUTH_REQ_PASSWORD: i32 = 3;
const AUTH_REQ_CRYPT: i32 = 4;
const AUTH_REQ_MD5: i32 = 5;
const AUTH_REQ_GSS: i32 = 7;
const AUTH_REQ_GSS_CONT: i32 = 8;
const AUTH_REQ_SSPI: i32 = 9;
const AUTH_REQ_SASL: i32 = 10;
const AUTH_REQ_SASL_CONT: i32 = 11;
const AUTH_REQ_SASL_FIN: i32 = 12;

const NO_PASSWORD: &str = "fe_sendauth: no password supplied";

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let b = [ch[0], *ch.get(1).unwrap_or(&0), *ch.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if ch.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if ch.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = B64.iter().position(|&x| x == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

fn md5_hex(data: &[u8]) -> String {
    let d = purecrypto::hash::md5(data);
    d.iter().map(|b| format!("{:02x}", b)).collect()
}

/// pg_md5_encrypt: "md5" + md5(passwd + salt)
pub fn md5_encrypt(passwd: &[u8], salt: &[u8]) -> String {
    let mut v = passwd.to_vec();
    v.extend_from_slice(salt);
    format!("md5{}", md5_hex(&v))
}

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let t = HmacSha256::mac(key, data);
    let mut out = [0u8; 32];
    out.copy_from_slice(t.as_ref());
    out
}

fn sha256(data: &[u8]) -> [u8; 32] {
    purecrypto::hash::sha256(data)
}

fn salted_password(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    purecrypto::kdf::pbkdf2::<Sha256>(password, salt, iterations, &mut out);
    out
}

fn random_bytes(n: usize) -> Vec<u8> {
    use purecrypto::rng::RngCore;
    let mut v = vec![0u8; n];
    purecrypto::rng::OsRng.fill_bytes(&mut v);
    v
}

/// The password to use (conn->connhost[].password / pgpass)
fn password(conn: &PgConn) -> Option<Vec<u8>> {
    match conn.opts.get("password") {
        Some(p) if !p.is_empty() => Some(p.as_bytes().to_vec()),
        _ => None,
    }
}

/// pg_fe_sendauth: answer an authentication request
pub fn send_auth(conn: &mut PgConn, areq: i32, data: &[u8]) -> bool {
    match areq {
        AUTH_REQ_KRB4 | AUTH_REQ_KRB5 => {
            conn.append_error("Kerberos 5 authentication not supported");
            false
        }
        AUTH_REQ_GSS | AUTH_REQ_GSS_CONT => {
            conn.append_error("GSSAPI authentication not supported");
            false
        }
        AUTH_REQ_SSPI => {
            conn.append_error("SSPI authentication not supported");
            false
        }
        AUTH_REQ_CRYPT => {
            conn.append_error("Crypt authentication not supported");
            false
        }
        AUTH_REQ_PASSWORD | AUTH_REQ_MD5 => {
            conn.password_needed = true;
            let Some(pw) = password(conn) else {
                conn.append_error(NO_PASSWORD);
                return false;
            };
            let msg = if areq == AUTH_REQ_MD5 {
                let salt = &data[..4.min(data.len())];
                let user = conn.user().as_bytes().to_vec();
                let inner = md5_encrypt(&pw, &user);
                md5_encrypt(&inner.as_bytes()[3..], salt).into_bytes()
            } else {
                pw
            };
            let mut body = msg;
            body.push(0);
            conn.put_msg(b'p', &body);
            true
        }
        AUTH_REQ_SASL => sasl_start(conn, data),
        AUTH_REQ_SASL_CONT | AUTH_REQ_SASL_FIN => {
            sasl_continue(conn, data, areq == AUTH_REQ_SASL_FIN)
        }
        _ => {
            conn.append_error(&format!("authentication method {} not supported", areq));
            false
        }
    }
}

/// pg_SASL_init
fn sasl_start(conn: &mut PgConn, data: &[u8]) -> bool {
    if conn.sasl.is_some() {
        conn.append_error("duplicate SASL authentication request");
        return false;
    }
    // the mechanism list: NUL-terminated names, then an empty one
    let mut mechs = Vec::new();
    for m in data.split(|&c| c == 0) {
        if m.is_empty() {
            break;
        }
        mechs.push(String::from_utf8_lossy(m).into_owned());
    }
    let channel_binding = conn
        .opts
        .get("channel_binding")
        .unwrap_or("prefer")
        .to_string();
    if !mechs.iter().any(|m| m == "SCRAM-SHA-256") {
        if mechs.iter().any(|m| m == "SCRAM-SHA-256-PLUS") {
            conn.append_error("channel binding is required, but client does not support it");
        } else {
            conn.append_error("none of the server's SASL authentication mechanisms are supported");
        }
        return false;
    }
    if channel_binding == "require" {
        conn.append_error(
            "channel binding is required, but server did not offer an authentication method that supports channel binding",
        );
        return false;
    }
    conn.password_needed = true;
    if password(conn).is_none() {
        conn.append_error(NO_PASSWORD);
        return false;
    }
    let nonce = b64_encode(&random_bytes(18));
    let bare = format!("n=,r={}", nonce);
    let first = format!("n,,{}", bare);
    conn.sasl = Some(ScramState {
        client_nonce: nonce,
        client_first_bare: bare,
        server_first: String::new(),
        salted_password: [0; 32],
        auth_message: String::new(),
        done: false,
    });
    let mut body = b"SCRAM-SHA-256\0".to_vec();
    body.extend_from_slice(&(first.len() as i32).to_be_bytes());
    body.extend_from_slice(first.as_bytes());
    conn.put_msg(b'p', &body);
    true
}

/// read_attr_value: the value of "x=" at the start of `s`
fn attr(s: &str, name: char) -> Option<&str> {
    for part in s.split(',') {
        if let Some(v) = part.strip_prefix(name) {
            if let Some(v) = v.strip_prefix('=') {
                return Some(v);
            }
        }
    }
    None
}

/// pg_SASL_continue: the server-first and server-final messages
fn sasl_continue(conn: &mut PgConn, data: &[u8], final_msg: bool) -> bool {
    let Some(mut st) = conn.sasl.take() else {
        conn.append_error("fe_sendauth: invalid authentication request from server: AUTH_REQ_SASL_CONT without AUTH_REQ_SASL");
        return false;
    };
    let msg = String::from_utf8_lossy(data).into_owned();
    if st.server_first.is_empty() {
        if final_msg {
            conn.append_error("AuthenticationSASLFinal received from server, but SASL authentication was not completed");
            return false;
        }
        // server-first-message: r=<nonce>,s=<salt>,i=<iterations>
        let Some(r) = attr(&msg, 'r') else {
            conn.append_error("malformed SCRAM message (attribute \"r\" expected)");
            return false;
        };
        if !r.starts_with(&st.client_nonce) {
            conn.append_error("invalid SCRAM response (nonce mismatch)");
            return false;
        }
        let Some(salt) = attr(&msg, 's').and_then(b64_decode) else {
            conn.append_error("malformed SCRAM message (invalid salt)");
            return false;
        };
        let Some(iters) = attr(&msg, 'i').and_then(|i| i.parse::<u32>().ok()) else {
            conn.append_error("malformed SCRAM message (invalid iteration count)");
            return false;
        };
        let pw = password(conn).unwrap_or_default();
        st.salted_password = salted_password(&pw, &salt, iters.max(1));
        st.server_first = msg.clone();
        let without_proof = format!("c=biws,r={}", r);
        st.auth_message = format!(
            "{},{},{}",
            st.client_first_bare, st.server_first, without_proof
        );
        let client_key = hmac(&st.salted_password, b"Client Key");
        let stored_key = sha256(&client_key);
        let sig = hmac(&stored_key, st.auth_message.as_bytes());
        let proof: Vec<u8> = client_key
            .iter()
            .zip(sig.iter())
            .map(|(a, b)| a ^ b)
            .collect();
        let fin = format!("{},p={}", without_proof, b64_encode(&proof));
        conn.put_msg(b'p', fin.as_bytes());
        conn.sasl = Some(st);
        return true;
    }
    if !final_msg {
        conn.append_error("malformed SCRAM message (unexpected server message)");
        return false;
    }
    if let Some(e) = attr(&msg, 'e') {
        conn.append_error(&format!(
            "error received from server in SCRAM exchange: {}",
            e
        ));
        return false;
    }
    let Some(v) = attr(&msg, 'v').and_then(b64_decode) else {
        conn.append_error("malformed SCRAM message (invalid server signature)");
        return false;
    };
    let server_key = hmac(&st.salted_password, b"Server Key");
    let expected = hmac(&server_key, st.auth_message.as_bytes());
    if v != expected {
        conn.append_error("incorrect server signature");
        return false;
    }
    st.done = true;
    conn.sasl = Some(st);
    true
}

/// pg_fe_scram_build_secret: SCRAM-SHA-256$<iter>:<salt>$<StoredKey>:<ServerKey>
pub fn scram_build_secret(password: &[u8]) -> String {
    let iterations = 4096;
    let salt = random_bytes(16);
    let salted = salted_password(password, &salt, iterations);
    let client_key = hmac(&salted, b"Client Key");
    let stored = sha256(&client_key);
    let server_key = hmac(&salted, b"Server Key");
    format!(
        "SCRAM-SHA-256${}:{}${}:{}",
        iterations,
        b64_encode(&salt),
        b64_encode(&stored),
        b64_encode(&server_key)
    )
}

/// PQencryptPasswordConn: the algorithm defaults to the server's
/// password_encryption setting (asked with SHOW).
pub fn encrypt_password_conn(
    conn: &mut PgConn,
    passwd: &[u8],
    user: &[u8],
    algorithm: Option<&str>,
) -> Option<String> {
    let algo = match algorithm {
        Some(a) => a.to_string(),
        None => {
            let r = conn.exec(b"show password_encryption");
            match r {
                Some(r) if r.status == super::ExecStatus::TuplesOk && r.ntuples() == 1 => {
                    String::from_utf8_lossy(r.value(0, 0)).into_owned()
                }
                _ => return None,
            }
        }
    };
    match algo.as_str() {
        "on" | "md5" => Some(md5_encrypt(passwd, user)),
        "scram-sha-256" => Some(scram_build_secret(passwd)),
        other => {
            conn.append_error(&format!(
                "unrecognized password encryption algorithm \"{}\"",
                other
            ));
            None
        }
    }
}
