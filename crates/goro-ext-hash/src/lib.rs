use goro_core::array::PhpArray;
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};
use std::cell::RefCell;
use std::rc::Rc;

/// Register all hash extension functions
pub fn register(vm: &mut Vm) {
    vm.register_function(b"crc32", crc32_fn);
    vm.register_function(b"md5", md5_fn);
    vm.register_function(b"sha1", sha1_fn);
    vm.register_function(b"hash", hash_fn);
    vm.register_function(b"hash_algos", hash_algos_fn);
    vm.register_function(b"hash_equals", hash_equals_fn);
    vm.register_function(b"hash_hmac", hash_hmac_fn);
    vm.register_function(b"hash_init", hash_init_fn);
    vm.register_function(b"hash_update", hash_update_fn);
    vm.register_function(b"hash_final", hash_final_fn);
    vm.register_function(b"hash_copy", hash_copy_fn);
    vm.register_function(b"hash_pbkdf2", hash_pbkdf2_fn);
    vm.register_function(b"hash_hmac_algos", hash_hmac_algos_fn);
    vm.register_function(b"hash_file", hash_file_fn);
    vm.register_function(b"hash_hkdf", hash_hkdf_fn);
    vm.register_function(b"md5_file", md5_file_fn);
    vm.register_function(b"sha1_file", sha1_file_fn);
}

fn crc32_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let bytes = data.as_bytes();
    // CRC32B (standard CRC32 used by PHP's crc32() function)
    let crc = crc32b_compute(bytes);
    // PHP 64-bit returns unsigned 32-bit value as positive integer
    Ok(Value::Long(crc as i64))
}

fn md5_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let digest = md5_hash(data.as_bytes());
    if raw {
        Ok(Value::String(PhpString::from_vec(digest.to_vec())))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

fn sha1_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let data = args.first().unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let digest = sha1_hash(data.as_bytes());
    if raw {
        Ok(Value::String(PhpString::from_vec(digest.to_vec())))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

fn compute_hash(algo: &str, data: &[u8]) -> Option<Vec<u8>> {
    match algo {
        "md5" => Some(md5_hash(data).to_vec()),
        "sha1" | "sha-1" => Some(sha1_hash(data).to_vec()),
        "sha256" | "sha-256" => Some(sha256_hash(data).to_vec()),
        "sha384" | "sha-384" => Some(sha384_hash(data)),
        "sha512" | "sha-512" => Some(sha512_hash(data)),
        "sha224" | "sha-224" => Some(sha224_hash(data).to_vec()),
        "crc32" => Some(crc32_compute(data).to_le_bytes().to_vec()),
        "crc32b" => Some(crc32b_compute(data).to_be_bytes().to_vec()),
        "crc32c" => Some(crc32c_compute(data).to_be_bytes().to_vec()),
        "adler32" => Some(adler32_compute(data).to_be_bytes().to_vec()),
        "fnv132" => Some(fnv1_32(data).to_be_bytes().to_vec()),
        "fnv1a32" => Some(fnv1a_32(data).to_be_bytes().to_vec()),
        "fnv164" => Some(fnv1_64(data).to_be_bytes().to_vec()),
        "fnv1a64" => Some(fnv1a_64(data).to_be_bytes().to_vec()),
        "md4" => Some(md4_hash(data).to_vec()),
        _ => None,
    }
}

fn hash_block_size(algo: &str) -> usize {
    match algo {
        "md5" | "md4" => 64,
        "sha1" | "sha-1" | "sha256" | "sha-256" | "sha224" | "sha-224" => 64,
        "sha384" | "sha-384" | "sha512" | "sha-512" => 128,
        _ => 64,
    }
}

fn hash_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);
    let algo_lower = algo.to_ascii_lowercase();

    match compute_hash(&algo_lower, data.as_bytes()) {
        Some(digest) => {
            if raw {
                Ok(Value::String(PhpString::from_vec(digest)))
            } else {
                let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(Value::String(PhpString::from_string(hex)))
            }
        }
        None => Ok(Value::False),
    }
}

