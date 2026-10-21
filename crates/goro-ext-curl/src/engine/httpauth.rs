//! HTTP Digest and NTLM authentication, ported from libcurl 8.14.1
//! (`lib/http_digest.c`, `lib/vauth/digest.c`, `lib/http_ntlm.c`,
//! `lib/vauth/ntlm.c`, `lib/curl_ntlm_core.c`).
//!
//! Byte strings are treated like the C strings libcurl sees: everything from
//! the first NUL byte on is ignored.

use purecrypto::cipher::{BlockCipher64, Des};
use purecrypto::hash::{Hmac, Md5, md4, md5, sha256, sha512_256};
use purecrypto::rng::{OsRng, RngCore};

/// CURLE_NOT_BUILT_IN
pub const CURLE_NOT_BUILT_IN: u32 = 4;
/// CURLE_REMOTE_ACCESS_DENIED
pub const CURLE_REMOTE_ACCESS_DENIED: u32 = 9;
/// CURLE_OUT_OF_MEMORY
pub const CURLE_OUT_OF_MEMORY: u32 = 27;
/// CURLE_BAD_CONTENT_ENCODING
pub const CURLE_BAD_CONTENT_ENCODING: u32 = 61;

/// Truncates at the first NUL, as a C string would be.
fn cstr(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

fn random_bytes(buf: &mut [u8]) {
    OsRng.fill_bytes(buf);
}

fn hex_lower(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 15) as usize]);
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// curlx_base64_encode()
fn base64_encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let x = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[(x >> 18) as usize & 63]);
        out.push(B64[(x >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 {
            B64[(x >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            B64[x as usize & 63]
        } else {
            b'='
        });
    }
    out
}

/// curlx_base64_decode(): strict, `src` is a C string.
fn base64_decode(src: &[u8]) -> Result<Vec<u8>, u32> {
    let src = cstr(src);
    let srclen = src.len();
    if srclen == 0 || !srclen.is_multiple_of(4) {
        return Err(CURLE_BAD_CONTENT_ENCODING);
    }
    let mut padding = 0usize;
    while src[srclen - 1 - padding] == b'=' {
        padding += 1;
        if padding > 2 {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
    }
    let lookup = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let num_quantums = srclen / 4;
    let full_quantums = num_quantums - usize::from(padding != 0);
    let mut out = Vec::with_capacity(num_quantums * 3 - padding);
    let mut i = 0;
    for _ in 0..full_quantums {
        let mut x = 0u32;
        for _ in 0..4 {
            x = (x << 6) | lookup(src[i]).ok_or(CURLE_BAD_CONTENT_ENCODING)?;
            i += 1;
        }
        out.extend_from_slice(&[(x >> 16) as u8, (x >> 8) as u8, x as u8]);
    }
    if padding != 0 {
        let mut x = 0u32;
        let mut padc = 0;
        for _ in 0..4 {
            if src[i] == b'=' {
                x <<= 6;
                padc += 1;
                if padc > padding {
                    return Err(CURLE_BAD_CONTENT_ENCODING);
                }
            } else {
                x = (x << 6) | lookup(src[i]).ok_or(CURLE_BAD_CONTENT_ENCODING)?;
            }
            i += 1;
        }
        out.push((x >> 16) as u8);
        if padding == 1 {
            out.push((x >> 8) as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Digest
// ---------------------------------------------------------------------------

const SESSION_ALGO: u8 = 1;
/// ALGO_MD5
pub const ALGO_MD5: u8 = 0;
/// ALGO_MD5SESS
pub const ALGO_MD5SESS: u8 = ALGO_MD5 | SESSION_ALGO;
/// ALGO_SHA256
pub const ALGO_SHA256: u8 = 2;
/// ALGO_SHA256SESS
pub const ALGO_SHA256SESS: u8 = ALGO_SHA256 | SESSION_ALGO;
/// ALGO_SHA512_256
pub const ALGO_SHA512_256: u8 = 4;
/// ALGO_SHA512_256SESS
pub const ALGO_SHA512_256SESS: u8 = ALGO_SHA512_256 | SESSION_ALGO;

const DIGEST_MAX_VALUE_LENGTH: i32 = 256;
const DIGEST_MAX_CONTENT_LENGTH: i32 = 1024;

/// `struct digestdata` (one per easy handle for the host, one for the proxy).
#[derive(Clone, Default, Debug)]
pub struct DigestData {
    pub nonce: Option<Vec<u8>>,
    pub cnonce: Option<Vec<u8>>,
    pub realm: Option<Vec<u8>>,
    pub opaque: Option<Vec<u8>>,
    /// "auth" or "auth-int"
    pub qop: Option<&'static [u8]>,
    pub algorithm: Option<Vec<u8>>,
    /// nonce count
    pub nc: u32,
    /// ALGO_* value
    pub algo: u8,
    pub stale: bool,
    pub userhash: bool,
}

/// Curl_auth_digest_get_pair(): parses `value=content` starting at `pos`.
/// Returns (value, content, position after the pair).
fn digest_get_pair(s: &[u8], pos: usize) -> Option<(Vec<u8>, Vec<u8>, usize)> {
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut i = pos;
    let mut value = Vec::new();
    let mut c = DIGEST_MAX_VALUE_LENGTH - 1;
    while at(i) != 0 && at(i) != b'=' && {
        let t = c;
        c -= 1;
        t != 0
    } {
        value.push(at(i));
        i += 1;
    }
    if at(i) != b'=' {
        return None;
    }
    i += 1;
    let mut quoted = false;
    if at(i) == b'"' {
        i += 1;
        quoted = true;
    }
    let mut escape = false;
    let mut content = Vec::new();
    let mut c = DIGEST_MAX_CONTENT_LENGTH - 1;
    loop {
        if at(i) == 0 {
            break;
        }
        let t = c;
        c -= 1;
        if t == 0 {
            break;
        }
        let ch = at(i);
        if !escape {
            match ch {
                b'\\' if quoted => {
                    escape = true;
                    i += 1;
                    continue;
                }
                b',' if !quoted => {
                    c = 0;
                    i += 1;
                    continue;
                }
                b'\r' | b'\n' => {
                    if quoted {
                        return None;
                    }
                    c = 0;
                    i += 1;
                    continue;
                }
                b'"' => {
                    if quoted {
                        c = 0;
                        i += 1;
                        continue;
                    }
                    return None;
                }
                _ => {}
            }
        }
        escape = false;
        content.push(ch);
        i += 1;
    }
    if escape {
        return None;
    }
    Some((value, content, i))
}

/// curlx_str_until(.., 32, ',') + curlx_str_casecompare loop used for qop.
fn digest_qop_tokens(content: &[u8]) -> (bool, bool) {
    let mut found_auth = false;
    let mut found_auth_int = false;
    let mut i = 0;
    while i < content.len() && is_blank(content[i]) {
        i += 1;
    }
    loop {
        // curlx_str_until
        let start = i;
        let mut j = i;
        let mut len = 0;
        let mut ok = true;
        while j < content.len() && content[j] != b',' {
            j += 1;
            len += 1;
            if len > 32 {
                ok = false;
                break;
            }
        }
        if !ok || len == 0 {
            break;
        }
        let tok = &content[start..j];
        i = j;
        if tok.eq_ignore_ascii_case(b"auth") {
            found_auth = true;
        } else if tok.eq_ignore_ascii_case(b"auth-int") {
            found_auth_int = true;
        }
        if content.get(i) != Some(&b',') {
            break;
        }
        i += 1;
        while i < content.len() && is_blank(content[i]) {
            i += 1;
        }
    }
    (found_auth, found_auth_int)
}

/// auth_digest_string_quoted()
fn digest_quoted(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 2);
    for &b in s {
        if b == b'"' || b == b'\\' {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

impl DigestData {
    /// Curl_auth_digest_cleanup()
    pub fn cleanup(&mut self) {
        *self = DigestData::default();
    }

    /// Curl_input_digest(): `header` is the `WWW-Authenticate:` /
    /// `Proxy-Authenticate:` value starting with the word "Digest".
    ///
    /// Any error (always CURLE_BAD_CONTENT_ENCODING in practice) is what
    /// makes http.c log "Digest authentication problem, ignoring." and set
    /// `authproblem`. As in libcurl, the state is left partially updated on
    /// error (it is reset first, then filled as far as parsing went).
    pub fn input(&mut self, header: &[u8]) -> Result<(), u32> {
        let header = cstr(header);
        if header.len() < 6 || !header[..6].eq_ignore_ascii_case(b"Digest") {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        if !header.get(6).is_some_and(|&c| is_blank(c)) {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        let mut i = 6;
        while i < header.len() && is_blank(header[i]) {
            i += 1;
        }
        self.decode_http_message(&header[i..])
    }

    /// Curl_auth_decode_digest_http_message()
    fn decode_http_message(&mut self, chlg: &[u8]) -> Result<(), u32> {
        let before = self.nonce.is_some();
        self.cleanup();
        let mut i = 0;
        loop {
            while i < chlg.len() && is_blank(chlg[i]) {
                i += 1;
            }
            let Some((value, content, end)) = digest_get_pair(chlg, i) else {
                break;
            };
            i = end;
            if value.eq_ignore_ascii_case(b"nonce") {
                self.nonce = Some(content);
            } else if value.eq_ignore_ascii_case(b"stale") {
                if content.eq_ignore_ascii_case(b"true") {
                    self.stale = true;
                    self.nc = 1;
                }
            } else if value.eq_ignore_ascii_case(b"realm") {
                self.realm = Some(content);
            } else if value.eq_ignore_ascii_case(b"opaque") {
                self.opaque = Some(content);
            } else if value.eq_ignore_ascii_case(b"qop") {
                let (auth, auth_int) = digest_qop_tokens(&content);
                if auth {
                    self.qop = Some(b"auth");
                } else if auth_int {
                    self.qop = Some(b"auth-int");
                }
            } else if value.eq_ignore_ascii_case(b"algorithm") {
                let c = content.as_slice();
                self.algorithm = Some(content.clone());
                if c.eq_ignore_ascii_case(b"MD5-sess") {
                    self.algo = ALGO_MD5SESS;
                } else if c.eq_ignore_ascii_case(b"MD5") {
                    self.algo = ALGO_MD5;
                } else if c.eq_ignore_ascii_case(b"SHA-256") {
                    self.algo = ALGO_SHA256;
                } else if c.eq_ignore_ascii_case(b"SHA-256-SESS") {
                    self.algo = ALGO_SHA256SESS;
                } else if c.eq_ignore_ascii_case(b"SHA-512-256") {
                    self.algo = ALGO_SHA512_256;
                } else if c.eq_ignore_ascii_case(b"SHA-512-256-SESS") {
                    self.algo = ALGO_SHA512_256SESS;
                } else {
                    return Err(CURLE_BAD_CONTENT_ENCODING);
                }
            } else if value.eq_ignore_ascii_case(b"userhash")
                && content.eq_ignore_ascii_case(b"true")
            {
                self.userhash = true;
            }
            while i < chlg.len() && is_blank(chlg[i]) {
                i += 1;
            }
            if chlg.get(i) == Some(&b',') {
                i += 1;
            }
        }
        if before && !self.stale {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        if self.nonce.is_none() {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        if self.qop.is_none() && (self.algo & SESSION_ALGO) != 0 {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        Ok(())
    }

    fn hash_hex(&self, data: &[u8]) -> Vec<u8> {
        if self.algo <= ALGO_MD5SESS {
            hex_lower(&md5(data))
        } else if self.algo <= ALGO_SHA256SESS {
            hex_lower(&sha256(data))
        } else {
            hex_lower(&sha512_256(data))
        }
    }

    /// Curl_output_digest(): returns `Ok(None)` when no challenge (nonce) is
    /// stored — libcurl then sets `authp->done = FALSE` and sends nothing —
    /// and `Ok(Some(line))` with the complete
    /// `"[Proxy-]Authorization: Digest ...\r\n"` line otherwise (libcurl sets
    /// `authp->done = TRUE`).
    ///
    /// `iestyle` is `authp->iestyle` (CURLAUTH_DIGEST_IE): the path is cut at
    /// the first '?'. `cnonce` injects the client nonce used when none is
    /// stored yet (libcurl: base64 of 12 random bytes); `None` = random.
    #[allow(clippy::too_many_arguments)]
    pub fn output(
        &mut self,
        proxy: bool,
        user: &[u8],
        passwd: &[u8],
        method: &[u8],
        path: &[u8],
        iestyle: bool,
        cnonce: Option<&[u8]>,
    ) -> Result<Option<Vec<u8>>, u32> {
        if self.nonce.is_none() {
            return Ok(None);
        }
        let mut path = cstr(path);
        if iestyle && let Some(q) = path.iter().position(|&b| b == b'?') {
            path = &path[..q];
        }
        let response = self.create_http_message(
            cstr(user),
            cstr(passwd),
            cstr(method),
            path,
            cnonce.map(cstr),
        )?;
        let prefix: &[u8] = if proxy { b"Proxy-" } else { b"" };
        Ok(Some(cat(&[
            prefix,
            b"Authorization: Digest ",
            &response,
            b"\r\n",
        ])))
    }

    /// auth_create_digest_http_message()
    fn create_http_message(
        &mut self,
        userp: &[u8],
        passwdp: &[u8],
        request: &[u8],
        uripath: &[u8],
        cnonce: Option<&[u8]>,
    ) -> Result<Vec<u8>, u32> {
        if self.algo > ALGO_SHA512_256SESS {
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        if self.nc == 0 {
            self.nc = 1;
        }
        if self.cnonce.is_none() {
            let c = match cnonce {
                Some(c) => c.to_vec(),
                None => {
                    let mut buf = [0u8; 12];
                    random_bytes(&mut buf);
                    base64_encode(&buf)
                }
            };
            self.cnonce = Some(c);
        }
        let realm: &[u8] = self.realm.as_deref().unwrap_or(b"");
        let nonce: &[u8] = self.nonce.as_deref().unwrap_or(b"");
        let cn: &[u8] = self.cnonce.as_deref().unwrap_or(b"");

        let userh = if self.userhash {
            Some(self.hash_hex(&cat(&[userp, b":", realm])))
        } else {
            None
        };

        let mut ha1 = self.hash_hex(&cat(&[userp, b":", realm, b":", passwdp]));
        if self.algo & SESSION_ALGO != 0 {
            ha1 = self.hash_hex(&cat(&[&ha1, b":", nonce, b":", cn]));
        }

        let mut hashthis = cat(&[request, b":", uripath]);
        if self
            .qop
            .is_some_and(|q| q.eq_ignore_ascii_case(b"auth-int"))
        {
            let hashed = self.hash_hex(b"");
            hashthis = cat(&[&hashthis, b":", &hashed]);
        }
        let ha2 = self.hash_hex(&hashthis);

        let nc_hex = format!("{:08x}", self.nc);
        let request_digest = match self.qop {
            Some(qop) => self.hash_hex(&cat(&[
                &ha1,
                b":",
                nonce,
                b":",
                nc_hex.as_bytes(),
                b":",
                cn,
                b":",
                qop,
                b":",
                &ha2,
            ])),
            None => self.hash_hex(&cat(&[&ha1, b":", nonce, b":", &ha2])),
        };

        let userp_quoted = digest_quoted(userh.as_deref().unwrap_or(userp));
        let realm_quoted = digest_quoted(realm);
        let nonce_quoted = digest_quoted(nonce);

        let mut response = match self.qop {
            Some(qop) => cat(&[
                b"username=\"",
                &userp_quoted,
                b"\", realm=\"",
                &realm_quoted,
                b"\", nonce=\"",
                &nonce_quoted,
                b"\", uri=\"",
                uripath,
                b"\", cnonce=\"",
                cn,
                b"\", nc=",
                nc_hex.as_bytes(),
                b", qop=",
                qop,
                b", response=\"",
                &request_digest,
                b"\"",
            ]),
            None => cat(&[
                b"username=\"",
                &userp_quoted,
                b"\", realm=\"",
                &realm_quoted,
                b"\", nonce=\"",
                &nonce_quoted,
                b"\", uri=\"",
                uripath,
                b"\", response=\"",
                &request_digest,
                b"\"",
            ]),
        };
        if self.qop.is_some() {
            self.nc = self.nc.wrapping_add(1);
        }
        if let Some(opaque) = &self.opaque {
            response.extend_from_slice(b", opaque=\"");
            response.extend_from_slice(&digest_quoted(opaque));
            response.push(b'"');
        }
        if let Some(alg) = &self.algorithm {
            response.extend_from_slice(b", algorithm=");
            response.extend_from_slice(alg);
        }
        if self.userhash {
            response.extend_from_slice(b", userhash=true");
        }
        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// NTLM core (curl_ntlm_core.c)
// ---------------------------------------------------------------------------

const NTLMFLAG_NEGOTIATE_UNICODE: u32 = 1 << 0;
const NTLMFLAG_NEGOTIATE_OEM: u32 = 1 << 1;
const NTLMFLAG_REQUEST_TARGET: u32 = 1 << 2;
const NTLMFLAG_NEGOTIATE_NTLM_KEY: u32 = 1 << 9;
const NTLMFLAG_NEGOTIATE_ALWAYS_SIGN: u32 = 1 << 15;
const NTLMFLAG_NEGOTIATE_NTLM2_KEY: u32 = 1 << 19;
const NTLMFLAG_NEGOTIATE_TARGET_INFO: u32 = 1 << 23;

const NTLMSSP_SIGNATURE: &[u8; 8] = b"NTLMSSP\0";
const NTLM_BUFSIZE: usize = 1024;
const CURL_MAX_INPUT_LENGTH: usize = 8_000_000;

/// extend_key_56_to_64() (+ DES_set_odd_parity, which DES ignores anyway)
fn extend_key_56_to_64(k: &[u8]) -> [u8; 8] {
    let mut key = [
        k[0],
        (k[0] << 7) | (k[1] >> 1),
        (k[1] << 6) | (k[2] >> 2),
        (k[2] << 5) | (k[3] >> 3),
        (k[3] << 4) | (k[4] >> 4),
        (k[4] << 3) | (k[5] >> 5),
        (k[5] << 2) | (k[6] >> 6),
        k[6] << 1,
    ];
    for b in key.iter_mut() {
        let hi = *b & 0xfe;
        *b = hi | u8::from(hi.count_ones() % 2 == 0);
    }
    key
}

fn des_encrypt_56(key56: &[u8], plain: &[u8; 8]) -> [u8; 8] {
    let des = Des::new(&extend_key_56_to_64(key56));
    let mut block = *plain;
    des.encrypt_block(&mut block);
    block
}

/// Curl_ntlm_core_lm_resp(): 21-byte keys, 8-byte challenge -> 24 bytes.
pub fn ntlm_core_lm_resp(keys: &[u8; 21], plaintext: &[u8; 8]) -> [u8; 24] {
    let mut out = [0u8; 24];
    out[..8].copy_from_slice(&des_encrypt_56(&keys[0..7], plaintext));
    out[8..16].copy_from_slice(&des_encrypt_56(&keys[7..14], plaintext));
    out[16..].copy_from_slice(&des_encrypt_56(&keys[14..21], plaintext));
    out
}

/// Curl_ntlm_core_mk_lm_hash(): 21 bytes (16 + 5 zeroes).
pub fn ntlm_core_mk_lm_hash(password: &[u8]) -> [u8; 21] {
    const MAGIC: &[u8; 8] = b"KGS!@#$%";
    let password = cstr(password);
    let len = password.len().min(14);
    let mut pw = [0u8; 14];
    for (d, s) in pw.iter_mut().zip(&password[..len]) {
        *d = s.to_ascii_uppercase();
    }
    let mut out = [0u8; 21];
    out[..8].copy_from_slice(&des_encrypt_56(&pw[0..7], MAGIC));
    out[8..16].copy_from_slice(&des_encrypt_56(&pw[7..14], MAGIC));
    out
}

fn ascii_to_unicode_le(src: &[u8], upper: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(if upper { b.to_ascii_uppercase() } else { b });
        out.push(0);
    }
    out
}

/// Curl_ntlm_core_mk_nt_hash(): 21 bytes (MD4 + 5 zeroes).
pub fn ntlm_core_mk_nt_hash(password: &[u8]) -> [u8; 21] {
    let pw = ascii_to_unicode_le(cstr(password), false);
    let mut out = [0u8; 21];
    out[..16].copy_from_slice(&md4(&pw));
    out
}

fn hmac_md5(key: &[u8], data: &[u8]) -> [u8; 16] {
    Hmac::<Md5>::mac(key, data)
}

/// Curl_ntlm_core_mk_ntlmv2_hash()
pub fn ntlm_core_mk_ntlmv2_hash(
    user: &[u8],
    domain: &[u8],
    ntlmhash: &[u8; 21],
) -> Result<[u8; 16], u32> {
    if user.len() > CURL_MAX_INPUT_LENGTH || domain.len() > CURL_MAX_INPUT_LENGTH {
        return Err(CURLE_OUT_OF_MEMORY);
    }
    let mut identity = ascii_to_unicode_le(user, true);
    identity.extend_from_slice(&ascii_to_unicode_le(domain, false));
    Ok(hmac_md5(&ntlmhash[..16], &identity))
}

/// time2filetime() for a 64-bit time_t: returns (low, high).
fn time2filetime(t: i64) -> (u32, u32) {
    let t = t.wrapping_add(11_644_473_600).wrapping_mul(10_000_000);
    ((t & 0xFFFF_FFFF) as u32, (t >> 32) as u32)
}

/// Curl_ntlm_core_mk_ntlmv2_resp(); `time` is the time(NULL) value.
pub fn ntlm_core_mk_ntlmv2_resp(
    ntlmv2hash: &[u8; 16],
    challenge_client: &[u8; 8],
    server_nonce: &[u8; 8],
    target_info: &[u8],
    time: i64,
) -> Vec<u8> {
    let (lo, hi) = time2filetime(time);
    let blob_len = 44 - 16 + target_info.len() + 4;
    let len = 16 + blob_len;
    let mut ptr = vec![0u8; len];
    ptr[16..20].copy_from_slice(&[1, 1, 0, 0]);
    ptr[24..28].copy_from_slice(&lo.to_le_bytes());
    ptr[28..32].copy_from_slice(&hi.to_le_bytes());
    ptr[32..40].copy_from_slice(challenge_client);
    ptr[44..44 + target_info.len()].copy_from_slice(target_info);
    ptr[8..16].copy_from_slice(server_nonce);
    let mac = hmac_md5(ntlmv2hash, &ptr[8..8 + blob_len + 8]);
    ptr[..16].copy_from_slice(&mac);
    ptr
}

/// Curl_ntlm_core_mk_lmv2_resp()
pub fn ntlm_core_mk_lmv2_resp(
    ntlmv2hash: &[u8; 16],
    challenge_client: &[u8; 8],
    challenge_server: &[u8; 8],
) -> [u8; 24] {
    let mut data = [0u8; 16];
    data[..8].copy_from_slice(challenge_server);
    data[8..].copy_from_slice(challenge_client);
    let mac = hmac_md5(ntlmv2hash, &data);
    let mut out = [0u8; 24];
    out[..16].copy_from_slice(&mac);
    out[16..].copy_from_slice(challenge_client);
    out
}

// ---------------------------------------------------------------------------
// NTLM (vauth/ntlm.c + http_ntlm.c)
// ---------------------------------------------------------------------------

/// curlntlm values
pub const NTLMSTATE_NONE: u8 = 0;
pub const NTLMSTATE_TYPE1: u8 = 1;
pub const NTLMSTATE_TYPE2: u8 = 2;
pub const NTLMSTATE_TYPE3: u8 = 3;
pub const NTLMSTATE_LAST: u8 = 4;

/// `struct ntlmdata` + the matching `curlntlm` state (`conn->ntlm` /
/// `conn->http_ntlm_state`, or the proxy pair). Per connection.
#[derive(Clone, Default, Debug)]
pub struct NtlmData {
    pub state: u8,
    pub flags: u32,
    /// server challenge from the type-2 message
    pub nonce: [u8; 8],
    pub target_info: Vec<u8>,
}

/// Result of [`NtlmData::output`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NtlmOut {
    /// `"[Proxy-]Authorization: NTLM <base64>\r\n"`, or None when nothing is
    /// sent (NTLMSTATE_LAST; libcurl also frees any previous header then).
    pub header: Option<Vec<u8>>,
    /// value libcurl leaves in `authp->done`
    pub done: bool,
    /// libcurl forces `data->info.httpauthpicked` (or `proxyauthpicked`) to
    /// CURLAUTH_NTLM (NTLMSTATE_LAST case)
    pub force_picked: bool,
}

impl NtlmData {
    /// Curl_auth_cleanup_ntlm()
    pub fn cleanup(&mut self) {
        self.target_info.clear();
    }

    /// The curlntlm state (NTLMSTATE_*).
    pub fn state(&self) -> u8 {
        self.state
    }

    /// Curl_input_ntlm(): `header` is the `*-Authenticate:` value starting
    /// with "NTLM". infof() texts are appended to `info`.
    ///
    /// libcurl's Curl_http_auth_cleanup_ntlm() (on "NTLM auth restarted" and
    /// "NTLM handshake rejected") cleans both the host and the proxy
    /// ntlmdata of the connection; this only cleans `self`. The caller should
    /// call `cleanup()` on the other one in those two cases to be exact (it
    /// only drops a stored target-info block).
    pub fn input(&mut self, header: &[u8], info: &mut Vec<String>) -> Result<(), u32> {
        let header = cstr(header);
        if header.len() < 4 || !header[..4].eq_ignore_ascii_case(b"NTLM") {
            return Ok(());
        }
        let mut i = 4;
        while i < header.len() && is_blank(header[i]) {
            i += 1;
        }
        if i < header.len() {
            let msg = base64_decode(&header[i..])?;
            self.decode_type2(&msg, info)?;
            self.state = NTLMSTATE_TYPE2;
        } else {
            if self.state == NTLMSTATE_LAST {
                info.push("NTLM auth restarted".into());
                self.cleanup();
            } else if self.state == NTLMSTATE_TYPE3 {
                info.push("NTLM handshake rejected".into());
                self.cleanup();
                self.state = NTLMSTATE_NONE;
                return Err(CURLE_REMOTE_ACCESS_DENIED);
            } else if self.state >= NTLMSTATE_TYPE1 {
                info.push("NTLM handshake failure (internal error)".into());
                return Err(CURLE_REMOTE_ACCESS_DENIED);
            }
            self.state = NTLMSTATE_TYPE1;
        }
        Ok(())
    }

    /// Curl_auth_decode_ntlm_type2_message()
    fn decode_type2(&mut self, type2: &[u8], info: &mut Vec<String>) -> Result<(), u32> {
        self.flags = 0;
        if type2.len() < 32 || &type2[..8] != NTLMSSP_SIGNATURE || type2[8..12] != [2, 0, 0, 0] {
            info.push("NTLM handshake failure (bad type-2 message)".into());
            return Err(CURLE_BAD_CONTENT_ENCODING);
        }
        self.flags = u32::from_le_bytes([type2[20], type2[21], type2[22], type2[23]]);
        self.nonce.copy_from_slice(&type2[24..32]);
        if self.flags & NTLMFLAG_NEGOTIATE_TARGET_INFO != 0 {
            // ntlm_decode_type2_target()
            let mut ti_len = 0usize;
            if type2.len() >= 48 {
                ti_len = u16::from_le_bytes([type2[40], type2[41]]) as usize;
                let ti_off =
                    u32::from_le_bytes([type2[44], type2[45], type2[46], type2[47]]) as usize;
                if ti_len > 0 {
                    if ti_off > type2.len() || ti_off + ti_len > type2.len() || ti_off < 48 {
                        info.push(
                            "NTLM handshake failure (bad type-2 message). \
                             Target Info Offset Len is set incorrect by the peer"
                                .into(),
                        );
                        info.push("NTLM handshake failure (bad type-2 message)".into());
                        return Err(CURLE_BAD_CONTENT_ENCODING);
                    }
                    self.target_info = type2[ti_off..ti_off + ti_len].to_vec();
                }
            }
            // target_info_len = 0 makes any previous block unused
            self.target_info.truncate(ti_len);
        }
        Ok(())
    }

    /// Curl_auth_create_ntlm_type1_message()
    fn create_type1(&mut self) -> Vec<u8> {
        self.cleanup();
        let flags = NTLMFLAG_NEGOTIATE_OEM
            | NTLMFLAG_REQUEST_TARGET
            | NTLMFLAG_NEGOTIATE_NTLM_KEY
            | NTLMFLAG_NEGOTIATE_NTLM2_KEY
            | NTLMFLAG_NEGOTIATE_ALWAYS_SIGN;
        let mut m = Vec::with_capacity(32);
        m.extend_from_slice(NTLMSSP_SIGNATURE);
        m.extend_from_slice(&[1, 0, 0, 0]);
        m.extend_from_slice(&flags.to_le_bytes());
        m.extend_from_slice(&[0u8; 16]); // empty domain & host security buffers
        m
    }

    /// Curl_auth_create_ntlm_type3_message(). `entropy` / `time` are the
    /// Curl_rand() client challenge and the time(NULL) value (NTLMv2 only).
    fn create_type3(
        &mut self,
        userp: &[u8],
        passwdp: &[u8],
        entropy: [u8; 8],
        time: i64,
        fail: &mut Vec<String>,
    ) -> Result<Vec<u8>, u32> {
        const HOST: &[u8] = b"WORKSTATION";
        let unicode = self.flags & NTLMFLAG_NEGOTIATE_UNICODE != 0;
        let (domain, user): (&[u8], &[u8]) = match userp
            .iter()
            .position(|&b| b == b'\\')
            .or_else(|| userp.iter().position(|&b| b == b'/'))
        {
            Some(p) => (&userp[..p], &userp[p + 1..]),
            None => (b"", userp),
        };
        let mut lmresp = [0u8; 24];
        let ntresp: Vec<u8>;
        if self.flags & NTLMFLAG_NEGOTIATE_NTLM2_KEY != 0 {
            let ntbuffer = ntlm_core_mk_nt_hash(passwdp);
            let v2hash = ntlm_core_mk_ntlmv2_hash(user, domain, &ntbuffer)?;
            lmresp = ntlm_core_mk_lmv2_resp(&v2hash, &entropy, &self.nonce);
            ntresp =
                ntlm_core_mk_ntlmv2_resp(&v2hash, &entropy, &self.nonce, &self.target_info, time);
        } else {
            let ntbuffer = ntlm_core_mk_nt_hash(passwdp);
            ntresp = ntlm_core_lm_resp(&ntbuffer, &self.nonce).to_vec();
            let lmbuffer = ntlm_core_mk_lm_hash(passwdp);
            lmresp.copy_from_slice(&ntlm_core_lm_resp(&lmbuffer, &self.nonce));
            self.flags &= !NTLMFLAG_NEGOTIATE_NTLM2_KEY;
        }
        let mul = if unicode { 2 } else { 1 };
        let domlen = domain.len() * mul;
        let userlen = user.len() * mul;
        let hostlen = HOST.len() * mul;
        let ntresplen = ntresp.len();

        let lmrespoff = 64usize;
        let ntrespoff = lmrespoff + 0x18;
        let domoff = ntrespoff + ntresplen;
        let useroff = domoff + domlen;
        let hostoff = useroff + userlen;

        let short = |v: usize| -> [u8; 2] { [(v & 0xff) as u8, ((v >> 8) & 0xff) as u8] };
        let mut m = Vec::with_capacity(NTLM_BUFSIZE);
        m.extend_from_slice(NTLMSSP_SIGNATURE);
        m.extend_from_slice(&[3, 0, 0, 0]);
        for (len, off) in [
            (0x18, lmrespoff),
            (ntresplen, ntrespoff),
            (domlen, domoff),
            (userlen, useroff),
            (hostlen, hostoff),
            (0, 0),
        ] {
            m.extend_from_slice(&short(len));
            m.extend_from_slice(&short(len));
            m.extend_from_slice(&short(off));
            m.extend_from_slice(&[0, 0]);
        }
        m.extend_from_slice(&self.flags.to_le_bytes());
        debug_assert_eq!(m.len(), 64);
        m.extend_from_slice(&lmresp);
        if ntresplen + m.len() > NTLM_BUFSIZE {
            fail.push("incoming NTLM message too big".into());
            return Err(CURLE_OUT_OF_MEMORY);
        }
        m.extend_from_slice(&ntresp);
        if m.len() + userlen + domlen + hostlen >= NTLM_BUFSIZE {
            fail.push("user + domain + hostname too big".into());
            return Err(CURLE_OUT_OF_MEMORY);
        }
        for s in [domain, user, HOST] {
            if unicode {
                m.extend_from_slice(&ascii_to_unicode_le(s, false));
            } else {
                m.extend_from_slice(s);
            }
        }
        self.cleanup();
        Ok(m)
    }

    /// Curl_output_ntlm(). `rng` injects the 8-byte NTLMv2 client challenge
    /// and `timestamp` the time(NULL) seconds put (as FILETIME) into the
    /// NTLMv2 blob; None = random / current time. failf() texts are appended
    /// to `fail`.
    ///
    /// The type-1 message never carries a host or domain and the type-3
    /// "workstation" is always the literal "WORKSTATION", so no host name
    /// argument is needed.
    pub fn output(
        &mut self,
        proxy: bool,
        user: &[u8],
        passwd: &[u8],
        rng: Option<[u8; 8]>,
        timestamp: Option<i64>,
        fail: &mut Vec<String>,
    ) -> Result<NtlmOut, u32> {
        let prefix: &[u8] = if proxy { b"Proxy-" } else { b"" };
        let mkline = |msg: &[u8]| {
            cat(&[
                prefix,
                b"Authorization: NTLM ",
                &base64_encode(msg),
                b"\r\n",
            ])
        };
        if self.state == NTLMSTATE_TYPE3 {
            self.state = NTLMSTATE_LAST;
        }
        match self.state {
            NTLMSTATE_TYPE2 => {
                let entropy = rng.unwrap_or_else(|| {
                    let mut b = [0u8; 8];
                    random_bytes(&mut b);
                    b
                });
                let time = timestamp.unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0)
                });
                let msg = self.create_type3(cstr(user), cstr(passwd), entropy, time, fail)?;
                self.state = NTLMSTATE_TYPE3;
                Ok(NtlmOut {
                    header: Some(mkline(&msg)),
                    done: true,
                    force_picked: false,
                })
            }
            NTLMSTATE_LAST => Ok(NtlmOut {
                header: None,
                done: true,
                force_picked: true,
            }),
            _ => {
                let msg = self.create_type1();
                Ok(NtlmOut {
                    header: Some(mkline(&msg)),
                    done: false,
                    force_picked: false,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        let s: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        s.chunks(2)
            .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
            .collect()
    }

    fn s(v: &[u8]) -> String {
        String::from_utf8_lossy(v).into_owned()
    }

    #[test]
    fn base64_roundtrip_and_strictness() {
        assert_eq!(base64_encode(b"foob"), b"Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), b"Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), b"Zm9vYmFy");
        assert_eq!(base64_decode(b"Zm9vYg==").unwrap(), b"foob");
        assert_eq!(base64_decode(b"Zm9vYmE=").unwrap(), b"fooba");
        assert!(base64_decode(b"Zm9vYg=").is_err());
        assert!(base64_decode(b"Zm9v===").is_err());
        assert!(base64_decode(b"Zm=vYmFy").is_err());
        assert!(base64_decode(b"Z===").is_err());
        assert!(base64_decode(b"").is_err());
    }

    // ---------------- Digest ----------------

    /// input(challenge) then output(GET path) with an injected cnonce.
    #[allow(clippy::too_many_arguments)]
    fn digest_once(
        d: &mut DigestData,
        chal: &str,
        proxy: bool,
        user: &str,
        pw: &str,
        path: &str,
        ie: bool,
        cnonce: &str,
    ) -> String {
        d.input(chal.as_bytes()).unwrap();
        let out = d
            .output(
                proxy,
                user.as_bytes(),
                pw.as_bytes(),
                b"GET",
                path.as_bytes(),
                ie,
                Some(cnonce.as_bytes()),
            )
            .unwrap()
            .unwrap();
        s(&out)
    }

    #[test]
    fn digest_rfc2617_example() {
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            "Digest realm=\"testrealm@host.com\", qop=\"auth,auth-int\", \
             nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", \
             opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"",
            false,
            "Mufasa",
            "Circle Of Life",
            "/dir/index.html",
            false,
            "0a4f113b",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"Mufasa\", realm=\"testrealm@host.com\", \
             nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", uri=\"/dir/index.html\", \
             cnonce=\"0a4f113b\", nc=00000001, qop=auth, \
             response=\"6629fae49393a05397450978507c4ef1\", \
             opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"\r\n"
        );
    }

    #[test]
    fn digest_rfc7616_examples() {
        let chal = |alg: &str| {
            format!(
                "Digest realm=\"http-auth@example.org\", qop=\"auth, auth-int\", \
                 algorithm={alg}, nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", \
                 opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\""
            )
        };
        let cn = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        for (alg, resp) in [
            ("MD5", "8ca523f5e9506fed4657c9700eebdbec"),
            (
                "SHA-256",
                "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1",
            ),
        ] {
            let mut d = DigestData::default();
            let got = digest_once(
                &mut d,
                &chal(alg),
                false,
                "Mufasa",
                "Circle of Life",
                "/dir/index.html",
                false,
                cn,
            );
            assert_eq!(
                got,
                format!(
                    "Authorization: Digest username=\"Mufasa\", \
                     realm=\"http-auth@example.org\", \
                     nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", \
                     uri=\"/dir/index.html\", cnonce=\"{cn}\", nc=00000001, qop=auth, \
                     response=\"{resp}\", \
                     opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\", \
                     algorithm={alg}\r\n"
                )
            );
        }
    }

    // The following expectations were captured from real libcurl 8.14.1
    // (the cnonce is the one libcurl generated, re-injected here).

    #[test]
    fn digest_libcurl_no_qop() {
        let mut d = DigestData::default();
        d.input(b"Digest realm=\"testrealm\", nonce=\"1053604598\"")
            .unwrap();
        let out = d
            .output(
                false,
                b"testuser",
                b"testpass",
                b"GET",
                b"/d1/64",
                false,
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            s(&out),
            "Authorization: Digest username=\"testuser\", realm=\"testrealm\", \
             nonce=\"1053604598\", uri=\"/d1/64\", \
             response=\"04555562982b8f8aee12c5a9fc128b29\"\r\n"
        );
        // no qop: nc is not incremented
        assert_eq!(d.nc, 1);
    }

    #[test]
    fn digest_libcurl_md5_opaque() {
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            "Digest realm=\"testrealm@host.com\", qop=\"auth,auth-int\", \
             nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", \
             opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"",
            false,
            "Mufasa",
            "Circle Of Life",
            "/d2/dir/index.html",
            false,
            "SFhJd+CIDjGx9pjS",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"Mufasa\", realm=\"testrealm@host.com\", \
             nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", uri=\"/d2/dir/index.html\", \
             cnonce=\"SFhJd+CIDjGx9pjS\", nc=00000001, qop=auth, \
             response=\"f038f9342b71a8700273eeb96a7b65f9\", \
             opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"\r\n"
        );
    }

    #[test]
    fn digest_libcurl_sha256() {
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            "Digest realm=\"http-auth@example.org\", qop=\"auth, auth-int\", \
             algorithm=SHA-256, nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", \
             opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\"",
            false,
            "Mufasa",
            "Circle of Life",
            "/d3/dir/index.html",
            false,
            "hKO1DZ6fqsVUm+u+",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"Mufasa\", realm=\"http-auth@example.org\", \
             nonce=\"7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v\", \
             uri=\"/d3/dir/index.html\", cnonce=\"hKO1DZ6fqsVUm+u+\", nc=00000001, \
             qop=auth, \
             response=\"1e8e888cd7017fba0a644028e57b2607c0960de003918541e0766568a1329782\", \
             opaque=\"FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS\", algorithm=SHA-256\r\n"
        );
    }

    #[test]
    fn digest_libcurl_md5sess_authint_iestyle() {
        let chal = "Digest realm=\"r\", nonce=\"n1\", qop=\"auth-int\", algorithm=MD5-sess";
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            chal,
            false,
            "us\"er",
            "pw",
            "/d4/a?b=c",
            true,
            "7wBYf3nwNktAkukH",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"us\\\"er\", realm=\"r\", nonce=\"n1\", \
             uri=\"/d4/a\", cnonce=\"7wBYf3nwNktAkukH\", nc=00000001, qop=auth-int, \
             response=\"332c23d439013677a63a1cb7714e1618\", algorithm=MD5-sess\r\n"
        );
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            chal,
            false,
            "us\"er",
            "pw",
            "/d4/a?b=c",
            false,
            "1grJpYxDLwjm+GII",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"us\\\"er\", realm=\"r\", nonce=\"n1\", \
             uri=\"/d4/a?b=c\", cnonce=\"1grJpYxDLwjm+GII\", nc=00000001, qop=auth-int, \
             response=\"e260d4424a265f26f11053df522ef6e9\", algorithm=MD5-sess\r\n"
        );
    }

    #[test]
    fn digest_libcurl_proxy_sha512_256_sess_userhash_escapes() {
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            "Digest realm=\"r\\\"q\", nonce=\"n\\\\2\", qop=\"auth\", \
             algorithm=SHA-512-256-sess, userhash=true, opaque=\"o\\\"p\"",
            true,
            "u\\ser",
            "pw",
            "/d5/x",
            false,
            "VjlTFC8xbqYIHX0k",
        );
        assert_eq!(d.realm.as_deref(), Some(&b"r\"q"[..]));
        assert_eq!(d.nonce.as_deref(), Some(&b"n\\2"[..]));
        assert_eq!(
            got,
            "Proxy-Authorization: Digest \
             username=\"ae082134315a30a0f849d19ec3d6d86da89c5f01420f5c5b7c9351c55a1c9d04\", \
             realm=\"r\\\"q\", nonce=\"n\\\\2\", uri=\"/d5/x\", cnonce=\"VjlTFC8xbqYIHX0k\", \
             nc=00000001, qop=auth, \
             response=\"0a5e159bf1df1e919ede2d8c722b2c01a11739e58e173c3c3448ca6332e335aa\", \
             opaque=\"o\\\"p\", algorithm=SHA-512-256-sess, userhash=true\r\n"
        );
    }

    #[test]
    fn digest_libcurl_sha512_256_userhash_no_qop() {
        let mut d = DigestData::default();
        d.input(b"Digest realm=\"r\", nonce=\"n1\", algorithm=SHA-512-256, userhash=true")
            .unwrap();
        let out = d
            .output(false, b"u", b"p", b"GET", b"/d7/y", false, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            s(&out),
            "Authorization: Digest \
             username=\"c738f2ad401bc87a71fe78096015c97b9a26d55f15e9f50ac3a6de73ddcd3d08\", \
             realm=\"r\", nonce=\"n1\", uri=\"/d7/y\", \
             response=\"70491d5bb8b93e86368dff6a1318373654efceb7b59fa8ef5231bcb92c2052db\", \
             algorithm=SHA-512-256, userhash=true\r\n"
        );
    }

    #[test]
    fn digest_libcurl_stale_renonce() {
        let mut d = DigestData::default();
        let got = digest_once(
            &mut d,
            "Digest realm=\"r\", nonce=\"n1\", qop=\"auth\", algorithm=SHA-256-sess",
            false,
            "u",
            "p",
            "/d6/x",
            false,
            "Nj6lLKU4cjjnhp5l",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"u\", realm=\"r\", nonce=\"n1\", uri=\"/d6/x\", \
             cnonce=\"Nj6lLKU4cjjnhp5l\", nc=00000001, qop=auth, \
             response=\"dbde614ff0fca998bd5e0ea0a8c98c5a02649e2ea4e0b6221af0d6dacba7e1bd\", \
             algorithm=SHA-256-sess\r\n"
        );
        // stale=true: accepted although we had a nonce; a new cnonce, nc=1
        let got = digest_once(
            &mut d,
            "Digest realm=\"r\", nonce=\"n2\", qop=\"auth\", algorithm=SHA-256-sess, \
             stale=true",
            false,
            "u",
            "p",
            "/d6/x",
            false,
            "YoQmwplLvRnVP2jO",
        );
        assert_eq!(
            got,
            "Authorization: Digest username=\"u\", realm=\"r\", nonce=\"n2\", uri=\"/d6/x\", \
             cnonce=\"YoQmwplLvRnVP2jO\", nc=00000001, qop=auth, \
             response=\"c02ec6b959d2dda2dc22a5c3f9465f59de62214446a5a84bf28dcf639da54634\", \
             algorithm=SHA-256-sess\r\n"
        );
        // without stale: bad credentials
        assert_eq!(
            d.input(b"Digest realm=\"r\", nonce=\"n3\", qop=\"auth\""),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
    }

    #[test]
    fn digest_libcurl_unquoted_and_nc_counting() {
        let mut d = DigestData::default();
        let first = digest_once(
            &mut d,
            "Digest nonce=abc, realm=unq, qop=auth",
            false,
            "u",
            "p",
            "/d8/z",
            false,
            "cbovFJjIZa4auIqc",
        );
        assert_eq!(
            first,
            "Authorization: Digest username=\"u\", realm=\"unq\", nonce=\"abc\", \
             uri=\"/d8/z\", cnonce=\"cbovFJjIZa4auIqc\", nc=00000001, qop=auth, \
             response=\"4194577be9196bce05be5ecc48d29ee8\"\r\n"
        );
        // second request on the same handle: same cnonce (injection ignored), nc=2
        let second = d
            .output(false, b"u", b"p", b"GET", b"/d8/z", false, Some(b"ignored"))
            .unwrap()
            .unwrap();
        assert_eq!(
            s(&second),
            "Authorization: Digest username=\"u\", realm=\"unq\", nonce=\"abc\", \
             uri=\"/d8/z\", cnonce=\"cbovFJjIZa4auIqc\", nc=00000002, qop=auth, \
             response=\"c00b58e3ec66cf82e907172a04e22b1e\"\r\n"
        );
    }

    #[test]
    fn digest_input_errors_and_parsing() {
        let mut d = DigestData::default();
        assert_eq!(d.input(b"Digest"), Err(CURLE_BAD_CONTENT_ENCODING));
        assert_eq!(
            d.input(b"Digestrealm=\"x\""),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        assert_eq!(
            d.input(b"Basic realm=\"x\""),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        // no nonce
        assert_eq!(
            d.input(b"Digest realm=\"x\""),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        // -sess without qop
        let mut d = DigestData::default();
        assert_eq!(
            d.input(b"Digest nonce=\"x\", algorithm=MD5-sess"),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        // unknown algorithm; the nonce parsed before it stays stored
        let mut d = DigestData::default();
        assert_eq!(
            d.input(b"Digest nonce=\"x\", algorithm=SHA-1"),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        assert_eq!(d.nonce.as_deref(), Some(&b"x"[..]));
        // case-insensitive scheme / keys / algorithm, tabs as blanks
        let mut d = DigestData::default();
        d.input(b"dIgEsT\t NONCE=\"x\",\tAlgorithm=sha-256, QOP=\"auth-conf, AUTH-INT\"")
            .unwrap();
        // ... but an unquoted value keeps trailing blanks: "MD5 " is unknown
        let mut e = DigestData::default();
        assert_eq!(
            e.input(b"Digest nonce=\"x\", algorithm=MD5 , qop=auth"),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        assert_eq!(d.algo, ALGO_SHA256);
        assert_eq!(d.algorithm.as_deref(), Some(&b"sha-256"[..]));
        assert_eq!(d.qop, Some(&b"auth-int"[..]));
        // qop with only unknown values -> none selected
        let mut d = DigestData::default();
        d.input(b"Digest nonce=\"x\", qop=\"auth-conf\"").unwrap();
        assert_eq!(d.qop, None);
        // "auth " (trailing blank inside the token) does not match
        let mut d = DigestData::default();
        d.input(b"Digest nonce=\"x\", qop=\"auth ,auth-int\"")
            .unwrap();
        assert_eq!(d.qop, Some(&b"auth-int"[..]));
        // a quoted value broken by CR ends the parse: no nonce seen
        let mut d = DigestData::default();
        assert_eq!(
            d.input(b"Digest realm=\"ab\r\n\", nonce=\"x\""),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        // an unquoted value containing a quote ends the parse
        let mut d = DigestData::default();
        assert!(d.input(b"Digest nonce=\"x\", realm=a\"b").is_ok());
        assert_eq!(d.realm, None);
        // commas are kept inside quotes
        let mut d = DigestData::default();
        d.input(b"Digest realm=\"a,b\", nonce=\"x\"").unwrap();
        assert_eq!(d.realm.as_deref(), Some(&b"a,b"[..]));
        // NUL terminates, like a C string
        let mut d = DigestData::default();
        d.input(b"Digest nonce=\"x\"\0, opaque=\"y\"").unwrap();
        assert_eq!(d.opaque, None);
        // stale=false is ignored, userhash=false too
        let mut d = DigestData::default();
        d.input(b"Digest nonce=\"x\", stale=false, userhash=false")
            .unwrap();
        assert!(!d.stale && !d.userhash);
    }

    #[test]
    fn digest_get_pair_limits() {
        // a 255-byte key is accepted, 256 is not
        let k = "k".repeat(255);
        let line = format!("{k}=v");
        assert!(digest_get_pair(line.as_bytes(), 0).is_some());
        let line = format!("k{k}=v");
        assert!(digest_get_pair(line.as_bytes(), 0).is_none());
        // content is cut at 1023 bytes, the rest is left for the next round
        let v = "v".repeat(1030);
        let line = format!("k=\"{v}\"");
        let (_, content, end) = digest_get_pair(line.as_bytes(), 0).unwrap();
        assert_eq!(content.len(), 1023);
        assert_eq!(end, 3 + 1023);
        // a trailing backslash is an error
        assert!(digest_get_pair(b"k=\"ab\\", 0).is_none());
        // unquoted value ends at the comma, which is consumed
        let (k, c, end) = digest_get_pair(b"k=ab,x=1", 0).unwrap();
        assert_eq!(
            (k.as_slice(), c.as_slice(), end),
            (&b"k"[..], &b"ab"[..], 5)
        );
    }

    #[test]
    fn digest_output_without_challenge_and_random_cnonce() {
        let mut d = DigestData::default();
        assert_eq!(
            d.output(false, b"u", b"p", b"GET", b"/", false, None),
            Ok(None)
        );
        d.input(b"Digest nonce=\"x\", qop=auth").unwrap();
        let out = d
            .output(false, b"u", b"p", b"GET", b"/", false, None)
            .unwrap()
            .unwrap();
        let cn = d.cnonce.clone().unwrap();
        assert_eq!(cn.len(), 16);
        assert!(cn.iter().all(|c| B64.contains(c)));
        assert!(s(&out).contains(&format!("cnonce=\"{}\"", s(&cn))));
        assert_eq!(d.nc, 2);
    }

    // ---------------- NTLM ----------------

    #[test]
    fn ntlm_core_ms_nlmp_v1() {
        // MS-NLMP 4.2.2
        let nt = ntlm_core_mk_nt_hash(b"Password");
        assert_eq!(nt[..16], unhex("a4f49c406510bdcab6824ee7c30fd852")[..]);
        assert_eq!(nt[16..], [0u8; 5]);
        let lm = ntlm_core_mk_lm_hash(b"Password");
        assert_eq!(lm[..16], unhex("e52cac67419a9a224a3b108f3fa6cb6d")[..]);
        let chal: [u8; 8] = unhex("0123456789abcdef").try_into().unwrap();
        assert_eq!(
            ntlm_core_lm_resp(&nt, &chal).to_vec(),
            unhex("67c43011f30298a2ad35ece64f16331c44bdbed927841f94")
        );
        assert_eq!(
            ntlm_core_lm_resp(&lm, &chal).to_vec(),
            unhex("98def7b87f88aa5dafe2df779688a172def11c7d5ccdef13")
        );
    }

    #[test]
    fn ntlm_core_ms_nlmp_v2() {
        // MS-NLMP 4.2.4
        let nt = ntlm_core_mk_nt_hash(b"Password");
        let v2 = ntlm_core_mk_ntlmv2_hash(b"User", b"Domain", &nt).unwrap();
        assert_eq!(v2.to_vec(), unhex("0c868a403bfd7a93a3001ef22ef02e3f"));
        let server: [u8; 8] = unhex("0123456789abcdef").try_into().unwrap();
        let client = [0xaa; 8];
        assert_eq!(
            ntlm_core_mk_lmv2_resp(&v2, &client, &server).to_vec(),
            unhex("86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa")
        );
        let ti =
            unhex("02000c0044006f006d00610069006e00 01000c00530065007200760065007200 00000000");
        // FILETIME 0 == unix time -11644473600
        let resp = ntlm_core_mk_ntlmv2_resp(&v2, &client, &server, &ti, -11_644_473_600);
        assert_eq!(resp[..16], unhex("68cd0ab851e51c96aabc927bebef6a1c")[..]);
        assert_eq!(resp.len(), 16 + 28 + ti.len() + 4);
        assert_eq!(resp[16..24], [1, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(resp[24..32], [0u8; 8]);
        assert_eq!(resp[32..40], client);
    }

    #[test]
    fn ntlm_time2filetime() {
        assert_eq!(time2filetime(-11_644_473_600), (0, 0));
        let (lo, hi) = time2filetime(1_700_000_000);
        let ft = ((hi as u64) << 32) | lo as u64;
        assert_eq!(ft, (1_700_000_000u64 + 11_644_473_600) * 10_000_000);
    }

    #[test]
    fn ntlm_lm_hash_long_and_lowercase_password() {
        // uppercased and cut at 14 bytes
        assert_eq!(
            ntlm_core_mk_lm_hash(b"longpassword12345"),
            ntlm_core_mk_lm_hash(b"LONGPASSWORD12")
        );
    }

    const TYPE1_B64: &str = "TlRMTVNTUAABAAAABoIIAAAAAAAAAAAAAAAAAAAAAAA=";

    /// Runs a full handshake and compares the type-3 message libcurl 8.14.1
    /// produced. The random client challenge and the time are recovered from
    /// libcurl's message (LMv2 response tail and NTLMv2 blob).
    fn ntlm_flow(proxy: bool, type2: &str, user: &str, pw: &str, expect3: &str) {
        let hname = if proxy {
            "Proxy-Authorization"
        } else {
            "Authorization"
        };
        let mut info = Vec::new();
        let mut fail = Vec::new();
        let mut n = NtlmData::default();
        assert_eq!(n.state(), NTLMSTATE_NONE);
        n.input(b"NTLM", &mut info).unwrap();
        assert_eq!(n.state(), NTLMSTATE_TYPE1);
        let o = n
            .output(proxy, user.as_bytes(), pw.as_bytes(), None, None, &mut fail)
            .unwrap();
        assert_eq!(
            s(&o.header.unwrap()),
            format!("{hname}: NTLM {TYPE1_B64}\r\n")
        );
        assert!(!o.done);
        assert_eq!(n.state(), NTLMSTATE_TYPE1);

        n.input(format!("NTLM {type2}").as_bytes(), &mut info)
            .unwrap();
        assert_eq!(n.state(), NTLMSTATE_TYPE2);

        let exp = base64_decode(expect3.as_bytes()).unwrap();
        let entropy: [u8; 8] = exp[64 + 16..64 + 24].try_into().unwrap();
        let ntlen = u16::from_le_bytes([exp[20], exp[21]]) as usize;
        let ts = if ntlen > 24 {
            let ft = u64::from_le_bytes(exp[88 + 24..88 + 32].try_into().unwrap());
            Some((ft / 10_000_000) as i64 - 11_644_473_600)
        } else {
            None
        };
        let o = n
            .output(
                proxy,
                user.as_bytes(),
                pw.as_bytes(),
                Some(entropy),
                ts,
                &mut fail,
            )
            .unwrap();
        assert_eq!(
            s(&o.header.unwrap()),
            format!("{hname}: NTLM {expect3}\r\n")
        );
        assert!(o.done && !o.force_picked);
        assert_eq!(n.state(), NTLMSTATE_TYPE3);
        assert!(n.target_info.is_empty());

        // authenticated connection: nothing more is sent
        let o = n
            .output(proxy, user.as_bytes(), pw.as_bytes(), None, None, &mut fail)
            .unwrap();
        assert_eq!(
            o,
            NtlmOut {
                header: None,
                done: true,
                force_picked: true
            }
        );
        assert_eq!(n.state(), NTLMSTATE_LAST);
        assert!(info.is_empty() && fail.is_empty());

        // a new bare challenge restarts
        n.input(b"NTLM", &mut info).unwrap();
        assert_eq!(info, ["NTLM auth restarted"]);
        assert_eq!(n.state(), NTLMSTATE_TYPE1);
    }

    #[test]
    fn ntlm_libcurl_v2_unicode_target_info() {
        ntlm_flow(
            false,
            "TlRMTVNTUAACAAAADAAMADAAAAAzgoriASNFZ4mrze8AAAAAAAAAACQAJAA8AAAARABvAG0AYQBpAG4A\
             AgAMAEQAbwBtAGEAaQBuAAEADABTAGUAcgB2AGUAcgAAAAAA",
            "Domain\\User",
            "Password",
            "TlRMTVNTUAADAAAAGAAYAEAAAABUAFQAWAAAAAwADACsAAAACAAIALgAAAAWABYAwAAAAAAAAAAAAAAA\
             M4KK4unM+4rxefrl+IGRJ73CL5MfXUxNLAVAA4OAEFZEWOscdFmXLkwuHe8BAQAAAAAAAAC85IKWYd0B\
             H11MTSwFQAMAAAAAAgAMAEQAbwBtAGEAaQBuAAEADABTAGUAcgB2AGUAcgAAAAAAAAAAAEQAbwBtAGEA\
             aQBuAFUAcwBlAHIAVwBPAFIASwBTAFQAQQBUAEkATwBOAA==",
        );
    }

    #[test]
    fn ntlm_libcurl_v1_unicode() {
        // LM/NT responses equal MS-NLMP 4.2.2.2
        ntlm_flow(
            false,
            "TlRMTVNTUAACAAAADAAMACAAAAAzggLiASNFZ4mrze9EAG8AbQBhAGkAbgA=",
            "Domain\\User",
            "Password",
            "TlRMTVNTUAADAAAAGAAYAEAAAAAYABgAWAAAAAwADABwAAAACAAIAHwAAAAWABYAhAAAAAAAAAAAAAAA\
             M4IC4pje97h/iKpdr+Lfd5aIoXLe8Rx9XM3vE2fEMBHzApiirTXs5k8WMxxEvb7ZJ4QflEQAbwBtAGEA\
             aQBuAFUAcwBlAHIAVwBPAFIASwBTAFQAQQBUAEkATwBOAA==",
        );
    }

    #[test]
    fn ntlm_libcurl_v1_oem_slash_domain_long_password() {
        ntlm_flow(
            false,
            "TlRMTVNTUAACAAAABgAGACAAAAAGAgAAASNFZ4mrze9Eb21haW4=",
            "dom/usr",
            "longpassword12345",
            "TlRMTVNTUAADAAAAGAAYAEAAAAAYABgAWAAAAAMAAwBwAAAAAwADAHMAAAALAAsAdgAAAAAAAAAAAAAA\
             BgIAAL94hz/GHX76FYR9HLhwhXpfqhxjL6gEK+18zuHXS5k7SqUU1Ujs+PNzqJgTnru7VGRvbXVzcldP\
             UktTVEFUSU9O",
        );
    }

    #[test]
    fn ntlm_libcurl_v2_oem_no_target_info() {
        ntlm_flow(
            false,
            "TlRMTVNTUAACAAAABgAGACAAAAAGgggAASNFZ4mrze9Eb21haW4=",
            "usr",
            "pw",
            "TlRMTVNTUAADAAAAGAAYAEAAAAAwADAAWAAAAAAAAACIAAAAAwADAIgAAAALAAsAiwAAAAAAAAAAAAAA\
             BoIIAGhxKY7ji0y8KjqeY+Y3twlYmXnp15DVTw7EWNkRYzneiUO/SSwMbYIBAQAAAAAAAAC85IKWYd0B\
             WJl56deQ1U8AAAAAAAAAAHVzcldPUktTVEFUSU9O",
        );
    }

    #[test]
    fn ntlm_libcurl_v2_oem_target_info_proxy() {
        ntlm_flow(
            true,
            "TlRMTVNTUAACAAAABgAGADAAAAAGgogAASNFZ4mrze8AAAAAAAAAACQAJAA2AAAARG9tYWluAgAMAEQA\
             bwBtAGEAaQBuAAEADABTAGUAcgB2AGUAcgAAAAAA",
            "DOM\\usr",
            "pw",
            "TlRMTVNTUAADAAAAGAAYAEAAAABUAFQAWAAAAAMAAwCsAAAAAwADAK8AAAALAAsAsgAAAAAAAAAAAAAA\
             BoKIAAYTjMr1owGMO4w/Rn6vypsnqLe/yHLtG/v+pa8E2ikR4bhDQOXHnlcBAQAAAAAAAAC85IKWYd0B\
             J6i3v8hy7RsAAAAAAgAMAEQAbwBtAGEAaQBuAAEADABTAGUAcgB2AGUAcgAAAAAAAAAAAERPTXVzcldP\
             UktTVEFUSU9O",
        );
    }

    #[test]
    fn ntlm_input_state_machine_and_errors() {
        let mut info = Vec::new();
        let mut fail = Vec::new();
        let mut n = NtlmData::default();
        // not NTLM: ignored
        assert_eq!(n.input(b"Basic realm=x", &mut info), Ok(()));
        assert_eq!(n.state(), NTLMSTATE_NONE);
        // no prior type-1: output from NONE also sends a type-1
        let o = n.output(false, b"u", b"p", None, None, &mut fail).unwrap();
        assert_eq!(
            s(&o.header.unwrap()),
            format!("Authorization: NTLM {TYPE1_B64}\r\n")
        );
        // bad base64
        assert_eq!(
            n.input(b"NTLM abc", &mut info),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        assert!(info.is_empty());
        // too short / wrong type
        assert_eq!(
            n.input(
                b"NTLM TlRMTVNTUAABAAAABoIIAAAAAAAAAAAAAAAAAAAAAAA=",
                &mut info
            ),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        assert_eq!(info, ["NTLM handshake failure (bad type-2 message)"]);
        info.clear();
        // bare NTLM while in TYPE1 -> internal error
        n.state = NTLMSTATE_TYPE1;
        assert_eq!(
            n.input(b"NTLM  ", &mut info),
            Err(CURLE_REMOTE_ACCESS_DENIED)
        );
        assert_eq!(info, ["NTLM handshake failure (internal error)"]);
        info.clear();
        // bare NTLM after type-3 -> rejected, state reset
        n.state = NTLMSTATE_TYPE3;
        assert_eq!(n.input(b"ntlm", &mut info), Err(CURLE_REMOTE_ACCESS_DENIED));
        assert_eq!(info, ["NTLM handshake rejected"]);
        assert_eq!(n.state(), NTLMSTATE_NONE);
        info.clear();

        // bad target info offset
        let mut t2 = Vec::new();
        t2.extend_from_slice(b"NTLMSSP\0\x02\0\0\0");
        t2.extend_from_slice(&[0, 0, 0, 0, 48, 0, 0, 0]);
        t2.extend_from_slice(&(NTLMFLAG_NEGOTIATE_TARGET_INFO).to_le_bytes());
        t2.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        t2.extend_from_slice(&[0u8; 8]);
        t2.extend_from_slice(&[4, 0, 4, 0, 47, 0, 0, 0]);
        t2.extend_from_slice(&[9, 9, 9, 9]);
        let hdr = format!("NTLM {}", s(&base64_encode(&t2)));
        assert_eq!(
            n.input(hdr.as_bytes(), &mut info),
            Err(CURLE_BAD_CONTENT_ENCODING)
        );
        assert_eq!(info.len(), 2);
        assert_eq!(n.nonce, [1, 2, 3, 4, 5, 6, 7, 8]);
        // fixed: offset 48
        t2[44] = 48;
        let hdr = format!("NTLM {}", s(&base64_encode(&t2)));
        n.input(hdr.as_bytes(), &mut Vec::new()).unwrap();
        assert_eq!(n.target_info, [9, 9, 9, 9]);
        assert_eq!(n.state(), NTLMSTATE_TYPE2);
    }

    #[test]
    fn ntlm_type3_too_big() {
        let mut n = NtlmData {
            state: NTLMSTATE_TYPE2,
            flags: NTLMFLAG_NEGOTIATE_UNICODE,
            ..Default::default()
        };
        let user = "u".repeat(500);
        let mut fail = Vec::new();
        assert_eq!(
            n.output(false, user.as_bytes(), b"p", None, None, &mut fail),
            Err(CURLE_OUT_OF_MEMORY)
        );
        assert_eq!(fail, ["user + domain + hostname too big"]);
        assert_eq!(n.state(), NTLMSTATE_TYPE2);
        // large target info
        let mut n = NtlmData {
            state: NTLMSTATE_TYPE2,
            flags: NTLMFLAG_NEGOTIATE_NTLM2_KEY,
            target_info: vec![0; 1000],
            ..Default::default()
        };
        fail.clear();
        assert_eq!(
            n.output(false, b"u", b"p", None, None, &mut fail),
            Err(CURLE_OUT_OF_MEMORY)
        );
        assert_eq!(fail, ["incoming NTLM message too big"]);
    }
}
