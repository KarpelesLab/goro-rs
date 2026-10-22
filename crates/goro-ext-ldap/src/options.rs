//! ldap_get_option / ldap_set_option (ldap.c), over goro_ldap::Options.

use std::cell::RefCell;
use std::rc::Rc;

use goro_core::value::Value;
use goro_core::vm::Vm;
use goro_ldap::Options;
use goro_ldap::consts::*;

use crate::ctl;

pub enum SetOutcome {
    Ok,
    False,
    Throw,
}

fn sval(b: &[u8]) -> Value {
    Value::String(goro_core::string::PhpString::from_bytes(b))
}

/// ldap_get_option: Ok(Some(v)) assigns & returns true, Ok(None) returns
/// false, Err(()) is an invalid-option ValueError.
pub fn get_option(
    option: i32,
    opts: &Options,
    errno: i32,
    diagnostic: &[u8],
    matched: &[u8],
) -> Result<Option<Value>, ()> {
    let long = |n: i64| Ok(Some(Value::Long(n)));
    match option {
        LDAP_OPT_DEREF => long(opts.deref as i64),
        LDAP_OPT_SIZELIMIT => long(opts.sizelimit as i64),
        LDAP_OPT_TIMELIMIT => long(opts.timelimit as i64),
        LDAP_OPT_PROTOCOL_VERSION => long(opts.version as i64),
        LDAP_OPT_ERROR_NUMBER => long(errno as i64),
        LDAP_OPT_REFERRALS => long(opts.referrals as i64),
        LDAP_OPT_RESTART => long(opts.restart as i64),
        LDAP_OPT_X_SASL_NOCANON => long(opts.sasl_nocanon as i64),
        LDAP_OPT_X_TLS_REQUIRE_CERT => long(opts.tls_require_cert as i64),
        LDAP_OPT_X_TLS_CRLCHECK => long(opts.tls_crlcheck as i64),
        LDAP_OPT_X_TLS_PROTOCOL_MIN => long(opts.tls_protocol_min as i64),
        LDAP_OPT_X_TLS_PROTOCOL_MAX => long(opts.tls_protocol_max as i64),
        LDAP_OPT_DEBUG_LEVEL => long(opts.debug_level as i64),
        LDAP_OPT_X_KEEPALIVE_IDLE => long(opts.keepalive_idle as i64),
        LDAP_OPT_X_KEEPALIVE_PROBES => long(opts.keepalive_probes as i64),
        LDAP_OPT_X_KEEPALIVE_INTERVAL => long(opts.keepalive_interval as i64),
        LDAP_OPT_NETWORK_TIMEOUT => {
            if opts.network_timeout < 0 {
                Ok(None)
            } else {
                long(opts.network_timeout)
            }
        }
        LDAP_OPT_TIMEOUT => {
            if opts.timeout < 0 {
                Ok(None)
            } else {
                long(opts.timeout)
            }
        }
        // ERROR_STRING/MATCHED_DN round-trip through the option store (as
        // ldap_set_option writes them); the live diagnostic/matched DN from
        // the last operation seed them.
        LDAP_OPT_ERROR_STRING => match &opts.diagnostic {
            Some(b) if !b.is_empty() => Ok(Some(sval(b))),
            _ => str_or_false(diagnostic),
        },
        LDAP_OPT_MATCHED_DN => match &opts.matched_dn {
            Some(b) if !b.is_empty() => Ok(Some(sval(b))),
            _ => str_or_false(matched),
        },
        LDAP_OPT_X_TLS_CACERTFILE => opt_str(&opts.tls_cacertfile),
        LDAP_OPT_X_TLS_CACERTDIR => opt_str(&opts.tls_cacertdir),
        LDAP_OPT_X_TLS_CERTFILE => opt_str(&opts.tls_certfile),
        LDAP_OPT_X_TLS_KEYFILE => opt_str(&opts.tls_keyfile),
        LDAP_OPT_X_TLS_CIPHER_SUITE => opt_str(&opts.tls_cipher_suite),
        LDAP_OPT_X_TLS_RANDOM_FILE => opt_str(&opts.tls_randfile),
        LDAP_OPT_X_TLS_CRLFILE => opt_str(&opts.tls_crlfile),
        LDAP_OPT_X_TLS_DHFILE => opt_str(&opts.tls_dhfile),
        LDAP_OPT_X_SASL_MECH => opt_str(&opts.sasl_mech),
        LDAP_OPT_X_SASL_REALM => opt_str(&opts.sasl_realm),
        LDAP_OPT_X_SASL_AUTHCID => opt_str(&opts.sasl_authcid),
        LDAP_OPT_X_SASL_AUTHZID => opt_str(&opts.sasl_authzid),
        LDAP_OPT_HOST_NAME => opt_str(&opts.host_name),
        LDAP_OPT_SERVER_CONTROLS => controls_opt(&opts.server_controls),
        LDAP_OPT_CLIENT_CONTROLS => controls_opt(&opts.client_controls),
        _ => Err(()),
    }
}