fn hash_hmac_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args
        .first()
        .unwrap_or(&Value::Null)
        .to_php_string()
        .to_string_lossy()
        .to_ascii_lowercase();
    let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let key = args.get(2).unwrap_or(&Value::Null).to_php_string();
    let raw = args.get(3).map(|v| v.is_truthy()).unwrap_or(false);

    let block_size = hash_block_size(&algo);

    // HMAC implementation
    let mut key_bytes = key.as_bytes().to_vec();
    if key_bytes.len() > block_size {
        key_bytes = match compute_hash(&algo, &key_bytes) {
            Some(h) => h,
            None => return Ok(Value::False),
        };
    }
    while key_bytes.len() < block_size {
        key_bytes.push(0);
    }

    let mut i_key_pad = vec![0u8; block_size];
    let mut o_key_pad = vec![0u8; block_size];
    for i in 0..block_size {
        i_key_pad[i] = key_bytes[i] ^ 0x36;
        o_key_pad[i] = key_bytes[i] ^ 0x5c;
    }

    // inner hash
    let mut inner_data = i_key_pad;
    inner_data.extend_from_slice(data.as_bytes());
    let inner_hash = match compute_hash(&algo, &inner_data) {
        Some(h) => h,
        None => return Ok(Value::False),
    };

    // outer hash
    let mut outer_data = o_key_pad;
    outer_data.extend_from_slice(&inner_hash);
    let digest = match compute_hash(&algo, &outer_data) {
        Some(h) => h,
        None => return Ok(Value::False),
    };

    if raw {
        Ok(Value::String(PhpString::from_vec(digest)))
    } else {
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

/// hash_init(): Create a HashContext object (we use an Object with algo + data properties)
fn hash_init_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_ascii_lowercase();
    let options = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    let key = args.get(2).map(|v| v.to_php_string());

    // Verify algorithm is valid
    if compute_hash(&algo, b"").is_none() {
        let exc = vm.create_exception(b"ValueError", &format!("hash_init(): Argument #1 ($algo) must be a valid hashing algorithm, \"{}\" given", algo), 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: format!("hash_init(): Unknown hashing algorithm: {}", algo), line: 0 });
    }

    let obj_id = vm.next_object_id();
    let mut obj = goro_core::object::PhpObject::new(b"HashContext".to_vec(), obj_id);
    obj.set_property(b"algo".to_vec(), Value::String(PhpString::from_string(algo.clone())));
    obj.set_property(b"data".to_vec(), Value::String(PhpString::from_vec(Vec::new())));

    // HASH_HMAC = 1
    if options & 1 != 0 {
        if let Some(k) = key {
            let block_size = hash_block_size(&algo);
            let mut key_bytes = k.as_bytes().to_vec();
            if key_bytes.len() > block_size {
                key_bytes = compute_hash(&algo, &key_bytes).unwrap_or_default();
            }
            while key_bytes.len() < block_size {
                key_bytes.push(0);
            }
            obj.set_property(b"hmac_key".to_vec(), Value::String(PhpString::from_vec(key_bytes)));
            obj.set_property(b"is_hmac".to_vec(), Value::True);
        }
    }

    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

fn hash_update_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let data = args.get(1).unwrap_or(&Value::Null).to_php_string();
        let mut obj = obj.borrow_mut();
        let existing = obj.get_property(b"data").to_php_string();
        let mut combined = existing.as_bytes().to_vec();
        combined.extend_from_slice(data.as_bytes());
        obj.set_property(b"data".to_vec(), Value::String(PhpString::from_vec(combined)));
        return Ok(Value::True);
    }
    Ok(Value::False)
}

