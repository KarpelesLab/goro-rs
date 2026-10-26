//! php-src's ext/ldap (ldap.c): the LDAP\Connection, LDAP\Result and
//! LDAP\ResultEntry classes and the ldap_* functions over goro-ldap.

use std::cell::RefCell;

use goro_core::object::ClassEntry;
use goro_core::value::Value;
use goro_core::vm::{BuiltinFn, NativeClassHandler, Vm};
use goro_ldap::Options;
use goro_ldap::consts::*;

mod ctl;
mod funcs;
mod handles;
mod options;
mod zpp;

/// extension_loaded('ldap')
pub const NAME: &[u8] = b"ldap";

thread_local! {
    /// The global ldapoptions (the ld == NULL target of ldap_set_option).
    static GLOBAL_OPTS: RefCell<Options> = RefCell::new(Options::default());
    /// num_links (ldap.max_links is read per-connect from the INI).
    static NUM_LINKS: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn with_global_opts<T>(f: impl FnOnce(&mut Options) -> T) -> T {
    GLOBAL_OPTS.with(|o| f(&mut o.borrow_mut()))
}
pub(crate) fn clone_global_opts() -> Options {
    GLOBAL_OPTS.with(|o| o.borrow().clone())
}
pub(crate) fn num_links() -> i64 {
    NUM_LINKS.with(|n| n.get())
}
pub(crate) fn inc_links() {
    NUM_LINKS.with(|n| n.set(n.get() + 1));
}
pub(crate) fn dec_links() {
    NUM_LINKS.with(|n| n.set((n.get() - 1).max(0)));
}

const LONG_CONSTS: &[(&str, i64)] = &[
    ("LDAP_DEREF_NEVER", LDAP_DEREF_NEVER as i64),
    ("LDAP_DEREF_SEARCHING", LDAP_DEREF_SEARCHING as i64),
    ("LDAP_DEREF_FINDING", LDAP_DEREF_FINDING as i64),
    ("LDAP_DEREF_ALWAYS", LDAP_DEREF_ALWAYS as i64),
    ("LDAP_MODIFY_BATCH_ADD", 0x01),
    ("LDAP_MODIFY_BATCH_REMOVE", 0x02),
    ("LDAP_MODIFY_BATCH_REMOVE_ALL", 0x12),
    ("LDAP_MODIFY_BATCH_REPLACE", 0x03),
    ("LDAP_OPT_DEREF", LDAP_OPT_DEREF as i64),
    ("LDAP_OPT_SIZELIMIT", LDAP_OPT_SIZELIMIT as i64),
    ("LDAP_OPT_TIMELIMIT", LDAP_OPT_TIMELIMIT as i64),
    ("LDAP_OPT_NETWORK_TIMEOUT", LDAP_OPT_NETWORK_TIMEOUT as i64),
    ("LDAP_OPT_TIMEOUT", LDAP_OPT_TIMEOUT as i64),
    (
        "LDAP_OPT_PROTOCOL_VERSION",
        LDAP_OPT_PROTOCOL_VERSION as i64,
    ),
    ("LDAP_OPT_ERROR_NUMBER", LDAP_OPT_ERROR_NUMBER as i64),
    ("LDAP_OPT_REFERRALS", LDAP_OPT_REFERRALS as i64),
    ("LDAP_OPT_RESTART", LDAP_OPT_RESTART as i64),
    ("LDAP_OPT_HOST_NAME", LDAP_OPT_HOST_NAME as i64),
    ("LDAP_OPT_ERROR_STRING", LDAP_OPT_ERROR_STRING as i64),
    ("LDAP_OPT_MATCHED_DN", LDAP_OPT_MATCHED_DN as i64),
    ("LDAP_OPT_SERVER_CONTROLS", LDAP_OPT_SERVER_CONTROLS as i64),
    ("LDAP_OPT_CLIENT_CONTROLS", LDAP_OPT_CLIENT_CONTROLS as i64),
    ("LDAP_OPT_DEBUG_LEVEL", LDAP_OPT_DEBUG_LEVEL as i64),
    (
        "LDAP_OPT_DIAGNOSTIC_MESSAGE",
        LDAP_OPT_DIAGNOSTIC_MESSAGE as i64,
    ),
    ("LDAP_OPT_X_SASL_MECH", LDAP_OPT_X_SASL_MECH as i64),
    ("LDAP_OPT_X_SASL_REALM", LDAP_OPT_X_SASL_REALM as i64),
    ("LDAP_OPT_X_SASL_AUTHCID", LDAP_OPT_X_SASL_AUTHCID as i64),
    ("LDAP_OPT_X_SASL_AUTHZID", LDAP_OPT_X_SASL_AUTHZID as i64),
    ("LDAP_OPT_X_SASL_NOCANON", LDAP_OPT_X_SASL_NOCANON as i64),
    ("LDAP_OPT_X_SASL_USERNAME", LDAP_OPT_X_SASL_USERNAME as i64),
    (
        "LDAP_OPT_X_TLS_REQUIRE_CERT",
        LDAP_OPT_X_TLS_REQUIRE_CERT as i64,
    ),
    ("LDAP_OPT_X_TLS_NEVER", LDAP_OPT_X_TLS_NEVER as i64),
    ("LDAP_OPT_X_TLS_HARD", LDAP_OPT_X_TLS_HARD as i64),
    ("LDAP_OPT_X_TLS_DEMAND", LDAP_OPT_X_TLS_DEMAND as i64),
    ("LDAP_OPT_X_TLS_ALLOW", LDAP_OPT_X_TLS_ALLOW as i64),
    ("LDAP_OPT_X_TLS_TRY", LDAP_OPT_X_TLS_TRY as i64),
    ("LDAP_OPT_X_TLS_CACERTDIR", LDAP_OPT_X_TLS_CACERTDIR as i64),
    (
        "LDAP_OPT_X_TLS_CACERTFILE",
        LDAP_OPT_X_TLS_CACERTFILE as i64,
    ),
    ("LDAP_OPT_X_TLS_CERTFILE", LDAP_OPT_X_TLS_CERTFILE as i64),
    (
        "LDAP_OPT_X_TLS_CIPHER_SUITE",
        LDAP_OPT_X_TLS_CIPHER_SUITE as i64,
    ),
    ("LDAP_OPT_X_TLS_KEYFILE", LDAP_OPT_X_TLS_KEYFILE as i64),
    (
        "LDAP_OPT_X_TLS_RANDOM_FILE",
        LDAP_OPT_X_TLS_RANDOM_FILE as i64,
    ),
    ("LDAP_OPT_X_TLS_CRLCHECK", LDAP_OPT_X_TLS_CRLCHECK as i64),
    ("LDAP_OPT_X_TLS_CRL_NONE", LDAP_OPT_X_TLS_CRL_NONE as i64),
    ("LDAP_OPT_X_TLS_CRL_PEER", LDAP_OPT_X_TLS_CRL_PEER as i64),
    ("LDAP_OPT_X_TLS_CRL_ALL", LDAP_OPT_X_TLS_CRL_ALL as i64),
    ("LDAP_OPT_X_TLS_DHFILE", LDAP_OPT_X_TLS_DHFILE as i64),
    ("LDAP_OPT_X_TLS_CRLFILE", LDAP_OPT_X_TLS_CRLFILE as i64),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_MIN",
        LDAP_OPT_X_TLS_PROTOCOL_MIN as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_SSL2",
        LDAP_OPT_X_TLS_PROTOCOL_SSL2 as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_SSL3",
        LDAP_OPT_X_TLS_PROTOCOL_SSL3 as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_TLS1_0",
        LDAP_OPT_X_TLS_PROTOCOL_TLS1_0 as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_TLS1_1",
        LDAP_OPT_X_TLS_PROTOCOL_TLS1_1 as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_TLS1_2",
        LDAP_OPT_X_TLS_PROTOCOL_TLS1_2 as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_TLS1_3",
        LDAP_OPT_X_TLS_PROTOCOL_TLS1_3 as i64,
    ),
    (
        "LDAP_OPT_X_TLS_PROTOCOL_MAX",
        LDAP_OPT_X_TLS_PROTOCOL_MAX as i64,
    ),
    ("LDAP_OPT_X_TLS_PACKAGE", LDAP_OPT_X_TLS_PACKAGE as i64),
    (
        "LDAP_OPT_X_KEEPALIVE_IDLE",
        LDAP_OPT_X_KEEPALIVE_IDLE as i64,
    ),
    (
        "LDAP_OPT_X_KEEPALIVE_PROBES",
        LDAP_OPT_X_KEEPALIVE_PROBES as i64,
    ),
    (
        "LDAP_OPT_X_KEEPALIVE_INTERVAL",
        LDAP_OPT_X_KEEPALIVE_INTERVAL as i64,
    ),
    ("LDAP_ESCAPE_FILTER", goro_ldap::escape::ESCAPE_FILTER),
    ("LDAP_ESCAPE_DN", goro_ldap::escape::ESCAPE_DN),
];

