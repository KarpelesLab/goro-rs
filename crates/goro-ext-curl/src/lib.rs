mod handle;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use goro_core::array::{ArrayKey, PhpArray};
use goro_core::object::{ClassEntry, PhpObject};
use goro_core::string::PhpString;
use goro_core::value::Value;
use goro_core::vm::{Vm, VmError};

use handle::{CurlHandle, CURL_HANDLES as CURL_HANDLES_STORE};

/// Helper: access the curl handles RefCell<HashMap>.
/// Wraps the thread_local HandleStore so we can clear `private` on Drop.
fn with_curl_handles<R>(f: impl FnOnce(&std::cell::RefCell<std::collections::HashMap<i64, CurlHandle>>) -> R) -> R {
    CURL_HANDLES_STORE.with(|store| f(&store.handles))
}

// CURLOPT constants
const CURLOPT_URL: i64 = 10002;
const CURLOPT_RETURNTRANSFER: i64 = 19913;
const CURLOPT_POST: i64 = 47;
const CURLOPT_POSTFIELDS: i64 = 10015;
const CURLOPT_HTTPHEADER: i64 = 10023;
const CURLOPT_FOLLOWLOCATION: i64 = 52;
const CURLOPT_MAXREDIRS: i64 = 68;
const CURLOPT_TIMEOUT: i64 = 13;
const CURLOPT_CONNECTTIMEOUT: i64 = 78;
const CURLOPT_USERAGENT: i64 = 10018;
const CURLOPT_HEADER: i64 = 42;
const CURLOPT_NOBODY: i64 = 44;
const CURLOPT_SSL_VERIFYPEER: i64 = 64;
const CURLOPT_SSL_VERIFYHOST: i64 = 81;
const CURLOPT_CUSTOMREQUEST: i64 = 10036;
const CURLOPT_ENCODING: i64 = 10102;
const CURLOPT_HTTPGET: i64 = 80;
const CURLOPT_COOKIE: i64 = 10022;
const CURLOPT_USERPWD: i64 = 10005;
const CURLOPT_HTTPAUTH: i64 = 107;
const CURLOPT_FAILONERROR: i64 = 45;
const CURLOPT_PUT: i64 = 54;
const CURLOPT_PRIVATE: i64 = 10103;

// CURLINFO constants
const CURLINFO_HTTP_CODE: i64 = 2097154;
const CURLINFO_EFFECTIVE_URL: i64 = 1048577;
const CURLINFO_CONTENT_TYPE: i64 = 1048594;
const CURLINFO_TOTAL_TIME: i64 = 3145731;
const CURLINFO_REDIRECT_COUNT: i64 = 2097172;
const CURLINFO_HEADER_SIZE: i64 = 2097163;
const CURLINFO_PRIVATE: i64 = 1048597;

// Multi handle registry
thread_local! {
    static MULTI_HANDLES: RefCell<HashMap<i64, Vec<i64>>> = RefCell::new(HashMap::new());
    static SHARE_HANDLES: RefCell<HashMap<i64, ()>> = RefCell::new(HashMap::new());
    /// Per-share errno tracking (keyed by share id)
    static SHARE_ERRNOS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
    /// Per-multi errno tracking
    static MULTI_ERRNOS: RefCell<HashMap<i64, i64>> = RefCell::new(HashMap::new());
}

