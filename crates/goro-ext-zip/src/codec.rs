//! The data layers of libzip's sources: compression algorithms
//! (zip_algorithm_deflate.c / zip_algorithm_bzip2.c / zip_algorithm_xz.c /
//! zip_algorithm_zstd.c, the codecs being goro-zlib and compcol, and the
//! "better of deflate or store" logic of zip_source_compress.c), traditional PKWARE
//! encryption (zip_pkware.c, zip_source_pkware_{en,de}code.c) and WinZip
//! AES (zip_winzip_aes.c, zip_source_winzip_aes_{en,de}code.c).
//!
//! Sources work on whole buffers here; errors that libzip reports while
//! reading are kept to be reported after the data decoded until then.

use crate::zipint::*;

/// zip_source_compress.c's BUFSIZE
const BUFSIZE: usize = 8192;

/// zip_compression_method_supported()
pub fn compression_supported(method: i32, _compress: bool) -> bool {
    matches!(
        ZipInt::cm_actual(method),
        CM_STORE | CM_DEFLATE | CM_BZIP2 | CM_LZMA | CM_XZ | CM_ZSTD
    )
}

/// zip_encryption_method_supported()
pub fn encryption_supported(method: u16, _encode: bool) -> bool {
    matches!(
        method,
        EM_NONE | EM_TRAD_PKWARE | EM_AES_128 | EM_AES_192 | EM_AES_256
    )
}

/// algorithm->version_needed
pub fn version_needed(method: u16) -> u16 {
    match method {
        CM_BZIP2 => 46,
        // zip_algorithm_xz.c / zip_algorithm_zstd.c: 20 (WinZip does not
        // unpack 63)
        _ => 20,
    }
}

/// The result of compressing a whole input.
pub struct Compressed {
    pub data: Vec<u8>,
    /// The method actually used (store when CM_DEFAULT found deflate
    /// not worth it).
    pub method: u16,
    /// general_purpose_bit_flags of the algorithm
    pub gpbf: u16,
    pub version_needed: u16,
}

fn deflate_raw(input: &[u8], level: i32, mem_level: i32) -> Result<Vec<u8>, ZipError> {
    use goro_zlib::{Deflate, Io, Z_DEFAULT_STRATEGY, Z_DEFLATED, Z_FINISH, Z_OK, Z_STREAM_END};
    let mut d = Deflate::new(level, Z_DEFLATED, -15, mem_level, Z_DEFAULT_STRATEGY)
        .map_err(|e| ZipError::new(ER_ZLIB, e))?;
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    let mut pos = 0;
    loop {
        let mut io = Io::new(&input[pos..], &mut buf);
        let st = d.deflate(&mut io, Z_FINISH);
        pos += io.in_pos;
        let n = io.out_pos;
        out.extend_from_slice(&buf[..n]);
        if st == Z_STREAM_END {
            break;
        }
        if st != Z_OK {
            return Err(ZipError::new(ER_ZLIB, st));
        }
    }
    Ok(out)
}

