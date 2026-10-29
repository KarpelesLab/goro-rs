//! The session serializers (PHP manual: session.serialize_handler):
//! `php` (`name|value` for each variable), `php_binary` (a length byte,
//! the name and the value) and `php_serialize` (serialize() of the whole
//! array). Variables share one value table, so references between them
//! survive (tests 019.phpt, bug79031.phpt).

use goro_runtime::{KeyRef, PhpArray, Value};

use super::warn;
use crate::vm::{R, Vm};

pub(super) fn known(name: &[u8]) -> bool {
    matches!(name, b"php" | b"php_binary" | b"php_serialize")
}

/// What decoding gives.
pub(super) enum Decoded {
    /// Variables to set in $_SESSION.
    Merge(Vec<(Vec<u8>, Value)>),
    /// The new $_SESSION (php_serialize).
    Replace(PhpArray),
    /// No variables (php_serialize data that is not an array).
    Nothing,
}

/// Encodes the session variables; None when a name cannot be stored
/// (reported).
pub(super) fn encode(
    vm: &mut Vm,
    handler: &[u8],
    vars: &PhpArray,
    _for_write: bool,
) -> R<Option<Vec<u8>>> {
    if handler == b"php_serialize" {
        return vm.serialize_value(&Value::Array(vars.clone())).map(Some);
    }
    let binary = handler == b"php_binary";
    let vars = vars.clone();
    vm.with_ser_ctx(|vm| {
        let mut out = Vec::new();
        for (k, v) in vars.iter() {
            let name = match k {
                KeyRef::Int(i) => {
                    warn(vm, &format!("Skipping numeric key {i}"))?;
                    continue;
                }
                KeyRef::Str(s) => s.to_vec(),
            };
            if binary {
                // Names longer than 127 bytes don't fit the length byte
                // (observed on PHP 8.5: they are left out).
                if name.len() > 127 {
                    continue;
                }
                out.push(name.len() as u8);
                out.extend_from_slice(&name);
            } else {
                if name.contains(&b'|') {
                    warn(
                        vm,
                        &format!(
                            "Failed to write session data. Data contains invalid key \"{}\"",
                            String::from_utf8_lossy(&name)
                        ),
                    )?;
                    return Ok(None);
                }
                out.extend_from_slice(&name);
                out.push(b'|');
            }
            let s = vm.serialize_value(v)?;
            out.extend_from_slice(&s);
        }
        Ok(Some(out))
    })
}

/// Decodes session data; None for malformed data.
pub(super) fn decode(vm: &mut Vm, handler: &[u8], data: &[u8]) -> R<Option<Decoded>> {
    if data.is_empty() {
        return Ok(Some(Decoded::Merge(Vec::new())));
    }
    match handler {
        b"php_serialize" => {
            let Some((v, n)) = vm.unserialize_prefix(data) else {
                return Ok(None);
            };
            if n != data.len() {
                return Ok(None);
            }
            Ok(Some(match v {
                Value::Array(a) => Decoded::Replace(a),
                _ => Decoded::Nothing,
            }))
        }
        b"php_binary" => {
            let mut name_at = |d: &[u8], pos: usize| {
                if pos >= d.len() {
                    return Ok(None);
                }
                let n = d[pos] as usize;
                if n & 0x80 != 0 || pos + 1 + n > d.len() {
                    return Err(());
                }
                Ok(Some((d[pos + 1..pos + 1 + n].to_vec(), pos + 1 + n)))
            };
            Ok(vm
                .unserialize_vars(data, "session_decode()", &mut name_at)?
                .map(Decoded::Merge))
        }
        _ => {
            let mut name_at = |d: &[u8], pos: usize| {
                if pos >= d.len() {
                    return Ok(None);
                }
                match d[pos..].iter().position(|&c| c == b'|') {
                    Some(p) => Ok(Some((d[pos..pos + p].to_vec(), pos + p + 1))),
                    None => Err(()),
                }
            };
            Ok(vm
                .unserialize_vars(data, "session_decode()", &mut name_at)?
                .map(Decoded::Merge))
        }
    }
}
