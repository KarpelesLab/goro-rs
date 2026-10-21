//! Authentication (mysqlnd_auth.c): the handshake/change-user driver and
//! the built-in plugins (mysql_native_password, mysql_clear_password,
//! sha256_password, caching_sha2_password).

use super::charset;
use super::conn::Conn;
use super::*;

const MYSQLND_OLD_PASSWD: &str = "mysqlnd cannot connect to MySQL 4.1+ using the old insecure authentication. Please use an administration tool to reset your password with the command SET PASSWORD = PASSWORD('your_existing_password'). This will store a new, and more secure, hash value in mysql.user. If this user is used in other scripts executed by PHP 5.2 or earlier you might need to remove the old-passwords flag from your my.cnf file";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Plugin {
    Native,
    Clear,
    Sha256,
    CachingSha2,
}

fn find_plugin(name: &str) -> Option<Plugin> {
    match name {
        "mysql_native_password" => Some(Plugin::Native),
        "mysql_clear_password" => Some(Plugin::Clear),
        "sha256_password" => Some(Plugin::Sha256),
        "caching_sha2_password" => Some(Plugin::CachingSha2),
        _ => None,
    }
}

/// The outcome of one authentication round.
struct Switch {
    protocol: Option<Vec<u8>>,
    data: Option<Vec<u8>>,
}

/// mysqlnd_run_authentication
#[allow(clippy::too_many_arguments)]
pub fn run_authentication(
    conn: &mut Conn,
    user: &[u8],
    passwd: &[u8],
    db: &[u8],
    auth_plugin_data: &[u8],
    auth_protocol: Option<&str>,
    charset_no: u32,
    _server_capabilities: u32,
    silent: bool,
    is_change_user: bool,
    mysql_flags: u32,
) -> bool {
    let mut ret;
    let mut first_call = true;
    let mut plugin_data = auth_plugin_data.to_vec();
    let mut requested_protocol = auth_protocol
        .unwrap_or(MYSQLND_DEFAULT_AUTH_PROTOCOL)
        .to_string();
    loop {
        let mut plugin = find_plugin(&requested_protocol);
        if plugin.is_none() {
            if first_call {
                requested_protocol = MYSQLND_DEFAULT_AUTH_PROTOCOL.to_string();
                plugin = None;
            } else {
                let msg = format!(
                    "The server requested authentication method unknown to the client [{}]",
                    requested_protocol
                );
                conn.error_info
                    .set_str(CR_NOT_IMPLEMENTED, UNKNOWN_SQLSTATE, &msg);
                return false;
            }
        }
        conn.authentication_plugin_data = plugin_data.clone();
        let scrambled = match plugin {
            Some(p) => get_auth_data(p, conn, passwd, &plugin_data),
            None => None,
        };
        if conn.error_info.error_no != 0 {
            return false;
        }
        let mut sw = Switch {
            protocol: None,
            data: None,
        };
        let scrambled = scrambled.unwrap_or_default();
        ret = if !is_change_user {
            auth_handshake(
                conn,
                user,
                passwd,
                db,
                mysql_flags,
                charset_no,
                first_call,
                &requested_protocol,
                plugin,
                &plugin_data,
                &scrambled,
                &mut sw,
            )
        } else {
            auth_change_user(
                conn,
                user,
                passwd,
                db,
                silent,
                first_call,
                &requested_protocol,
                plugin,
                &plugin_data,
                &scrambled,
                &mut sw,
            )
        };
        first_call = false;
        let switched = sw.protocol.is_some();
        if let Some(p) = sw.protocol {
            requested_protocol = String::from_utf8_lossy(&p).into_owned();
        }
        plugin_data = sw.data.unwrap_or_default();
        if !(!ret && conn.error_info.error_no == 0 && switched) {
            break;
        }
    }
    if ret {
        conn.options.auth_protocol = Some(requested_protocol);
    }
    ret
}