/// Helper: create a CurlHandle object, registering the handle in CURL_HANDLES.
fn make_curl_handle_obj(vm: &mut Vm, handle: CurlHandle) -> Value {
    let id = vm.next_object_id() as i64;
    with_curl_handles(|h| {
        h.borrow_mut().insert(id, handle);
    });
    let obj = PhpObject::new(b"CurlHandle".to_vec(), id as u64);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Helper: create a CurlMultiHandle object
fn make_multi_handle_obj(vm: &mut Vm) -> Value {
    let id = vm.next_object_id() as i64;
    MULTI_HANDLES.with(|h| {
        h.borrow_mut().insert(id, Vec::new());
    });
    let obj = PhpObject::new(b"CurlMultiHandle".to_vec(), id as u64);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Helper: create a CurlShareHandle object
fn make_share_handle_obj(vm: &mut Vm) -> Value {
    let id = vm.next_object_id() as i64;
    SHARE_HANDLES.with(|h| {
        h.borrow_mut().insert(id, ());
    });
    let obj = PhpObject::new(b"CurlShareHandle".to_vec(), id as u64);
    Value::Object(Rc::new(RefCell::new(obj)))
}

/// Extract curl id from a Value (Object or Long fallback)
fn arg_curl_id(arg: Option<&Value>) -> Option<i64> {
    match arg {
        Some(Value::Object(obj)) => {
            let b = obj.borrow();
            if b.class_name == b"CurlHandle" {
                Some(b.object_id as i64)
            } else {
                None
            }
        }
        Some(Value::Long(n)) => Some(*n),
        Some(Value::Reference(r)) => arg_curl_id(Some(&r.borrow().clone())),
        _ => None,
    }
}

fn arg_multi_id(arg: Option<&Value>) -> Option<i64> {
    match arg {
        Some(Value::Object(obj)) => {
            let b = obj.borrow();
            if b.class_name == b"CurlMultiHandle" {
                Some(b.object_id as i64)
            } else {
                None
            }
        }
        Some(Value::Long(n)) => Some(*n),
        Some(Value::Reference(r)) => arg_multi_id(Some(&r.borrow().clone())),
        _ => None,
    }
}

fn arg_share_id(arg: Option<&Value>) -> Option<i64> {
    match arg {
        Some(Value::Object(obj)) => {
            let b = obj.borrow();
            if b.class_name == b"CurlShareHandle" {
                Some(b.object_id as i64)
            } else {
                None
            }
        }
        Some(Value::Long(n)) => Some(*n),
        Some(Value::Reference(r)) => arg_share_id(Some(&r.borrow().clone())),
        _ => None,
    }
}

/// Throw a ValueError
fn throw_value_error(vm: &mut Vm, msg: &str) -> VmError {
    let exc = vm.create_exception(b"ValueError", msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError { message: msg.to_string(), line: vm.current_line }
}

/// Throw a TypeError
fn throw_type_error(vm: &mut Vm, msg: &str) -> VmError {
    let exc = vm.create_exception(b"TypeError", msg, vm.current_line);
    vm.current_exception = Some(exc);
    VmError { message: msg.to_string(), line: vm.current_line }
}

/// Register all curl extension functions and constants
pub fn register(vm: &mut Vm) {
    vm.register_extension(b"curl");

    // Register classes
    vm.classes.insert(b"curlhandle".to_vec(), ClassEntry::new(b"CurlHandle".to_vec()));
    vm.classes.insert(b"curlmultihandle".to_vec(), ClassEntry::new(b"CurlMultiHandle".to_vec()));
    vm.classes.insert(b"curlsharehandle".to_vec(), ClassEntry::new(b"CurlShareHandle".to_vec()));
    let mut curlfile = ClassEntry::new(b"CURLFile".to_vec());
    curlfile.allow_dynamic_properties = true;
    vm.classes.insert(b"curlfile".to_vec(), curlfile);

    // Register functions
    vm.register_function(b"curl_init", curl_init);
    vm.register_function(b"curl_setopt", curl_setopt);
    vm.register_function(b"curl_setopt_array", curl_setopt_array);
    vm.register_function(b"curl_exec", curl_exec);
    vm.register_function(b"curl_close", curl_close);
    vm.register_function(b"curl_error", curl_error);
    vm.register_function(b"curl_errno", curl_errno);
    vm.register_function(b"curl_getinfo", curl_getinfo);
    vm.register_function(b"curl_reset", curl_reset);
    vm.register_function(b"curl_version", curl_version);
    vm.register_function(b"curl_multi_init", curl_multi_init);
    vm.register_function(b"curl_multi_add_handle", curl_multi_add_handle);
    vm.register_function(b"curl_multi_remove_handle", curl_multi_remove_handle);
    vm.register_function(b"curl_multi_exec", curl_multi_exec);
    vm.register_function(b"curl_multi_getcontent", curl_multi_getcontent);
    vm.register_function(b"curl_multi_close", curl_multi_close);
    vm.register_function(b"curl_multi_errno", curl_multi_errno);
    vm.register_function(b"curl_multi_strerror", curl_multi_strerror);
    vm.register_function(b"curl_multi_setopt", curl_multi_setopt);
    vm.register_function(b"curl_multi_info_read", curl_multi_info_read);
    vm.register_function(b"curl_multi_select", curl_multi_select);
    vm.register_function(b"curl_multi_get_handles", curl_multi_get_handles);
    vm.register_function(b"curl_copy_handle", curl_copy_handle);
    vm.register_function(b"curl_file_create", curl_file_create);
    vm.register_function(b"curl_share_init", curl_share_init);
    vm.register_function(b"curl_share_init_persistent", curl_share_init_persistent);
    vm.register_function(b"curl_share_setopt", curl_share_setopt);
    vm.register_function(b"curl_share_close", curl_share_close);
    vm.register_function(b"curl_share_strerror", curl_share_strerror);
    vm.register_function(b"curl_share_errno", curl_share_errno);
    vm.register_function(b"curl_strerror", curl_strerror);
    vm.register_function(b"curl_pause", curl_pause);
    vm.register_function(b"curl_upkeep", curl_upkeep);
    vm.register_function(b"curl_escape", curl_escape);
    vm.register_function(b"curl_unescape", curl_unescape);

    // CURLOPT constants
    vm.constants.insert(b"CURLOPT_URL".to_vec(), Value::Long(10002));
    vm.constants.insert(b"CURLOPT_RETURNTRANSFER".to_vec(), Value::Long(19913));
    vm.constants.insert(b"CURLOPT_POST".to_vec(), Value::Long(47));
    vm.constants.insert(b"CURLOPT_POSTFIELDS".to_vec(), Value::Long(10015));
    vm.constants.insert(b"CURLOPT_HTTPHEADER".to_vec(), Value::Long(10023));
    vm.constants.insert(b"CURLOPT_FOLLOWLOCATION".to_vec(), Value::Long(52));
    vm.constants.insert(b"CURLOPT_MAXREDIRS".to_vec(), Value::Long(68));
    vm.constants.insert(b"CURLOPT_TIMEOUT".to_vec(), Value::Long(13));
    vm.constants.insert(b"CURLOPT_CONNECTTIMEOUT".to_vec(), Value::Long(78));
    vm.constants.insert(b"CURLOPT_USERAGENT".to_vec(), Value::Long(10018));
    vm.constants.insert(b"CURLOPT_HEADER".to_vec(), Value::Long(42));
    vm.constants.insert(b"CURLOPT_NOBODY".to_vec(), Value::Long(44));
    vm.constants.insert(b"CURLOPT_SSL_VERIFYPEER".to_vec(), Value::Long(64));
    vm.constants.insert(b"CURLOPT_SSL_VERIFYHOST".to_vec(), Value::Long(81));
    vm.constants.insert(b"CURLOPT_CUSTOMREQUEST".to_vec(), Value::Long(10036));
    vm.constants.insert(b"CURLOPT_ENCODING".to_vec(), Value::Long(10102));
    vm.constants.insert(b"CURLOPT_HTTPGET".to_vec(), Value::Long(80));
    vm.constants.insert(b"CURLOPT_COOKIE".to_vec(), Value::Long(10022));
    vm.constants.insert(b"CURLOPT_COOKIEFILE".to_vec(), Value::Long(10031));
    vm.constants.insert(b"CURLOPT_USERPWD".to_vec(), Value::Long(10005));
    vm.constants.insert(b"CURLOPT_HTTPAUTH".to_vec(), Value::Long(107));
    vm.constants.insert(b"CURLOPT_HTTP_VERSION".to_vec(), Value::Long(84));
    vm.constants.insert(b"CURLOPT_VERBOSE".to_vec(), Value::Long(41));
    vm.constants.insert(b"CURLOPT_HEADERFUNCTION".to_vec(), Value::Long(20079));
    vm.constants.insert(b"CURLOPT_WRITEFUNCTION".to_vec(), Value::Long(20011));
    vm.constants.insert(b"CURLOPT_READFUNCTION".to_vec(), Value::Long(20012));
    vm.constants.insert(b"CURLOPT_INFILESIZE".to_vec(), Value::Long(14));
    vm.constants.insert(b"CURLOPT_PUT".to_vec(), Value::Long(54));
    vm.constants.insert(b"CURLOPT_FAILONERROR".to_vec(), Value::Long(45));
    vm.constants.insert(b"CURLOPT_SHARE".to_vec(), Value::Long(10100));
    vm.constants.insert(b"CURLOPT_PRIVATE".to_vec(), Value::Long(10103));

    // CURLINFO constants
    vm.constants.insert(b"CURLINFO_HTTP_CODE".to_vec(), Value::Long(2097154));
    vm.constants.insert(b"CURLINFO_EFFECTIVE_URL".to_vec(), Value::Long(1048577));
    vm.constants.insert(b"CURLINFO_CONTENT_TYPE".to_vec(), Value::Long(1048594));
    vm.constants.insert(b"CURLINFO_TOTAL_TIME".to_vec(), Value::Long(3145731));
    vm.constants.insert(b"CURLINFO_REDIRECT_COUNT".to_vec(), Value::Long(2097172));
    vm.constants.insert(b"CURLINFO_HEADER_SIZE".to_vec(), Value::Long(2097163));
    vm.constants.insert(b"CURLINFO_RESPONSE_CODE".to_vec(), Value::Long(2097154));
    vm.constants.insert(b"CURLINFO_PRIVATE".to_vec(), Value::Long(1048597));

    // CURLE error constants
    vm.constants.insert(b"CURLE_OK".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLE_UNSUPPORTED_PROTOCOL".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLE_URL_MALFORMAT".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURLE_COULDNT_RESOLVE_HOST".to_vec(), Value::Long(6));
    vm.constants.insert(b"CURLE_COULDNT_CONNECT".to_vec(), Value::Long(7));
    vm.constants.insert(b"CURLE_OPERATION_TIMEDOUT".to_vec(), Value::Long(28));
    vm.constants.insert(b"CURLE_SSL_CONNECT_ERROR".to_vec(), Value::Long(35));

    // HTTP version constants
    vm.constants.insert(b"CURL_HTTP_VERSION_NONE".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURL_HTTP_VERSION_1_0".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURL_HTTP_VERSION_1_1".to_vec(), Value::Long(2));

    // Auth constants
    vm.constants.insert(b"CURLAUTH_BASIC".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLAUTH_DIGEST".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURLAUTH_ANY".to_vec(), Value::Long(-17));

    // CurlMulti constants
    vm.constants.insert(b"CURLM_OK".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLM_CALL_MULTI_PERFORM".to_vec(), Value::Long(-1));
    vm.constants.insert(b"CURLM_INTERNAL_ERROR".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURLM_BAD_HANDLE".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLM_BAD_EASY_HANDLE".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURLM_OUT_OF_MEMORY".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURLM_UNKNOWN_OPTION".to_vec(), Value::Long(6));
    vm.constants.insert(b"CURLMSG_DONE".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLMOPT_MAXCONNECTS".to_vec(), Value::Long(6));
    vm.constants.insert(b"CURLMOPT_PIPELINING".to_vec(), Value::Long(3));

    // Additional CURLOPT constants
    vm.constants.insert(b"CURLOPT_FILE".to_vec(), Value::Long(10001));
    vm.constants.insert(b"CURLOPT_STDERR".to_vec(), Value::Long(10037));
    vm.constants.insert(b"CURLOPT_SAFE_UPLOAD".to_vec(), Value::Long(148));
    vm.constants.insert(b"CURLOPT_NOPROGRESS".to_vec(), Value::Long(43));
    vm.constants.insert(b"CURLOPT_UPLOAD".to_vec(), Value::Long(46));
    vm.constants.insert(b"CURLOPT_PROGRESSFUNCTION".to_vec(), Value::Long(20056));
    vm.constants.insert(b"CURLOPT_PROTOCOLS_STR".to_vec(), Value::Long(10318));
    vm.constants.insert(b"CURLOPT_SSH_HOSTKEYFUNCTION".to_vec(), Value::Long(20316));
    vm.constants.insert(b"CURLINFO_HEADER_OUT".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURL_LOCK_DATA_DNS".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURLOPT_INFILE".to_vec(), Value::Long(10009));
    vm.constants.insert(b"CURLOPT_REFERER".to_vec(), Value::Long(10016));
    vm.constants.insert(b"CURLOPT_PROXY".to_vec(), Value::Long(10004));
    vm.constants.insert(b"CURLOPT_PROXYTYPE".to_vec(), Value::Long(101));
    vm.constants.insert(b"CURLOPT_SSLCERT".to_vec(), Value::Long(10025));
    vm.constants.insert(b"CURLOPT_SSLKEY".to_vec(), Value::Long(10087));
    vm.constants.insert(b"CURLOPT_CAINFO".to_vec(), Value::Long(10065));
    vm.constants.insert(b"CURLOPT_CAPATH".to_vec(), Value::Long(10097));
    vm.constants.insert(b"CURLOPT_RANGE".to_vec(), Value::Long(10007));
    vm.constants.insert(b"CURLOPT_RESUME_FROM".to_vec(), Value::Long(21));
    vm.constants.insert(b"CURLOPT_BUFFERSIZE".to_vec(), Value::Long(98));
    vm.constants.insert(b"CURLOPT_LOW_SPEED_LIMIT".to_vec(), Value::Long(19));
    vm.constants.insert(b"CURLOPT_LOW_SPEED_TIME".to_vec(), Value::Long(20));
    vm.constants.insert(b"CURLOPT_TIMEOUT_MS".to_vec(), Value::Long(155));
    vm.constants.insert(b"CURLOPT_CONNECTTIMEOUT_MS".to_vec(), Value::Long(156));
    vm.constants.insert(b"CURLOPT_PORT".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURLOPT_COOKIEJAR".to_vec(), Value::Long(10082));
    vm.constants.insert(b"CURLOPT_COOKIESESSION".to_vec(), Value::Long(96));
    vm.constants.insert(b"CURLOPT_SSLVERSION".to_vec(), Value::Long(32));
    vm.constants.insert(b"CURLOPT_INTERFACE".to_vec(), Value::Long(10062));
    vm.constants.insert(b"CURLOPT_XOAUTH2_BEARER".to_vec(), Value::Long(10220));
    vm.constants.insert(b"CURLOPT_ACCEPT_ENCODING".to_vec(), Value::Long(10102));
    vm.constants.insert(b"CURLOPT_TRANSFER_ENCODING".to_vec(), Value::Long(207));
    vm.constants.insert(b"CURLOPT_MAXFILESIZE".to_vec(), Value::Long(114));
    vm.constants.insert(b"CURLOPT_RESOLVE".to_vec(), Value::Long(10203));
    vm.constants.insert(b"CURLOPT_DNS_SERVERS".to_vec(), Value::Long(10211));
    vm.constants.insert(b"CURLOPT_XFERINFOFUNCTION".to_vec(), Value::Long(20219));
    vm.constants.insert(b"CURLOPT_WRITEHEADER".to_vec(), Value::Long(10029));
    vm.constants.insert(b"CURLOPT_DEBUGFUNCTION".to_vec(), Value::Long(20094));
    vm.constants.insert(b"CURLOPT_READDATA".to_vec(), Value::Long(10009));
    vm.constants.insert(b"CURLOPT_COOKIELIST".to_vec(), Value::Long(10135));
    vm.constants.insert(b"CURLINFO_COOKIELIST".to_vec(), Value::Long(4194332));
    vm.constants.insert(b"CURLOPT_FRESH_CONNECT".to_vec(), Value::Long(74));
    vm.constants.insert(b"CURLOPT_FORBID_REUSE".to_vec(), Value::Long(75));
    vm.constants.insert(b"CURLOPT_UPKEEP_INTERVAL_MS".to_vec(), Value::Long(281));
    vm.constants.insert(b"CURLMOPT_PUSHFUNCTION".to_vec(), Value::Long(20014));

    // CURLPAUSE constants
    vm.constants.insert(b"CURLPAUSE_ALL".to_vec(), Value::Long(5));
    vm.constants.insert(b"CURLPAUSE_CONT".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLPAUSE_RECV".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLPAUSE_RECV_CONT".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLPAUSE_SEND".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURLPAUSE_SEND_CONT".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURL_READFUNC_PAUSE".to_vec(), Value::Long(0x10000001));

    // Additional CURLINFO constants
    vm.constants.insert(b"CURLINFO_SIZE_DOWNLOAD".to_vec(), Value::Long(3145736));
    vm.constants.insert(b"CURLINFO_SIZE_UPLOAD".to_vec(), Value::Long(3145735));
    vm.constants.insert(b"CURLINFO_SPEED_DOWNLOAD".to_vec(), Value::Long(3145737));
    vm.constants.insert(b"CURLINFO_SPEED_UPLOAD".to_vec(), Value::Long(3145738));
    vm.constants.insert(b"CURLINFO_FILETIME".to_vec(), Value::Long(2097166));
    vm.constants.insert(b"CURLINFO_CONTENT_LENGTH_DOWNLOAD".to_vec(), Value::Long(3145743));
    vm.constants.insert(b"CURLINFO_CONTENT_LENGTH_UPLOAD".to_vec(), Value::Long(3145744));
    vm.constants.insert(b"CURLINFO_REDIRECT_URL".to_vec(), Value::Long(1048607));
    vm.constants.insert(b"CURLINFO_PRIMARY_IP".to_vec(), Value::Long(1048608));
    vm.constants.insert(b"CURLINFO_LOCAL_IP".to_vec(), Value::Long(1048617));
    vm.constants.insert(b"CURLINFO_LOCAL_PORT".to_vec(), Value::Long(2097194));
    vm.constants.insert(b"CURLINFO_PRIMARY_PORT".to_vec(), Value::Long(2097192));

    // CURL share constants
    vm.constants.insert(b"CURL_LOCK_DATA_COOKIE".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURL_LOCK_DATA_SSL_SESSION".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURL_LOCK_DATA_CONNECT".to_vec(), Value::Long(5));
    vm.constants.insert(b"CURL_LOCK_DATA_PSL".to_vec(), Value::Long(6));
    vm.constants.insert(b"CURLSHOPT_NONE".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLSHOPT_SHARE".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLSHOPT_UNSHARE".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURLSHE_OK".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLSHE_BAD_OPTION".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLSHE_IN_USE".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURLSHE_INVALID".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURLSHE_NOMEM".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURLSHE_NOT_BUILT_IN".to_vec(), Value::Long(5));

    // CURLcode constants
    vm.constants.insert(b"CURLE_ABORTED_BY_CALLBACK".to_vec(), Value::Long(42));
    vm.constants.insert(b"CURLE_GOT_NOTHING".to_vec(), Value::Long(52));
    vm.constants.insert(b"CURLE_RECV_ERROR".to_vec(), Value::Long(56));
    vm.constants.insert(b"CURLE_SEND_ERROR".to_vec(), Value::Long(55));

    // CURLPROXY constants
    vm.constants.insert(b"CURLPROXY_HTTP".to_vec(), Value::Long(0));
    vm.constants.insert(b"CURLPROXY_SOCKS4".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURLPROXY_SOCKS5".to_vec(), Value::Long(5));

    // CURL version constants
    vm.constants.insert(b"CURL_VERSION_IPV6".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURL_VERSION_KERBEROS4".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURL_VERSION_SSL".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURL_VERSION_LIBZ".to_vec(), Value::Long(8));
    vm.constants.insert(b"CURL_VERSION_NTLM".to_vec(), Value::Long(16));
    vm.constants.insert(b"CURL_VERSION_GSSNEGOTIATE".to_vec(), Value::Long(32));
    vm.constants.insert(b"CURL_VERSION_DEBUG".to_vec(), Value::Long(64));
    vm.constants.insert(b"CURL_VERSION_ASYNCHDNS".to_vec(), Value::Long(128));
    vm.constants.insert(b"CURL_VERSION_SPNEGO".to_vec(), Value::Long(256));
    vm.constants.insert(b"CURL_VERSION_LARGEFILE".to_vec(), Value::Long(512));
    vm.constants.insert(b"CURL_VERSION_IDN".to_vec(), Value::Long(1024));
    vm.constants.insert(b"CURL_VERSION_SSPI".to_vec(), Value::Long(2048));
    vm.constants.insert(b"CURL_VERSION_CONV".to_vec(), Value::Long(4096));
    vm.constants.insert(b"CURL_VERSION_CURLDEBUG".to_vec(), Value::Long(8192));
    vm.constants.insert(b"CURL_VERSION_TLSAUTH_SRP".to_vec(), Value::Long(16384));
    vm.constants.insert(b"CURL_VERSION_NTLM_WB".to_vec(), Value::Long(32768));
    vm.constants.insert(b"CURL_VERSION_HTTP2".to_vec(), Value::Long(65536));
    vm.constants.insert(b"CURL_VERSION_GSSAPI".to_vec(), Value::Long(131072));
    vm.constants.insert(b"CURL_VERSION_KERBEROS5".to_vec(), Value::Long(262144));
    vm.constants.insert(b"CURL_VERSION_UNIX_SOCKETS".to_vec(), Value::Long(524288));
    vm.constants.insert(b"CURL_VERSION_PSL".to_vec(), Value::Long(1048576));
    vm.constants.insert(b"CURL_VERSION_HTTPS_PROXY".to_vec(), Value::Long(2097152));
    vm.constants.insert(b"CURL_VERSION_MULTI_SSL".to_vec(), Value::Long(4194304));
    vm.constants.insert(b"CURL_VERSION_BROTLI".to_vec(), Value::Long(8388608));
    vm.constants.insert(b"CURL_VERSION_ALTSVC".to_vec(), Value::Long(16777216));
    vm.constants.insert(b"CURL_VERSION_HTTP3".to_vec(), Value::Long(33554432));
    vm.constants.insert(b"CURL_VERSION_ZSTD".to_vec(), Value::Long(67108864));
    vm.constants.insert(b"CURL_VERSION_UNICODE".to_vec(), Value::Long(134217728));
    vm.constants.insert(b"CURL_VERSION_HSTS".to_vec(), Value::Long(268435456));
    vm.constants.insert(b"CURL_VERSION_GSASL".to_vec(), Value::Long(536870912));

    // CURL sslversion constants
    vm.constants.insert(b"CURL_SSLVERSION_TLSv1".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURL_SSLVERSION_TLSv1_0".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURL_SSLVERSION_TLSv1_1".to_vec(), Value::Long(5));
    vm.constants.insert(b"CURL_SSLVERSION_TLSv1_2".to_vec(), Value::Long(6));
    vm.constants.insert(b"CURL_SSLVERSION_TLSv1_3".to_vec(), Value::Long(7));

    // HTTP version constants
    vm.constants.insert(b"CURL_HTTP_VERSION_2_0".to_vec(), Value::Long(3));
    vm.constants.insert(b"CURL_HTTP_VERSION_2TLS".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURL_HTTP_VERSION_2_PRIOR_KNOWLEDGE".to_vec(), Value::Long(5));

    // CURLPROTO_* constants
    vm.constants.insert(b"CURLPROTO_HTTP".to_vec(), Value::Long(1));
    vm.constants.insert(b"CURLPROTO_HTTPS".to_vec(), Value::Long(2));
    vm.constants.insert(b"CURLPROTO_FTP".to_vec(), Value::Long(4));
    vm.constants.insert(b"CURLPROTO_FTPS".to_vec(), Value::Long(8));
    vm.constants.insert(b"CURLPROTO_SCP".to_vec(), Value::Long(16));
    vm.constants.insert(b"CURLPROTO_SFTP".to_vec(), Value::Long(32));
    vm.constants.insert(b"CURLPROTO_TELNET".to_vec(), Value::Long(64));
    vm.constants.insert(b"CURLPROTO_LDAP".to_vec(), Value::Long(128));
    vm.constants.insert(b"CURLPROTO_LDAPS".to_vec(), Value::Long(256));
    vm.constants.insert(b"CURLPROTO_DICT".to_vec(), Value::Long(512));
    vm.constants.insert(b"CURLPROTO_FILE".to_vec(), Value::Long(1024));
    vm.constants.insert(b"CURLPROTO_TFTP".to_vec(), Value::Long(2048));
    vm.constants.insert(b"CURLPROTO_IMAP".to_vec(), Value::Long(4096));
    vm.constants.insert(b"CURLPROTO_IMAPS".to_vec(), Value::Long(8192));
    vm.constants.insert(b"CURLPROTO_POP3".to_vec(), Value::Long(16384));
    vm.constants.insert(b"CURLPROTO_POP3S".to_vec(), Value::Long(32768));
    vm.constants.insert(b"CURLPROTO_SMTP".to_vec(), Value::Long(65536));
    vm.constants.insert(b"CURLPROTO_SMTPS".to_vec(), Value::Long(131072));
    vm.constants.insert(b"CURLPROTO_RTSP".to_vec(), Value::Long(262144));
    vm.constants.insert(b"CURLPROTO_RTMP".to_vec(), Value::Long(524288));
    vm.constants.insert(b"CURLPROTO_RTMPT".to_vec(), Value::Long(1048576));
    vm.constants.insert(b"CURLPROTO_RTMPE".to_vec(), Value::Long(2097152));
    vm.constants.insert(b"CURLPROTO_RTMPTE".to_vec(), Value::Long(4194304));
    vm.constants.insert(b"CURLPROTO_RTMPS".to_vec(), Value::Long(8388608));
    vm.constants.insert(b"CURLPROTO_RTMPTS".to_vec(), Value::Long(16777216));
    vm.constants.insert(b"CURLPROTO_GOPHER".to_vec(), Value::Long(33554432));
    vm.constants.insert(b"CURLPROTO_SMB".to_vec(), Value::Long(67108864));
    vm.constants.insert(b"CURLPROTO_SMBS".to_vec(), Value::Long(134217728));
    vm.constants.insert(b"CURLPROTO_MQTT".to_vec(), Value::Long(268435456));
    vm.constants.insert(b"CURLPROTO_ALL".to_vec(), Value::Long(-1));
}

/// curl_init(?string $url = null): CurlHandle|false
fn curl_init(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let mut handle = CurlHandle::new();

    if let Some(url_val) = args.first() {
        let url_val = url_val.deref();
        if !matches!(url_val, Value::Null | Value::Undef) {
            let url_str = url_val.to_php_string().to_string_lossy();
            handle.url = url_str.clone();
            handle.effective_url = url_str;
        }
    }

    Ok(make_curl_handle_obj(vm, handle))
}

/// Determine if option is a known CURLOPT for validation purposes
fn is_known_curlopt(option: i64) -> bool {
    // Accept anything we recognize from the constant registrations.
    // For now accept any value in 1..200 OR 10000..11000 OR 19000..20100 OR 20000..21000 etc.
    // PHP: just check it's a valid known CURLOPT_*
    matches!(option,
        // Integer options 1..300
        1..=300
        // String options 10000+
        | 10000..=10999
        // Boolean options 0..3 ? - covered by 1..300
        // Function options 20000+
        | 20000..=20999
        // Off_t options 30000+
        | 30000..=31000
        // RETURNTRANSFER
        | 19913
        // BLOB options
        | 40000..=41000
    )
}

/// Apply a single curl option to a handle.
/// Returns (success, exception_msg) where exception_msg signals a ValueError.
fn apply_setopt(vm: &mut Vm, handle: &mut CurlHandle, option: i64, value: &Value) -> Result<bool, VmError> {
    // Special validation per option
    if option == CURLOPT_URL {
        let url_str = value.to_php_string();
        // Check for null bytes
        if url_str.as_bytes().contains(&0) {
            return Err(throw_value_error(vm, "curl_setopt(): cURL option must not contain any null bytes"));
        }
        let s = url_str.to_string_lossy();
        handle.url = s.clone();
        handle.effective_url = s;
        return Ok(true);
    }

    if option == CURLOPT_SSL_VERIFYHOST {
        let n = value.to_long();
        if n == 1 {
            vm.emit_notice_at(
                "curl_setopt(): CURLOPT_SSL_VERIFYHOST no longer accepts the value 1, value 2 will be used instead",
                vm.current_line,
            );
            handle.ssl_verify_host = 2;
        } else {
            handle.ssl_verify_host = n;
        }
        return Ok(true);
    }

    if option == CURLOPT_PRIVATE {
        handle.private = value.clone();
        return Ok(true);
    }

    match option {
        CURLOPT_RETURNTRANSFER => handle.return_transfer = value.to_long() != 0,
        CURLOPT_POST => {
            if value.to_long() != 0 {
                handle.method = "POST".to_string();
            }
        }
        CURLOPT_POSTFIELDS => match value {
            Value::String(s) => {
                handle.post_fields = Some(s.as_bytes().to_vec());
                handle.method = "POST".to_string();
            }
            Value::Array(arr) => {
                let arr = arr.borrow();
                let mut parts = Vec::new();
                for (key, val) in arr.iter() {
                    let k = match key {
                        ArrayKey::Int(n) => n.to_string(),
                        ArrayKey::String(s) => s.to_string_lossy(),
                    };
                    let v = val.to_php_string().to_string_lossy();
                    parts.push(format!("{}={}", url_encode(&k), url_encode(&v)));
                }
                handle.post_fields = Some(parts.join("&").into_bytes());
                handle.method = "POST".to_string();
            }
            _ => {
                let s = value.to_php_string();
                handle.post_fields = Some(s.as_bytes().to_vec());
                handle.method = "POST".to_string();
            }
        },
        CURLOPT_HTTPHEADER => {
            if let Value::Array(arr) = value {
                handle.headers.clear();
                let arr = arr.borrow();
                for (_key, val) in arr.iter() {
                    let header_str = val.to_php_string().to_string_lossy();
                    if let Some(pos) = header_str.find(':') {
                        let name = header_str[..pos].trim().to_string();
                        let hval = header_str[pos + 1..].trim().to_string();
                        handle.headers.push((name, hval));
                    }
                }
            } else {
                return Err(throw_type_error(vm, "curl_setopt(): The CURLOPT_HTTPHEADER option must have an array value"));
            }
        }
        CURLOPT_FOLLOWLOCATION => handle.follow_location = value.to_long() != 0,
        CURLOPT_MAXREDIRS => handle.max_redirects = value.to_long(),
        CURLOPT_TIMEOUT => handle.timeout = value.to_long() as u64,
        CURLOPT_CONNECTTIMEOUT => handle.connect_timeout = value.to_long() as u64,
        CURLOPT_USERAGENT => handle.user_agent = value.to_php_string().to_string_lossy(),
        CURLOPT_HEADER => handle.include_header = value.to_long() != 0,
        CURLOPT_NOBODY => {
            handle.nobody = value.to_long() != 0;
            if handle.nobody {
                handle.method = "HEAD".to_string();
            }
        }
        CURLOPT_SSL_VERIFYPEER => handle.ssl_verify_peer = value.to_long() != 0,
        CURLOPT_CUSTOMREQUEST => {
            let method = value.to_php_string().to_string_lossy();
            handle.custom_request = Some(method);
        }
        CURLOPT_ENCODING => {
            let enc = value.to_php_string().to_string_lossy();
            handle.encoding = Some(enc);
        }
        CURLOPT_HTTPGET => {
            if value.to_long() != 0 {
                handle.method = "GET".to_string();
                handle.post_fields = None;
            }
        }
        CURLOPT_COOKIE => handle.cookie = Some(value.to_php_string().to_string_lossy()),
        CURLOPT_USERPWD => handle.userpwd = Some(value.to_php_string().to_string_lossy()),
        CURLOPT_HTTPAUTH => handle.http_auth = value.to_long(),
        CURLOPT_FAILONERROR => handle.fail_on_error = value.to_long() != 0,
        CURLOPT_PUT => {
            if value.to_long() != 0 {
                handle.method = "PUT".to_string();
            }
        }
        _ => {
            // For unknown options check validity
            if !is_known_curlopt(option) {
                return Err(throw_value_error(
                    vm,
                    "curl_setopt(): Argument #2 ($option) is not a valid cURL option",
                ));
            }
            // Known but not specifically handled - silently accept
        }
    }
    Ok(true)
}

/// curl_setopt(CurlHandle $handle, int $option, mixed $value): bool
fn curl_setopt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };

    // Check option is an int
    let opt_arg = args.get(1).unwrap_or(&Value::Null);
    let opt_arg_deref = opt_arg.deref();
    if let Value::String(_) = opt_arg_deref {
        return Err(throw_type_error(vm, "curl_setopt(): Argument #2 ($option) must be of type int, string given"));
    }
    let option = opt_arg_deref.to_long();
    let value = args.get(2).cloned().unwrap_or(Value::Null);

    // First validate option is known
    if !is_known_curlopt(option) {
        return Err(throw_value_error(vm, "curl_setopt(): Argument #2 ($option) is not a valid cURL option"));
    }

    // Take handle out, apply, put back
    let mut handle = with_curl_handles(|h| {
        let mut hs = h.borrow_mut();
        hs.remove(&handle_id)
    });

    let result = if let Some(ref mut h) = handle {
        apply_setopt(vm, h, option, &value)?
    } else {
        false
    };

    if let Some(h) = handle {
        with_curl_handles(|hs| {
            hs.borrow_mut().insert(handle_id, h);
        });
    }

    Ok(if result { Value::True } else { Value::False })
}

/// curl_setopt_array(CurlHandle $handle, array $options): bool
fn curl_setopt_array(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };
    let options = args.get(1).cloned().unwrap_or(Value::Null);

    if let Value::Array(arr) = options {
        let opts: Vec<(i64, Value)> = arr
            .borrow()
            .iter()
            .map(|(key, val)| {
                let opt = match key {
                    ArrayKey::Int(n) => *n,
                    ArrayKey::String(s) => s.to_string_lossy().parse::<i64>().unwrap_or(0),
                };
                (opt, val.clone())
            })
            .collect();

        let mut handle = with_curl_handles(|h| h.borrow_mut().remove(&handle_id));
        let mut all_ok = true;
        if let Some(ref mut h) = handle {
            for (option, value) in &opts {
                match apply_setopt(vm, h, *option, value) {
                    Ok(true) => {}
                    Ok(false) => {
                        all_ok = false;
                        break;
                    }
                    Err(e) => {
                        // Put handle back before propagating
                        if let Some(h) = handle.take() {
                            with_curl_handles(|hs| {
                                hs.borrow_mut().insert(handle_id, h);
                            });
                        }
                        return Err(e);
                    }
                }
            }
        } else {
            all_ok = false;
        }

        if let Some(h) = handle {
            with_curl_handles(|hs| {
                hs.borrow_mut().insert(handle_id, h);
            });
        }

        Ok(if all_ok { Value::True } else { Value::False })
    } else {
        Ok(Value::False)
    }
}

/// curl_exec(CurlHandle $handle): string|bool
fn curl_exec(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };

    let request_data = with_curl_handles(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&handle_id) {
            Some((
                handle.url.clone(),
                handle.custom_request.clone().unwrap_or_else(|| handle.method.clone()),
                handle.headers.clone(),
                handle.post_fields.clone(),
                handle.timeout,
                handle.connect_timeout,
                handle.user_agent.clone(),
                handle.follow_location,
                handle.max_redirects,
                handle.encoding.clone(),
                handle.cookie.clone(),
                handle.userpwd.clone(),
                handle.return_transfer,
                handle.include_header,
                handle.nobody,
                handle.fail_on_error,
            ))
        } else {
            None
        }
    });

    let (
        url,
        method,
        headers,
        post_fields,
        timeout,
        _connect_timeout,
        user_agent,
        follow_location,
        max_redirects,
        encoding,
        cookie,
        userpwd,
        return_transfer,
        include_header,
        nobody,
        fail_on_error,
    ) = match request_data {
        Some(data) => data,
        None => return Ok(Value::False),
    };

    if url.is_empty() {
        with_curl_handles(|handles| {
            let mut handles = handles.borrow_mut();
            if let Some(handle) = handles.get_mut(&handle_id) {
                handle.error_message = "No URL set!".to_string();
                handle.error_number = 3;
            }
        });
        return Ok(Value::False);
    }

    let start_time = Instant::now();

    let mut config_builder = ureq::Agent::config_builder();
    if timeout > 0 {
        config_builder = config_builder.timeout_global(Some(Duration::from_secs(timeout)));
    }
    if follow_location {
        let max_redir = if max_redirects < 0 { 30 } else { max_redirects as u32 };
        config_builder = config_builder.max_redirects(max_redir);
    } else {
        config_builder = config_builder.max_redirects(0);
    }
    config_builder = config_builder.http_status_as_error(false);

    let agent: ureq::Agent = config_builder.build().into();
    let effective_method = if nobody { "HEAD" } else { &method };

    let result = execute_request(
        &agent,
        effective_method,
        &url,
        &headers,
        &post_fields,
        &user_agent,
        &encoding,
        &cookie,
        &userpwd,
    );

    let elapsed = start_time.elapsed().as_secs_f64();

    match result {
        Ok((status, resp_headers, body, content_type, effective_url)) => {
            let mut header_text = Vec::new();
            if include_header {
                header_text.extend_from_slice(format!("HTTP/1.1 {} OK\r\n", status).as_bytes());
                for (name, value) in &resp_headers {
                    header_text.extend_from_slice(format!("{}: {}\r\n", name, value).as_bytes());
                }
                header_text.extend_from_slice(b"\r\n");
            }
            let header_size = header_text.len();

            with_curl_handles(|handles| {
                let mut handles = handles.borrow_mut();
                if let Some(handle) = handles.get_mut(&handle_id) {
                    handle.response_code = status as i64;
                    handle.response_headers = resp_headers;
                    handle.response_body = body.clone();
                    handle.header_size = header_size;
                    handle.content_type = content_type;
                    handle.effective_url = effective_url;
                    handle.total_time = elapsed;
                    handle.error_message.clear();
                    handle.error_number = 0;
                }
            });

            if fail_on_error && status >= 400 {
                with_curl_handles(|handles| {
                    let mut handles = handles.borrow_mut();
                    if let Some(handle) = handles.get_mut(&handle_id) {
                        handle.error_message = format!("The requested URL returned error: {}", status);
                        handle.error_number = 22;
                    }
                });
                return Ok(Value::False);
            }

            if return_transfer {
                let mut result_body = Vec::new();
                if include_header {
                    result_body.extend_from_slice(&header_text);
                }
                result_body.extend_from_slice(&body);
                Ok(Value::String(PhpString::from_vec(result_body)))
            } else {
                if include_header {
                    vm.write_output(&header_text);
                }
                vm.write_output(&body);
                Ok(Value::True)
            }
        }
        Err((error_number, error_message)) => {
            with_curl_handles(|handles| {
                let mut handles = handles.borrow_mut();
                if let Some(handle) = handles.get_mut(&handle_id) {
                    handle.error_message = error_message;
                    handle.error_number = error_number;
                    handle.total_time = elapsed;
                }
            });
            Ok(Value::False)
        }
    }
}

fn execute_request(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    post_fields: &Option<Vec<u8>>,
    user_agent: &str,
    encoding: &Option<String>,
    cookie: &Option<String>,
    userpwd: &Option<String>,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>, String, String), (i64, String)> {
    let has_body = matches!(method, "POST" | "PUT" | "PATCH") || post_fields.is_some();

    let response = if has_body {
        let mut request = match method {
            "POST" => agent.post(url),
            "PUT" => agent.put(url),
            "PATCH" => agent.patch(url),
            _ => agent.post(url),
        };

        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if !user_agent.is_empty() {
            request = request.header("User-Agent", user_agent);
        }
        if let Some(enc) = encoding {
            request = request.header("Accept-Encoding", enc.as_str());
        }
        if let Some(cookie_val) = cookie {
            request = request.header("Cookie", cookie_val.as_str());
        }
        if let Some(userpwd_val) = userpwd {
            if let Some(pos) = userpwd_val.find(':') {
                let user = &userpwd_val[..pos];
                let pass = &userpwd_val[pos + 1..];
                let credentials = base64_encode(&format!("{}:{}", user, pass));
                request = request.header("Authorization", &format!("Basic {}", credentials));
            }
        }

        if let Some(body) = post_fields {
            let has_content_type = headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-type"));
            if !has_content_type {
                request = request.header("Content-Type", "application/x-www-form-urlencoded");
            }
            request.send(body.as_slice())
        } else {
            request.send_empty()
        }
    } else {
        let mut request = match method {
            "GET" => agent.get(url),
            "HEAD" => agent.head(url),
            "DELETE" => agent.delete(url),
            "OPTIONS" => agent.options(url),
            _ => agent.get(url),
        };

        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if !user_agent.is_empty() {
            request = request.header("User-Agent", user_agent);
        }
        if let Some(enc) = encoding {
            request = request.header("Accept-Encoding", enc.as_str());
        }
        if let Some(cookie_val) = cookie {
            request = request.header("Cookie", cookie_val.as_str());
        }
        if let Some(userpwd_val) = userpwd {
            if let Some(pos) = userpwd_val.find(':') {
                let user = &userpwd_val[..pos];
                let pass = &userpwd_val[pos + 1..];
                let credentials = base64_encode(&format!("{}:{}", user, pass));
                request = request.header("Authorization", &format!("Basic {}", credentials));
            }
        }

        request.call()
    };

    match response {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            let mut resp_headers = Vec::new();
            for (name, value) in resp.headers().iter() {
                if let Ok(v) = value.to_str() {
                    resp_headers.push((name.as_str().to_string(), v.to_string()));
                }
            }
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let effective_url = url.to_string();
            let body = resp.body_mut().read_to_vec().unwrap_or_default();
            Ok((status, resp_headers, body, content_type, effective_url))
        }
        Err(e) => {
            let (error_number, error_message) = classify_ureq_error(&e);
            Err((error_number, error_message))
        }
    }
}

fn classify_ureq_error(e: &ureq::Error) -> (i64, String) {
    let msg = e.to_string();
    // Heuristic: classify based on error message keywords
    let lower = msg.to_lowercase();
    if lower.contains("name or service not known")
        || lower.contains("nodename nor servname")
        || lower.contains("lookup address")
        || lower.contains("no such host")
    {
        return (6, format!("Could not resolve host: {}", msg));
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return (28, msg);
    }
    if lower.contains("connection refused") {
        return (7, msg);
    }
    if lower.contains("unsupported") || lower.contains("scheme") || lower.contains("protocol") {
        return (1, format!("Unsupported protocol: {}", msg));
    }
    match e {
        ureq::Error::Timeout(_) => (28, msg),
        ureq::Error::HostNotFound => (6, msg),
        ureq::Error::ConnectionFailed => (7, msg),
        ureq::Error::Tls(_) => (35, msg),
        ureq::Error::Rustls(_) => (35, msg),
        ureq::Error::BadUri(_) => (3, msg),
        ureq::Error::TooManyRedirects => (47, msg),
        _ => (7, msg),
    }
}

/// curl_close(CurlHandle $handle): void
/// PHP 8.5: deprecated since 8.5, no-op since 8.0
fn curl_close(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated("Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0");
    // No-op (handle stays alive); just consume the argument
    let _ = arg_curl_id(args.first());
    Ok(Value::Null)
}

/// curl_error(CurlHandle $handle): string
fn curl_error(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::String(PhpString::empty())),
    };
    let msg = with_curl_handles(|handles| {
        let handles = handles.borrow();
        handles.get(&handle_id).map(|h| h.error_message.clone()).unwrap_or_default()
    });
    Ok(Value::String(PhpString::from_string(msg)))
}

/// curl_errno(CurlHandle $handle): int
fn curl_errno(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Long(0)),
    };
    let errno = with_curl_handles(|handles| {
        let handles = handles.borrow();
        handles.get(&handle_id).map(|h| h.error_number).unwrap_or(0)
    });
    Ok(Value::Long(errno))
}