const STR_CONSTS: &[(&str, &str)] = &[
    ("LDAP_MODIFY_BATCH_ATTRIB", "attrib"),
    ("LDAP_MODIFY_BATCH_MODTYPE", "modtype"),
    ("LDAP_MODIFY_BATCH_VALUES", "values"),
    ("LDAP_EXOP_START_TLS", LDAP_EXOP_START_TLS),
    ("LDAP_EXOP_MODIFY_PASSWD", LDAP_EXOP_MODIFY_PASSWD),
    ("LDAP_EXOP_REFRESH", LDAP_EXOP_REFRESH),
    ("LDAP_EXOP_WHO_AM_I", LDAP_EXOP_WHO_AM_I),
    ("LDAP_EXOP_TURN", LDAP_EXOP_TURN),
    ("LDAP_CONTROL_MANAGEDSAIT", LDAP_CONTROL_MANAGEDSAIT),
    ("LDAP_CONTROL_PROXY_AUTHZ", LDAP_CONTROL_PROXY_AUTHZ),
    ("LDAP_CONTROL_SUBENTRIES", LDAP_CONTROL_SUBENTRIES),
    (
        "LDAP_CONTROL_VALUESRETURNFILTER",
        LDAP_CONTROL_VALUESRETURNFILTER,
    ),
    ("LDAP_CONTROL_ASSERT", LDAP_CONTROL_ASSERT),
    ("LDAP_CONTROL_PRE_READ", LDAP_CONTROL_PRE_READ),
    ("LDAP_CONTROL_POST_READ", LDAP_CONTROL_POST_READ),
    ("LDAP_CONTROL_SORTREQUEST", LDAP_CONTROL_SORTREQUEST),
    ("LDAP_CONTROL_SORTRESPONSE", LDAP_CONTROL_SORTRESPONSE),
    ("LDAP_CONTROL_PAGEDRESULTS", LDAP_CONTROL_PAGEDRESULTS),
    ("LDAP_CONTROL_AUTHZID_REQUEST", LDAP_CONTROL_AUTHZID_REQUEST),
    (
        "LDAP_CONTROL_AUTHZID_RESPONSE",
        LDAP_CONTROL_AUTHZID_RESPONSE,
    ),
    ("LDAP_CONTROL_SYNC", LDAP_CONTROL_SYNC),
    ("LDAP_CONTROL_SYNC_STATE", LDAP_CONTROL_SYNC_STATE),
    ("LDAP_CONTROL_SYNC_DONE", LDAP_CONTROL_SYNC_DONE),
    ("LDAP_CONTROL_DONTUSECOPY", LDAP_CONTROL_DONTUSECOPY),
    (
        "LDAP_CONTROL_PASSWORDPOLICYREQUEST",
        LDAP_CONTROL_PASSWORDPOLICYREQUEST,
    ),
    (
        "LDAP_CONTROL_PASSWORDPOLICYRESPONSE",
        LDAP_CONTROL_PASSWORDPOLICYRESPONSE,
    ),
    (
        "LDAP_CONTROL_X_INCREMENTAL_VALUES",
        LDAP_CONTROL_X_INCREMENTAL_VALUES,
    ),
    ("LDAP_CONTROL_X_DOMAIN_SCOPE", LDAP_CONTROL_X_DOMAIN_SCOPE),
    (
        "LDAP_CONTROL_X_PERMISSIVE_MODIFY",
        LDAP_CONTROL_X_PERMISSIVE_MODIFY,
    ),
    (
        "LDAP_CONTROL_X_SEARCH_OPTIONS",
        LDAP_CONTROL_X_SEARCH_OPTIONS,
    ),
    ("LDAP_CONTROL_X_TREE_DELETE", LDAP_CONTROL_X_TREE_DELETE),
    ("LDAP_CONTROL_X_EXTENDED_DN", LDAP_CONTROL_X_EXTENDED_DN),
    ("LDAP_CONTROL_VLVREQUEST", LDAP_CONTROL_VLVREQUEST),
    ("LDAP_CONTROL_VLVRESPONSE", LDAP_CONTROL_VLVRESPONSE),
];