/// zip_source_compress(): `method` may be CM_DEFAULT (then stored when
/// deflating a single input chunk does not make it smaller).
pub fn compress(input: &[u8], method: i32, flags: u32) -> Result<Compressed, ZipError> {
    let actual = ZipInt::cm_actual(method);
    let can_store = ZipInt::cm_is_default(method);
    let (data, gpbf) = match actual {
        CM_DEFLATE => {
            let level = if (1..=9).contains(&flags) {
                flags as i32
            } else {
                9
            };
            let mem_level = if flags == TORRENTZIP_COMPRESSION_FLAGS {
                8
            } else {
                9
            };
            let gpbf = if level < 3 {
                2 << 1
            } else if level > 7 {
                1 << 1
            } else {
                0
            };
            (deflate_raw(input, level, mem_level)?, gpbf)
        }
        CM_BZIP2 => {
            let level = if (1..=9).contains(&flags) { flags } else { 9 };
            (
                goro_ext_standard::bz2codec::compress_run_finish(input, level, 30),
                0,
            )
        }
        CM_LZMA => (lzma_compress(input, flags)?, 1 << 1),
        CM_XZ => {
            let level = if flags <= 9 { flags as u8 } else { 6 };
            let enc = compcol::xz::Encoder::with_config(compcol::xz::EncoderConfig { level });
            (encode_all(enc, input)?, 0)
        }
        CM_ZSTD => {
            // the (16-bit) flags if a zstd level, else 0: zstd's default (3)
            let level = if (1..=22).contains(&flags) {
                flags as i32
            } else {
                3
            };
            let enc = compcol::zstd::Encoder::with_config(compcol::zstd::EncoderConfig { level });
            (encode_all(enc, input)?, 0)
        }
        _ => return Err(ZipError::new(ER_COMPNOTSUPP, 0)),
    };
    // compress_read(): the whole input fit the first read and the output
    // of that read is not smaller -> keep it stored.
    if can_store && input.len() <= BUFSIZE && data.len() <= BUFSIZE && input.len() <= data.len() {
        return Ok(Compressed {
            data: input.to_vec(),
            method: CM_STORE,
            gpbf: 0,
            version_needed: version_needed(actual),
        });
    }
    Ok(Compressed {
        data,
        method: actual,
        gpbf,
        version_needed: version_needed(actual),
    })
}

/// zip_source_decompress() of a whole stream: the data decoded and the
/// error hit (if any). `size`: the uncompressed size for an LZMA stream
/// written without end-of-stream marker (general purpose bits 1 and 2
/// clear). A truncated stream ends the data without error (libzip leaves
/// that to the CRC check).
pub fn decompress(input: &[u8], method: u16, size: Option<u64>) -> (Vec<u8>, Option<ZipError>) {
    match method {
        CM_STORE => (input.to_vec(), None),
        CM_DEFLATE => inflate_raw(input),
        CM_BZIP2 => match goro_ext_standard::bz2codec::decompress(input) {
            (out, Ok(_)) => (out, None),
            (out, Err(_)) => (out, Some(ZipError::new(ER_COMPRESSED_DATA, 0))),
        },
        CM_LZMA => {
            // 2 bytes LZMA SDK version, 2 bytes properties size (5), the
            // properties: rebuilt into an "lzma alone" header
            if input.len() < 9 {
                return (Vec::new(), None);
            }
            if input[2] != 0x05 || input[3] != 0x00 {
                return (Vec::new(), Some(ZipError::new(ER_COMPRESSED_DATA, 0)));
            }
            let mut alone = Vec::with_capacity(input.len() + 4);
            alone.extend_from_slice(&input[4..9]);
            alone.extend_from_slice(&size.unwrap_or(u64::MAX).to_le_bytes());
            alone.extend_from_slice(&input[9..]);
            decode_all(compcol::lzma::Decoder::new(), &alone)
        }
        CM_XZ => decode_all(compcol::xz::Decoder::new(), input),
        CM_ZSTD => decode_all(compcol::zstd::Decoder::new(), input),
        _ => (Vec::new(), Some(ZipError::new(ER_COMPNOTSUPP, 0))),
    }
}

/// zip_algorithm_xz.c for ZIP_CM_LZMA: the "lzma alone" stream with its
/// 8-byte size replaced by the LZMA SDK version (9.20) and properties
/// size; an end-of-stream marker ends the data.
fn lzma_compress(input: &[u8], flags: u32) -> Result<Vec<u8>, ZipError> {
    let level = if flags <= 9 { flags as u8 } else { 6 };
    let enc = compcol::lzma::Encoder::with_config(compcol::lzma::EncoderConfig { level });
    let alone = encode_all(enc, input)?;
    if alone.len() < 13 {
        return Err(ZipError::new(ER_INTERNAL, 0));
    }
    let mut out = Vec::with_capacity(alone.len() - 4);
    out.extend_from_slice(&[0x09, 0x14, 0x05, 0x00]);
    out.extend_from_slice(&alone[..5]);
    out.extend_from_slice(&alone[13..]);
    Ok(out)
}