/// curl_getinfo(CurlHandle $handle, ?int $option = null): mixed
fn curl_getinfo(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };
    let option = args.get(1);

    with_curl_handles(|handles| {
        let handles = handles.borrow();
        if let Some(handle) = handles.get(&handle_id) {
            if let Some(opt) = option {
                let opt = opt.deref();
                if matches!(opt, Value::Null | Value::Undef) {
                    return Ok(build_info_array(handle));
                }
                let opt_val = opt.to_long();
                match opt_val {
                    CURLINFO_HTTP_CODE => Ok(Value::Long(handle.response_code)),
                    CURLINFO_EFFECTIVE_URL => {
                        Ok(Value::String(PhpString::from_string(handle.effective_url.clone())))
                    }
                    CURLINFO_CONTENT_TYPE => {
                        if handle.content_type.is_empty() {
                            Ok(Value::Null)
                        } else {
                            Ok(Value::String(PhpString::from_string(handle.content_type.clone())))
                        }
                    }
                    CURLINFO_TOTAL_TIME => Ok(Value::Double(handle.total_time)),
                    CURLINFO_REDIRECT_COUNT => Ok(Value::Long(handle.redirect_count)),
                    CURLINFO_HEADER_SIZE => Ok(Value::Long(handle.header_size as i64)),
                    CURLINFO_PRIVATE => Ok(handle.private.clone()),
                    4194332 => {
                        // CURLINFO_COOKIELIST - return empty array
                        Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new()))))
                    }
                    _ => Ok(Value::False),
                }
            } else {
                Ok(build_info_array(handle))
            }
        } else {
            Ok(Value::False)
        }
    })
}

