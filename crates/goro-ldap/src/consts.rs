//! Constants of OpenLDAP's ldap.h (2.6).

pub const LDAP_VENDOR_NAME: &str = "OpenLDAP";
pub const LDAP_VENDOR_VERSION: i32 = 20610;
pub const LDAP_API_VERSION: i32 = 3001;
/// The TLS implementation name reported by LDAP_OPT_X_TLS_PACKAGE: the
/// TLS layer follows libldap's OpenSSL backend (tls_o.c).
pub const TLS_PACKAGE: &str = "OpenSSL";

pub const LDAP_VERSION2: i32 = 2;
pub const LDAP_VERSION3: i32 = 3;
pub const LDAP_VERSION_MIN: i32 = LDAP_VERSION2;
pub const LDAP_VERSION_MAX: i32 = LDAP_VERSION3;

pub const LDAP_PORT: i32 = 389;
pub const LDAPS_PORT: i32 = 636;
pub const LDAP_NO_LIMIT: i32 = 0;

// options
pub const LDAP_OPT_API_INFO: i32 = 0x0000;
pub const LDAP_OPT_DESC: i32 = 0x0001;
pub const LDAP_OPT_DEREF: i32 = 0x0002;
pub const LDAP_OPT_SIZELIMIT: i32 = 0x0003;
pub const LDAP_OPT_TIMELIMIT: i32 = 0x0004;
pub const LDAP_OPT_REFERRALS: i32 = 0x0008;
pub const LDAP_OPT_RESTART: i32 = 0x0009;
pub const LDAP_OPT_PROTOCOL_VERSION: i32 = 0x0011;
pub const LDAP_OPT_SERVER_CONTROLS: i32 = 0x0012;
pub const LDAP_OPT_CLIENT_CONTROLS: i32 = 0x0013;
pub const LDAP_OPT_API_FEATURE_INFO: i32 = 0x0015;
pub const LDAP_OPT_HOST_NAME: i32 = 0x0030;
pub const LDAP_OPT_RESULT_CODE: i32 = 0x0031;
pub const LDAP_OPT_ERROR_NUMBER: i32 = LDAP_OPT_RESULT_CODE;
pub const LDAP_OPT_DIAGNOSTIC_MESSAGE: i32 = 0x0032;
pub const LDAP_OPT_ERROR_STRING: i32 = LDAP_OPT_DIAGNOSTIC_MESSAGE;
pub const LDAP_OPT_MATCHED_DN: i32 = 0x0033;
pub const LDAP_OPT_DEBUG_LEVEL: i32 = 0x5001;
pub const LDAP_OPT_TIMEOUT: i32 = 0x5002;
pub const LDAP_OPT_REFHOPLIMIT: i32 = 0x5003;
pub const LDAP_OPT_NETWORK_TIMEOUT: i32 = 0x5005;
pub const LDAP_OPT_URI: i32 = 0x5006;
pub const LDAP_OPT_REFERRAL_URLS: i32 = 0x5007;
pub const LDAP_OPT_DEFBASE: i32 = 0x5009;
pub const LDAP_OPT_CONNECT_ASYNC: i32 = 0x5010;
pub const LDAP_OPT_KEEPCONN: i32 = 0x5013;
pub const LDAP_OPT_SOCKET_BIND_ADDRESSES: i32 = 0x5014;
pub const LDAP_OPT_TCP_USER_TIMEOUT: i32 = 0x5015;

