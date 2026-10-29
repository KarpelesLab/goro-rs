//! The session.* settings: defaults (in PHP's registration order, which
//! is the order of the startup diagnostics: tests 015.phpt, gh13891.phpt),
//! their checks and deprecations (PHP manual: Session configuration;
//! messages observed on PHP 8.5).

use goro_runtime::PhpStr;

use super::{Phase, st, warn};
use crate::vm::errors::{E_DEPRECATED, E_WARNING};
use crate::vm::{R, Vm};

pub(super) const TRANS_SID_TAGS: &str = "a=href,area=href,frame=src,form=";

pub(super) static INI: &[(&str, &str)] = &[
    ("session.save_path", ""),
    ("session.name", "PHPSESSID"),
    ("session.save_handler", "files"),
    ("session.auto_start", "0"),
    ("session.gc_probability", "1"),
    ("session.gc_divisor", "100"),
    ("session.gc_maxlifetime", "1440"),
    ("session.serialize_handler", "php"),
    ("session.cookie_lifetime", "0"),
    ("session.cookie_path", "/"),
    ("session.cookie_domain", ""),
    ("session.cookie_secure", "0"),
    ("session.cookie_partitioned", "0"),
    ("session.cookie_httponly", ""),
    ("session.cookie_samesite", ""),
    ("session.use_cookies", "1"),
    ("session.use_only_cookies", "1"),
    ("session.use_strict_mode", "0"),
    ("session.referer_check", ""),
    ("session.cache_limiter", "nocache"),
    ("session.cache_expire", "180"),
    ("session.use_trans_sid", "0"),
    ("session.sid_length", "32"),
    ("session.sid_bits_per_character", "4"),
    ("session.lazy_write", "1"),
    ("session.trans_sid_tags", TRANS_SID_TAGS),
    ("session.trans_sid_hosts", ""),
    ("session.upload_progress.enabled", "1"),
    ("session.upload_progress.cleanup", "1"),
    ("session.upload_progress.prefix", "upload_progress_"),
    ("session.upload_progress.name", "PHP_SESSION_UPLOAD_PROGRESS"),
    ("session.upload_progress.freq", "1%"),
    ("session.upload_progress.min_freq", "1"),
    ("url_rewriter.tags", "form="),
    ("url_rewriter.hosts", ""),
];

/// An ini boolean (PHP manual: ini values "1", "On", "Yes", "True").
pub(crate) fn truthy(v: &[u8]) -> bool {
    let l = v.trim_ascii().to_ascii_lowercase();
    match &l[..] {
        b"on" | b"yes" | b"true" => true,
        b"" | b"off" | b"no" | b"false" | b"none" => false,
        _ => goro_runtime::convert::str_to_int(&l) != 0,
    }
}

/// Settings only php.ini / the command line can change (PHP_INI_PERDIR).
fn startup_only(k: &[u8]) -> bool {
    k == b"session.auto_start" || k.starts_with(b"session.upload_progress.")
}

/// The deprecation a value of `k` raises, if any (RFC: Deprecate GET/POST
/// sessions; RFC: Deprecations for PHP 8.4).
fn deprecation(k: &[u8], v: &[u8]) -> Option<String> {
    let name = String::from_utf8_lossy(k);
    match k {
        b"session.use_only_cookies" if !truthy(v) => {
            Some(format!("Disabling {name} INI setting is deprecated"))
        }
        b"session.use_trans_sid" if truthy(v) => {
            Some(format!("Enabling {name} INI setting is deprecated"))
        }
        b"session.trans_sid_tags" if v != TRANS_SID_TAGS.as_bytes() => {
            Some(format!("Usage of {name} INI setting is deprecated"))
        }
        b"session.trans_sid_hosts" | b"session.referer_check" if !v.is_empty() => {
            Some(format!("Usage of {name} INI setting is deprecated"))
        }
        b"session.sid_length" if v != b"32" => Some(format!("{name} INI setting is deprecated")),
        b"session.sid_bits_per_character" if v != b"4" => {
            Some(format!("{name} INI setting is deprecated"))
        }
        _ => None,
    }
}