fn str_or_false(b: &[u8]) -> Result<Option<Value>, ()> {
    if b.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sval(b)))
    }
}
fn opt_str(v: &Option<Vec<u8>>) -> Result<Option<Value>, ()> {
    match v {
        Some(b) if !b.is_empty() => Ok(Some(sval(b))),
        _ => Ok(None),
    }
}
fn controls_opt(ctrls: &[goro_ldap::message::Control]) -> Result<Option<Value>, ()> {
    if ctrls.is_empty() {
        return Ok(None);
    }
    let a = ctl::controls_to_array(ctrls, true);
    Ok(Some(Value::Array(Rc::new(RefCell::new(a)))))
}

/// zval_try_get_long (weak): None on failure.
fn try_long(v: &Value) -> Option<i64> {
    match v.deref() {
        Value::Long(n) => Some(n),
        Value::True => Some(1),
        Value::False | Value::Null => Some(0),
        Value::Double(d) => Some(d as i64),
        Value::String(s) => {
            let t = String::from_utf8_lossy(s.as_bytes());
            let t = t.trim();
            t.parse::<i64>()
                .ok()
                .or_else(|| t.parse::<f64>().ok().map(|f| f as i64))
        }
        _ => None,
    }
}

const INT_MAX: i64 = i32::MAX as i64;
const INT_MIN: i64 = i32::MIN as i64;