/// mysqlnd_command::enable_ssl (via mysqlnd_switch_to_ssl_if_needed)
fn enable_ssl(
    conn: &mut Conn,
    client_capabilities: u32,
    server_capabilities: u32,
    charset_no: u32,
) -> bool {
    if client_capabilities & CLIENT_SSL == 0 {
        return true;
    }
    let close_conn = |conn: &mut Conn| {
        conn.state = ConnState::QuitSent;
        conn.send_close();
        conn.error_info
            .set_str(CR_SERVER_GONE_ERROR, UNKNOWN_SQLSTATE, MYSQLND_SERVER_GONE);
        false
    };
    if server_capabilities & CLIENT_SSL == 0 {
        return close_conn(conn);
    }
    // the auth packet without auth data: the SSL request
    let mut p = Vec::new();
    int4store(&mut p, client_capabilities);
    int4store(&mut p, MYSQLND_ASSEMBLED_PACKET_MAX_SIZE);
    p.push(charset_no as u8);
    p.extend_from_slice(&[0u8; 23]);
    let sent = conn
        .pfc
        .send(&mut conn.vio, &p, &mut conn.stats, &mut conn.error_info);
    if sent == 0 {
        return close_conn(conn);
    }
    conn.vio.ssl_verify_peer = if client_capabilities & CLIENT_SSL_VERIFY_SERVER_CERT != 0 {
        super::vio::SslPeer::Verify
    } else if client_capabilities & CLIENT_SSL_DONT_VERIFY_SERVER_CERT != 0 {
        super::vio::SslPeer::DontVerify
    } else {
        super::vio::SslPeer::Default
    };
    if !conn.vio.enable_ssl() {
        conn.state = ConnState::QuitSent;
        conn.error_info.set_str(
            CR_CONNECTION_ERROR,
            UNKNOWN_SQLSTATE,
            "Cannot connect to MySQL using SSL",
        );
        return false;
    }
    true
}

/// mysqlnd_connect_run_authentication
#[allow(clippy::too_many_arguments)]
pub fn connect_run_authentication(
    conn: &mut Conn,
    user: &[u8],
    passwd: &[u8],
    db: &[u8],
    auth_plugin_data: &[u8],
    auth_protocol: Option<&str>,
    charset_no: u32,
    server_capabilities: u32,
    mysql_flags: u32,
) -> bool {
    if !enable_ssl(conn, mysql_flags, server_capabilities, charset_no) {
        return false;
    }
    run_authentication(
        conn,
        user,
        passwd,
        db,
        auth_plugin_data,
        auth_protocol,
        charset_no,
        server_capabilities,
        false,
        false,
        mysql_flags,
    )
}

/// mysqlnd_auth_handshake
#[allow(clippy::too_many_arguments)]
fn auth_handshake(
    conn: &mut Conn,
    user: &[u8],
    passwd: &[u8],
    db: &[u8],
    mysql_flags: u32,
    server_charset_no: u32,
    use_full_blown_auth_packet: bool,
    auth_protocol: &str,
    plugin: Option<Plugin>,
    orig_auth_plugin_data: &[u8],
    auth_plugin_data: &[u8],
    sw: &mut Switch,
) -> bool {
    if !use_full_blown_auth_packet {
        if conn.write_change_auth_response(auth_plugin_data) == 0 {
            conn.state = ConnState::QuitSent;
            conn.error_info
                .set_str(CR_SERVER_GONE_ERROR, UNKNOWN_SQLSTATE, MYSQLND_SERVER_GONE);
            return false;
        }
    } else {
        let charset_no = match conn
            .options
            .charset_name
            .as_deref()
            .and_then(charset::find_charset_name)
        {
            Some(cs) => cs.nr,
            None => server_charset_no,
        };
        let attrs = if conn.server_capabilities & CLIENT_CONNECT_ATTRS != 0 {
            conn.options.connect_attr.clone()
        } else {
            None
        };
        let max = conn.options.max_allowed_packet;
        if conn.write_auth(
            mysql_flags,
            max,
            charset_no,
            user,
            auth_plugin_data,
            db,
            Some(auth_protocol.as_bytes()),
            attrs.as_deref(),
        ) == 0
        {
            return false;
        }
        conn.charset = charset::find_charset_nr(charset_no);
    }
    if plugin == Some(Plugin::CachingSha2)
        && !caching_sha2_handle_server_response(conn, orig_auth_plugin_data, passwd, sw)
    {
        return false;
    }
    let resp = conn.read_auth_response();
    let resp = match resp {
        Ok(r) if r.response_code < 0xFE => r,
        Ok(r) => {
            if r.response_code == 0xFE {
                match r.new_auth_protocol {
                    None => {
                        conn.error_info.set_str(
                            CR_UNKNOWN_ERROR,
                            UNKNOWN_SQLSTATE,
                            MYSQLND_OLD_PASSWD,
                        );
                    }
                    Some(p) => {
                        sw.protocol = Some(p);
                        sw.data = r.new_auth_protocol_data;
                    }
                }
            } else if r.response_code == 0xFF {
                conn.error_info
                    .set(r.error_no, UNKNOWN_SQLSTATE.as_bytes(), &r.error);
            }
            return false;
        }
        Err(()) => return false,
    };
    conn.last_message = resp.message;
    true
}