fn hash_final_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
        let obj_borrow = obj.borrow();
        let algo = obj_borrow.get_property(b"algo").to_php_string().to_string_lossy().to_ascii_lowercase();
        let data = obj_borrow.get_property(b"data").to_php_string();
        let is_hmac = obj_borrow.get_property(b"is_hmac").is_truthy();
        let hmac_key_val = obj_borrow.get_property(b"hmac_key");
        let hmac_key = hmac_key_val.to_php_string();
        drop(obj_borrow);

        let digest = if is_hmac {
            let block_size = hash_block_size(&algo);
            let key_bytes = hmac_key.as_bytes();

            let mut i_key_pad = vec![0u8; block_size];
            let mut o_key_pad = vec![0u8; block_size];
            for i in 0..block_size {
                let kb = if i < key_bytes.len() { key_bytes[i] } else { 0 };
                i_key_pad[i] = kb ^ 0x36;
                o_key_pad[i] = kb ^ 0x5c;
            }

            let mut inner_data = i_key_pad;
            inner_data.extend_from_slice(data.as_bytes());
            let inner_hash = compute_hash(&algo, &inner_data).unwrap_or_default();

            let mut outer_data = o_key_pad;
            outer_data.extend_from_slice(&inner_hash);
            compute_hash(&algo, &outer_data).unwrap_or_default()
        } else {
            compute_hash(&algo, data.as_bytes()).unwrap_or_default()
        };

        if raw {
            return Ok(Value::String(PhpString::from_vec(digest)));
        } else {
            let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
            return Ok(Value::String(PhpString::from_string(hex)));
        }
    }
    Ok(Value::False)
}

fn hash_copy_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Object(obj)) = args.first() {
        let obj_borrow = obj.borrow();
        let algo = obj_borrow.get_property(b"algo");
        let data = obj_borrow.get_property(b"data");
        let is_hmac = obj_borrow.get_property(b"is_hmac");
        let hmac_key = obj_borrow.get_property(b"hmac_key");
        drop(obj_borrow);

        let new_id = 0; // simplified
        let mut new_obj = goro_core::object::PhpObject::new(b"HashContext".to_vec(), new_id);
        new_obj.set_property(b"algo".to_vec(), algo);
        new_obj.set_property(b"data".to_vec(), data);
        new_obj.set_property(b"is_hmac".to_vec(), is_hmac);
        new_obj.set_property(b"hmac_key".to_vec(), hmac_key);
        return Ok(Value::Object(Rc::new(RefCell::new(new_obj))));
    }
    Ok(Value::False)
}

fn hash_pbkdf2_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_ascii_lowercase();
    let password = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let salt = args.get(2).unwrap_or(&Value::Null).to_php_string();
    let iterations = args.get(3).map(|v| v.to_long()).unwrap_or(1) as usize;
    let length = args.get(4).map(|v| v.to_long()).unwrap_or(0) as usize;
    let raw = args.get(5).map(|v| v.is_truthy()).unwrap_or(false);

    let hash_len = compute_hash(&algo, b"").map(|h| h.len()).unwrap_or(0);
    if hash_len == 0 {
        return Ok(Value::False);
    }

    let output_len = if length == 0 { hash_len } else if raw { length } else { length / 2 };

    // PBKDF2 implementation
    let block_size = hash_block_size(&algo);
    let mut key_bytes = password.as_bytes().to_vec();
    if key_bytes.len() > block_size {
        key_bytes = compute_hash(&algo, &key_bytes).unwrap_or_default();
    }
    while key_bytes.len() < block_size {
        key_bytes.push(0);
    }

    let hmac = |data: &[u8]| -> Vec<u8> {
        let mut i_pad = vec![0u8; block_size];
        let mut o_pad = vec![0u8; block_size];
        for i in 0..block_size {
            i_pad[i] = key_bytes[i] ^ 0x36;
            o_pad[i] = key_bytes[i] ^ 0x5c;
        }
        let mut inner = i_pad;
        inner.extend_from_slice(data);
        let inner_hash = compute_hash(&algo, &inner).unwrap_or_default();
        let mut outer = o_pad;
        outer.extend_from_slice(&inner_hash);
        compute_hash(&algo, &outer).unwrap_or_default()
    };

    let mut result = Vec::new();
    let blocks = (output_len + hash_len - 1) / hash_len;

    for block in 1..=blocks {
        let mut salt_block = salt.as_bytes().to_vec();
        salt_block.extend_from_slice(&(block as u32).to_be_bytes());
        let mut u = hmac(&salt_block);
        let mut t = u.clone();
        for _ in 1..iterations {
            u = hmac(&u);
            for j in 0..t.len() {
                t[j] ^= u[j];
            }
        }
        result.extend_from_slice(&t);
    }

    result.truncate(output_len);

    if raw {
        Ok(Value::String(PhpString::from_vec(result)))
    } else {
        let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
        let hex = if length > 0 && hex.len() > length {
            hex[..length].to_string()
        } else {
            hex
        };
        Ok(Value::String(PhpString::from_string(hex)))
    }
}