fn encode_all<E: compcol::Encoder>(mut enc: E, input: &[u8]) -> Result<Vec<u8>, ZipError> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    let mut pos = 0;
    while pos < input.len() {
        let (p, _) = enc
            .encode(&input[pos..], &mut buf)
            .map_err(|_| ZipError::new(ER_INTERNAL, 0))?;
        out.extend_from_slice(&buf[..p.written]);
        pos += p.consumed;
    }
    loop {
        let (p, s) = enc
            .finish(&mut buf)
            .map_err(|_| ZipError::new(ER_INTERNAL, 0))?;
        out.extend_from_slice(&buf[..p.written]);
        if s == compcol::Status::StreamEnd {
            return Ok(out);
        }
    }
}

fn decode_all<D: compcol::Decoder>(mut dec: D, input: &[u8]) -> (Vec<u8>, Option<ZipError>) {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    let mut pos = 0;
    loop {
        match dec.decode(&input[pos..], &mut buf) {
            Ok((p, s)) => {
                out.extend_from_slice(&buf[..p.written]);
                pos += p.consumed;
                if s == compcol::Status::StreamEnd {
                    return (out, None);
                }
                if pos >= input.len() && p.written < buf.len() {
                    break;
                }
            }
            Err(_) => return (out, Some(ZipError::new(ER_COMPRESSED_DATA, 0))),
        }
    }
    // end of input: drain what the codec still holds
    loop {
        match dec.finish(&mut buf) {
            Ok((p, s)) => {
                out.extend_from_slice(&buf[..p.written]);
                if s == compcol::Status::StreamEnd || p.written == 0 {
                    return (out, None);
                }
            }
            // truncated: the CRC check reports it
            Err(compcol::Error::UnexpectedEnd) => return (out, None),
            Err(_) => return (out, Some(ZipError::new(ER_COMPRESSED_DATA, 0))),
        }
    }
}

fn inflate_raw(input: &[u8]) -> (Vec<u8>, Option<ZipError>) {
    use goro_zlib::{Inflate, Io, Z_BUF_ERROR, Z_OK, Z_STREAM_END, Z_SYNC_FLUSH};
    let mut inf = match Inflate::new(-15) {
        Ok(i) => i,
        Err(e) => return (Vec::new(), Some(ZipError::new(ER_ZLIB, e))),
    };
    let mut out = Vec::new();
    let mut buf = vec![0u8; 65536];
    let mut pos = 0;
    loop {
        let mut io = Io::new(&input[pos..], &mut buf);
        let st = inf.inflate(&mut io, Z_SYNC_FLUSH);
        let consumed = io.in_pos;
        let n = io.out_pos;
        pos += consumed;
        out.extend_from_slice(&buf[..n]);
        match st {
            Z_STREAM_END => return (out, None),
            Z_OK => {
                if n == 0 && consumed == 0 {
                    // no progress possible: input exhausted
                    return (out, None);
                }
            }
            Z_BUF_ERROR => {
                if pos >= input.len() {
                    // NEED_DATA at end of input: the stream is truncated
                    return (out, None);
                }
                return (out, Some(ZipError::new(ER_ZLIB, st)));
            }
            e => return (out, Some(ZipError::new(ER_ZLIB, e))),
        }
    }
}

// ---------------------------------------------------------------------------
// traditional PKWARE encryption

const PKWARE_HEADERLEN: usize = 12;

struct PkKeys([u32; 3]);

fn crc_byte(crc: u32, b: u8) -> u32 {
    goro_zlib::crc32(crc ^ 0xffff_ffff, &[b]) ^ 0xffff_ffff
}

impl PkKeys {
    fn new(password: &[u8]) -> PkKeys {
        let mut k = PkKeys([305419896, 591751049, 878082192]);
        for &b in password {
            k.update(b);
        }
        k
    }