pub const LDAP_OPT_X_TLS: i32 = 0x6000;
pub const LDAP_OPT_X_TLS_CACERTFILE: i32 = 0x6002;
pub const LDAP_OPT_X_TLS_CACERTDIR: i32 = 0x6003;
pub const LDAP_OPT_X_TLS_CERTFILE: i32 = 0x6004;
pub const LDAP_OPT_X_TLS_KEYFILE: i32 = 0x6005;
pub const LDAP_OPT_X_TLS_REQUIRE_CERT: i32 = 0x6006;
pub const LDAP_OPT_X_TLS_PROTOCOL_MIN: i32 = 0x6007;
pub const LDAP_OPT_X_TLS_CIPHER_SUITE: i32 = 0x6008;
pub const LDAP_OPT_X_TLS_RANDOM_FILE: i32 = 0x6009;
pub const LDAP_OPT_X_TLS_CRLCHECK: i32 = 0x600b;
pub const LDAP_OPT_X_TLS_DHFILE: i32 = 0x600e;
pub const LDAP_OPT_X_TLS_NEWCTX: i32 = 0x600f;
pub const LDAP_OPT_X_TLS_CRLFILE: i32 = 0x6010;
pub const LDAP_OPT_X_TLS_PACKAGE: i32 = 0x6011;
pub const LDAP_OPT_X_TLS_ECNAME: i32 = 0x6012;
pub const LDAP_OPT_X_TLS_VERSION: i32 = 0x6013;
pub const LDAP_OPT_X_TLS_CIPHER: i32 = 0x6014;
pub const LDAP_OPT_X_TLS_REQUIRE_SAN: i32 = 0x601a;
pub const LDAP_OPT_X_TLS_PROTOCOL_MAX: i32 = 0x601b;

pub const LDAP_OPT_X_TLS_NEVER: i32 = 0;
pub const LDAP_OPT_X_TLS_HARD: i32 = 1;
pub const LDAP_OPT_X_TLS_DEMAND: i32 = 2;
pub const LDAP_OPT_X_TLS_ALLOW: i32 = 3;
pub const LDAP_OPT_X_TLS_TRY: i32 = 4;

pub const LDAP_OPT_X_TLS_CRL_NONE: i32 = 0;
pub const LDAP_OPT_X_TLS_CRL_PEER: i32 = 1;
pub const LDAP_OPT_X_TLS_CRL_ALL: i32 = 2;

pub const LDAP_OPT_X_TLS_PROTOCOL_SSL2: i32 = 2 << 8;
pub const LDAP_OPT_X_TLS_PROTOCOL_SSL3: i32 = 3 << 8;
pub const LDAP_OPT_X_TLS_PROTOCOL_TLS1_0: i32 = (3 << 8) + 1;
pub const LDAP_OPT_X_TLS_PROTOCOL_TLS1_1: i32 = (3 << 8) + 2;
pub const LDAP_OPT_X_TLS_PROTOCOL_TLS1_2: i32 = (3 << 8) + 3;
pub const LDAP_OPT_X_TLS_PROTOCOL_TLS1_3: i32 = (3 << 8) + 4;

pub const LDAP_OPT_X_SASL_MECH: i32 = 0x6100;
pub const LDAP_OPT_X_SASL_REALM: i32 = 0x6101;
pub const LDAP_OPT_X_SASL_AUTHCID: i32 = 0x6102;
pub const LDAP_OPT_X_SASL_AUTHZID: i32 = 0x6103;
pub const LDAP_OPT_X_SASL_SSF: i32 = 0x6104;
pub const LDAP_OPT_X_SASL_SSF_EXTERNAL: i32 = 0x6105;
pub const LDAP_OPT_X_SASL_SECPROPS: i32 = 0x6106;
pub const LDAP_OPT_X_SASL_SSF_MIN: i32 = 0x6107;
pub const LDAP_OPT_X_SASL_SSF_MAX: i32 = 0x6108;
pub const LDAP_OPT_X_SASL_MAXBUFSIZE: i32 = 0x6109;
pub const LDAP_OPT_X_SASL_MECHLIST: i32 = 0x610a;
pub const LDAP_OPT_X_SASL_NOCANON: i32 = 0x610b;
pub const LDAP_OPT_X_SASL_USERNAME: i32 = 0x610c;

pub const LDAP_OPT_X_KEEPALIVE_IDLE: i32 = 0x6300;
pub const LDAP_OPT_X_KEEPALIVE_PROBES: i32 = 0x6301;
pub const LDAP_OPT_X_KEEPALIVE_INTERVAL: i32 = 0x6302;

pub const LDAP_OPT_SUCCESS: i32 = 0;
pub const LDAP_OPT_ERROR: i32 = -1;