fn hash_hmac_algos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for algo in &[
        "md4", "md5", "sha1", "sha224", "sha256", "sha384", "sha512",
    ] {
        result.push(Value::String(PhpString::from_bytes(algo.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn hash_algos_fn(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut result = PhpArray::new();
    for algo in &[
        "md4", "md5", "sha1", "sha224", "sha256", "sha384", "sha512",
        "fnv132", "fnv1a32", "fnv164", "fnv1a64",
        "adler32", "crc32", "crc32b", "crc32c",
    ] {
        result.push(Value::String(PhpString::from_bytes(algo.as_bytes())));
    }
    Ok(Value::Array(Rc::new(RefCell::new(result))))
}

fn hash_equals_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let known = args.first().unwrap_or(&Value::Null).to_php_string();
    let user = args.get(1).unwrap_or(&Value::Null).to_php_string();
    Ok(if known.as_bytes() == user.as_bytes() {
        Value::True
    } else {
        Value::False
    })
}

// --- CRC32 variants ---

/// CRC32 (bzip2 variant) - used by hash('crc32', ...)
fn crc32_compute(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            if crc & 0x80000000 != 0 {
                crc = (crc << 1) ^ 0x04C11DB7;
            } else {
                crc <<= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

/// CRC32B (ISO 3309 / ITU-T V.42) - used by PHP's crc32() function and hash('crc32b', ...)
fn crc32b_compute(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

/// CRC32C (Castagnoli) - used by hash('crc32c', ...)
fn crc32c_compute(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x82F63B78;
            } else {
                crc >>= 1;
            }
        }
    }
    crc ^ 0xFFFFFFFF
}

// --- ADLER32 ---

fn adler32_compute(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

// --- FNV hash algorithms ---

fn fnv1_32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for &byte in data {
        hash = hash.wrapping_mul(0x01000193);
        hash ^= byte as u32;
    }
    hash
}

fn fnv1a_32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn fnv1_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash = hash.wrapping_mul(0x00000100000001B3);
        hash ^= byte as u64;
    }
    hash
}

fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x00000100000001B3);
    }
    hash
}

// --- MD4 hash implementation ---