    fn update(&mut self, b: u8) {
        self.0[0] = crc_byte(self.0[0], b);
        self.0[1] = self.0[1]
            .wrapping_add(self.0[0] & 0xff)
            .wrapping_mul(134775813)
            .wrapping_add(1);
        let b = (self.0[1] >> 24) as u8;
        self.0[2] = crc_byte(self.0[2], b);
    }

    fn crypt_byte(&self) -> u8 {
        let tmp = (self.0[2] | 2) as u16;
        (((tmp as u32) * ((tmp ^ 1) as u32)) >> 8) as u8
    }

    fn encrypt(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            let t = self.crypt_byte();
            self.update(*b);
            *b ^= t;
        }
    }

    fn decrypt(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            let t = self.crypt_byte();
            *b ^= t;
            self.update(*b);
        }
    }
}

fn random_bytes(buf: &mut [u8]) {
    use purecrypto::rng::RngCore;
    purecrypto::rng::OsRng.fill_bytes(buf);
}

/// zip_source_pkware_encode: header (random + high byte of the DOS time)
/// then the encrypted data.
pub fn pkware_encrypt(data: &[u8], password: &[u8], dostime: u16) -> Vec<u8> {
    let mut keys = PkKeys::new(password);
    let mut header = [0u8; PKWARE_HEADERLEN];
    random_bytes(&mut header[..PKWARE_HEADERLEN - 1]);
    header[PKWARE_HEADERLEN - 1] = (dostime >> 8) as u8;
    keys.encrypt(&mut header);
    let mut out = header.to_vec();
    let mut body = data.to_vec();
    keys.encrypt(&mut body);
    out.extend_from_slice(&body);
    out
}

/// zip_source_pkware_decode: `check` holds the password check values
/// (DOS time high byte, CRC high byte) when the entry has a CRC.
pub fn pkware_decrypt(
    data: &[u8],
    password: &[u8],
    check: Option<(u8, u8)>,
) -> Result<Vec<u8>, ZipError> {
    if data.len() < PKWARE_HEADERLEN {
        return Err(ZipError::new(ER_EOF, 0));
    }
    let mut keys = PkKeys::new(password);
    let mut header = [0u8; PKWARE_HEADERLEN];
    header.copy_from_slice(&data[..PKWARE_HEADERLEN]);
    keys.decrypt(&mut header);
    if let Some((t, c)) = check {
        let h = header[PKWARE_HEADERLEN - 1];
        if h != t && h != c {
            return Err(ZipError::new(ER_WRONGPASSWD, 0));
        }
    }
    let mut body = data[PKWARE_HEADERLEN..].to_vec();
    keys.decrypt(&mut body);
    Ok(body)
}

// ---------------------------------------------------------------------------
// WinZip AES

const HMAC_LENGTH: usize = 10;
const PASSWORD_VERIFY_LENGTH: usize = 2;

pub fn salt_length(method: u16) -> usize {
    match method {
        EM_AES_128 => 8,
        EM_AES_192 => 12,
        _ => 16,
    }
}

enum AesKey {
    K128(purecrypto::cipher::Aes128),
    K192(purecrypto::cipher::Aes192),
    K256(purecrypto::cipher::Aes256),
}

impl AesKey {
    fn encrypt_block(&self, b: &mut [u8; 16]) {
        use purecrypto::cipher::BlockCipher;
        match self {
            AesKey::K128(k) => k.encrypt_block(b),
            AesKey::K192(k) => k.encrypt_block(b),
            AesKey::K256(k) => k.encrypt_block(b),
        }
    }
}

struct WinzipAes {
    aes: AesKey,
    hmac: purecrypto::hash::Hmac<purecrypto::hash::Sha1>,
    counter: [u8; 16],
    pad: [u8; 16],
    pad_offset: usize,
}