fn build_info_array(handle: &CurlHandle) -> Value {
    let mut arr = PhpArray::new();

    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"url")),
        Value::String(PhpString::from_string(handle.effective_url.clone())),
    );
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"content_type")),
        if handle.content_type.is_empty() {
            Value::Null
        } else {
            Value::String(PhpString::from_string(handle.content_type.clone()))
        },
    );
    arr.set(ArrayKey::String(PhpString::from_bytes(b"http_code")), Value::Long(handle.response_code));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"header_size")), Value::Long(handle.header_size as i64));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"request_size")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"filetime")), Value::Long(-1));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"ssl_verify_result")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"redirect_count")), Value::Long(handle.redirect_count));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"total_time")), Value::Double(handle.total_time));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"namelookup_time")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"connect_time")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"pretransfer_time")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"size_upload")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"size_download")), Value::Double(handle.response_body.len() as f64));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"speed_download")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"speed_upload")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"download_content_length")), Value::Double(-1.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"upload_content_length")), Value::Double(-1.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"starttransfer_time")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"redirect_time")), Value::Double(0.0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"redirect_url")), Value::String(PhpString::empty()));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"primary_ip")), Value::String(PhpString::empty()));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"certinfo")), Value::Array(Rc::new(RefCell::new(PhpArray::new()))));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"primary_port")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"local_ip")), Value::String(PhpString::empty()));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"local_port")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"http_version")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"protocol")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"ssl_verifyresult")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"scheme")), Value::String(PhpString::empty()));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"appconnect_time_us")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"connect_time_us")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"namelookup_time_us")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"pretransfer_time_us")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"redirect_time_us")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"starttransfer_time_us")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"total_time_us")), Value::Long((handle.total_time * 1e6) as i64));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"effective_method")), Value::String(PhpString::from_string(handle.method.clone())));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"capath")), Value::String(PhpString::empty()));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"cainfo")), Value::String(PhpString::empty()));

    Value::Array(Rc::new(RefCell::new(arr)))
}