fn md4_hash(data: &[u8]) -> [u8; 16] {
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut x = [0u32; 16];
        for i in 0..16 {
            x[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        // Round 1
        for &i in &[0, 4, 8, 12] {
            a = a.wrapping_add((b & c) | ((!b) & d)).wrapping_add(x[i]).rotate_left(3);
            d = d.wrapping_add((a & b) | ((!a) & c)).wrapping_add(x[i + 1]).rotate_left(7);
            c = c.wrapping_add((d & a) | ((!d) & b)).wrapping_add(x[i + 2]).rotate_left(11);
            b = b.wrapping_add((c & d) | ((!c) & a)).wrapping_add(x[i + 3]).rotate_left(19);
        }

        // Round 2
        for &i in &[0, 1, 2, 3] {
            a = a.wrapping_add((b & c) | (b & d) | (c & d)).wrapping_add(x[i]).wrapping_add(0x5A827999).rotate_left(3);
            d = d.wrapping_add((a & b) | (a & c) | (b & c)).wrapping_add(x[i + 4]).wrapping_add(0x5A827999).rotate_left(5);
            c = c.wrapping_add((d & a) | (d & b) | (a & b)).wrapping_add(x[i + 8]).wrapping_add(0x5A827999).rotate_left(9);
            b = b.wrapping_add((c & d) | (c & a) | (d & a)).wrapping_add(x[i + 12]).wrapping_add(0x5A827999).rotate_left(13);
        }

        // Round 3
        for &i in &[0, 2, 1, 3] {
            a = a.wrapping_add(b ^ c ^ d).wrapping_add(x[i]).wrapping_add(0x6ED9EBA1).rotate_left(3);
            d = d.wrapping_add(a ^ b ^ c).wrapping_add(x[i + 8]).wrapping_add(0x6ED9EBA1).rotate_left(9);
            c = c.wrapping_add(d ^ a ^ b).wrapping_add(x[i + 4]).wrapping_add(0x6ED9EBA1).rotate_left(11);
            b = b.wrapping_add(c ^ d ^ a).wrapping_add(x[i + 12]).wrapping_add(0x6ED9EBA1).rotate_left(15);
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
}

/// MD5 hash implementation (RFC 1321)
fn md5_hash(data: &[u8]) -> [u8; 16] {
    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // Pre-processing: adding padding bits
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    // Per-round shift amounts
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(m[g])).rotate_left(s[i]),
            );
            a = temp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut result = [0u8; 16];
    result[0..4].copy_from_slice(&a0.to_le_bytes());
    result[4..8].copy_from_slice(&b0.to_le_bytes());
    result[8..12].copy_from_slice(&c0.to_le_bytes());
    result[12..16].copy_from_slice(&d0.to_le_bytes());
    result
}

/// SHA1 hash implementation (FIPS 180-1)
fn sha1_hash(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut result = [0u8; 20];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result
}

/// SHA-256 hash implementation (FIPS 180-4)
fn sha256_hash(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for i in 0..8 {
        result[i * 4..(i + 1) * 4].copy_from_slice(&h[i].to_be_bytes());
    }
    result
}

/// SHA-224 hash (truncated SHA-256 with different IVs)
fn sha224_hash(data: &[u8]) -> [u8; 28] {
    let mut h: [u32; 8] = [
        0xc1059ed8, 0x367cd507, 0x3070dd17, 0xf70e5939,
        0xffc00b31, 0x68581511, 0x64f98fa7, 0xbefa4fa4,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 28];
    for i in 0..7 {
        result[i * 4..(i + 1) * 4].copy_from_slice(&h[i].to_be_bytes());
    }
    result
}

/// SHA-512 hash implementation (FIPS 180-4)
fn sha512_hash(data: &[u8]) -> Vec<u8> {
    sha512_core(data, &[
        0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
        0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
    ], 64)
}

/// SHA-384 hash (truncated SHA-512 with different IVs)
fn sha384_hash(data: &[u8]) -> Vec<u8> {
    sha512_core(data, &[
        0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939,
        0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4,
    ], 48)
}

fn sha512_core(data: &[u8], init: &[u64; 8], output_len: usize) -> Vec<u8> {
    let mut h = *init;

    let k: [u64; 80] = [
        0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
        0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
        0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
        0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
        0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
        0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
        0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
        0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
        0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
        0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
        0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
        0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
        0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
        0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
    ];

    let bit_len = (data.len() as u128) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            w[i] = u64::from_be_bytes([
                chunk[i * 8], chunk[i * 8 + 1], chunk[i * 8 + 2], chunk[i * 8 + 3],
                chunk[i * 8 + 4], chunk[i * 8 + 5], chunk[i * 8 + 6], chunk[i * 8 + 7],
            ]);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = Vec::with_capacity(64);
    for val in &h {
        result.extend_from_slice(&val.to_be_bytes());
    }
    result.truncate(output_len);
    result
}

fn hash_file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_ascii_lowercase();
    let filename = args.get(1).unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let raw = args.get(2).map(|v| v.is_truthy()).unwrap_or(false);

    match std::fs::read(&filename) {
        Ok(data) => {
            match compute_hash(&algo, &data) {
                Some(digest) => {
                    if raw {
                        Ok(Value::String(PhpString::from_vec(digest)))
                    } else {
                        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
                        Ok(Value::String(PhpString::from_string(hex)))
                    }
                }
                None => Ok(Value::False),
            }
        }
        Err(_) => Ok(Value::False),
    }
}