pub fn set_option(
    vm: &mut Vm,
    f: &str,
    option: i32,
    newval: &Value,
    opts: &mut Options,
) -> SetOutcome {
    match option {
        // int-valued options
        LDAP_OPT_DEREF
        | LDAP_OPT_SIZELIMIT
        | LDAP_OPT_TIMELIMIT
        | LDAP_OPT_PROTOCOL_VERSION
        | LDAP_OPT_ERROR_NUMBER
        | LDAP_OPT_DEBUG_LEVEL
        | LDAP_OPT_X_KEEPALIVE_IDLE
        | LDAP_OPT_X_KEEPALIVE_PROBES
        | LDAP_OPT_X_KEEPALIVE_INTERVAL
        | LDAP_OPT_X_TLS_REQUIRE_CERT
        | LDAP_OPT_X_TLS_CRLCHECK
        | LDAP_OPT_X_TLS_PROTOCOL_MIN
        | LDAP_OPT_X_TLS_PROTOCOL_MAX => {
            let Some(lval) = try_long(newval) else {
                crate::zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be of type int for the given option, {} given",
                        Vm::value_type_name(&newval.deref())
                    ),
                );
                return SetOutcome::Throw;
            };
            if !(INT_MIN..=INT_MAX).contains(&lval) {
                crate::zpp::value_error(vm, f, 3, "is too large");
                return SetOutcome::Throw;
            }
            let val = lval as i32;
            match option {
                LDAP_OPT_DEREF => opts.deref = val,
                LDAP_OPT_SIZELIMIT => opts.sizelimit = val,
                LDAP_OPT_TIMELIMIT => opts.timelimit = val,
                LDAP_OPT_PROTOCOL_VERSION => {
                    if !(LDAP_VERSION_MIN..=LDAP_VERSION_MAX).contains(&val) {
                        return SetOutcome::False;
                    }
                    opts.version = val;
                }
                LDAP_OPT_ERROR_NUMBER => {}
                LDAP_OPT_DEBUG_LEVEL => opts.debug_level = val,
                LDAP_OPT_X_KEEPALIVE_IDLE => opts.keepalive_idle = val,
                LDAP_OPT_X_KEEPALIVE_PROBES => opts.keepalive_probes = val,
                LDAP_OPT_X_KEEPALIVE_INTERVAL => opts.keepalive_interval = val,
                LDAP_OPT_X_TLS_REQUIRE_CERT => {
                    if !(LDAP_OPT_X_TLS_NEVER..=LDAP_OPT_X_TLS_TRY).contains(&val) {
                        return SetOutcome::False;
                    }
                    opts.tls_require_cert = val;
                }
                LDAP_OPT_X_TLS_CRLCHECK => {
                    if !(LDAP_OPT_X_TLS_CRL_NONE..=LDAP_OPT_X_TLS_CRL_ALL).contains(&val) {
                        return SetOutcome::False;
                    }
                    opts.tls_crlcheck = val;
                }
                LDAP_OPT_X_TLS_PROTOCOL_MIN => opts.tls_protocol_min = val,
                LDAP_OPT_X_TLS_PROTOCOL_MAX => opts.tls_protocol_max = val,
                _ => unreachable!(),
            }
            SetOutcome::Ok
        }
        LDAP_OPT_NETWORK_TIMEOUT => {
            let Some(lval) = try_long(newval) else {
                crate::zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be of type int for the LDAP_OPT_NETWORK_TIMEOUT option, {} given",
                        Vm::value_type_name(&newval.deref())
                    ),
                );
                return SetOutcome::Throw;
            };
            opts.network_timeout = lval;
            SetOutcome::Ok
        }
        LDAP_OPT_TIMEOUT => {
            let Some(lval) = try_long(newval) else {
                crate::zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be of type int for the LDAP_OPT_TIMEOUT option, {} given",
                        Vm::value_type_name(&newval.deref())
                    ),
                );
                return SetOutcome::Throw;
            };
            opts.timeout = lval;
            SetOutcome::Ok
        }
        // string-valued options
        LDAP_OPT_ERROR_STRING
        | LDAP_OPT_MATCHED_DN
        | LDAP_OPT_HOST_NAME
        | LDAP_OPT_X_SASL_MECH
        | LDAP_OPT_X_SASL_REALM
        | LDAP_OPT_X_SASL_AUTHCID
        | LDAP_OPT_X_SASL_AUTHZID
        | LDAP_OPT_X_TLS_CACERTFILE
        | LDAP_OPT_X_TLS_CACERTDIR
        | LDAP_OPT_X_TLS_CERTFILE
        | LDAP_OPT_X_TLS_CIPHER_SUITE
        | LDAP_OPT_X_TLS_KEYFILE
        | LDAP_OPT_X_TLS_RANDOM_FILE
        | LDAP_OPT_X_TLS_CRLFILE
        | LDAP_OPT_X_TLS_DHFILE => {
            let s = vm.value_to_string(&newval.deref()).as_bytes().to_vec();
            if vm.current_exception.is_some() {
                return SetOutcome::Throw;
            }
            let os = if s.is_empty() { None } else { Some(s.clone()) };
            match option {
                LDAP_OPT_MATCHED_DN => opts.matched_dn = os,
                LDAP_OPT_ERROR_STRING => opts.diagnostic = os,
                LDAP_OPT_HOST_NAME => opts.host_name = os,
                LDAP_OPT_X_SASL_MECH => opts.sasl_mech = os,
                LDAP_OPT_X_SASL_REALM => opts.sasl_realm = os,
                LDAP_OPT_X_SASL_AUTHCID => opts.sasl_authcid = os,
                LDAP_OPT_X_SASL_AUTHZID => opts.sasl_authzid = os,
                LDAP_OPT_X_TLS_CACERTFILE => opts.tls_cacertfile = os,
                LDAP_OPT_X_TLS_CACERTDIR => opts.tls_cacertdir = os,
                LDAP_OPT_X_TLS_CERTFILE => opts.tls_certfile = os,
                LDAP_OPT_X_TLS_CIPHER_SUITE => opts.tls_cipher_suite = os,
                LDAP_OPT_X_TLS_KEYFILE => opts.tls_keyfile = os,
                LDAP_OPT_X_TLS_RANDOM_FILE => opts.tls_randfile = os,
                LDAP_OPT_X_TLS_CRLFILE => opts.tls_crlfile = os,
                LDAP_OPT_X_TLS_DHFILE => opts.tls_dhfile = os,
                _ => unreachable!(),
            }
            SetOutcome::Ok
        }
        // boolean options
        LDAP_OPT_REFERRALS => {
            opts.referrals = newval.is_truthy();
            SetOutcome::Ok
        }
        LDAP_OPT_RESTART => {
            opts.restart = newval.is_truthy();
            SetOutcome::Ok
        }
        LDAP_OPT_X_SASL_NOCANON => {
            opts.sasl_nocanon = newval.is_truthy();
            SetOutcome::Ok
        }
        // control-list options
        LDAP_OPT_SERVER_CONTROLS | LDAP_OPT_CLIENT_CONTROLS => {
            let Value::Array(a) = &newval.deref() else {
                crate::zpp::type_error(
                    vm,
                    f,
                    3,
                    &format!(
                        "must be of type array for the LDAP_OPT_CLIENT_CONTROLS option, {} given",
                        Vm::value_type_name(&newval.deref())
                    ),
                );
                return SetOutcome::Throw;
            };
            let a = a.borrow().clone();
            match ctl::controls_from_array(vm, f, &a, 3) {
                Ok(ctrls) => {
                    if option == LDAP_OPT_SERVER_CONTROLS {
                        opts.server_controls = ctrls;
                    } else {
                        opts.client_controls = ctrls;
                    }
                    SetOutcome::Ok
                }
                Err(()) => SetOutcome::Throw,
            }
        }
        _ => {
            crate::zpp::value_error(vm, f, 2, "must be a valid LDAP option");
            SetOutcome::Throw
        }
    }
}