/// curl_reset(CurlHandle $handle): void
fn curl_reset(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    with_curl_handles(|handles| {
        let mut handles = handles.borrow_mut();
        if let Some(handle) = handles.get_mut(&handle_id) {
            handle.reset();
        }
    });
    Ok(Value::Null)
}

/// curl_version(int $age = 0): array|false
fn curl_version(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    let mut arr = PhpArray::new();

    arr.set(ArrayKey::String(PhpString::from_bytes(b"version_number")), Value::Long(0x075500));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"age")), Value::Long(9));
    // Features bitmask: SSL | IPV6 | LIBZ | LARGEFILE | NTLM | HTTP2 | UNIX_SOCKETS | HTTPS_PROXY
    arr.set(ArrayKey::String(PhpString::from_bytes(b"features")), Value::Long(4 | 1 | 8 | 512 | 16 | 65536 | 524288 | 2097152));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"ssl_version_number")), Value::Long(0));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"version")), Value::String(PhpString::from_bytes(b"7.85.0")));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"host")), Value::String(PhpString::from_bytes(b"rust")));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"ssl_version")), Value::String(PhpString::from_bytes(b"rustls/0.23")));
    arr.set(ArrayKey::String(PhpString::from_bytes(b"libz_version")), Value::String(PhpString::from_bytes(b"1.2.13")));
    let protos = {
        let mut protos = PhpArray::new();
        protos.push(Value::String(PhpString::from_bytes(b"http")));
        protos.push(Value::String(PhpString::from_bytes(b"https")));
        Value::Array(Rc::new(RefCell::new(protos)))
    };
    arr.set(ArrayKey::String(PhpString::from_bytes(b"protocols")), protos);

    // Feature list - all bool false
    let mut feature_list = PhpArray::new();
    for &k in &[
        "AsynchDNS", "CharConv", "Debug", "GSS-Negotiate", "IDN", "IPv6",
        "krb4", "Largefile", "libz", "NTLM", "NTLMWB", "SPNEGO",
        "SSL", "SSPI", "TLS-SRP", "HTTP2", "GSSAPI", "KERBEROS5",
        "UNIX_SOCKETS", "PSL", "HTTPS_PROXY", "MULTI_SSL", "BROTLI",
    ] {
        feature_list.set(
            ArrayKey::String(PhpString::from_bytes(k.as_bytes())),
            Value::False,
        );
    }
    // Set a few to true
    feature_list.set(ArrayKey::String(PhpString::from_bytes(b"SSL")), Value::True);
    feature_list.set(ArrayKey::String(PhpString::from_bytes(b"IPv6")), Value::True);
    feature_list.set(ArrayKey::String(PhpString::from_bytes(b"libz")), Value::True);
    arr.set(
        ArrayKey::String(PhpString::from_bytes(b"feature_list")),
        Value::Array(Rc::new(RefCell::new(feature_list))),
    );

    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push('+'),
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