/// The session name rules (PHP manual: session_name()).
fn valid_name(v: &[u8]) -> bool {
    !v.is_empty()
        && !goro_runtime::numeric::is_numeric_str(v)
        && !v.iter().any(|c| b"=,;.[ \t\r\n\x0b\x0c\0".contains(c))
}

/// The save path without its "N;MODE;" prefix.
pub(super) fn save_dir(v: &[u8]) -> &[u8] {
    match v.iter().rposition(|&c| c == b';') {
        Some(p) => &v[p + 1..],
        None => v,
    }
}

/// Checks a new value: its deprecation, then whether it is valid
/// (Ok(false), reported). `runtime` is false for the startup settings.
fn check(vm: &mut Vm, k: &[u8], v: &[u8], runtime: bool) -> R<bool> {
    if let Some(d) = deprecation(k, v) {
        super::deprecated(vm, &d)?;
    }
    let int = |v: &[u8]| goro_runtime::convert::str_to_int(v);
    match k {
        b"session.name" if !valid_name(v) => {
            warn(
                vm,
                &format!(
                    "session.name \"{}\" must not be numeric, empty, contain null bytes or any of the following characters \"=,;.[ \\t\\r\\n\\013\\014\"",
                    String::from_utf8_lossy(v)
                ),
            )?;
            return Ok(false);
        }
        b"session.save_handler" => {
            if v == b"user" {
                if runtime {
                    warn(vm, "Session save handler \"user\" cannot be set by ini_set()")?;
                    return Ok(false);
                }
            } else if v != b"files" && runtime {
                warn(
                    vm,
                    &format!(
                        "Session save handler \"{}\" cannot be found",
                        String::from_utf8_lossy(v)
                    ),
                )?;
                return Ok(false);
            }
        }
        b"session.serialize_handler" if runtime && !super::serial::known(v) => {
            warn(
                vm,
                &format!(
                    "Serialization handler \"{}\" cannot be found",
                    String::from_utf8_lossy(v)
                ),
            )?;
            return Ok(false);
        }
        b"session.gc_probability" if int(v) < 0 => {
            warn(vm, "session.gc_probability must be greater than or equal to 0")?;
            return Ok(false);
        }
        b"session.gc_divisor" if int(v) <= 0 => {
            warn(vm, "session.gc_divisor must be greater than 0")?;
            return Ok(false);
        }
        b"session.cookie_lifetime" | b"session.cache_expire" => {
            let (n, w) = crate::vm::setup::parse_quantity(v);
            if let Some(w) = w {
                let name = String::from_utf8_lossy(k);
                let msg = format!("Invalid \"{name}\" setting. {w}");
                match st(vm).phase {
                    Phase::Run => vm.warn(&msg)?,
                    _ => super::raise(vm, E_WARNING, &msg)?,
                }
            }
            if k == b"session.cookie_lifetime" && n < 0 {
                warn(vm, "CookieLifetime cannot be negative")?;
                return Ok(false);
            }
        }
        b"session.sid_length" if !(22..=256).contains(&int(v)) => {
            warn(
                vm,
                "session.configuration \"session.sid_length\" must be between 22 and 256",
            )?;
            return Ok(false);
        }
        b"session.sid_bits_per_character" if !(4..=6).contains(&int(v)) => {
            warn(
                vm,
                "session.configuration \"session.sid_bits_per_character\" must be between 4 and 6",
            )?;
            return Ok(false);
        }
        b"session.upload_progress.freq" => {
            let t = v.trim_ascii();
            let (num, pct) = match t.strip_suffix(b"%") {
                Some(n) => (goro_runtime::convert::str_to_float(n), true),
                None => (goro_runtime::convert::str_to_float(t), false),
            };
            if num < 0.0 {
                warn(
                    vm,
                    "session.upload_progress.freq must be greater than or equal to 0",
                )?;
                return Ok(false);
            }
            if pct && num > 100.0 {
                warn(
                    vm,
                    "session.upload_progress.freq must be less than or equal to 100%",
                )?;
                return Ok(false);
            }
        }
        b"session.save_path" if runtime => {
            let dir = save_dir(v);
            if !dir.is_empty() && !vm.basedir_allows(dir) {
                let bd = vm
                    .get_ini(b"open_basedir")
                    .map(|v| v.to_string_lossy())
                    .unwrap_or_default();
                warn(
                    vm,
                    &format!(
                        "open_basedir restriction in effect. File({}) is not within the allowed path(s): ({})",
                        String::from_utf8_lossy(dir),
                        bd
                    ),
                )?;
                return Ok(false);
            }
        }
        _ => {}
    }
    Ok(true)
}