fn hash_hkdf_fn(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let algo = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy().to_ascii_lowercase();
    let ikm = args.get(1).unwrap_or(&Value::Null).to_php_string();
    let length = args.get(2).map(|v| v.to_long()).unwrap_or(0);
    let info = args.get(3).unwrap_or(&Value::Null).to_php_string();
    let salt = args.get(4).unwrap_or(&Value::Null).to_php_string();

    let hash_len = match algo.as_str() {
        "md5" | "md4" => 16,
        "sha1" => 20,
        "sha224" => 28,
        "sha256" => 32,
        "sha384" => 48,
        "sha512" => 64,
        _ => {
            let msg = format!("hash_hkdf(): Argument #1 ($algo) must be a valid hashing algorithm, \"{}\" given", algo);
            let exc = vm.create_exception(b"ValueError", &msg, 0);
            vm.current_exception = Some(exc);
            return Err(VmError { message: msg, line: 0 });
        }
    };

    let output_len = if length == 0 { hash_len } else { length as usize };
    if output_len > 255 * hash_len {
        let msg = "hash_hkdf(): Argument #3 ($length) must be greater than or equal to 0 and less than or equal to 255 * HashLen";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: 0 });
    }

    if ikm.as_bytes().is_empty() {
        let msg = "hash_hkdf(): Argument #2 ($key) must not be empty";
        let exc = vm.create_exception(b"ValueError", msg, 0);
        vm.current_exception = Some(exc);
        return Err(VmError { message: msg.to_string(), line: 0 });
    }

    // HKDF-Extract: PRK = HMAC-Hash(salt, IKM)
    let salt_bytes = if salt.as_bytes().is_empty() {
        vec![0u8; hash_len]
    } else {
        salt.as_bytes().to_vec()
    };
    let prk = compute_hmac(&algo, &salt_bytes, ikm.as_bytes());

    // HKDF-Expand
    let mut okm = Vec::with_capacity(output_len);
    let mut t = Vec::new();
    let mut counter: u8 = 1;
    while okm.len() < output_len {
        let mut input = t.clone();
        input.extend_from_slice(info.as_bytes());
        input.push(counter);
        t = compute_hmac(&algo, &prk, &input);
        okm.extend_from_slice(&t);
        counter += 1;
    }
    okm.truncate(output_len);

    Ok(Value::String(PhpString::from_vec(okm)))
}

fn compute_hmac(algo: &str, key: &[u8], data: &[u8]) -> Vec<u8> {
    let block_size = hash_block_size(algo);
    let key_block = if key.len() > block_size {
        let mut k = compute_hash(algo, key).unwrap_or_default();
        k.resize(block_size, 0);
        k
    } else {
        let mut k = key.to_vec();
        k.resize(block_size, 0);
        k
    };

    let mut ipad = vec![0x36u8; block_size];
    let mut opad = vec![0x5cu8; block_size];
    for i in 0..block_size {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner_data = ipad;
    inner_data.extend_from_slice(data);
    let inner_hash = compute_hash(algo, &inner_data).unwrap_or_default();

    let mut outer_data = opad;
    outer_data.extend_from_slice(&inner_hash);
    compute_hash(algo, &outer_data).unwrap_or_default()
}

fn md5_file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    match std::fs::read(&filename) {
        Ok(data) => {
            let digest = md5_hash(&data);
            if raw {
                Ok(Value::String(PhpString::from_vec(digest.to_vec())))
            } else {
                let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(Value::String(PhpString::from_string(hex)))
            }
        }
        Err(_) => Ok(Value::False),
    }
}

fn sha1_file_fn(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().unwrap_or(&Value::Null).to_php_string().to_string_lossy();
    let raw = args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    match std::fs::read(&filename) {
        Ok(data) => {
            let digest = sha1_hash(&data);
            if raw {
                Ok(Value::String(PhpString::from_vec(digest.to_vec())))
            } else {
                let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(Value::String(PhpString::from_string(hex)))
            }
        }
        Err(_) => Ok(Value::False),
    }
}