// === curl_multi_* ===

fn curl_multi_init(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(make_multi_handle_obj(vm))
}

fn curl_multi_add_handle(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let multi_id = match arg_multi_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Long(1)),
    };
    let handle_id = match arg_curl_id(args.get(1)) {
        Some(id) => id,
        None => return Ok(Value::Long(2)),
    };
    MULTI_HANDLES.with(|handles| {
        if let Some(list) = handles.borrow_mut().get_mut(&multi_id) {
            list.push(handle_id);
        }
    });
    Ok(Value::Long(0))
}

fn curl_multi_remove_handle(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let multi_id = match arg_multi_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Long(1)),
    };
    let handle_id = match arg_curl_id(args.get(1)) {
        Some(id) => id,
        None => return Ok(Value::Long(2)),
    };
    MULTI_HANDLES.with(|handles| {
        if let Some(list) = handles.borrow_mut().get_mut(&multi_id) {
            list.retain(|&id| id != handle_id);
        }
    });
    Ok(Value::Long(0))
}

fn curl_multi_exec(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::Long(0);
    }
    Ok(Value::Long(0))
}

fn curl_multi_getcontent(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Null),
    };
    let result = with_curl_handles(|handles| {
        if let Some(handle) = handles.borrow().get(&handle_id) {
            if handle.response_body.is_empty() {
                Value::Null
            } else {
                Value::String(PhpString::from_vec(handle.response_body.clone()))
            }
        } else {
            Value::Null
        }
    });
    Ok(result)
}