/// mysqlnd_auth_change_user
#[allow(clippy::too_many_arguments)]
fn auth_change_user(
    conn: &mut Conn,
    user: &[u8],
    passwd: &[u8],
    db: &[u8],
    silent: bool,
    use_full_blown_auth_packet: bool,
    auth_protocol: &str,
    plugin: Option<Plugin>,
    orig_auth_plugin_data: &[u8],
    auth_plugin_data: &[u8],
    sw: &mut Switch,
) -> bool {
    if !use_full_blown_auth_packet {
        if conn.write_change_auth_response(auth_plugin_data) == 0 {
            conn.state = ConnState::QuitSent;
            conn.error_info
                .set_str(CR_SERVER_GONE_ERROR, UNKNOWN_SQLSTATE, MYSQLND_SERVER_GONE);
            return false;
        }
    } else {
        let attrs = if conn.server_capabilities & CLIENT_CONNECT_ATTRS != 0 {
            conn.options.connect_attr.clone()
        } else {
            None
        };
        let charset_no = if conn.get_server_version() >= 50123 {
            conn.charset.map(|c| c.nr).unwrap_or(0)
        } else {
            0
        };
        let payload = conn.build_auth_payload(
            true,
            0,
            0,
            charset_no,
            user,
            auth_plugin_data,
            db,
            Some(auth_protocol.as_bytes()),
            attrs.as_deref(),
        );
        let ok = match payload {
            Some(p) => conn.send_command(COM_CHANGE_USER, &p, silent),
            None => false,
        };
        if !ok {
            conn.state = ConnState::QuitSent;
            conn.error_info
                .set_str(CR_SERVER_GONE_ERROR, UNKNOWN_SQLSTATE, MYSQLND_SERVER_GONE);
            return false;
        }
    }
    if plugin == Some(Plugin::CachingSha2)
        && !caching_sha2_handle_server_response(conn, orig_auth_plugin_data, passwd, sw)
    {
        return false;
    }
    let (mut ret, resp) = conn.read_chg_user_response();
    let ei = resp.error_info.clone();
    conn.error_info.copy_from(&ei);
    if resp.response_code == 0xFE {
        ret = false;
        match resp.new_auth_protocol {
            None => {
                conn.error_info
                    .set_str(CR_UNKNOWN_ERROR, UNKNOWN_SQLSTATE, MYSQLND_OLD_PASSWD);
            }
            Some(p) => {
                sw.protocol = Some(p);
                sw.data = resp.new_auth_protocol_data;
            }
        }
    }
    if conn.error_info.error_no != 0 {
        ret = false;
    }
    if ret {
        conn.last_message = None;
        conn.upsert.reset();
    } else if resp.server_asked_323_auth {
        conn.error_info
            .set_str(CR_UNKNOWN_ERROR, UNKNOWN_SQLSTATE, MYSQLND_OLD_PASSWD);
    }
    ret
}