// deref
pub const LDAP_DEREF_NEVER: i32 = 0;
pub const LDAP_DEREF_SEARCHING: i32 = 1;
pub const LDAP_DEREF_FINDING: i32 = 2;
pub const LDAP_DEREF_ALWAYS: i32 = 3;

// scope
pub const LDAP_SCOPE_BASE: i32 = 0;
pub const LDAP_SCOPE_ONELEVEL: i32 = 1;
pub const LDAP_SCOPE_SUBTREE: i32 = 2;
pub const LDAP_SCOPE_SUBORDINATE: i32 = 3;
pub const LDAP_SCOPE_DEFAULT: i32 = -1;

// modifications
pub const LDAP_MOD_ADD: i32 = 0;
pub const LDAP_MOD_DELETE: i32 = 1;
pub const LDAP_MOD_REPLACE: i32 = 2;
pub const LDAP_MOD_INCREMENT: i32 = 3;

// protocol tags
pub const LDAP_TAG_MESSAGE: u32 = 0x30;
pub const LDAP_TAG_MSGID: u32 = 0x02;
pub const LDAP_TAG_LDAPDN: u32 = 0x04;
pub const LDAP_TAG_LDAPCRED: u32 = 0x04;
pub const LDAP_TAG_CONTROLS: u32 = 0xa0;
pub const LDAP_TAG_REFERRAL: u32 = 0xa3;
pub const LDAP_TAG_NEWSUPERIOR: u32 = 0x80;
pub const LDAP_TAG_EXOP_REQ_OID: u32 = 0x80;
pub const LDAP_TAG_EXOP_REQ_VALUE: u32 = 0x81;
pub const LDAP_TAG_EXOP_RES_OID: u32 = 0x8a;
pub const LDAP_TAG_EXOP_RES_VALUE: u32 = 0x8b;
pub const LDAP_TAG_SASL_RES_CREDS: u32 = 0x87;

pub const LDAP_REQ_BIND: u32 = 0x60;
pub const LDAP_REQ_UNBIND: u32 = 0x42;
pub const LDAP_REQ_SEARCH: u32 = 0x63;
pub const LDAP_REQ_MODIFY: u32 = 0x66;
pub const LDAP_REQ_ADD: u32 = 0x68;
pub const LDAP_REQ_DELETE: u32 = 0x4a;
pub const LDAP_REQ_MODDN: u32 = 0x6c;
pub const LDAP_REQ_COMPARE: u32 = 0x6e;
pub const LDAP_REQ_ABANDON: u32 = 0x50;
pub const LDAP_REQ_EXTENDED: u32 = 0x77;

pub const LDAP_RES_BIND: u32 = 0x61;
pub const LDAP_RES_SEARCH_ENTRY: u32 = 0x64;
pub const LDAP_RES_SEARCH_REFERENCE: u32 = 0x73;
pub const LDAP_RES_SEARCH_RESULT: u32 = 0x65;
pub const LDAP_RES_MODIFY: u32 = 0x67;
pub const LDAP_RES_ADD: u32 = 0x69;
pub const LDAP_RES_DELETE: u32 = 0x6b;
pub const LDAP_RES_MODDN: u32 = 0x6d;
pub const LDAP_RES_COMPARE: u32 = 0x6f;
pub const LDAP_RES_EXTENDED: u32 = 0x78;
pub const LDAP_RES_INTERMEDIATE: u32 = 0x79;

pub const LDAP_AUTH_SIMPLE: u32 = 0x80;
pub const LDAP_AUTH_SASL: u32 = 0xa3;

