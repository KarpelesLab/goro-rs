//! mbstring's zend_multibyte_functions (mbstring.c): the encodings
//! zend.multibyte converts scripts with.

use std::rc::Rc;

use goro_core::vm::Vm;
use goro_parser::multibyte::{EncodingId, Provider, Settings};

use crate::mbfl::{self, Encoding, MBFL_ENCTYPE_GL_UNSAFE, MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR};
use crate::state;

fn id(e: &'static Encoding) -> EncodingId {
    e as *const Encoding as usize
}

fn enc(id: EncodingId) -> &'static Encoding {
    // SAFETY: ids are only made by `id()` from `&'static Encoding`s.
    unsafe { &*(id as *const Encoding) }
}

struct MbProvider {
    /// MBSTRG(internal_encoding) as the INI settings set it at startup
    internal: &'static Encoding,
}

impl Provider for MbProvider {
    /// php_mb_zend_encoding_fetcher()
    fn fetch(&self, name: &[u8]) -> Option<EncodingId> {
        mbfl::name2encoding(name).map(id)
    }

    /// php_mb_zend_encoding_lexer_compatibility_checker()
    fn lexer_compatible(&self, e: EncodingId) -> bool {
        enc(e).flag & MBFL_ENCTYPE_GL_UNSAFE == 0
    }

    /// php_mb_zend_encoding_converter()
    fn convert(&self, data: &[u8], to: EncodingId, from: EncodingId) -> Vec<u8> {
        let (subst, mode) = state::MBSTRG.with(|s| {
            let s = s.borrow();
            if s.initialized {
                (
                    s.current_filter_illegal_substchar,
                    s.current_filter_illegal_mode,
                )
            } else {
                ('?' as u32, MBFL_OUTPUTFILTER_ILLEGAL_MODE_CHAR)
            }
        });
        mbfl::fast_convert(data, enc(from), enc(to), subst, mode).0
    }

    /// php_mb_zend_encoding_detector()
    fn detect(&self, data: &[u8], list: &[EncodingId]) -> Option<EncodingId> {
        let list: Vec<&'static Encoding> = list.iter().map(|&e| enc(e)).collect();
        if list.len() == 1 && list[0].no == mbfl::EncNo::Pass {
            return None;
        }
        crate::mbstring::guess_encoding(&[data], &list, false, false).map(id)
    }

    /// php_mb_zend_internal_encoding_getter()
    fn internal_encoding(&self) -> Option<EncodingId> {
        let cur = state::MBSTRG.with(|s| {
            let s = s.borrow();
            s.initialized.then_some(s.internal_encoding)
        });
        Some(id(cur.unwrap_or(self.internal)))
    }
}

fn ini_truthy(v: &str) -> bool {
    let v = v.trim().to_ascii_lowercase();
    matches!(v.as_str(), "on" | "yes" | "true") || v.parse::<i64>().is_ok_and(|n| n != 0)
}

/// Apply zend.multibyte, zend.script_encoding and zend.detect_unicode
/// (`ini` looks a setting up) to the scripts compiled from now on.
pub fn install(ini: &dyn Fn(&str) -> Option<String>) {
    if !ini("zend.multibyte").is_some_and(|v| ini_truthy(&v)) {
        goro_parser::multibyte::set_multibyte(None);
        return;
    }
    // MBSTRG(internal_encoding): mbstring.internal_encoding, else
    // internal_encoding, else default_charset.
    let internal = [
        "mbstring.internal_encoding",
        "internal_encoding",
        "default_charset",
    ]
    .iter()
    .filter_map(|k| ini(k).filter(|v| !v.is_empty()))
    .next()
    .and_then(|n| mbfl::name2encoding(n.as_bytes()))
    .unwrap_or(&mbfl::basic::ENC_UTF8);
    let script_encodings = ini("zend.script_encoding")
        .and_then(|v| {
            state::MBSTRG.with(|s| state::parse_encoding_list(&s.borrow(), v.as_bytes()).ok())
        })
        .unwrap_or_default()
        .into_iter()
        .map(id)
        .collect();
    let detect_unicode = ini("zend.detect_unicode").is_none_or(|v| ini_truthy(&v));
    goro_parser::multibyte::set_multibyte(Some(Settings {
        provider: Rc::new(MbProvider { internal }),
        detect_unicode,
        script_encodings,
    }));
}

/// OnUpdateScriptEncoding at startup: an invalid zend.script_encoding warns.
pub fn startup(vm: &mut Vm) {
    let on = vm
        .constants
        .get(b"zend.multibyte".as_slice())
        .is_some_and(|v| ini_truthy(&v.to_php_string().to_string_lossy()));
    if !on {
        return;
    }
    let Some(v) = vm.constants.get(b"zend.script_encoding".as_slice()) else {
        return;
    };
    let v = v.to_php_string().as_bytes().to_vec();
    if v.is_empty() {
        return;
    }
    let bad = state::MBSTRG.with(|s| state::parse_encoding_list(&s.borrow(), &v).err());
    if let Some(bad) = bad {
        if vm.error_reporting & 2 != 0 {
            vm.write_output(
                format!(
                    "\nWarning: PHP Startup: INI setting contains invalid encoding \"{}\" in Unknown on line 0\n",
                    String::from_utf8_lossy(&bad)
                )
                .as_bytes(),
            );
        }
    }
}