/// The plugins' get_auth_data
fn get_auth_data(p: Plugin, conn: &mut Conn, passwd: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    match p {
        Plugin::Native => {
            if data.len() < SCRAMBLE_LENGTH {
                conn.error_info.set_str(
                    CR_MALFORMED_PACKET,
                    UNKNOWN_SQLSTATE,
                    "The server sent wrong length for scramble",
                );
                return None;
            }
            if passwd.is_empty() {
                return None;
            }
            Some(scramble(&data[..SCRAMBLE_LENGTH], passwd))
        }
        Plugin::Clear => {
            if passwd.is_empty() {
                return None;
            }
            let mut v = passwd.to_vec();
            v.push(0);
            Some(v)
        }
        Plugin::Sha256 => {
            if conn.vio.ssl {
                let mut v = passwd.to_vec();
                v.push(0);
                return Some(v);
            }
            let key = sha256_get_rsa_key(conn)?;
            let mut xor = passwd.to_vec();
            xor.push(0);
            xor_string(&mut xor, data);
            public_encrypt(conn, &key, passwd.len(), &xor)
        }
        Plugin::CachingSha2 => {
            if data.len() < SCRAMBLE_LENGTH {
                conn.error_info.set_str(
                    CR_MALFORMED_PACKET,
                    UNKNOWN_SQLSTATE,
                    "The server sent wrong length for scramble",
                );
                return None;
            }
            if passwd.is_empty() {
                return None;
            }
            Some(scramble_sha2(&data[..SCRAMBLE_LENGTH], passwd))
        }
    }
}

/// php_mysqlnd_scramble
pub fn scramble(scramble: &[u8], password: &[u8]) -> Vec<u8> {
    use purecrypto::hash::sha1;
    let sha1_1 = sha1(password);
    let sha1_2 = sha1(&sha1_1);
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(scramble);
    buf.extend_from_slice(&sha1_2);
    let h = sha1(&buf);
    h.iter().zip(sha1_1.iter()).map(|(a, b)| a ^ b).collect()
}

/// php_mysqlnd_scramble_sha2
pub fn scramble_sha2(scramble: &[u8], password: &[u8]) -> Vec<u8> {
    use purecrypto::hash::sha256;
    let s1 = sha256(password);
    let s2 = sha256(&s1);
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&s2);
    buf.extend_from_slice(scramble);
    let h = sha256(&buf);
    s1.iter().zip(h.iter()).map(|(a, b)| a ^ b).collect()
}

/// mysqlnd_xor_string (dst includes the trailing NUL; i <= dst_len)
fn xor_string(dst: &mut [u8], xor_str: &[u8]) {
    let key = &xor_str[..xor_str.len().min(SCRAMBLE_LENGTH)];
    if key.is_empty() {
        return;
    }
    for (i, b) in dst.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
}

fn parse_pem_key(pem: &[u8]) -> Option<purecrypto::rsa::BoxedRsaPublicKey> {
    let s = String::from_utf8_lossy(pem);
    let s = s.trim_end_matches('\0');
    purecrypto::rsa::BoxedRsaPublicKey::from_spki_pem(s).ok()
}

fn read_key_file(fname: &str) -> Option<purecrypto::rsa::BoxedRsaPublicKey> {
    match std::fs::read(fname) {
        Ok(b) => parse_pem_key(&b),
        Err(e) => {
            php_error_docref(
                E_WARNING,
                format!(
                    "{}: Failed to open stream: {}",
                    fname,
                    super::vio::strerror(&e)
                ),
            );
            None
        }
    }
}

fn configured_key_file(conn: &Conn) -> Option<String> {
    match &conn.pfc.sha256_server_public_key {
        Some(k) if !k.is_empty() => Some(k.clone()),
        _ => globals().sha256_server_public_key.filter(|k| !k.is_empty()),
    }
}