// filter tags
pub const LDAP_FILTER_AND: u32 = 0xa0;
pub const LDAP_FILTER_OR: u32 = 0xa1;
pub const LDAP_FILTER_NOT: u32 = 0xa2;
pub const LDAP_FILTER_EQUALITY: u32 = 0xa3;
pub const LDAP_FILTER_SUBSTRINGS: u32 = 0xa4;
pub const LDAP_FILTER_GE: u32 = 0xa5;
pub const LDAP_FILTER_LE: u32 = 0xa6;
pub const LDAP_FILTER_PRESENT: u32 = 0x87;
pub const LDAP_FILTER_APPROX: u32 = 0xa8;
pub const LDAP_FILTER_EXT: u32 = 0xa9;
pub const LDAP_FILTER_EXT_OID: u32 = 0x81;
pub const LDAP_FILTER_EXT_TYPE: u32 = 0x82;
pub const LDAP_FILTER_EXT_VALUE: u32 = 0x83;
pub const LDAP_FILTER_EXT_DNATTRS: u32 = 0x84;
pub const LDAP_SUBSTRING_INITIAL: u32 = 0x80;
pub const LDAP_SUBSTRING_ANY: u32 = 0x81;
pub const LDAP_SUBSTRING_FINAL: u32 = 0x82;

// result codes
pub const LDAP_SUCCESS: i32 = 0x00;
pub const LDAP_OPERATIONS_ERROR: i32 = 0x01;
pub const LDAP_PROTOCOL_ERROR: i32 = 0x02;
pub const LDAP_TIMELIMIT_EXCEEDED: i32 = 0x03;
pub const LDAP_SIZELIMIT_EXCEEDED: i32 = 0x04;
pub const LDAP_COMPARE_FALSE: i32 = 0x05;
pub const LDAP_COMPARE_TRUE: i32 = 0x06;
pub const LDAP_AUTH_METHOD_NOT_SUPPORTED: i32 = 0x07;
pub const LDAP_STRONG_AUTH_REQUIRED: i32 = 0x08;
pub const LDAP_PARTIAL_RESULTS: i32 = 0x09;
pub const LDAP_REFERRAL: i32 = 0x0a;
pub const LDAP_ADMINLIMIT_EXCEEDED: i32 = 0x0b;
pub const LDAP_UNAVAILABLE_CRITICAL_EXTENSION: i32 = 0x0c;
pub const LDAP_CONFIDENTIALITY_REQUIRED: i32 = 0x0d;
pub const LDAP_SASL_BIND_IN_PROGRESS: i32 = 0x0e;
pub const LDAP_NO_SUCH_ATTRIBUTE: i32 = 0x10;
pub const LDAP_UNDEFINED_TYPE: i32 = 0x11;
pub const LDAP_INAPPROPRIATE_MATCHING: i32 = 0x12;
pub const LDAP_CONSTRAINT_VIOLATION: i32 = 0x13;
pub const LDAP_TYPE_OR_VALUE_EXISTS: i32 = 0x14;
pub const LDAP_INVALID_SYNTAX: i32 = 0x15;
pub const LDAP_NO_SUCH_OBJECT: i32 = 0x20;
pub const LDAP_ALIAS_PROBLEM: i32 = 0x21;
pub const LDAP_INVALID_DN_SYNTAX: i32 = 0x22;
pub const LDAP_IS_LEAF: i32 = 0x23;
pub const LDAP_ALIAS_DEREF_PROBLEM: i32 = 0x24;
pub const LDAP_X_PROXY_AUTHZ_FAILURE: i32 = 0x2f;
pub const LDAP_INAPPROPRIATE_AUTH: i32 = 0x30;
pub const LDAP_INVALID_CREDENTIALS: i32 = 0x31;
pub const LDAP_INSUFFICIENT_ACCESS: i32 = 0x32;
pub const LDAP_BUSY: i32 = 0x33;
pub const LDAP_UNAVAILABLE: i32 = 0x34;
pub const LDAP_UNWILLING_TO_PERFORM: i32 = 0x35;
pub const LDAP_LOOP_DETECT: i32 = 0x36;
pub const LDAP_NAMING_VIOLATION: i32 = 0x40;
pub const LDAP_OBJECT_CLASS_VIOLATION: i32 = 0x41;
pub const LDAP_NOT_ALLOWED_ON_NONLEAF: i32 = 0x42;
pub const LDAP_NOT_ALLOWED_ON_RDN: i32 = 0x43;
pub const LDAP_ALREADY_EXISTS: i32 = 0x44;
pub const LDAP_NO_OBJECT_CLASS_MODS: i32 = 0x45;
pub const LDAP_RESULTS_TOO_LARGE: i32 = 0x46;
pub const LDAP_AFFECTS_MULTIPLE_DSAS: i32 = 0x47;
pub const LDAP_VLV_ERROR: i32 = 0x4c;
pub const LDAP_OTHER: i32 = 0x50;
pub const LDAP_CUP_RESOURCES_EXHAUSTED: i32 = 0x71;
pub const LDAP_CUP_SECURITY_VIOLATION: i32 = 0x72;
pub const LDAP_CUP_INVALID_DATA: i32 = 0x73;
pub const LDAP_CUP_UNSUPPORTED_SCHEME: i32 = 0x74;
pub const LDAP_CUP_RELOAD_REQUIRED: i32 = 0x75;
pub const LDAP_CANCELLED: i32 = 0x76;
pub const LDAP_NO_SUCH_OPERATION: i32 = 0x77;
pub const LDAP_TOO_LATE: i32 = 0x78;
pub const LDAP_CANNOT_CANCEL: i32 = 0x79;
pub const LDAP_ASSERTION_FAILED: i32 = 0x7a;
pub const LDAP_PROXIED_AUTHORIZATION_DENIED: i32 = 0x7b;
pub const LDAP_SYNC_REFRESH_REQUIRED: i32 = 0x1000;
pub const LDAP_X_SYNC_REFRESH_REQUIRED: i32 = 0x4100;
pub const LDAP_X_ASSERTION_FAILED: i32 = 0x410f;
pub const LDAP_X_NO_OPERATION: i32 = 0x410e;
pub const LDAP_TXN_SPECIFY_OKAY: i32 = 0x4120;
pub const LDAP_TXN_ID_INVALID: i32 = 0x4121;