/// ini_set() / ini_restore() of a session setting.
pub(super) fn on_ini_set(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    if !k.starts_with(b"session.") {
        return Ok(true);
    }
    if startup_only(k) {
        return Ok(false);
    }
    if super::active_refusal(vm, "Session ini settings cannot be changed")? {
        return Ok(false);
    }
    if super::sent_refusal(vm, "Session ini settings cannot be changed")? {
        return Ok(false);
    }
    check(vm, k, v, true)
}

/// Stores a value as ini_set() does (restored at the end of the request).
pub(super) fn store(vm: &mut Vm, k: &[u8], v: &[u8]) {
    let key = PhpStr::from(k.to_vec());
    if let Some(old) = vm.get_ini(k) {
        vm.ini.originals.entry(key).or_insert(Some(old));
    }
    vm.set_ini(k, v);
}

/// Sets a session setting from a session function (the checks of
/// ini_set(), with that function's name in the messages).
pub(super) fn set_internal(vm: &mut Vm, k: &[u8], v: &[u8]) -> R<bool> {
    if startup_only(k) || !check(vm, k, v, true)? {
        return Ok(false);
    }
    store(vm, k, v);
    Ok(true)
}

/// The startup settings' diagnostics ("PHP Startup: ..."); a refused
/// value keeps the default. Then session.auto_start.
pub(super) fn startup_ini(vm: &mut Vm, given: &[(Vec<u8>, Vec<u8>)]) {
    st(vm).phase = Phase::Startup;
    for (k, def) in INI {
        let Some((_, v)) = given.iter().rev().find(|(n, _)| n == k.as_bytes()) else {
            continue;
        };
        let v = v.clone();
        match check(vm, k.as_bytes(), &v, false) {
            Ok(true) => {}
            Ok(false) => vm.set_ini(k.as_bytes(), def.as_bytes()),
            Err(_) => {}
        }
    }
    st(vm).phase = Phase::Run;
    auto_start(vm);
}

/// session.auto_start: the session starts before the script runs.
pub(crate) fn auto_start(vm: &mut Vm) {
    if !super::ini_bool(vm, "session.auto_start") || st(vm).active {
        return;
    }
    st(vm).phase = Phase::RequestStartup;
    st(vm).started_at = None;
    let r = super::php_session_start(vm, false);
    st(vm).phase = Phase::Run;
    vm.handle_unwind(r.map(|_| ()));
}

/// The settings changed at run time get their startup values back at the
/// end of the request, with the deprecations those values raise (test
/// gh13891.phpt).
pub(super) fn shutdown_restore(vm: &mut Vm) {
    let mut changed: Vec<(PhpStr, PhpStr)> = vm
        .ini
        .originals
        .iter()
        .filter(|(k, _)| k.starts_with(b"session."))
        .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
        .collect();
    // Registration order.
    changed.sort_by_key(|(k, _)| INI.iter().position(|(n, _)| n.as_bytes() == &k[..]));
    for (k, v) in changed {
        if let Some(d) = deprecation(&k, &v) {
            let _ = vm.raise_at(
                E_DEPRECATED,
                &format!("PHP Request Shutdown: {d}"),
                PhpStr::from("Unknown"),
                0,
            );
        }
    }
}