/// mysqlnd_sha256_get_rsa_key
fn sha256_get_rsa_key(conn: &mut Conn) -> Option<purecrypto::rsa::BoxedRsaPublicKey> {
    match configured_key_file(conn) {
        None => {
            if conn.write_pk_request(1) == 0 {
                php_error(
                    E_WARNING,
                    format!(
                        "Error while sending public key request packet. PID={}",
                        std::process::id()
                    ),
                );
                conn.state = ConnState::QuitSent;
                return None;
            }
            match conn.read_pk_response() {
                Ok(pk) => parse_pem_key(&pk),
                Err(()) => {
                    php_error(
                        E_WARNING,
                        format!(
                            "Error while receiving public key. PID={}",
                            std::process::id()
                        ),
                    );
                    conn.state = ConnState::QuitSent;
                    None
                }
            }
        }
        Some(f) => read_key_file(&f),
    }
}

/// mysqlnd_caching_sha2_get_key
fn caching_sha2_get_key(conn: &mut Conn) -> Option<purecrypto::rsa::BoxedRsaPublicKey> {
    match configured_key_file(conn) {
        None => {
            // req_packet.request = 1: sends '\2'
            if conn.write_pk_request(2) == 0 {
                php_error(
                    E_WARNING,
                    format!(
                        "Error while sending public key request packet. PID={}",
                        std::process::id()
                    ),
                );
                conn.state = ConnState::QuitSent;
                return None;
            }
            match conn.read_pk_response() {
                Ok(pk) => parse_pem_key(&pk),
                Err(()) => {
                    php_error(
                        E_WARNING,
                        format!(
                            "Error while receiving public key. PID={}",
                            std::process::id()
                        ),
                    );
                    conn.state = ConnState::QuitSent;
                    None
                }
            }
        }
        Some(f) => read_key_file(&f),
    }
}

/// mysqlnd_sha256_public_encrypt / mysqlnd_caching_sha2_public_encrypt
fn public_encrypt(
    conn: &mut Conn,
    key: &purecrypto::rsa::BoxedRsaPublicKey,
    passwd_len: usize,
    xor_str: &[u8],
) -> Option<Vec<u8>> {
    let k = key.modulus().bit_len().div_ceil(8);
    if k <= passwd_len + 41 {
        conn.error_info
            .set_str(CR_UNKNOWN_ERROR, UNKNOWN_SQLSTATE, "password is too long");
        return None;
    }
    key.encrypt_oaep::<purecrypto::hash::Sha1, _>(xor_str, &[], &mut purecrypto::rng::OsRng)
        .ok()
}

/// mysqlnd_caching_sha2_handle_server_response
fn caching_sha2_handle_server_response(
    conn: &mut Conn,
    auth_plugin_data: &[u8],
    passwd: &[u8],
    sw: &mut Switch,
) -> bool {
    if passwd.is_empty() {
        return true;
    }
    let r = match conn.read_cached_sha2_result() {
        Ok(r) => r,
        Err(()) => return true,
    };
    match r.response_code {
        0xFF => {
            conn.error_info
                .set(r.error_no, UNKNOWN_SQLSTATE.as_bytes(), &r.error);
            false
        }
        0xFE => {
            sw.protocol = r.new_auth_protocol;
            sw.data = r.new_auth_protocol_data;
            false
        }
        3 => true,
        4 => {
            let secure = conn.vio.ssl || conn.vio.is_unix;
            if secure {
                let mut p = passwd.to_vec();
                p.push(0);
                conn.write_raw(&p);
            } else {
                let crypted = caching_sha2_get_key(conn).and_then(|key| {
                    let mut xor = passwd.to_vec();
                    xor.push(0);
                    xor_string(&mut xor, auth_plugin_data);
                    public_encrypt(conn, &key, passwd.len(), &xor)
                });
                conn.write_raw(&crypted.unwrap_or_default());
            }
            true
        }
        code => {
            let msg = format!(
                "Unexpected server response while doing caching_sha2 auth: {}",
                code
            );
            conn.error_info
                .set_str(CR_NOT_IMPLEMENTED, UNKNOWN_SQLSTATE, &msg);
            true
        }
    }
}