pub const LDAP_SERVER_DOWN: i32 = -1;
pub const LDAP_LOCAL_ERROR: i32 = -2;
pub const LDAP_ENCODING_ERROR: i32 = -3;
pub const LDAP_DECODING_ERROR: i32 = -4;
pub const LDAP_TIMEOUT: i32 = -5;
pub const LDAP_AUTH_UNKNOWN: i32 = -6;
pub const LDAP_FILTER_ERROR: i32 = -7;
pub const LDAP_USER_CANCELLED: i32 = -8;
pub const LDAP_PARAM_ERROR: i32 = -9;
pub const LDAP_NO_MEMORY: i32 = -10;
pub const LDAP_CONNECT_ERROR: i32 = -11;
pub const LDAP_NOT_SUPPORTED: i32 = -12;
pub const LDAP_CONTROL_NOT_FOUND: i32 = -13;
pub const LDAP_NO_RESULTS_RETURNED: i32 = -14;
pub const LDAP_MORE_RESULTS_TO_RETURN: i32 = -15;
pub const LDAP_CLIENT_LOOP: i32 = -16;
pub const LDAP_REFERRAL_LIMIT_EXCEEDED: i32 = -17;
pub const LDAP_X_CONNECTING: i32 = -18;

// controls
pub const LDAP_CONTROL_MANAGEDSAIT: &str = "2.16.840.1.113730.3.4.2";
pub const LDAP_CONTROL_PROXY_AUTHZ: &str = "2.16.840.1.113730.3.4.18";
pub const LDAP_CONTROL_SUBENTRIES: &str = "1.3.6.1.4.1.4203.1.10.1";
pub const LDAP_CONTROL_VALUESRETURNFILTER: &str = "1.2.826.0.1.3344810.2.3";
pub const LDAP_CONTROL_ASSERT: &str = "1.3.6.1.1.12";
pub const LDAP_CONTROL_PRE_READ: &str = "1.3.6.1.1.13.1";
pub const LDAP_CONTROL_POST_READ: &str = "1.3.6.1.1.13.2";
pub const LDAP_CONTROL_SORTREQUEST: &str = "1.2.840.113556.1.4.473";
pub const LDAP_CONTROL_SORTRESPONSE: &str = "1.2.840.113556.1.4.474";
pub const LDAP_CONTROL_PAGEDRESULTS: &str = "1.2.840.113556.1.4.319";
pub const LDAP_CONTROL_AUTHZID_REQUEST: &str = "2.16.840.1.113730.3.4.16";
pub const LDAP_CONTROL_AUTHZID_RESPONSE: &str = "2.16.840.1.113730.3.4.15";
pub const LDAP_CONTROL_SYNC: &str = "1.3.6.1.4.1.4203.1.9.1.1";
pub const LDAP_CONTROL_SYNC_STATE: &str = "1.3.6.1.4.1.4203.1.9.1.2";
pub const LDAP_CONTROL_SYNC_DONE: &str = "1.3.6.1.4.1.4203.1.9.1.3";
pub const LDAP_CONTROL_DONTUSECOPY: &str = "1.3.6.1.1.22";
pub const LDAP_CONTROL_PASSWORDPOLICYREQUEST: &str = "1.3.6.1.4.1.42.2.27.8.5.1";
pub const LDAP_CONTROL_PASSWORDPOLICYRESPONSE: &str = "1.3.6.1.4.1.42.2.27.8.5.1";
pub const LDAP_CONTROL_X_INCREMENTAL_VALUES: &str = "1.2.840.113556.1.4.802";
pub const LDAP_CONTROL_X_DOMAIN_SCOPE: &str = "1.2.840.113556.1.4.1339";
pub const LDAP_CONTROL_X_PERMISSIVE_MODIFY: &str = "1.2.840.113556.1.4.1413";
pub const LDAP_CONTROL_X_SEARCH_OPTIONS: &str = "1.2.840.113556.1.4.1340";
pub const LDAP_CONTROL_X_TREE_DELETE: &str = "1.2.840.113556.1.4.805";
pub const LDAP_CONTROL_X_EXTENDED_DN: &str = "1.2.840.113556.1.4.529";
pub const LDAP_CONTROL_VLVREQUEST: &str = "2.16.840.1.113730.3.4.9";
pub const LDAP_CONTROL_VLVRESPONSE: &str = "2.16.840.1.113730.3.4.10";