/// curl_multi_close: no-op (PHP 8.0+ doesn't actually close)
fn curl_multi_close(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _ = arg_multi_id(args.first());
    Ok(Value::Null)
}

fn curl_multi_errno(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let multi_id = match arg_multi_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Long(0)),
    };
    let e = MULTI_ERRNOS.with(|e| e.borrow().get(&multi_id).copied().unwrap_or(0));
    Ok(Value::Long(e))
}

fn curl_multi_strerror(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(|v| v.to_long()).unwrap_or(0);
    let msg = match code {
        0 => "No error",
        1 => "Invalid multi handle",
        2 => "Invalid easy handle",
        3 => "Out of memory",
        4 => "Internal error",
        5 => "Invalid socket argument",
        6 => "Unknown option",
        7 => "Called multi_perform without ever having added an easy handle",
        _ => return Ok(Value::Null),
    };
    Ok(Value::String(PhpString::from_string(msg.to_string())))
}

fn curl_multi_setopt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let multi_id = match arg_multi_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };
    let option = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if !(1..=30).contains(&option) && !(20000..=20030).contains(&option) {
        MULTI_ERRNOS.with(|e| {
            e.borrow_mut().insert(multi_id, 6); // CURLM_UNKNOWN_OPTION
        });
        return Err(throw_value_error(
            vm,
            "curl_multi_setopt(): Argument #2 ($option) is not a valid cURL multi option",
        ));
    }
    Ok(Value::True)
}