impl WinzipAes {
    /// _zip_winzip_aes_new(): the context and the password verifier
    fn new(password: &[u8], salt: &[u8], method: u16) -> Option<(WinzipAes, [u8; 2])> {
        let key_len = match method {
            EM_AES_128 => 16,
            EM_AES_192 => 24,
            EM_AES_256 => 32,
            _ => return None,
        };
        if password.is_empty() {
            return None;
        }
        let mut buf = vec![0u8; 2 * key_len + PASSWORD_VERIFY_LENGTH];
        purecrypto::kdf::pbkdf2::<purecrypto::hash::Sha1>(password, salt, 1000, &mut buf);
        let aes = match key_len {
            16 => AesKey::K128(purecrypto::cipher::Aes128::new(buf[..16].try_into().ok()?)),
            24 => AesKey::K192(purecrypto::cipher::Aes192::new(buf[..24].try_into().ok()?)),
            _ => AesKey::K256(purecrypto::cipher::Aes256::new(buf[..32].try_into().ok()?)),
        };
        let hmac =
            purecrypto::hash::Hmac::<purecrypto::hash::Sha1>::new(&buf[key_len..2 * key_len]);
        let verify = [buf[2 * key_len], buf[2 * key_len + 1]];
        Some((
            WinzipAes {
                aes,
                hmac,
                counter: [0; 16],
                pad: [0; 16],
                pad_offset: 16,
            },
            verify,
        ))
    }

    fn crypt(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            if self.pad_offset == 16 {
                for j in 0..8 {
                    self.counter[j] = self.counter[j].wrapping_add(1);
                    if self.counter[j] != 0 {
                        break;
                    }
                }
                let mut blk = self.counter;
                self.aes.encrypt_block(&mut blk);
                self.pad = blk;
                self.pad_offset = 0;
            }
            *b ^= self.pad[self.pad_offset];
            self.pad_offset += 1;
        }
    }
}

/// zip_source_winzip_aes_encode: salt, password verifier, data, HMAC.
pub fn aes_encrypt(data: &[u8], password: &[u8], method: u16) -> Result<Vec<u8>, ZipError> {
    let mut salt = vec![0u8; salt_length(method)];
    random_bytes(&mut salt);
    let (mut ctx, verify) =
        WinzipAes::new(password, &salt, method).ok_or(ZipError::new(ER_INVAL, 0))?;
    let mut out = salt;
    out.extend_from_slice(&verify);
    let mut body = data.to_vec();
    ctx.crypt(&mut body);
    ctx.hmac.update(&body);
    out.extend_from_slice(&body);
    let mac = ctx.hmac.finalize();
    out.extend_from_slice(&mac.as_ref()[..HMAC_LENGTH]);
    Ok(out)
}

/// zip_source_winzip_aes_decode: Err for the open-time errors (bad
/// password); the data and the end-of-data error (HMAC mismatch) otherwise.
pub fn aes_decrypt(
    data: &[u8],
    password: &[u8],
    method: u16,
) -> Result<(Vec<u8>, Option<ZipError>), ZipError> {
    let sl = salt_length(method);
    let aux = PASSWORD_VERIFY_LENGTH + sl + HMAC_LENGTH;
    if data.len() < aux {
        return Err(ZipError::new(ER_OPNOTSUPP, 0));
    }
    let salt = &data[..sl];
    let pv = &data[sl..sl + PASSWORD_VERIFY_LENGTH];
    let (mut ctx, verify) =
        WinzipAes::new(password, salt, method).ok_or(ZipError::new(ER_INVAL, 0))?;
    if pv != verify {
        return Err(ZipError::new(ER_WRONGPASSWD, 0));
    }
    let body_end = data.len() - HMAC_LENGTH;
    let mut body = data[sl + PASSWORD_VERIFY_LENGTH..body_end].to_vec();
    ctx.hmac.update(&body);
    ctx.crypt(&mut body);
    let mac = ctx.hmac.finalize();
    let err = if mac.as_ref()[..HMAC_LENGTH] != data[body_end..] {
        Some(ZipError::new(ER_CRC, 0))
    } else {
        None
    };
    Ok((body, err))
}