// extended operations
pub const LDAP_EXOP_START_TLS: &str = "1.3.6.1.4.1.1466.20037";
pub const LDAP_EXOP_MODIFY_PASSWD: &str = "1.3.6.1.4.1.4203.1.11.1";
pub const LDAP_EXOP_REFRESH: &str = "1.3.6.1.4.1.1466.101.119.1";
pub const LDAP_EXOP_WHO_AM_I: &str = "1.3.6.1.4.1.4203.1.11.3";
pub const LDAP_EXOP_TURN: &str = "1.3.6.1.1.19";
pub const LDAP_EXOP_CANCEL: &str = "1.3.6.1.1.8";
pub const LDAP_NOTICE_OF_DISCONNECTION: &str = "1.3.6.1.4.1.1466.20036";

// password modify request/response tags
pub const LDAP_TAG_EXOP_MODIFY_PASSWD_ID: u32 = 0x80;
pub const LDAP_TAG_EXOP_MODIFY_PASSWD_OLD: u32 = 0x81;
pub const LDAP_TAG_EXOP_MODIFY_PASSWD_NEW: u32 = 0x82;
pub const LDAP_TAG_EXOP_MODIFY_PASSWD_GEN: u32 = 0x80;
// refresh
pub const LDAP_TAG_EXOP_REFRESH_REQ_DN: u32 = 0x80;
pub const LDAP_TAG_EXOP_REFRESH_REQ_TTL: u32 = 0x81;
pub const LDAP_TAG_EXOP_REFRESH_RES_TTL: u32 = 0x81;
// VLV
pub const LDAP_VLVBYINDEX_IDENTIFIER: u32 = 0xa0;
pub const LDAP_VLVBYVALUE_IDENTIFIER: u32 = 0x81;
pub const LDAP_VLVCONTEXT_IDENTIFIER: u32 = 0x04;
// sort
pub const LDAP_MATCHRULE_IDENTIFIER: u32 = 0x80;
pub const LDAP_REVERSEORDER_IDENTIFIER: u32 = 0x81;
pub const LDAP_ATTRTYPES_IDENTIFIER: u32 = 0x80;