fn curl_multi_info_read(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Reference(r)) = args.get(1) {
        *r.borrow_mut() = Value::Long(0);
    }
    Ok(Value::False)
}

fn curl_multi_select(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn curl_multi_get_handles(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let multi_id = match arg_multi_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Array(Rc::new(RefCell::new(PhpArray::new())))),
    };
    let ids = MULTI_HANDLES.with(|h| {
        h.borrow().get(&multi_id).cloned().unwrap_or_default()
    });
    let mut arr = PhpArray::new();
    for cid in ids {
        // Create a CurlHandle object referring to existing handle
        let obj = PhpObject::new(b"CurlHandle".to_vec(), cid as u64);
        arr.push(Value::Object(Rc::new(RefCell::new(obj))));
    }
    Ok(Value::Array(Rc::new(RefCell::new(arr))))
}

fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// === Additional functions ===

fn curl_copy_handle(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let handle_id = match arg_curl_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };
    let new_handle = with_curl_handles(|handles| {
        let handles = handles.borrow();
        handles.get(&handle_id).cloned()
    });
    match new_handle {
        Some(new_h) => Ok(make_curl_handle_obj(vm, new_h)),
        None => Ok(Value::False),
    }
}

fn curl_share_init(vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(make_share_handle_obj(vm))
}

fn curl_share_init_persistent(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::Array(arr)) = args.first() {
        let arr_ref = arr.borrow();
        if arr_ref.len() == 0 {
            return Err(throw_value_error(
                vm,
                "curl_share_init_persistent(): Argument #1 ($share_options) must not be empty",
            ));
        }
        for (_, val) in arr_ref.iter() {
            let n = val.to_long();
            if !matches!(n, 2 | 3 | 4 | 5 | 6) {
                return Err(throw_value_error(
                    vm,
                    "curl_share_init_persistent(): Argument #1 ($share_options) must contain only CURL_LOCK_DATA_* constants",
                ));
            }
            if n == 2 {
                return Err(throw_value_error(
                    vm,
                    "curl_share_init_persistent(): Argument #1 ($share_options) must not contain CURL_LOCK_DATA_COOKIE because sharing cookies across PHP requests is unsafe",
                ));
            }
        }
    }
    Ok(make_share_handle_obj(vm))
}

fn curl_share_setopt(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let share_id = match arg_share_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::False),
    };
    let option = args.get(1).map(|v| v.to_long()).unwrap_or(0);
    if !(1..=4).contains(&option) {
        // Mark share errno = 1 (CURLSHE_BAD_OPTION / Unknown share option)
        SHARE_ERRNOS.with(|e| {
            e.borrow_mut().insert(share_id, 1);
        });
        return Err(throw_value_error(
            vm,
            "curl_share_setopt(): Argument #2 ($option) is not a valid cURL share option",
        ));
    }
    Ok(Value::True)
}

/// curl_share_close: deprecated since 8.5, no-op since 8.0
fn curl_share_close(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    vm.emit_deprecated("Function curl_share_close() is deprecated since 8.5, as it has no effect since PHP 8.0");
    let _ = arg_share_id(args.first());
    Ok(Value::Null)
}

fn curl_share_strerror(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(|v| v.to_long()).unwrap_or(0);
    let msg = match code {
        0 => "No error",
        1 => "Unknown share option",
        2 => "Share currently in use",
        3 => "Invalid share handle",
        4 => "Out of memory",
        5 => "Feature not available in this library",
        _ => return Ok(Value::Null),
    };
    Ok(Value::String(PhpString::from_string(msg.to_string())))
}

fn curl_share_errno(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let share_id = match arg_share_id(args.first()) {
        Some(id) => id,
        None => return Ok(Value::Long(0)),
    };
    let e = SHARE_ERRNOS.with(|e| e.borrow().get(&share_id).copied().unwrap_or(0));
    Ok(Value::Long(e))
}

fn curl_strerror(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let code = args.first().map(|v| v.to_long()).unwrap_or(0);
    let msg = match code {
        0 => "No error",
        1 => "Unsupported protocol",
        3 => "URL using bad/illegal format or missing URL",
        6 => "Couldn't resolve host name",
        7 => "Couldn't connect to server",
        28 => "Connection timed out",
        35 => "SSL connect error",
        42 => "Operation was aborted by an application callback",
        47 => "Number of redirects hit maximum amount",
        52 => "Server returned nothing (no headers, no data)",
        55 => "Failed sending data to the peer",
        56 => "Failure when receiving data from the peer",
        _ => "Unknown error",
    };
    Ok(Value::String(PhpString::from_string(msg.to_string())))
}

fn curl_pause(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn curl_upkeep(_vm: &mut Vm, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::True)
}

fn curl_file_create(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let filename = args.first().map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::empty());
    let mime_type = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::from_bytes(b"application/octet-stream"));
    let posted_filename = args.get(2).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::empty());

    let id = vm.next_object_id();
    let mut obj = PhpObject::new(b"CURLFile".to_vec(), id);
    obj.set_property(b"name".to_vec(), Value::String(filename));
    obj.set_property(b"mime".to_vec(), Value::String(mime_type));
    obj.set_property(b"postname".to_vec(), Value::String(posted_filename));
    Ok(Value::Object(Rc::new(RefCell::new(obj))))
}

/// curl_escape(CurlHandle $handle, string $string): string|false
fn curl_escape(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _handle_id = arg_curl_id(args.first());
    let s = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::empty());
    let mut result = String::new();
    for byte in s.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(*byte as char);
            }
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    Ok(Value::String(PhpString::from_string(result)))
}

/// curl_unescape(CurlHandle $handle, string $string): string|false
fn curl_unescape(_vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let _handle_id = arg_curl_id(args.first());
    let s = args.get(1).map(|v| v.to_php_string()).unwrap_or_else(|| PhpString::empty());
    let bytes = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Decode %XX
            let hex = &bytes[i + 1..i + 3];
            let h1 = (hex[0] as char).to_digit(16);
            let h2 = (hex[1] as char).to_digit(16);
            if let (Some(a), Some(b)) = (h1, h2) {
                let byte = (a * 16 + b) as u8;
                // libcurl skips NUL bytes
                if byte != 0 {
                    result.push(byte);
                }
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    Ok(Value::String(PhpString::from_vec(result)))
}