const FUNCTIONS: &[(&str, BuiltinFn)] = &[
    ("ldap_connect", funcs::ldap_connect),
    ("ldap_unbind", funcs::ldap_unbind),
    ("ldap_close", funcs::ldap_unbind),
    ("ldap_bind", funcs::ldap_bind),
    ("ldap_bind_ext", funcs::ldap_bind_ext),
    ("ldap_sasl_bind", funcs::ldap_sasl_bind),
    ("ldap_read", funcs::ldap_read),
    ("ldap_list", funcs::ldap_list),
    ("ldap_search", funcs::ldap_search),
    ("ldap_free_result", funcs::ldap_free_result),
    ("ldap_count_entries", funcs::ldap_count_entries),
    ("ldap_first_entry", funcs::ldap_first_entry),
    ("ldap_next_entry", funcs::ldap_next_entry),
    ("ldap_get_entries", funcs::ldap_get_entries),
    ("ldap_first_attribute", funcs::ldap_first_attribute),
    ("ldap_next_attribute", funcs::ldap_next_attribute),
    ("ldap_get_attributes", funcs::ldap_get_attributes),
    ("ldap_get_values_len", funcs::ldap_get_values_len),
    ("ldap_get_values", funcs::ldap_get_values),
    ("ldap_get_dn", funcs::ldap_get_dn),
    ("ldap_explode_dn", funcs::ldap_explode_dn),
    ("ldap_dn2ufn", funcs::ldap_dn2ufn),
    ("ldap_add", funcs::ldap_add),
    ("ldap_add_ext", funcs::ldap_add_ext),
    ("ldap_delete", funcs::ldap_delete),
    ("ldap_delete_ext", funcs::ldap_delete_ext),
    ("ldap_modify_batch", funcs::ldap_modify_batch),
    ("ldap_mod_add", funcs::ldap_mod_add),
    ("ldap_mod_add_ext", funcs::ldap_mod_add_ext),
    ("ldap_mod_replace", funcs::ldap_mod_replace),
    ("ldap_mod_replace_ext", funcs::ldap_mod_replace_ext),
    ("ldap_modify", funcs::ldap_modify),
    ("ldap_mod_del", funcs::ldap_mod_del),
    ("ldap_mod_del_ext", funcs::ldap_mod_del_ext),
    ("ldap_errno", funcs::ldap_errno),
    ("ldap_error", funcs::ldap_error),
    ("ldap_err2str", funcs::ldap_err2str),
    ("ldap_compare", funcs::ldap_compare),
    ("ldap_rename", funcs::ldap_rename),
    ("ldap_rename_ext", funcs::ldap_rename_ext),
    ("ldap_get_option", funcs::ldap_get_option),
    ("ldap_set_option", funcs::ldap_set_option),
    ("ldap_count_references", funcs::ldap_count_references),
    ("ldap_first_reference", funcs::ldap_first_reference),
    ("ldap_next_reference", funcs::ldap_next_reference),
    ("ldap_parse_reference", funcs::ldap_parse_reference),
    ("ldap_parse_result", funcs::ldap_parse_result),
    ("ldap_set_rebind_proc", funcs::ldap_set_rebind_proc),
    ("ldap_start_tls", funcs::ldap_start_tls),
    ("ldap_escape", funcs::ldap_escape),
    ("ldap_exop", funcs::ldap_exop),
    ("ldap_exop_sync", funcs::ldap_exop_sync),
    ("ldap_exop_passwd", funcs::ldap_exop_passwd),
    ("ldap_exop_whoami", funcs::ldap_exop_whoami),
    ("ldap_exop_refresh", funcs::ldap_exop_refresh),
    ("ldap_parse_exop", funcs::ldap_parse_exop),
];

/// MINIT
pub fn register(vm: &mut Vm) {
    if vm.loaded_extensions.contains(NAME) {
        return;
    }
    vm.register_extension(NAME);
    NUM_LINKS.with(|n| n.set(0));
    GLOBAL_OPTS.with(|o| *o.borrow_mut() = Options::default());

    vm.phpinfo_hooks.push(|_| {
        format!(
            "\nldap\n\nLDAP Support => enabled\nTotal Links => {}/unlimited\nAPI Version => {}\nVendor Name => {}\nVendor Version => {}\nSASL Support => Enabled\n",
            num_links(),
            LDAP_API_VERSION,
            LDAP_VENDOR_NAME,
            LDAP_VENDOR_VERSION
        )
    });

    for class in [zpp::CONN_CLASS, zpp::RESULT_CLASS, zpp::ENTRY_CLASS] {
        let mut ce = match goro_core::stubs::class_entry(vm, class) {
            Some(stub) => stub.clone(),
            None => ClassEntry::new(class.to_vec()),
        };
        ce.is_final = true;
        let lower = class.to_ascii_lowercase();
        vm.classes.insert(lower.clone(), ce);
        vm.native_class_handlers.insert(
            lower,
            NativeClassHandler {
                has_method: |_c, m, _s| m.eq_ignore_ascii_case(b"__construct"),
                call: ctor_call,
                prop_added: None,
                prop_write: None,
                compare_check: None,
                prop_read: None,
                param_names: None,
            },
        );
    }

    for &(name, f) in FUNCTIONS {
        let params: Vec<&[u8]> = goro_core::stubs::find_function(name.as_bytes())
            .map(|s| s.args.iter().map(|a| a.name.as_bytes()).collect())
            .unwrap_or_default();
        vm.register_function_with_params(name.as_bytes(), f, &params);
        vm.zpp_extension_verified
            .insert(name.as_bytes().to_vec(), goro_core::arginfo::CHECK_ARITY);
    }
    let mut names: Vec<Vec<u8>> = Vec::new();
    for &(n, v) in LONG_CONSTS {
        vm.constants.insert(n.as_bytes().to_vec(), Value::Long(v));
        names.push(n.as_bytes().to_vec());
    }
    for &(n, v) in STR_CONSTS {
        vm.constants.insert(
            n.as_bytes().to_vec(),
            Value::String(goro_core::string::PhpString::from_bytes(v.as_bytes())),
        );
        names.push(n.as_bytes().to_vec());
    }
    vm.module_constants.push((NAME.to_vec(), names));
    // PHP_INI_BEGIN(): ldap.max_links "-1"
    if !vm.constants.contains_key(b"ldap.max_links".as_slice()) {
        vm.constants.insert(
            b"ldap.max_links".to_vec(),
            Value::String(goro_core::string::PhpString::from_bytes(b"-1")),
        );
    }
}

/// Constructors throw, as in ldap.c.
fn ctor_call(
    vm: &mut Vm,
    class: &[u8],
    method: &[u8],
    _called: &[u8],
    _this: Option<&Value>,
    _args: &[Value],
) -> Value {
    if method.eq_ignore_ascii_case(b"__construct") {
        let msg = match class.to_ascii_lowercase().as_slice() {
            b"ldap\\connection" => {
                "Cannot directly construct LDAP\\Connection, use ldap_connect() instead"
            }
            b"ldap\\result" => {
                "Cannot directly construct LDAP\\Result, use the dedicated functions instead"
            }
            _ => "Cannot directly construct LDAP\\ResultEntry, use the dedicated functions instead",
        };
        zpp::throw(vm, b"Error", msg);
    }
    Value::Null
}

/// Assign into a by-reference zval (ZEND_TRY_ASSIGN_REF).
pub(crate) fn assign_ref(target: &Value, newval: Value) {
    if let Value::Reference(r) = target {
        *r.borrow_mut() = newval;
    }
}
