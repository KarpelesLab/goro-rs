//! The ext/curl constants for the module table (values from
//! goro_ext_curl::consts, observed on PHP 8.5).

use crate::registry::ConstVal;
use goro_ext_curl::consts as c;

pub static CONSTANTS: &[(&str, ConstVal)] = &[
    ("CURLOPT_AUTOREFERER", ConstVal::Int(c::CURLOPT_AUTOREFERER)),
    (
        "CURLOPT_BINARYTRANSFER",
        ConstVal::Int(c::CURLOPT_BINARYTRANSFER),
    ),
    ("CURLOPT_BUFFERSIZE", ConstVal::Int(c::CURLOPT_BUFFERSIZE)),
    ("CURLOPT_CAINFO", ConstVal::Int(c::CURLOPT_CAINFO)),
    ("CURLOPT_CAPATH", ConstVal::Int(c::CURLOPT_CAPATH)),
    (
        "CURLOPT_CONNECTTIMEOUT",
        ConstVal::Int(c::CURLOPT_CONNECTTIMEOUT),
    ),
    ("CURLOPT_COOKIE", ConstVal::Int(c::CURLOPT_COOKIE)),
    ("CURLOPT_COOKIEFILE", ConstVal::Int(c::CURLOPT_COOKIEFILE)),
    ("CURLOPT_COOKIEJAR", ConstVal::Int(c::CURLOPT_COOKIEJAR)),
    (
        "CURLOPT_COOKIESESSION",
        ConstVal::Int(c::CURLOPT_COOKIESESSION),
    ),
    ("CURLOPT_CRLF", ConstVal::Int(c::CURLOPT_CRLF)),
    (
        "CURLOPT_CUSTOMREQUEST",
        ConstVal::Int(c::CURLOPT_CUSTOMREQUEST),
    ),
    (
        "CURLOPT_DNS_CACHE_TIMEOUT",
        ConstVal::Int(c::CURLOPT_DNS_CACHE_TIMEOUT),
    ),
    (
        "CURLOPT_DNS_USE_GLOBAL_CACHE",
        ConstVal::Int(c::CURLOPT_DNS_USE_GLOBAL_CACHE),
    ),
    ("CURLOPT_EGDSOCKET", ConstVal::Int(c::CURLOPT_EGDSOCKET)),
    ("CURLOPT_ENCODING", ConstVal::Int(c::CURLOPT_ENCODING)),
    ("CURLOPT_FAILONERROR", ConstVal::Int(c::CURLOPT_FAILONERROR)),
    ("CURLOPT_FILE", ConstVal::Int(c::CURLOPT_FILE)),
    ("CURLOPT_FILETIME", ConstVal::Int(c::CURLOPT_FILETIME)),
    (
        "CURLOPT_FOLLOWLOCATION",
        ConstVal::Int(c::CURLOPT_FOLLOWLOCATION),
    ),
    (
        "CURLOPT_FORBID_REUSE",
        ConstVal::Int(c::CURLOPT_FORBID_REUSE),
    ),
    (
        "CURLOPT_FRESH_CONNECT",
        ConstVal::Int(c::CURLOPT_FRESH_CONNECT),
    ),
    ("CURLOPT_FTPAPPEND", ConstVal::Int(c::CURLOPT_FTPAPPEND)),
    ("CURLOPT_FTPLISTONLY", ConstVal::Int(c::CURLOPT_FTPLISTONLY)),
    ("CURLOPT_FTPPORT", ConstVal::Int(c::CURLOPT_FTPPORT)),
    (
        "CURLOPT_FTP_USE_EPRT",
        ConstVal::Int(c::CURLOPT_FTP_USE_EPRT),
    ),
    (
        "CURLOPT_FTP_USE_EPSV",
        ConstVal::Int(c::CURLOPT_FTP_USE_EPSV),
    ),
    ("CURLOPT_HEADER", ConstVal::Int(c::CURLOPT_HEADER)),
    (
        "CURLOPT_HEADERFUNCTION",
        ConstVal::Int(c::CURLOPT_HEADERFUNCTION),
    ),
    (
        "CURLOPT_HTTP200ALIASES",
        ConstVal::Int(c::CURLOPT_HTTP200ALIASES),
    ),
    ("CURLOPT_HTTPGET", ConstVal::Int(c::CURLOPT_HTTPGET)),
    ("CURLOPT_HTTPHEADER", ConstVal::Int(c::CURLOPT_HTTPHEADER)),
    (
        "CURLOPT_HTTPPROXYTUNNEL",
        ConstVal::Int(c::CURLOPT_HTTPPROXYTUNNEL),
    ),
    (
        "CURLOPT_HTTP_VERSION",
        ConstVal::Int(c::CURLOPT_HTTP_VERSION),
    ),
    ("CURLOPT_INFILE", ConstVal::Int(c::CURLOPT_INFILE)),
    ("CURLOPT_INFILESIZE", ConstVal::Int(c::CURLOPT_INFILESIZE)),
    (
        "CURLOPT_INFILESIZE_LARGE",
        ConstVal::Int(c::CURLOPT_INFILESIZE_LARGE),
    ),
    ("CURLOPT_INTERFACE", ConstVal::Int(c::CURLOPT_INTERFACE)),
    ("CURLOPT_KRB4LEVEL", ConstVal::Int(c::CURLOPT_KRB4LEVEL)),
    (
        "CURLOPT_LOW_SPEED_LIMIT",
        ConstVal::Int(c::CURLOPT_LOW_SPEED_LIMIT),
    ),
    (
        "CURLOPT_LOW_SPEED_TIME",
        ConstVal::Int(c::CURLOPT_LOW_SPEED_TIME),
    ),
    ("CURLOPT_MAXCONNECTS", ConstVal::Int(c::CURLOPT_MAXCONNECTS)),
    ("CURLOPT_MAXREDIRS", ConstVal::Int(c::CURLOPT_MAXREDIRS)),
    ("CURLOPT_NETRC", ConstVal::Int(c::CURLOPT_NETRC)),
    ("CURLOPT_NOBODY", ConstVal::Int(c::CURLOPT_NOBODY)),
    ("CURLOPT_NOPROGRESS", ConstVal::Int(c::CURLOPT_NOPROGRESS)),
    ("CURLOPT_NOSIGNAL", ConstVal::Int(c::CURLOPT_NOSIGNAL)),
    ("CURLOPT_PORT", ConstVal::Int(c::CURLOPT_PORT)),
    ("CURLOPT_POST", ConstVal::Int(c::CURLOPT_POST)),
    ("CURLOPT_POSTFIELDS", ConstVal::Int(c::CURLOPT_POSTFIELDS)),
    ("CURLOPT_POSTQUOTE", ConstVal::Int(c::CURLOPT_POSTQUOTE)),
    ("CURLOPT_PREQUOTE", ConstVal::Int(c::CURLOPT_PREQUOTE)),
    ("CURLOPT_PRIVATE", ConstVal::Int(c::CURLOPT_PRIVATE)),
    (
        "CURLOPT_PROGRESSFUNCTION",
        ConstVal::Int(c::CURLOPT_PROGRESSFUNCTION),
    ),
    ("CURLOPT_PROXY", ConstVal::Int(c::CURLOPT_PROXY)),
    ("CURLOPT_PROXYPORT", ConstVal::Int(c::CURLOPT_PROXYPORT)),
    ("CURLOPT_PROXYTYPE", ConstVal::Int(c::CURLOPT_PROXYTYPE)),
    (
        "CURLOPT_PROXYUSERPWD",
        ConstVal::Int(c::CURLOPT_PROXYUSERPWD),
    ),
    ("CURLOPT_PUT", ConstVal::Int(c::CURLOPT_PUT)),
    ("CURLOPT_QUOTE", ConstVal::Int(c::CURLOPT_QUOTE)),
    ("CURLOPT_RANDOM_FILE", ConstVal::Int(c::CURLOPT_RANDOM_FILE)),
    ("CURLOPT_RANGE", ConstVal::Int(c::CURLOPT_RANGE)),
    ("CURLOPT_READDATA", ConstVal::Int(c::CURLOPT_READDATA)),
    (
        "CURLOPT_READFUNCTION",
        ConstVal::Int(c::CURLOPT_READFUNCTION),
    ),
    ("CURLOPT_REFERER", ConstVal::Int(c::CURLOPT_REFERER)),
    ("CURLOPT_RESUME_FROM", ConstVal::Int(c::CURLOPT_RESUME_FROM)),
    (
        "CURLOPT_RETURNTRANSFER",
        ConstVal::Int(c::CURLOPT_RETURNTRANSFER),
    ),
    ("CURLOPT_SHARE", ConstVal::Int(c::CURLOPT_SHARE)),
    ("CURLOPT_SSLCERT", ConstVal::Int(c::CURLOPT_SSLCERT)),
    (
        "CURLOPT_SSLCERTPASSWD",
        ConstVal::Int(c::CURLOPT_SSLCERTPASSWD),
    ),
    ("CURLOPT_SSLCERTTYPE", ConstVal::Int(c::CURLOPT_SSLCERTTYPE)),
    ("CURLOPT_SSLENGINE", ConstVal::Int(c::CURLOPT_SSLENGINE)),
    (
        "CURLOPT_SSLENGINE_DEFAULT",
        ConstVal::Int(c::CURLOPT_SSLENGINE_DEFAULT),
    ),
    ("CURLOPT_SSLKEY", ConstVal::Int(c::CURLOPT_SSLKEY)),
    (
        "CURLOPT_SSLKEYPASSWD",
        ConstVal::Int(c::CURLOPT_SSLKEYPASSWD),
    ),
    ("CURLOPT_SSLKEYTYPE", ConstVal::Int(c::CURLOPT_SSLKEYTYPE)),
    ("CURLOPT_SSLVERSION", ConstVal::Int(c::CURLOPT_SSLVERSION)),
    (
        "CURLOPT_SSL_CIPHER_LIST",
        ConstVal::Int(c::CURLOPT_SSL_CIPHER_LIST),
    ),
    (
        "CURLOPT_SSL_VERIFYHOST",
        ConstVal::Int(c::CURLOPT_SSL_VERIFYHOST),
    ),
    (
        "CURLOPT_SSL_VERIFYPEER",
        ConstVal::Int(c::CURLOPT_SSL_VERIFYPEER),
    ),
    ("CURLOPT_STDERR", ConstVal::Int(c::CURLOPT_STDERR)),
    ("CURLOPT_TCP_KEEPCNT", ConstVal::Int(c::CURLOPT_TCP_KEEPCNT)),
    (
        "CURLOPT_TELNETOPTIONS",
        ConstVal::Int(c::CURLOPT_TELNETOPTIONS),
    ),
    (
        "CURLOPT_TIMECONDITION",
        ConstVal::Int(c::CURLOPT_TIMECONDITION),
    ),
    ("CURLOPT_TIMEOUT", ConstVal::Int(c::CURLOPT_TIMEOUT)),
    ("CURLOPT_TIMEVALUE", ConstVal::Int(c::CURLOPT_TIMEVALUE)),
    (
        "CURLOPT_TRANSFERTEXT",
        ConstVal::Int(c::CURLOPT_TRANSFERTEXT),
    ),
    (
        "CURLOPT_UNRESTRICTED_AUTH",
        ConstVal::Int(c::CURLOPT_UNRESTRICTED_AUTH),
    ),
    ("CURLOPT_UPLOAD", ConstVal::Int(c::CURLOPT_UPLOAD)),
    ("CURLOPT_URL", ConstVal::Int(c::CURLOPT_URL)),
    ("CURLOPT_USERAGENT", ConstVal::Int(c::CURLOPT_USERAGENT)),
    ("CURLOPT_USERPWD", ConstVal::Int(c::CURLOPT_USERPWD)),
    ("CURLOPT_VERBOSE", ConstVal::Int(c::CURLOPT_VERBOSE)),
    (
        "CURLOPT_WRITEFUNCTION",
        ConstVal::Int(c::CURLOPT_WRITEFUNCTION),
    ),
    ("CURLOPT_WRITEHEADER", ConstVal::Int(c::CURLOPT_WRITEHEADER)),
    (
        "CURLOPT_XFERINFOFUNCTION",
        ConstVal::Int(c::CURLOPT_XFERINFOFUNCTION),
    ),
    (
        "CURLOPT_DEBUGFUNCTION",
        ConstVal::Int(c::CURLOPT_DEBUGFUNCTION),
    ),
    ("CURLFOLLOW_ALL", ConstVal::Int(c::CURLFOLLOW_ALL)),
    ("CURLFOLLOW_OBEYCODE", ConstVal::Int(c::CURLFOLLOW_OBEYCODE)),
    (
        "CURLFOLLOW_FIRSTONLY",
        ConstVal::Int(c::CURLFOLLOW_FIRSTONLY),
    ),
    ("CURLINFO_TEXT", ConstVal::Int(c::CURLINFO_TEXT)),
    ("CURLINFO_HEADER_IN", ConstVal::Int(c::CURLINFO_HEADER_IN)),
    ("CURLINFO_DATA_IN", ConstVal::Int(c::CURLINFO_DATA_IN)),
    ("CURLINFO_DATA_OUT", ConstVal::Int(c::CURLINFO_DATA_OUT)),
    (
        "CURLINFO_SSL_DATA_OUT",
        ConstVal::Int(c::CURLINFO_SSL_DATA_OUT),
    ),
    (
        "CURLINFO_SSL_DATA_IN",
        ConstVal::Int(c::CURLINFO_SSL_DATA_IN),
    ),
    (
        "CURLE_ABORTED_BY_CALLBACK",
        ConstVal::Int(c::CURLE_ABORTED_BY_CALLBACK),
    ),
    (
        "CURLE_BAD_CALLING_ORDER",
        ConstVal::Int(c::CURLE_BAD_CALLING_ORDER),
    ),
    (
        "CURLE_BAD_CONTENT_ENCODING",
        ConstVal::Int(c::CURLE_BAD_CONTENT_ENCODING),
    ),
    (
        "CURLE_BAD_DOWNLOAD_RESUME",
        ConstVal::Int(c::CURLE_BAD_DOWNLOAD_RESUME),
    ),
    (
        "CURLE_BAD_FUNCTION_ARGUMENT",
        ConstVal::Int(c::CURLE_BAD_FUNCTION_ARGUMENT),
    ),
    (
        "CURLE_BAD_PASSWORD_ENTERED",
        ConstVal::Int(c::CURLE_BAD_PASSWORD_ENTERED),
    ),
    (
        "CURLE_COULDNT_CONNECT",
        ConstVal::Int(c::CURLE_COULDNT_CONNECT),
    ),
    (
        "CURLE_COULDNT_RESOLVE_HOST",
        ConstVal::Int(c::CURLE_COULDNT_RESOLVE_HOST),
    ),
    (
        "CURLE_COULDNT_RESOLVE_PROXY",
        ConstVal::Int(c::CURLE_COULDNT_RESOLVE_PROXY),
    ),
    ("CURLE_FAILED_INIT", ConstVal::Int(c::CURLE_FAILED_INIT)),
    (
        "CURLE_FILE_COULDNT_READ_FILE",
        ConstVal::Int(c::CURLE_FILE_COULDNT_READ_FILE),
    ),
    (
        "CURLE_FTP_ACCESS_DENIED",
        ConstVal::Int(c::CURLE_FTP_ACCESS_DENIED),
    ),
    (
        "CURLE_FTP_BAD_DOWNLOAD_RESUME",
        ConstVal::Int(c::CURLE_FTP_BAD_DOWNLOAD_RESUME),
    ),
    (
        "CURLE_FTP_CANT_GET_HOST",
        ConstVal::Int(c::CURLE_FTP_CANT_GET_HOST),
    ),
    (
        "CURLE_FTP_CANT_RECONNECT",
        ConstVal::Int(c::CURLE_FTP_CANT_RECONNECT),
    ),
    (
        "CURLE_FTP_COULDNT_GET_SIZE",
        ConstVal::Int(c::CURLE_FTP_COULDNT_GET_SIZE),
    ),
    (
        "CURLE_FTP_COULDNT_RETR_FILE",
        ConstVal::Int(c::CURLE_FTP_COULDNT_RETR_FILE),
    ),
    (
        "CURLE_FTP_COULDNT_SET_ASCII",
        ConstVal::Int(c::CURLE_FTP_COULDNT_SET_ASCII),
    ),
    (
        "CURLE_FTP_COULDNT_SET_BINARY",
        ConstVal::Int(c::CURLE_FTP_COULDNT_SET_BINARY),
    ),
    (
        "CURLE_FTP_COULDNT_STOR_FILE",
        ConstVal::Int(c::CURLE_FTP_COULDNT_STOR_FILE),
    ),
    (
        "CURLE_FTP_COULDNT_USE_REST",
        ConstVal::Int(c::CURLE_FTP_COULDNT_USE_REST),
    ),
    (
        "CURLE_FTP_PARTIAL_FILE",
        ConstVal::Int(c::CURLE_FTP_PARTIAL_FILE),
    ),
    (
        "CURLE_FTP_PORT_FAILED",
        ConstVal::Int(c::CURLE_FTP_PORT_FAILED),
    ),
    (
        "CURLE_FTP_QUOTE_ERROR",
        ConstVal::Int(c::CURLE_FTP_QUOTE_ERROR),
    ),
    (
        "CURLE_FTP_USER_PASSWORD_INCORRECT",
        ConstVal::Int(c::CURLE_FTP_USER_PASSWORD_INCORRECT),
    ),
    (
        "CURLE_FTP_WEIRD_227_FORMAT",
        ConstVal::Int(c::CURLE_FTP_WEIRD_227_FORMAT),
    ),
    (
        "CURLE_FTP_WEIRD_PASS_REPLY",
        ConstVal::Int(c::CURLE_FTP_WEIRD_PASS_REPLY),
    ),
    (
        "CURLE_FTP_WEIRD_PASV_REPLY",
        ConstVal::Int(c::CURLE_FTP_WEIRD_PASV_REPLY),
    ),
    (
        "CURLE_FTP_WEIRD_SERVER_REPLY",
        ConstVal::Int(c::CURLE_FTP_WEIRD_SERVER_REPLY),
    ),
    (
        "CURLE_FTP_WEIRD_USER_REPLY",
        ConstVal::Int(c::CURLE_FTP_WEIRD_USER_REPLY),
    ),
    (
        "CURLE_FTP_WRITE_ERROR",
        ConstVal::Int(c::CURLE_FTP_WRITE_ERROR),
    ),
    (
        "CURLE_FUNCTION_NOT_FOUND",
        ConstVal::Int(c::CURLE_FUNCTION_NOT_FOUND),
    ),
    ("CURLE_GOT_NOTHING", ConstVal::Int(c::CURLE_GOT_NOTHING)),
    (
        "CURLE_HTTP_NOT_FOUND",
        ConstVal::Int(c::CURLE_HTTP_NOT_FOUND),
    ),
    (
        "CURLE_HTTP_PORT_FAILED",
        ConstVal::Int(c::CURLE_HTTP_PORT_FAILED),
    ),
    (
        "CURLE_HTTP_POST_ERROR",
        ConstVal::Int(c::CURLE_HTTP_POST_ERROR),
    ),
    (
        "CURLE_HTTP_RANGE_ERROR",
        ConstVal::Int(c::CURLE_HTTP_RANGE_ERROR),
    ),
    (
        "CURLE_HTTP_RETURNED_ERROR",
        ConstVal::Int(c::CURLE_HTTP_RETURNED_ERROR),
    ),
    (
        "CURLE_LDAP_CANNOT_BIND",
        ConstVal::Int(c::CURLE_LDAP_CANNOT_BIND),
    ),
    (
        "CURLE_LDAP_SEARCH_FAILED",
        ConstVal::Int(c::CURLE_LDAP_SEARCH_FAILED),
    ),
    (
        "CURLE_LIBRARY_NOT_FOUND",
        ConstVal::Int(c::CURLE_LIBRARY_NOT_FOUND),
    ),
    (
        "CURLE_MALFORMAT_USER",
        ConstVal::Int(c::CURLE_MALFORMAT_USER),
    ),
    ("CURLE_OBSOLETE", ConstVal::Int(c::CURLE_OBSOLETE)),
    ("CURLE_OK", ConstVal::Int(c::CURLE_OK)),
    (
        "CURLE_OPERATION_TIMEDOUT",
        ConstVal::Int(c::CURLE_OPERATION_TIMEDOUT),
    ),
    (
        "CURLE_OPERATION_TIMEOUTED",
        ConstVal::Int(c::CURLE_OPERATION_TIMEOUTED),
    ),
    ("CURLE_OUT_OF_MEMORY", ConstVal::Int(c::CURLE_OUT_OF_MEMORY)),
    ("CURLE_PARTIAL_FILE", ConstVal::Int(c::CURLE_PARTIAL_FILE)),
    ("CURLE_READ_ERROR", ConstVal::Int(c::CURLE_READ_ERROR)),
    ("CURLE_RECV_ERROR", ConstVal::Int(c::CURLE_RECV_ERROR)),
    ("CURLE_SEND_ERROR", ConstVal::Int(c::CURLE_SEND_ERROR)),
    ("CURLE_SHARE_IN_USE", ConstVal::Int(c::CURLE_SHARE_IN_USE)),
    ("CURLE_SSL_CACERT", ConstVal::Int(c::CURLE_SSL_CACERT)),
    (
        "CURLE_SSL_CERTPROBLEM",
        ConstVal::Int(c::CURLE_SSL_CERTPROBLEM),
    ),
    ("CURLE_SSL_CIPHER", ConstVal::Int(c::CURLE_SSL_CIPHER)),
    (
        "CURLE_SSL_CONNECT_ERROR",
        ConstVal::Int(c::CURLE_SSL_CONNECT_ERROR),
    ),
    (
        "CURLE_SSL_ENGINE_NOTFOUND",
        ConstVal::Int(c::CURLE_SSL_ENGINE_NOTFOUND),
    ),
    (
        "CURLE_SSL_ENGINE_SETFAILED",
        ConstVal::Int(c::CURLE_SSL_ENGINE_SETFAILED),
    ),
    (
        "CURLE_SSL_PEER_CERTIFICATE",
        ConstVal::Int(c::CURLE_SSL_PEER_CERTIFICATE),
    ),
    (
        "CURLE_SSL_PINNEDPUBKEYNOTMATCH",
        ConstVal::Int(c::CURLE_SSL_PINNEDPUBKEYNOTMATCH),
    ),
    (
        "CURLE_TELNET_OPTION_SYNTAX",
        ConstVal::Int(c::CURLE_TELNET_OPTION_SYNTAX),
    ),
    (
        "CURLE_TOO_MANY_REDIRECTS",
        ConstVal::Int(c::CURLE_TOO_MANY_REDIRECTS),
    ),
    (
        "CURLE_UNKNOWN_TELNET_OPTION",
        ConstVal::Int(c::CURLE_UNKNOWN_TELNET_OPTION),
    ),
    (
        "CURLE_UNSUPPORTED_PROTOCOL",
        ConstVal::Int(c::CURLE_UNSUPPORTED_PROTOCOL),
    ),
    ("CURLE_URL_MALFORMAT", ConstVal::Int(c::CURLE_URL_MALFORMAT)),
    (
        "CURLE_URL_MALFORMAT_USER",
        ConstVal::Int(c::CURLE_URL_MALFORMAT_USER),
    ),
    ("CURLE_WRITE_ERROR", ConstVal::Int(c::CURLE_WRITE_ERROR)),
    (
        "CURLINFO_CONNECT_TIME",
        ConstVal::Int(c::CURLINFO_CONNECT_TIME),
    ),
    (
        "CURLINFO_CONTENT_LENGTH_DOWNLOAD",
        ConstVal::Int(c::CURLINFO_CONTENT_LENGTH_DOWNLOAD),
    ),
    (
        "CURLINFO_CONTENT_LENGTH_UPLOAD",
        ConstVal::Int(c::CURLINFO_CONTENT_LENGTH_UPLOAD),
    ),
    (
        "CURLINFO_CONTENT_TYPE",
        ConstVal::Int(c::CURLINFO_CONTENT_TYPE),
    ),
    (
        "CURLINFO_EFFECTIVE_URL",
        ConstVal::Int(c::CURLINFO_EFFECTIVE_URL),
    ),
    ("CURLINFO_FILETIME", ConstVal::Int(c::CURLINFO_FILETIME)),
    ("CURLINFO_HEADER_OUT", ConstVal::Int(c::CURLINFO_HEADER_OUT)),
    (
        "CURLINFO_HEADER_SIZE",
        ConstVal::Int(c::CURLINFO_HEADER_SIZE),
    ),
    ("CURLINFO_HTTP_CODE", ConstVal::Int(c::CURLINFO_HTTP_CODE)),
    ("CURLINFO_LASTONE", ConstVal::Int(c::CURLINFO_LASTONE)),
    (
        "CURLINFO_NAMELOOKUP_TIME",
        ConstVal::Int(c::CURLINFO_NAMELOOKUP_TIME),
    ),
    (
        "CURLINFO_PRETRANSFER_TIME",
        ConstVal::Int(c::CURLINFO_PRETRANSFER_TIME),
    ),
    ("CURLINFO_PRIVATE", ConstVal::Int(c::CURLINFO_PRIVATE)),
    (
        "CURLINFO_REDIRECT_COUNT",
        ConstVal::Int(c::CURLINFO_REDIRECT_COUNT),
    ),
    (
        "CURLINFO_REDIRECT_TIME",
        ConstVal::Int(c::CURLINFO_REDIRECT_TIME),
    ),
    (
        "CURLINFO_REQUEST_SIZE",
        ConstVal::Int(c::CURLINFO_REQUEST_SIZE),
    ),
    (
        "CURLINFO_SIZE_DOWNLOAD",
        ConstVal::Int(c::CURLINFO_SIZE_DOWNLOAD),
    ),
    (
        "CURLINFO_SIZE_UPLOAD",
        ConstVal::Int(c::CURLINFO_SIZE_UPLOAD),
    ),
    (
        "CURLINFO_SPEED_DOWNLOAD",
        ConstVal::Int(c::CURLINFO_SPEED_DOWNLOAD),
    ),
    (
        "CURLINFO_SPEED_UPLOAD",
        ConstVal::Int(c::CURLINFO_SPEED_UPLOAD),
    ),
    (
        "CURLINFO_SSL_VERIFYRESULT",
        ConstVal::Int(c::CURLINFO_SSL_VERIFYRESULT),
    ),
    (
        "CURLINFO_STARTTRANSFER_TIME",
        ConstVal::Int(c::CURLINFO_STARTTRANSFER_TIME),
    ),
    ("CURLINFO_TOTAL_TIME", ConstVal::Int(c::CURLINFO_TOTAL_TIME)),
    (
        "CURLINFO_EFFECTIVE_METHOD",
        ConstVal::Int(c::CURLINFO_EFFECTIVE_METHOD),
    ),
    ("CURLINFO_CAPATH", ConstVal::Int(c::CURLINFO_CAPATH)),
    ("CURLINFO_CAINFO", ConstVal::Int(c::CURLINFO_CAINFO)),
    (
        "CURLINFO_HTTPAUTH_USED",
        ConstVal::Int(c::CURLINFO_HTTPAUTH_USED),
    ),
    (
        "CURLINFO_PROXYAUTH_USED",
        ConstVal::Int(c::CURLINFO_PROXYAUTH_USED),
    ),
    ("CURLMSG_DONE", ConstVal::Int(c::CURLMSG_DONE)),
    ("CURLVERSION_NOW", ConstVal::Int(c::CURLVERSION_NOW)),
    (
        "CURLM_BAD_EASY_HANDLE",
        ConstVal::Int(c::CURLM_BAD_EASY_HANDLE),
    ),
    ("CURLM_BAD_HANDLE", ConstVal::Int(c::CURLM_BAD_HANDLE)),
    (
        "CURLM_CALL_MULTI_PERFORM",
        ConstVal::Int(c::CURLM_CALL_MULTI_PERFORM),
    ),
    (
        "CURLM_INTERNAL_ERROR",
        ConstVal::Int(c::CURLM_INTERNAL_ERROR),
    ),
    ("CURLM_OK", ConstVal::Int(c::CURLM_OK)),
    ("CURLM_OUT_OF_MEMORY", ConstVal::Int(c::CURLM_OUT_OF_MEMORY)),
    ("CURLM_ADDED_ALREADY", ConstVal::Int(c::CURLM_ADDED_ALREADY)),
    ("CURLPROXY_HTTP", ConstVal::Int(c::CURLPROXY_HTTP)),
    ("CURLPROXY_SOCKS4", ConstVal::Int(c::CURLPROXY_SOCKS4)),
    ("CURLPROXY_SOCKS5", ConstVal::Int(c::CURLPROXY_SOCKS5)),
    ("CURLSHOPT_NONE", ConstVal::Int(c::CURLSHOPT_NONE)),
    ("CURLSHOPT_SHARE", ConstVal::Int(c::CURLSHOPT_SHARE)),
    ("CURLSHOPT_UNSHARE", ConstVal::Int(c::CURLSHOPT_UNSHARE)),
    (
        "CURL_HTTP_VERSION_1_0",
        ConstVal::Int(c::CURL_HTTP_VERSION_1_0),
    ),
    (
        "CURL_HTTP_VERSION_1_1",
        ConstVal::Int(c::CURL_HTTP_VERSION_1_1),
    ),
    (
        "CURL_HTTP_VERSION_NONE",
        ConstVal::Int(c::CURL_HTTP_VERSION_NONE),
    ),
    (
        "CURL_LOCK_DATA_COOKIE",
        ConstVal::Int(c::CURL_LOCK_DATA_COOKIE),
    ),
    ("CURL_LOCK_DATA_DNS", ConstVal::Int(c::CURL_LOCK_DATA_DNS)),
    (
        "CURL_LOCK_DATA_SSL_SESSION",
        ConstVal::Int(c::CURL_LOCK_DATA_SSL_SESSION),
    ),
    ("CURL_NETRC_IGNORED", ConstVal::Int(c::CURL_NETRC_IGNORED)),
    ("CURL_NETRC_OPTIONAL", ConstVal::Int(c::CURL_NETRC_OPTIONAL)),
    ("CURL_NETRC_REQUIRED", ConstVal::Int(c::CURL_NETRC_REQUIRED)),
    (
        "CURL_SSLVERSION_DEFAULT",
        ConstVal::Int(c::CURL_SSLVERSION_DEFAULT),
    ),
    (
        "CURL_SSLVERSION_SSLv2",
        ConstVal::Int(c::CURL_SSLVERSION_SSLv2),
    ),
    (
        "CURL_SSLVERSION_SSLv3",
        ConstVal::Int(c::CURL_SSLVERSION_SSLv3),
    ),
    (
        "CURL_SSLVERSION_TLSv1",
        ConstVal::Int(c::CURL_SSLVERSION_TLSv1),
    ),
    (
        "CURL_TIMECOND_IFMODSINCE",
        ConstVal::Int(c::CURL_TIMECOND_IFMODSINCE),
    ),
    (
        "CURL_TIMECOND_IFUNMODSINCE",
        ConstVal::Int(c::CURL_TIMECOND_IFUNMODSINCE),
    ),
    (
        "CURL_TIMECOND_LASTMOD",
        ConstVal::Int(c::CURL_TIMECOND_LASTMOD),
    ),
    ("CURL_TIMECOND_NONE", ConstVal::Int(c::CURL_TIMECOND_NONE)),
    (
        "CURL_VERSION_ASYNCHDNS",
        ConstVal::Int(c::CURL_VERSION_ASYNCHDNS),
    ),
    ("CURL_VERSION_CONV", ConstVal::Int(c::CURL_VERSION_CONV)),
    ("CURL_VERSION_DEBUG", ConstVal::Int(c::CURL_VERSION_DEBUG)),
    (
        "CURL_VERSION_GSSNEGOTIATE",
        ConstVal::Int(c::CURL_VERSION_GSSNEGOTIATE),
    ),
    ("CURL_VERSION_IDN", ConstVal::Int(c::CURL_VERSION_IDN)),
    ("CURL_VERSION_IPV6", ConstVal::Int(c::CURL_VERSION_IPV6)),
    (
        "CURL_VERSION_KERBEROS4",
        ConstVal::Int(c::CURL_VERSION_KERBEROS4),
    ),
    (
        "CURL_VERSION_LARGEFILE",
        ConstVal::Int(c::CURL_VERSION_LARGEFILE),
    ),
    ("CURL_VERSION_LIBZ", ConstVal::Int(c::CURL_VERSION_LIBZ)),
    ("CURL_VERSION_NTLM", ConstVal::Int(c::CURL_VERSION_NTLM)),
    ("CURL_VERSION_SPNEGO", ConstVal::Int(c::CURL_VERSION_SPNEGO)),
    ("CURL_VERSION_SSL", ConstVal::Int(c::CURL_VERSION_SSL)),
    ("CURL_VERSION_SSPI", ConstVal::Int(c::CURL_VERSION_SSPI)),
    ("CURLOPT_HTTPAUTH", ConstVal::Int(c::CURLOPT_HTTPAUTH)),
    ("CURLAUTH_ANY", ConstVal::Int(c::CURLAUTH_ANY)),
    ("CURLAUTH_ANYSAFE", ConstVal::Int(c::CURLAUTH_ANYSAFE)),
    ("CURLAUTH_BASIC", ConstVal::Int(c::CURLAUTH_BASIC)),
    ("CURLAUTH_DIGEST", ConstVal::Int(c::CURLAUTH_DIGEST)),
    (
        "CURLAUTH_GSSNEGOTIATE",
        ConstVal::Int(c::CURLAUTH_GSSNEGOTIATE),
    ),
    ("CURLAUTH_NONE", ConstVal::Int(c::CURLAUTH_NONE)),
    ("CURLAUTH_NTLM", ConstVal::Int(c::CURLAUTH_NTLM)),
    (
        "CURLINFO_HTTP_CONNECTCODE",
        ConstVal::Int(c::CURLINFO_HTTP_CONNECTCODE),
    ),
    (
        "CURLOPT_FTP_CREATE_MISSING_DIRS",
        ConstVal::Int(c::CURLOPT_FTP_CREATE_MISSING_DIRS),
    ),
    ("CURLOPT_PROXYAUTH", ConstVal::Int(c::CURLOPT_PROXYAUTH)),
    (
        "CURLE_FILESIZE_EXCEEDED",
        ConstVal::Int(c::CURLE_FILESIZE_EXCEEDED),
    ),
    (
        "CURLE_LDAP_INVALID_URL",
        ConstVal::Int(c::CURLE_LDAP_INVALID_URL),
    ),
    (
        "CURLINFO_HTTPAUTH_AVAIL",
        ConstVal::Int(c::CURLINFO_HTTPAUTH_AVAIL),
    ),
    (
        "CURLINFO_RESPONSE_CODE",
        ConstVal::Int(c::CURLINFO_RESPONSE_CODE),
    ),
    (
        "CURLINFO_PROXYAUTH_AVAIL",
        ConstVal::Int(c::CURLINFO_PROXYAUTH_AVAIL),
    ),
    (
        "CURLOPT_FTP_RESPONSE_TIMEOUT",
        ConstVal::Int(c::CURLOPT_FTP_RESPONSE_TIMEOUT),
    ),
    (
        "CURLOPT_SERVER_RESPONSE_TIMEOUT",
        ConstVal::Int(c::CURLOPT_SERVER_RESPONSE_TIMEOUT),
    ),
    ("CURLOPT_IPRESOLVE", ConstVal::Int(c::CURLOPT_IPRESOLVE)),
    ("CURLOPT_MAXFILESIZE", ConstVal::Int(c::CURLOPT_MAXFILESIZE)),
    ("CURL_IPRESOLVE_V4", ConstVal::Int(c::CURL_IPRESOLVE_V4)),
    ("CURL_IPRESOLVE_V6", ConstVal::Int(c::CURL_IPRESOLVE_V6)),
    (
        "CURL_IPRESOLVE_WHATEVER",
        ConstVal::Int(c::CURL_IPRESOLVE_WHATEVER),
    ),
    (
        "CURLE_FTP_SSL_FAILED",
        ConstVal::Int(c::CURLE_FTP_SSL_FAILED),
    ),
    ("CURLFTPSSL_ALL", ConstVal::Int(c::CURLFTPSSL_ALL)),
    ("CURLFTPSSL_CONTROL", ConstVal::Int(c::CURLFTPSSL_CONTROL)),
    ("CURLFTPSSL_NONE", ConstVal::Int(c::CURLFTPSSL_NONE)),
    ("CURLFTPSSL_TRY", ConstVal::Int(c::CURLFTPSSL_TRY)),
    ("CURLOPT_FTP_SSL", ConstVal::Int(c::CURLOPT_FTP_SSL)),
    ("CURLOPT_NETRC_FILE", ConstVal::Int(c::CURLOPT_NETRC_FILE)),
    (
        "CURLOPT_MAXFILESIZE_LARGE",
        ConstVal::Int(c::CURLOPT_MAXFILESIZE_LARGE),
    ),
    ("CURLOPT_TCP_NODELAY", ConstVal::Int(c::CURLOPT_TCP_NODELAY)),
    ("CURLFTPAUTH_DEFAULT", ConstVal::Int(c::CURLFTPAUTH_DEFAULT)),
    ("CURLFTPAUTH_SSL", ConstVal::Int(c::CURLFTPAUTH_SSL)),
    ("CURLFTPAUTH_TLS", ConstVal::Int(c::CURLFTPAUTH_TLS)),
    ("CURLOPT_FTPSSLAUTH", ConstVal::Int(c::CURLOPT_FTPSSLAUTH)),
    ("CURLOPT_FTP_ACCOUNT", ConstVal::Int(c::CURLOPT_FTP_ACCOUNT)),
    ("CURLINFO_OS_ERRNO", ConstVal::Int(c::CURLINFO_OS_ERRNO)),
    (
        "CURLINFO_NUM_CONNECTS",
        ConstVal::Int(c::CURLINFO_NUM_CONNECTS),
    ),
    (
        "CURLINFO_SSL_ENGINES",
        ConstVal::Int(c::CURLINFO_SSL_ENGINES),
    ),
    ("CURLINFO_COOKIELIST", ConstVal::Int(c::CURLINFO_COOKIELIST)),
    ("CURLOPT_COOKIELIST", ConstVal::Int(c::CURLOPT_COOKIELIST)),
    (
        "CURLOPT_IGNORE_CONTENT_LENGTH",
        ConstVal::Int(c::CURLOPT_IGNORE_CONTENT_LENGTH),
    ),
    (
        "CURLOPT_FTP_SKIP_PASV_IP",
        ConstVal::Int(c::CURLOPT_FTP_SKIP_PASV_IP),
    ),
    (
        "CURLOPT_FTP_FILEMETHOD",
        ConstVal::Int(c::CURLOPT_FTP_FILEMETHOD),
    ),
    (
        "CURLOPT_CONNECT_ONLY",
        ConstVal::Int(c::CURLOPT_CONNECT_ONLY),
    ),
    ("CURLOPT_LOCALPORT", ConstVal::Int(c::CURLOPT_LOCALPORT)),
    (
        "CURLOPT_LOCALPORTRANGE",
        ConstVal::Int(c::CURLOPT_LOCALPORTRANGE),
    ),
    (
        "CURLFTPMETHOD_DEFAULT",
        ConstVal::Int(c::CURLFTPMETHOD_DEFAULT),
    ),
    (
        "CURLFTPMETHOD_MULTICWD",
        ConstVal::Int(c::CURLFTPMETHOD_MULTICWD),
    ),
    ("CURLFTPMETHOD_NOCWD", ConstVal::Int(c::CURLFTPMETHOD_NOCWD)),
    (
        "CURLFTPMETHOD_SINGLECWD",
        ConstVal::Int(c::CURLFTPMETHOD_SINGLECWD),
    ),
    (
        "CURLINFO_FTP_ENTRY_PATH",
        ConstVal::Int(c::CURLINFO_FTP_ENTRY_PATH),
    ),
    (
        "CURLOPT_FTP_ALTERNATIVE_TO_USER",
        ConstVal::Int(c::CURLOPT_FTP_ALTERNATIVE_TO_USER),
    ),
    (
        "CURLOPT_MAX_RECV_SPEED_LARGE",
        ConstVal::Int(c::CURLOPT_MAX_RECV_SPEED_LARGE),
    ),
    (
        "CURLOPT_MAX_SEND_SPEED_LARGE",
        ConstVal::Int(c::CURLOPT_MAX_SEND_SPEED_LARGE),
    ),
    (
        "CURLE_SSL_CACERT_BADFILE",
        ConstVal::Int(c::CURLE_SSL_CACERT_BADFILE),
    ),
    (
        "CURLOPT_SSL_SESSIONID_CACHE",
        ConstVal::Int(c::CURLOPT_SSL_SESSIONID_CACHE),
    ),
    ("CURLMOPT_PIPELINING", ConstVal::Int(c::CURLMOPT_PIPELINING)),
    ("CURLE_SSH", ConstVal::Int(c::CURLE_SSH)),
    ("CURLOPT_FTP_SSL_CCC", ConstVal::Int(c::CURLOPT_FTP_SSL_CCC)),
    (
        "CURLOPT_SSH_AUTH_TYPES",
        ConstVal::Int(c::CURLOPT_SSH_AUTH_TYPES),
    ),
    (
        "CURLOPT_SSH_PRIVATE_KEYFILE",
        ConstVal::Int(c::CURLOPT_SSH_PRIVATE_KEYFILE),
    ),
    (
        "CURLOPT_SSH_PUBLIC_KEYFILE",
        ConstVal::Int(c::CURLOPT_SSH_PUBLIC_KEYFILE),
    ),
    (
        "CURLFTPSSL_CCC_ACTIVE",
        ConstVal::Int(c::CURLFTPSSL_CCC_ACTIVE),
    ),
    ("CURLFTPSSL_CCC_NONE", ConstVal::Int(c::CURLFTPSSL_CCC_NONE)),
    (
        "CURLFTPSSL_CCC_PASSIVE",
        ConstVal::Int(c::CURLFTPSSL_CCC_PASSIVE),
    ),
    (
        "CURLOPT_CONNECTTIMEOUT_MS",
        ConstVal::Int(c::CURLOPT_CONNECTTIMEOUT_MS),
    ),
    (
        "CURLOPT_HTTP_CONTENT_DECODING",
        ConstVal::Int(c::CURLOPT_HTTP_CONTENT_DECODING),
    ),
    (
        "CURLOPT_HTTP_TRANSFER_DECODING",
        ConstVal::Int(c::CURLOPT_HTTP_TRANSFER_DECODING),
    ),
    ("CURLOPT_TIMEOUT_MS", ConstVal::Int(c::CURLOPT_TIMEOUT_MS)),
    (
        "CURLMOPT_MAXCONNECTS",
        ConstVal::Int(c::CURLMOPT_MAXCONNECTS),
    ),
    ("CURLOPT_KRBLEVEL", ConstVal::Int(c::CURLOPT_KRBLEVEL)),
    (
        "CURLOPT_NEW_DIRECTORY_PERMS",
        ConstVal::Int(c::CURLOPT_NEW_DIRECTORY_PERMS),
    ),
    (
        "CURLOPT_NEW_FILE_PERMS",
        ConstVal::Int(c::CURLOPT_NEW_FILE_PERMS),
    ),
    ("CURLOPT_APPEND", ConstVal::Int(c::CURLOPT_APPEND)),
    ("CURLOPT_DIRLISTONLY", ConstVal::Int(c::CURLOPT_DIRLISTONLY)),
    ("CURLOPT_USE_SSL", ConstVal::Int(c::CURLOPT_USE_SSL)),
    ("CURLUSESSL_ALL", ConstVal::Int(c::CURLUSESSL_ALL)),
    ("CURLUSESSL_CONTROL", ConstVal::Int(c::CURLUSESSL_CONTROL)),
    ("CURLUSESSL_NONE", ConstVal::Int(c::CURLUSESSL_NONE)),
    ("CURLUSESSL_TRY", ConstVal::Int(c::CURLUSESSL_TRY)),
    (
        "CURLOPT_SSH_HOST_PUBLIC_KEY_MD5",
        ConstVal::Int(c::CURLOPT_SSH_HOST_PUBLIC_KEY_MD5),
    ),
    (
        "CURLOPT_PROXY_TRANSFER_MODE",
        ConstVal::Int(c::CURLOPT_PROXY_TRANSFER_MODE),
    ),
    ("CURLPAUSE_ALL", ConstVal::Int(c::CURLPAUSE_ALL)),
    ("CURLPAUSE_CONT", ConstVal::Int(c::CURLPAUSE_CONT)),
    ("CURLPAUSE_RECV", ConstVal::Int(c::CURLPAUSE_RECV)),
    ("CURLPAUSE_RECV_CONT", ConstVal::Int(c::CURLPAUSE_RECV_CONT)),
    ("CURLPAUSE_SEND", ConstVal::Int(c::CURLPAUSE_SEND)),
    ("CURLPAUSE_SEND_CONT", ConstVal::Int(c::CURLPAUSE_SEND_CONT)),
    ("CURL_READFUNC_PAUSE", ConstVal::Int(c::CURL_READFUNC_PAUSE)),
    (
        "CURL_WRITEFUNC_PAUSE",
        ConstVal::Int(c::CURL_WRITEFUNC_PAUSE),
    ),
    ("CURLPROXY_SOCKS4A", ConstVal::Int(c::CURLPROXY_SOCKS4A)),
    (
        "CURLPROXY_SOCKS5_HOSTNAME",
        ConstVal::Int(c::CURLPROXY_SOCKS5_HOSTNAME),
    ),
    (
        "CURLINFO_REDIRECT_URL",
        ConstVal::Int(c::CURLINFO_REDIRECT_URL),
    ),
    (
        "CURLINFO_APPCONNECT_TIME",
        ConstVal::Int(c::CURLINFO_APPCONNECT_TIME),
    ),
    ("CURLINFO_PRIMARY_IP", ConstVal::Int(c::CURLINFO_PRIMARY_IP)),
    (
        "CURLOPT_ADDRESS_SCOPE",
        ConstVal::Int(c::CURLOPT_ADDRESS_SCOPE),
    ),
    ("CURLOPT_CRLFILE", ConstVal::Int(c::CURLOPT_CRLFILE)),
    ("CURLOPT_ISSUERCERT", ConstVal::Int(c::CURLOPT_ISSUERCERT)),
    ("CURLOPT_KEYPASSWD", ConstVal::Int(c::CURLOPT_KEYPASSWD)),
    ("CURLSSH_AUTH_ANY", ConstVal::Int(c::CURLSSH_AUTH_ANY)),
    (
        "CURLSSH_AUTH_DEFAULT",
        ConstVal::Int(c::CURLSSH_AUTH_DEFAULT),
    ),
    ("CURLSSH_AUTH_HOST", ConstVal::Int(c::CURLSSH_AUTH_HOST)),
    (
        "CURLSSH_AUTH_KEYBOARD",
        ConstVal::Int(c::CURLSSH_AUTH_KEYBOARD),
    ),
    ("CURLSSH_AUTH_NONE", ConstVal::Int(c::CURLSSH_AUTH_NONE)),
    (
        "CURLSSH_AUTH_PASSWORD",
        ConstVal::Int(c::CURLSSH_AUTH_PASSWORD),
    ),
    (
        "CURLSSH_AUTH_PUBLICKEY",
        ConstVal::Int(c::CURLSSH_AUTH_PUBLICKEY),
    ),
    ("CURLINFO_CERTINFO", ConstVal::Int(c::CURLINFO_CERTINFO)),
    ("CURLOPT_CERTINFO", ConstVal::Int(c::CURLOPT_CERTINFO)),
    ("CURLOPT_PASSWORD", ConstVal::Int(c::CURLOPT_PASSWORD)),
    ("CURLOPT_POSTREDIR", ConstVal::Int(c::CURLOPT_POSTREDIR)),
    (
        "CURLOPT_PROXYPASSWORD",
        ConstVal::Int(c::CURLOPT_PROXYPASSWORD),
    ),
    (
        "CURLOPT_PROXYUSERNAME",
        ConstVal::Int(c::CURLOPT_PROXYUSERNAME),
    ),
    ("CURLOPT_USERNAME", ConstVal::Int(c::CURLOPT_USERNAME)),
    ("CURL_REDIR_POST_301", ConstVal::Int(c::CURL_REDIR_POST_301)),
    ("CURL_REDIR_POST_302", ConstVal::Int(c::CURL_REDIR_POST_302)),
    ("CURL_REDIR_POST_ALL", ConstVal::Int(c::CURL_REDIR_POST_ALL)),
    ("CURLAUTH_DIGEST_IE", ConstVal::Int(c::CURLAUTH_DIGEST_IE)),
    (
        "CURLINFO_CONDITION_UNMET",
        ConstVal::Int(c::CURLINFO_CONDITION_UNMET),
    ),
    ("CURLOPT_NOPROXY", ConstVal::Int(c::CURLOPT_NOPROXY)),
    ("CURLOPT_PROTOCOLS", ConstVal::Int(c::CURLOPT_PROTOCOLS)),
    (
        "CURLOPT_REDIR_PROTOCOLS",
        ConstVal::Int(c::CURLOPT_REDIR_PROTOCOLS),
    ),
    (
        "CURLOPT_SOCKS5_GSSAPI_NEC",
        ConstVal::Int(c::CURLOPT_SOCKS5_GSSAPI_NEC),
    ),
    (
        "CURLOPT_SOCKS5_GSSAPI_SERVICE",
        ConstVal::Int(c::CURLOPT_SOCKS5_GSSAPI_SERVICE),
    ),
    (
        "CURLOPT_TFTP_BLKSIZE",
        ConstVal::Int(c::CURLOPT_TFTP_BLKSIZE),
    ),
    ("CURLPROTO_ALL", ConstVal::Int(c::CURLPROTO_ALL)),
    ("CURLPROTO_DICT", ConstVal::Int(c::CURLPROTO_DICT)),
    ("CURLPROTO_FILE", ConstVal::Int(c::CURLPROTO_FILE)),
    ("CURLPROTO_FTP", ConstVal::Int(c::CURLPROTO_FTP)),
    ("CURLPROTO_FTPS", ConstVal::Int(c::CURLPROTO_FTPS)),
    ("CURLPROTO_HTTP", ConstVal::Int(c::CURLPROTO_HTTP)),
    ("CURLPROTO_HTTPS", ConstVal::Int(c::CURLPROTO_HTTPS)),
    ("CURLPROTO_LDAP", ConstVal::Int(c::CURLPROTO_LDAP)),
    ("CURLPROTO_LDAPS", ConstVal::Int(c::CURLPROTO_LDAPS)),
    ("CURLPROTO_SCP", ConstVal::Int(c::CURLPROTO_SCP)),
    ("CURLPROTO_SFTP", ConstVal::Int(c::CURLPROTO_SFTP)),
    ("CURLPROTO_TELNET", ConstVal::Int(c::CURLPROTO_TELNET)),
    ("CURLPROTO_TFTP", ConstVal::Int(c::CURLPROTO_TFTP)),
    ("CURLPROXY_HTTP_1_0", ConstVal::Int(c::CURLPROXY_HTTP_1_0)),
    ("CURLFTP_CREATE_DIR", ConstVal::Int(c::CURLFTP_CREATE_DIR)),
    (
        "CURLFTP_CREATE_DIR_NONE",
        ConstVal::Int(c::CURLFTP_CREATE_DIR_NONE),
    ),
    (
        "CURLFTP_CREATE_DIR_RETRY",
        ConstVal::Int(c::CURLFTP_CREATE_DIR_RETRY),
    ),
    (
        "CURL_VERSION_CURLDEBUG",
        ConstVal::Int(c::CURL_VERSION_CURLDEBUG),
    ),
    (
        "CURLOPT_SSH_KNOWNHOSTS",
        ConstVal::Int(c::CURLOPT_SSH_KNOWNHOSTS),
    ),
    ("CURLKHMATCH_OK", ConstVal::Int(c::CURLKHMATCH_OK)),
    (
        "CURLKHMATCH_MISMATCH",
        ConstVal::Int(c::CURLKHMATCH_MISMATCH),
    ),
    ("CURLKHMATCH_MISSING", ConstVal::Int(c::CURLKHMATCH_MISSING)),
    ("CURLKHMATCH_LAST", ConstVal::Int(c::CURLKHMATCH_LAST)),
    (
        "CURLINFO_RTSP_CLIENT_CSEQ",
        ConstVal::Int(c::CURLINFO_RTSP_CLIENT_CSEQ),
    ),
    (
        "CURLINFO_RTSP_CSEQ_RECV",
        ConstVal::Int(c::CURLINFO_RTSP_CSEQ_RECV),
    ),
    (
        "CURLINFO_RTSP_SERVER_CSEQ",
        ConstVal::Int(c::CURLINFO_RTSP_SERVER_CSEQ),
    ),
    (
        "CURLINFO_RTSP_SESSION_ID",
        ConstVal::Int(c::CURLINFO_RTSP_SESSION_ID),
    ),
    (
        "CURLOPT_FTP_USE_PRET",
        ConstVal::Int(c::CURLOPT_FTP_USE_PRET),
    ),
    ("CURLOPT_MAIL_FROM", ConstVal::Int(c::CURLOPT_MAIL_FROM)),
    ("CURLOPT_MAIL_RCPT", ConstVal::Int(c::CURLOPT_MAIL_RCPT)),
    (
        "CURLOPT_RTSP_CLIENT_CSEQ",
        ConstVal::Int(c::CURLOPT_RTSP_CLIENT_CSEQ),
    ),
    (
        "CURLOPT_RTSP_REQUEST",
        ConstVal::Int(c::CURLOPT_RTSP_REQUEST),
    ),
    (
        "CURLOPT_RTSP_SERVER_CSEQ",
        ConstVal::Int(c::CURLOPT_RTSP_SERVER_CSEQ),
    ),
    (
        "CURLOPT_RTSP_SESSION_ID",
        ConstVal::Int(c::CURLOPT_RTSP_SESSION_ID),
    ),
    (
        "CURLOPT_RTSP_STREAM_URI",
        ConstVal::Int(c::CURLOPT_RTSP_STREAM_URI),
    ),
    (
        "CURLOPT_RTSP_TRANSPORT",
        ConstVal::Int(c::CURLOPT_RTSP_TRANSPORT),
    ),
    ("CURLPROTO_IMAP", ConstVal::Int(c::CURLPROTO_IMAP)),
    ("CURLPROTO_IMAPS", ConstVal::Int(c::CURLPROTO_IMAPS)),
    ("CURLPROTO_POP3", ConstVal::Int(c::CURLPROTO_POP3)),
    ("CURLPROTO_POP3S", ConstVal::Int(c::CURLPROTO_POP3S)),
    ("CURLPROTO_RTSP", ConstVal::Int(c::CURLPROTO_RTSP)),
    ("CURLPROTO_SMTP", ConstVal::Int(c::CURLPROTO_SMTP)),
    ("CURLPROTO_SMTPS", ConstVal::Int(c::CURLPROTO_SMTPS)),
    (
        "CURL_RTSPREQ_ANNOUNCE",
        ConstVal::Int(c::CURL_RTSPREQ_ANNOUNCE),
    ),
    (
        "CURL_RTSPREQ_DESCRIBE",
        ConstVal::Int(c::CURL_RTSPREQ_DESCRIBE),
    ),
    (
        "CURL_RTSPREQ_GET_PARAMETER",
        ConstVal::Int(c::CURL_RTSPREQ_GET_PARAMETER),
    ),
    (
        "CURL_RTSPREQ_OPTIONS",
        ConstVal::Int(c::CURL_RTSPREQ_OPTIONS),
    ),
    ("CURL_RTSPREQ_PAUSE", ConstVal::Int(c::CURL_RTSPREQ_PAUSE)),
    ("CURL_RTSPREQ_PLAY", ConstVal::Int(c::CURL_RTSPREQ_PLAY)),
    (
        "CURL_RTSPREQ_RECEIVE",
        ConstVal::Int(c::CURL_RTSPREQ_RECEIVE),
    ),
    ("CURL_RTSPREQ_RECORD", ConstVal::Int(c::CURL_RTSPREQ_RECORD)),
    (
        "CURL_RTSPREQ_SET_PARAMETER",
        ConstVal::Int(c::CURL_RTSPREQ_SET_PARAMETER),
    ),
    ("CURL_RTSPREQ_SETUP", ConstVal::Int(c::CURL_RTSPREQ_SETUP)),
    (
        "CURL_RTSPREQ_TEARDOWN",
        ConstVal::Int(c::CURL_RTSPREQ_TEARDOWN),
    ),
    ("CURLINFO_LOCAL_IP", ConstVal::Int(c::CURLINFO_LOCAL_IP)),
    ("CURLINFO_LOCAL_PORT", ConstVal::Int(c::CURLINFO_LOCAL_PORT)),
    (
        "CURLINFO_PRIMARY_PORT",
        ConstVal::Int(c::CURLINFO_PRIMARY_PORT),
    ),
    (
        "CURLOPT_FNMATCH_FUNCTION",
        ConstVal::Int(c::CURLOPT_FNMATCH_FUNCTION),
    ),
    (
        "CURLOPT_WILDCARDMATCH",
        ConstVal::Int(c::CURLOPT_WILDCARDMATCH),
    ),
    ("CURLPROTO_RTMP", ConstVal::Int(c::CURLPROTO_RTMP)),
    ("CURLPROTO_RTMPE", ConstVal::Int(c::CURLPROTO_RTMPE)),
    ("CURLPROTO_RTMPS", ConstVal::Int(c::CURLPROTO_RTMPS)),
    ("CURLPROTO_RTMPT", ConstVal::Int(c::CURLPROTO_RTMPT)),
    ("CURLPROTO_RTMPTE", ConstVal::Int(c::CURLPROTO_RTMPTE)),
    ("CURLPROTO_RTMPTS", ConstVal::Int(c::CURLPROTO_RTMPTS)),
    (
        "CURL_FNMATCHFUNC_FAIL",
        ConstVal::Int(c::CURL_FNMATCHFUNC_FAIL),
    ),
    (
        "CURL_FNMATCHFUNC_MATCH",
        ConstVal::Int(c::CURL_FNMATCHFUNC_MATCH),
    ),
    (
        "CURL_FNMATCHFUNC_NOMATCH",
        ConstVal::Int(c::CURL_FNMATCHFUNC_NOMATCH),
    ),
    ("CURLPROTO_GOPHER", ConstVal::Int(c::CURLPROTO_GOPHER)),
    ("CURLAUTH_ONLY", ConstVal::Int(c::CURLAUTH_ONLY)),
    ("CURLOPT_RESOLVE", ConstVal::Int(c::CURLOPT_RESOLVE)),
    (
        "CURLOPT_TLSAUTH_PASSWORD",
        ConstVal::Int(c::CURLOPT_TLSAUTH_PASSWORD),
    ),
    (
        "CURLOPT_TLSAUTH_TYPE",
        ConstVal::Int(c::CURLOPT_TLSAUTH_TYPE),
    ),
    (
        "CURLOPT_TLSAUTH_USERNAME",
        ConstVal::Int(c::CURLOPT_TLSAUTH_USERNAME),
    ),
    ("CURL_TLSAUTH_SRP", ConstVal::Int(c::CURL_TLSAUTH_SRP)),
    (
        "CURL_VERSION_TLSAUTH_SRP",
        ConstVal::Int(c::CURL_VERSION_TLSAUTH_SRP),
    ),
    (
        "CURLOPT_ACCEPT_ENCODING",
        ConstVal::Int(c::CURLOPT_ACCEPT_ENCODING),
    ),
    (
        "CURLOPT_TRANSFER_ENCODING",
        ConstVal::Int(c::CURLOPT_TRANSFER_ENCODING),
    ),
    ("CURLAUTH_NTLM_WB", ConstVal::Int(c::CURLAUTH_NTLM_WB)),
    (
        "CURLGSSAPI_DELEGATION_FLAG",
        ConstVal::Int(c::CURLGSSAPI_DELEGATION_FLAG),
    ),
    (
        "CURLGSSAPI_DELEGATION_POLICY_FLAG",
        ConstVal::Int(c::CURLGSSAPI_DELEGATION_POLICY_FLAG),
    ),
    (
        "CURLOPT_GSSAPI_DELEGATION",
        ConstVal::Int(c::CURLOPT_GSSAPI_DELEGATION),
    ),
    (
        "CURL_VERSION_NTLM_WB",
        ConstVal::Int(c::CURL_VERSION_NTLM_WB),
    ),
    (
        "CURLOPT_ACCEPTTIMEOUT_MS",
        ConstVal::Int(c::CURLOPT_ACCEPTTIMEOUT_MS),
    ),
    ("CURLOPT_DNS_SERVERS", ConstVal::Int(c::CURLOPT_DNS_SERVERS)),
    ("CURLOPT_MAIL_AUTH", ConstVal::Int(c::CURLOPT_MAIL_AUTH)),
    ("CURLOPT_SSL_OPTIONS", ConstVal::Int(c::CURLOPT_SSL_OPTIONS)),
    (
        "CURLOPT_TCP_KEEPALIVE",
        ConstVal::Int(c::CURLOPT_TCP_KEEPALIVE),
    ),
    (
        "CURLOPT_TCP_KEEPIDLE",
        ConstVal::Int(c::CURLOPT_TCP_KEEPIDLE),
    ),
    (
        "CURLOPT_TCP_KEEPINTVL",
        ConstVal::Int(c::CURLOPT_TCP_KEEPINTVL),
    ),
    (
        "CURLSSLOPT_ALLOW_BEAST",
        ConstVal::Int(c::CURLSSLOPT_ALLOW_BEAST),
    ),
    ("CURL_REDIR_POST_303", ConstVal::Int(c::CURL_REDIR_POST_303)),
    ("CURLSSH_AUTH_AGENT", ConstVal::Int(c::CURLSSH_AUTH_AGENT)),
    (
        "CURLMOPT_CHUNK_LENGTH_PENALTY_SIZE",
        ConstVal::Int(c::CURLMOPT_CHUNK_LENGTH_PENALTY_SIZE),
    ),
    (
        "CURLMOPT_CONTENT_LENGTH_PENALTY_SIZE",
        ConstVal::Int(c::CURLMOPT_CONTENT_LENGTH_PENALTY_SIZE),
    ),
    (
        "CURLMOPT_MAX_HOST_CONNECTIONS",
        ConstVal::Int(c::CURLMOPT_MAX_HOST_CONNECTIONS),
    ),
    (
        "CURLMOPT_MAX_PIPELINE_LENGTH",
        ConstVal::Int(c::CURLMOPT_MAX_PIPELINE_LENGTH),
    ),
    (
        "CURLMOPT_MAX_TOTAL_CONNECTIONS",
        ConstVal::Int(c::CURLMOPT_MAX_TOTAL_CONNECTIONS),
    ),
    ("CURLOPT_SASL_IR", ConstVal::Int(c::CURLOPT_SASL_IR)),
    (
        "CURLOPT_DNS_INTERFACE",
        ConstVal::Int(c::CURLOPT_DNS_INTERFACE),
    ),
    (
        "CURLOPT_DNS_LOCAL_IP4",
        ConstVal::Int(c::CURLOPT_DNS_LOCAL_IP4),
    ),
    (
        "CURLOPT_DNS_LOCAL_IP6",
        ConstVal::Int(c::CURLOPT_DNS_LOCAL_IP6),
    ),
    (
        "CURLOPT_XOAUTH2_BEARER",
        ConstVal::Int(c::CURLOPT_XOAUTH2_BEARER),
    ),
    (
        "CURL_HTTP_VERSION_2_0",
        ConstVal::Int(c::CURL_HTTP_VERSION_2_0),
    ),
    ("CURL_VERSION_HTTP2", ConstVal::Int(c::CURL_VERSION_HTTP2)),
    (
        "CURLOPT_LOGIN_OPTIONS",
        ConstVal::Int(c::CURLOPT_LOGIN_OPTIONS),
    ),
    (
        "CURL_SSLVERSION_TLSv1_0",
        ConstVal::Int(c::CURL_SSLVERSION_TLSv1_0),
    ),
    (
        "CURL_SSLVERSION_TLSv1_1",
        ConstVal::Int(c::CURL_SSLVERSION_TLSv1_1),
    ),
    (
        "CURL_SSLVERSION_TLSv1_2",
        ConstVal::Int(c::CURL_SSLVERSION_TLSv1_2),
    ),
    (
        "CURLOPT_EXPECT_100_TIMEOUT_MS",
        ConstVal::Int(c::CURLOPT_EXPECT_100_TIMEOUT_MS),
    ),
    (
        "CURLOPT_SSL_ENABLE_ALPN",
        ConstVal::Int(c::CURLOPT_SSL_ENABLE_ALPN),
    ),
    (
        "CURLOPT_SSL_ENABLE_NPN",
        ConstVal::Int(c::CURLOPT_SSL_ENABLE_NPN),
    ),
    ("CURLHEADER_SEPARATE", ConstVal::Int(c::CURLHEADER_SEPARATE)),
    ("CURLHEADER_UNIFIED", ConstVal::Int(c::CURLHEADER_UNIFIED)),
    ("CURLOPT_HEADEROPT", ConstVal::Int(c::CURLOPT_HEADEROPT)),
    ("CURLOPT_PROXYHEADER", ConstVal::Int(c::CURLOPT_PROXYHEADER)),
    ("CURLAUTH_NEGOTIATE", ConstVal::Int(c::CURLAUTH_NEGOTIATE)),
    ("CURL_VERSION_GSSAPI", ConstVal::Int(c::CURL_VERSION_GSSAPI)),
    (
        "CURLOPT_PINNEDPUBLICKEY",
        ConstVal::Int(c::CURLOPT_PINNEDPUBLICKEY),
    ),
    (
        "CURLOPT_UNIX_SOCKET_PATH",
        ConstVal::Int(c::CURLOPT_UNIX_SOCKET_PATH),
    ),
    ("CURLPROTO_SMB", ConstVal::Int(c::CURLPROTO_SMB)),
    ("CURLPROTO_SMBS", ConstVal::Int(c::CURLPROTO_SMBS)),
    (
        "CURL_VERSION_KERBEROS5",
        ConstVal::Int(c::CURL_VERSION_KERBEROS5),
    ),
    (
        "CURL_VERSION_UNIX_SOCKETS",
        ConstVal::Int(c::CURL_VERSION_UNIX_SOCKETS),
    ),
    (
        "CURLOPT_SSL_VERIFYSTATUS",
        ConstVal::Int(c::CURLOPT_SSL_VERIFYSTATUS),
    ),
    ("CURLOPT_PATH_AS_IS", ConstVal::Int(c::CURLOPT_PATH_AS_IS)),
    (
        "CURLOPT_SSL_FALSESTART",
        ConstVal::Int(c::CURLOPT_SSL_FALSESTART),
    ),
    ("CURL_HTTP_VERSION_2", ConstVal::Int(c::CURL_HTTP_VERSION_2)),
    ("CURLOPT_PIPEWAIT", ConstVal::Int(c::CURLOPT_PIPEWAIT)),
    (
        "CURLOPT_PROXY_SERVICE_NAME",
        ConstVal::Int(c::CURLOPT_PROXY_SERVICE_NAME),
    ),
    (
        "CURLOPT_SERVICE_NAME",
        ConstVal::Int(c::CURLOPT_SERVICE_NAME),
    ),
    ("CURLPIPE_NOTHING", ConstVal::Int(c::CURLPIPE_NOTHING)),
    ("CURLPIPE_HTTP1", ConstVal::Int(c::CURLPIPE_HTTP1)),
    ("CURLPIPE_MULTIPLEX", ConstVal::Int(c::CURLPIPE_MULTIPLEX)),
    (
        "CURLSSLOPT_NO_REVOKE",
        ConstVal::Int(c::CURLSSLOPT_NO_REVOKE),
    ),
    (
        "CURLOPT_DEFAULT_PROTOCOL",
        ConstVal::Int(c::CURLOPT_DEFAULT_PROTOCOL),
    ),
    (
        "CURLOPT_STREAM_WEIGHT",
        ConstVal::Int(c::CURLOPT_STREAM_WEIGHT),
    ),
    (
        "CURLMOPT_PUSHFUNCTION",
        ConstVal::Int(c::CURLMOPT_PUSHFUNCTION),
    ),
    ("CURL_PUSH_OK", ConstVal::Int(c::CURL_PUSH_OK)),
    ("CURL_PUSH_DENY", ConstVal::Int(c::CURL_PUSH_DENY)),
    (
        "CURL_HTTP_VERSION_2TLS",
        ConstVal::Int(c::CURL_HTTP_VERSION_2TLS),
    ),
    ("CURL_VERSION_PSL", ConstVal::Int(c::CURL_VERSION_PSL)),
    (
        "CURLOPT_TFTP_NO_OPTIONS",
        ConstVal::Int(c::CURLOPT_TFTP_NO_OPTIONS),
    ),
    (
        "CURL_HTTP_VERSION_2_PRIOR_KNOWLEDGE",
        ConstVal::Int(c::CURL_HTTP_VERSION_2_PRIOR_KNOWLEDGE),
    ),
    ("CURLOPT_CONNECT_TO", ConstVal::Int(c::CURLOPT_CONNECT_TO)),
    (
        "CURLOPT_TCP_FASTOPEN",
        ConstVal::Int(c::CURLOPT_TCP_FASTOPEN),
    ),
    (
        "CURLINFO_HTTP_VERSION",
        ConstVal::Int(c::CURLINFO_HTTP_VERSION),
    ),
    (
        "CURLE_WEIRD_SERVER_REPLY",
        ConstVal::Int(c::CURLE_WEIRD_SERVER_REPLY),
    ),
    (
        "CURLOPT_KEEP_SENDING_ON_ERROR",
        ConstVal::Int(c::CURLOPT_KEEP_SENDING_ON_ERROR),
    ),
    (
        "CURL_SSLVERSION_TLSv1_3",
        ConstVal::Int(c::CURL_SSLVERSION_TLSv1_3),
    ),
    (
        "CURL_VERSION_HTTPS_PROXY",
        ConstVal::Int(c::CURL_VERSION_HTTPS_PROXY),
    ),
    ("CURLINFO_PROTOCOL", ConstVal::Int(c::CURLINFO_PROTOCOL)),
    (
        "CURLINFO_PROXY_SSL_VERIFYRESULT",
        ConstVal::Int(c::CURLINFO_PROXY_SSL_VERIFYRESULT),
    ),
    ("CURLINFO_SCHEME", ConstVal::Int(c::CURLINFO_SCHEME)),
    ("CURLOPT_PRE_PROXY", ConstVal::Int(c::CURLOPT_PRE_PROXY)),
    (
        "CURLOPT_PROXY_CAINFO",
        ConstVal::Int(c::CURLOPT_PROXY_CAINFO),
    ),
    (
        "CURLOPT_PROXY_CAPATH",
        ConstVal::Int(c::CURLOPT_PROXY_CAPATH),
    ),
    (
        "CURLOPT_PROXY_CRLFILE",
        ConstVal::Int(c::CURLOPT_PROXY_CRLFILE),
    ),
    (
        "CURLOPT_PROXY_KEYPASSWD",
        ConstVal::Int(c::CURLOPT_PROXY_KEYPASSWD),
    ),
    (
        "CURLOPT_PROXY_PINNEDPUBLICKEY",
        ConstVal::Int(c::CURLOPT_PROXY_PINNEDPUBLICKEY),
    ),
    (
        "CURLOPT_PROXY_SSL_CIPHER_LIST",
        ConstVal::Int(c::CURLOPT_PROXY_SSL_CIPHER_LIST),
    ),
    (
        "CURLOPT_PROXY_SSL_OPTIONS",
        ConstVal::Int(c::CURLOPT_PROXY_SSL_OPTIONS),
    ),
    (
        "CURLOPT_PROXY_SSL_VERIFYHOST",
        ConstVal::Int(c::CURLOPT_PROXY_SSL_VERIFYHOST),
    ),
    (
        "CURLOPT_PROXY_SSL_VERIFYPEER",
        ConstVal::Int(c::CURLOPT_PROXY_SSL_VERIFYPEER),
    ),
    (
        "CURLOPT_PROXY_SSLCERT",
        ConstVal::Int(c::CURLOPT_PROXY_SSLCERT),
    ),
    (
        "CURLOPT_PROXY_SSLCERTTYPE",
        ConstVal::Int(c::CURLOPT_PROXY_SSLCERTTYPE),
    ),
    (
        "CURLOPT_PROXY_SSLKEY",
        ConstVal::Int(c::CURLOPT_PROXY_SSLKEY),
    ),
    (
        "CURLOPT_PROXY_SSLKEYTYPE",
        ConstVal::Int(c::CURLOPT_PROXY_SSLKEYTYPE),
    ),
    (
        "CURLOPT_PROXY_SSLVERSION",
        ConstVal::Int(c::CURLOPT_PROXY_SSLVERSION),
    ),
    (
        "CURLOPT_PROXY_TLSAUTH_PASSWORD",
        ConstVal::Int(c::CURLOPT_PROXY_TLSAUTH_PASSWORD),
    ),
    (
        "CURLOPT_PROXY_TLSAUTH_TYPE",
        ConstVal::Int(c::CURLOPT_PROXY_TLSAUTH_TYPE),
    ),
    (
        "CURLOPT_PROXY_TLSAUTH_USERNAME",
        ConstVal::Int(c::CURLOPT_PROXY_TLSAUTH_USERNAME),
    ),
    ("CURLPROXY_HTTPS", ConstVal::Int(c::CURLPROXY_HTTPS)),
    ("CURL_MAX_READ_SIZE", ConstVal::Int(c::CURL_MAX_READ_SIZE)),
    (
        "CURLOPT_ABSTRACT_UNIX_SOCKET",
        ConstVal::Int(c::CURLOPT_ABSTRACT_UNIX_SOCKET),
    ),
    (
        "CURL_SSLVERSION_MAX_DEFAULT",
        ConstVal::Int(c::CURL_SSLVERSION_MAX_DEFAULT),
    ),
    (
        "CURL_SSLVERSION_MAX_NONE",
        ConstVal::Int(c::CURL_SSLVERSION_MAX_NONE),
    ),
    (
        "CURL_SSLVERSION_MAX_TLSv1_0",
        ConstVal::Int(c::CURL_SSLVERSION_MAX_TLSv1_0),
    ),
    (
        "CURL_SSLVERSION_MAX_TLSv1_1",
        ConstVal::Int(c::CURL_SSLVERSION_MAX_TLSv1_1),
    ),
    (
        "CURL_SSLVERSION_MAX_TLSv1_2",
        ConstVal::Int(c::CURL_SSLVERSION_MAX_TLSv1_2),
    ),
    (
        "CURL_SSLVERSION_MAX_TLSv1_3",
        ConstVal::Int(c::CURL_SSLVERSION_MAX_TLSv1_3),
    ),
    (
        "CURLOPT_SUPPRESS_CONNECT_HEADERS",
        ConstVal::Int(c::CURLOPT_SUPPRESS_CONNECT_HEADERS),
    ),
    ("CURLAUTH_GSSAPI", ConstVal::Int(c::CURLAUTH_GSSAPI)),
    (
        "CURLINFO_CONTENT_LENGTH_DOWNLOAD_T",
        ConstVal::Int(c::CURLINFO_CONTENT_LENGTH_DOWNLOAD_T),
    ),
    (
        "CURLINFO_CONTENT_LENGTH_UPLOAD_T",
        ConstVal::Int(c::CURLINFO_CONTENT_LENGTH_UPLOAD_T),
    ),
    (
        "CURLINFO_SIZE_DOWNLOAD_T",
        ConstVal::Int(c::CURLINFO_SIZE_DOWNLOAD_T),
    ),
    (
        "CURLINFO_SIZE_UPLOAD_T",
        ConstVal::Int(c::CURLINFO_SIZE_UPLOAD_T),
    ),
    (
        "CURLINFO_SPEED_DOWNLOAD_T",
        ConstVal::Int(c::CURLINFO_SPEED_DOWNLOAD_T),
    ),
    (
        "CURLINFO_SPEED_UPLOAD_T",
        ConstVal::Int(c::CURLINFO_SPEED_UPLOAD_T),
    ),
    (
        "CURLOPT_REQUEST_TARGET",
        ConstVal::Int(c::CURLOPT_REQUEST_TARGET),
    ),
    ("CURLOPT_SOCKS5_AUTH", ConstVal::Int(c::CURLOPT_SOCKS5_AUTH)),
    (
        "CURLOPT_SSH_COMPRESSION",
        ConstVal::Int(c::CURLOPT_SSH_COMPRESSION),
    ),
    (
        "CURL_VERSION_MULTI_SSL",
        ConstVal::Int(c::CURL_VERSION_MULTI_SSL),
    ),
    ("CURL_VERSION_BROTLI", ConstVal::Int(c::CURL_VERSION_BROTLI)),
    (
        "CURL_LOCK_DATA_CONNECT",
        ConstVal::Int(c::CURL_LOCK_DATA_CONNECT),
    ),
    ("CURLSSH_AUTH_GSSAPI", ConstVal::Int(c::CURLSSH_AUTH_GSSAPI)),
    ("CURLINFO_FILETIME_T", ConstVal::Int(c::CURLINFO_FILETIME_T)),
    (
        "CURLOPT_HAPPY_EYEBALLS_TIMEOUT_MS",
        ConstVal::Int(c::CURLOPT_HAPPY_EYEBALLS_TIMEOUT_MS),
    ),
    (
        "CURLOPT_TIMEVALUE_LARGE",
        ConstVal::Int(c::CURLOPT_TIMEVALUE_LARGE),
    ),
    (
        "CURLOPT_DNS_SHUFFLE_ADDRESSES",
        ConstVal::Int(c::CURLOPT_DNS_SHUFFLE_ADDRESSES),
    ),
    (
        "CURLOPT_HAPROXYPROTOCOL",
        ConstVal::Int(c::CURLOPT_HAPROXYPROTOCOL),
    ),
    ("CURL_LOCK_DATA_PSL", ConstVal::Int(c::CURL_LOCK_DATA_PSL)),
    ("CURLAUTH_BEARER", ConstVal::Int(c::CURLAUTH_BEARER)),
    (
        "CURLINFO_QUEUE_TIME_T",
        ConstVal::Int(c::CURLINFO_QUEUE_TIME_T),
    ),
    (
        "CURLINFO_APPCONNECT_TIME_T",
        ConstVal::Int(c::CURLINFO_APPCONNECT_TIME_T),
    ),
    (
        "CURLINFO_CONNECT_TIME_T",
        ConstVal::Int(c::CURLINFO_CONNECT_TIME_T),
    ),
    (
        "CURLINFO_NAMELOOKUP_TIME_T",
        ConstVal::Int(c::CURLINFO_NAMELOOKUP_TIME_T),
    ),
    (
        "CURLINFO_PRETRANSFER_TIME_T",
        ConstVal::Int(c::CURLINFO_PRETRANSFER_TIME_T),
    ),
    (
        "CURLINFO_REDIRECT_TIME_T",
        ConstVal::Int(c::CURLINFO_REDIRECT_TIME_T),
    ),
    (
        "CURLINFO_STARTTRANSFER_TIME_T",
        ConstVal::Int(c::CURLINFO_STARTTRANSFER_TIME_T),
    ),
    (
        "CURLINFO_TOTAL_TIME_T",
        ConstVal::Int(c::CURLINFO_TOTAL_TIME_T),
    ),
    ("CURLINFO_USED_PROXY", ConstVal::Int(c::CURLINFO_USED_PROXY)),
    (
        "CURLINFO_POSTTRANSFER_TIME_T",
        ConstVal::Int(c::CURLINFO_POSTTRANSFER_TIME_T),
    ),
    ("CURLINFO_CONN_ID", ConstVal::Int(c::CURLINFO_CONN_ID)),
    (
        "CURLOPT_DISALLOW_USERNAME_IN_URL",
        ConstVal::Int(c::CURLOPT_DISALLOW_USERNAME_IN_URL),
    ),
    (
        "CURLOPT_PROXY_TLS13_CIPHERS",
        ConstVal::Int(c::CURLOPT_PROXY_TLS13_CIPHERS),
    ),
    (
        "CURLOPT_TLS13_CIPHERS",
        ConstVal::Int(c::CURLOPT_TLS13_CIPHERS),
    ),
    ("CURLOPT_DOH_URL", ConstVal::Int(c::CURLOPT_DOH_URL)),
    (
        "CURLOPT_UPKEEP_INTERVAL_MS",
        ConstVal::Int(c::CURLOPT_UPKEEP_INTERVAL_MS),
    ),
    (
        "CURLOPT_UPLOAD_BUFFERSIZE",
        ConstVal::Int(c::CURLOPT_UPLOAD_BUFFERSIZE),
    ),
    (
        "CURLOPT_HTTP09_ALLOWED",
        ConstVal::Int(c::CURLOPT_HTTP09_ALLOWED),
    ),
    ("CURLALTSVC_H1", ConstVal::Int(c::CURLALTSVC_H1)),
    ("CURLALTSVC_H2", ConstVal::Int(c::CURLALTSVC_H2)),
    ("CURLALTSVC_H3", ConstVal::Int(c::CURLALTSVC_H3)),
    (
        "CURLALTSVC_READONLYFILE",
        ConstVal::Int(c::CURLALTSVC_READONLYFILE),
    ),
    ("CURLOPT_ALTSVC", ConstVal::Int(c::CURLOPT_ALTSVC)),
    ("CURLOPT_ALTSVC_CTRL", ConstVal::Int(c::CURLOPT_ALTSVC_CTRL)),
    ("CURL_VERSION_ALTSVC", ConstVal::Int(c::CURL_VERSION_ALTSVC)),
    ("CURLOPT_MAXAGE_CONN", ConstVal::Int(c::CURLOPT_MAXAGE_CONN)),
    (
        "CURLOPT_SASL_AUTHZID",
        ConstVal::Int(c::CURLOPT_SASL_AUTHZID),
    ),
    ("CURL_VERSION_HTTP3", ConstVal::Int(c::CURL_VERSION_HTTP3)),
    (
        "CURLINFO_RETRY_AFTER",
        ConstVal::Int(c::CURLINFO_RETRY_AFTER),
    ),
    ("CURL_HTTP_VERSION_3", ConstVal::Int(c::CURL_HTTP_VERSION_3)),
    (
        "CURLMOPT_MAX_CONCURRENT_STREAMS",
        ConstVal::Int(c::CURLMOPT_MAX_CONCURRENT_STREAMS),
    ),
    (
        "CURLSSLOPT_NO_PARTIALCHAIN",
        ConstVal::Int(c::CURLSSLOPT_NO_PARTIALCHAIN),
    ),
    (
        "CURLOPT_MAIL_RCPT_ALLLOWFAILS",
        ConstVal::Int(c::CURLOPT_MAIL_RCPT_ALLLOWFAILS),
    ),
    (
        "CURLSSLOPT_REVOKE_BEST_EFFORT",
        ConstVal::Int(c::CURLSSLOPT_REVOKE_BEST_EFFORT),
    ),
    (
        "CURLOPT_ISSUERCERT_BLOB",
        ConstVal::Int(c::CURLOPT_ISSUERCERT_BLOB),
    ),
    (
        "CURLOPT_PROXY_ISSUERCERT",
        ConstVal::Int(c::CURLOPT_PROXY_ISSUERCERT),
    ),
    (
        "CURLOPT_PROXY_ISSUERCERT_BLOB",
        ConstVal::Int(c::CURLOPT_PROXY_ISSUERCERT_BLOB),
    ),
    (
        "CURLOPT_PROXY_SSLCERT_BLOB",
        ConstVal::Int(c::CURLOPT_PROXY_SSLCERT_BLOB),
    ),
    (
        "CURLOPT_PROXY_SSLKEY_BLOB",
        ConstVal::Int(c::CURLOPT_PROXY_SSLKEY_BLOB),
    ),
    (
        "CURLOPT_SSLCERT_BLOB",
        ConstVal::Int(c::CURLOPT_SSLCERT_BLOB),
    ),
    ("CURLOPT_SSLKEY_BLOB", ConstVal::Int(c::CURLOPT_SSLKEY_BLOB)),
    ("CURLPROTO_MQTT", ConstVal::Int(c::CURLPROTO_MQTT)),
    (
        "CURLSSLOPT_NATIVE_CA",
        ConstVal::Int(c::CURLSSLOPT_NATIVE_CA),
    ),
    (
        "CURL_VERSION_UNICODE",
        ConstVal::Int(c::CURL_VERSION_UNICODE),
    ),
    ("CURL_VERSION_ZSTD", ConstVal::Int(c::CURL_VERSION_ZSTD)),
    ("CURLE_PROXY", ConstVal::Int(c::CURLE_PROXY)),
    (
        "CURLINFO_PROXY_ERROR",
        ConstVal::Int(c::CURLINFO_PROXY_ERROR),
    ),
    (
        "CURLOPT_SSL_EC_CURVES",
        ConstVal::Int(c::CURLOPT_SSL_EC_CURVES),
    ),
    (
        "CURLOPT_SSL_SIGNATURE_ALGORITHMS",
        ConstVal::Int(c::CURLOPT_SSL_SIGNATURE_ALGORITHMS),
    ),
    (
        "CURLPX_BAD_ADDRESS_TYPE",
        ConstVal::Int(c::CURLPX_BAD_ADDRESS_TYPE),
    ),
    ("CURLPX_BAD_VERSION", ConstVal::Int(c::CURLPX_BAD_VERSION)),
    ("CURLPX_CLOSED", ConstVal::Int(c::CURLPX_CLOSED)),
    ("CURLPX_GSSAPI", ConstVal::Int(c::CURLPX_GSSAPI)),
    (
        "CURLPX_GSSAPI_PERMSG",
        ConstVal::Int(c::CURLPX_GSSAPI_PERMSG),
    ),
    (
        "CURLPX_GSSAPI_PROTECTION",
        ConstVal::Int(c::CURLPX_GSSAPI_PROTECTION),
    ),
    ("CURLPX_IDENTD", ConstVal::Int(c::CURLPX_IDENTD)),
    (
        "CURLPX_IDENTD_DIFFER",
        ConstVal::Int(c::CURLPX_IDENTD_DIFFER),
    ),
    (
        "CURLPX_LONG_HOSTNAME",
        ConstVal::Int(c::CURLPX_LONG_HOSTNAME),
    ),
    ("CURLPX_LONG_PASSWD", ConstVal::Int(c::CURLPX_LONG_PASSWD)),
    ("CURLPX_LONG_USER", ConstVal::Int(c::CURLPX_LONG_USER)),
    ("CURLPX_NO_AUTH", ConstVal::Int(c::CURLPX_NO_AUTH)),
    ("CURLPX_OK", ConstVal::Int(c::CURLPX_OK)),
    ("CURLPX_RECV_ADDRESS", ConstVal::Int(c::CURLPX_RECV_ADDRESS)),
    ("CURLPX_RECV_AUTH", ConstVal::Int(c::CURLPX_RECV_AUTH)),
    ("CURLPX_RECV_CONNECT", ConstVal::Int(c::CURLPX_RECV_CONNECT)),
    ("CURLPX_RECV_REQACK", ConstVal::Int(c::CURLPX_RECV_REQACK)),
    (
        "CURLPX_REPLY_ADDRESS_TYPE_NOT_SUPPORTED",
        ConstVal::Int(c::CURLPX_REPLY_ADDRESS_TYPE_NOT_SUPPORTED),
    ),
    (
        "CURLPX_REPLY_COMMAND_NOT_SUPPORTED",
        ConstVal::Int(c::CURLPX_REPLY_COMMAND_NOT_SUPPORTED),
    ),
    (
        "CURLPX_REPLY_CONNECTION_REFUSED",
        ConstVal::Int(c::CURLPX_REPLY_CONNECTION_REFUSED),
    ),
    (
        "CURLPX_REPLY_GENERAL_SERVER_FAILURE",
        ConstVal::Int(c::CURLPX_REPLY_GENERAL_SERVER_FAILURE),
    ),
    (
        "CURLPX_REPLY_HOST_UNREACHABLE",
        ConstVal::Int(c::CURLPX_REPLY_HOST_UNREACHABLE),
    ),
    (
        "CURLPX_REPLY_NETWORK_UNREACHABLE",
        ConstVal::Int(c::CURLPX_REPLY_NETWORK_UNREACHABLE),
    ),
    (
        "CURLPX_REPLY_NOT_ALLOWED",
        ConstVal::Int(c::CURLPX_REPLY_NOT_ALLOWED),
    ),
    (
        "CURLPX_REPLY_TTL_EXPIRED",
        ConstVal::Int(c::CURLPX_REPLY_TTL_EXPIRED),
    ),
    (
        "CURLPX_REPLY_UNASSIGNED",
        ConstVal::Int(c::CURLPX_REPLY_UNASSIGNED),
    ),
    (
        "CURLPX_REQUEST_FAILED",
        ConstVal::Int(c::CURLPX_REQUEST_FAILED),
    ),
    ("CURLPX_RESOLVE_HOST", ConstVal::Int(c::CURLPX_RESOLVE_HOST)),
    ("CURLPX_SEND_AUTH", ConstVal::Int(c::CURLPX_SEND_AUTH)),
    ("CURLPX_SEND_CONNECT", ConstVal::Int(c::CURLPX_SEND_CONNECT)),
    ("CURLPX_SEND_REQUEST", ConstVal::Int(c::CURLPX_SEND_REQUEST)),
    ("CURLPX_UNKNOWN_FAIL", ConstVal::Int(c::CURLPX_UNKNOWN_FAIL)),
    ("CURLPX_UNKNOWN_MODE", ConstVal::Int(c::CURLPX_UNKNOWN_MODE)),
    (
        "CURLPX_USER_REJECTED",
        ConstVal::Int(c::CURLPX_USER_REJECTED),
    ),
    ("CURLHSTS_ENABLE", ConstVal::Int(c::CURLHSTS_ENABLE)),
    (
        "CURLHSTS_READONLYFILE",
        ConstVal::Int(c::CURLHSTS_READONLYFILE),
    ),
    ("CURLOPT_HSTS", ConstVal::Int(c::CURLOPT_HSTS)),
    ("CURLOPT_HSTS_CTRL", ConstVal::Int(c::CURLOPT_HSTS_CTRL)),
    ("CURL_VERSION_HSTS", ConstVal::Int(c::CURL_VERSION_HSTS)),
    ("CURLAUTH_AWS_SIGV4", ConstVal::Int(c::CURLAUTH_AWS_SIGV4)),
    ("CURLOPT_AWS_SIGV4", ConstVal::Int(c::CURLOPT_AWS_SIGV4)),
    ("CURLINFO_REFERER", ConstVal::Int(c::CURLINFO_REFERER)),
    (
        "CURLOPT_DOH_SSL_VERIFYHOST",
        ConstVal::Int(c::CURLOPT_DOH_SSL_VERIFYHOST),
    ),
    (
        "CURLOPT_DOH_SSL_VERIFYPEER",
        ConstVal::Int(c::CURLOPT_DOH_SSL_VERIFYPEER),
    ),
    (
        "CURLOPT_DOH_SSL_VERIFYSTATUS",
        ConstVal::Int(c::CURLOPT_DOH_SSL_VERIFYSTATUS),
    ),
    ("CURL_VERSION_GSASL", ConstVal::Int(c::CURL_VERSION_GSASL)),
    ("CURLOPT_CAINFO_BLOB", ConstVal::Int(c::CURLOPT_CAINFO_BLOB)),
    (
        "CURLOPT_PROXY_CAINFO_BLOB",
        ConstVal::Int(c::CURLOPT_PROXY_CAINFO_BLOB),
    ),
    (
        "CURLSSLOPT_AUTO_CLIENT_CERT",
        ConstVal::Int(c::CURLSSLOPT_AUTO_CLIENT_CERT),
    ),
    (
        "CURLOPT_MAXLIFETIME_CONN",
        ConstVal::Int(c::CURLOPT_MAXLIFETIME_CONN),
    ),
    (
        "CURLOPT_SSH_HOST_PUBLIC_KEY_SHA256",
        ConstVal::Int(c::CURLOPT_SSH_HOST_PUBLIC_KEY_SHA256),
    ),
    (
        "CURLOPT_PREREQFUNCTION",
        ConstVal::Int(c::CURLOPT_PREREQFUNCTION),
    ),
    ("CURL_PREREQFUNC_OK", ConstVal::Int(c::CURL_PREREQFUNC_OK)),
    (
        "CURL_PREREQFUNC_ABORT",
        ConstVal::Int(c::CURL_PREREQFUNC_ABORT),
    ),
    (
        "CURLOPT_MIME_OPTIONS",
        ConstVal::Int(c::CURLOPT_MIME_OPTIONS),
    ),
    (
        "CURLMIMEOPT_FORMESCAPE",
        ConstVal::Int(c::CURLMIMEOPT_FORMESCAPE),
    ),
    (
        "CURLOPT_SSH_HOSTKEYFUNCTION",
        ConstVal::Int(c::CURLOPT_SSH_HOSTKEYFUNCTION),
    ),
    (
        "CURLOPT_PROTOCOLS_STR",
        ConstVal::Int(c::CURLOPT_PROTOCOLS_STR),
    ),
    (
        "CURLOPT_REDIR_PROTOCOLS_STR",
        ConstVal::Int(c::CURLOPT_REDIR_PROTOCOLS_STR),
    ),
    ("CURLOPT_WS_OPTIONS", ConstVal::Int(c::CURLOPT_WS_OPTIONS)),
    ("CURLWS_RAW_MODE", ConstVal::Int(c::CURLWS_RAW_MODE)),
    (
        "CURLOPT_CA_CACHE_TIMEOUT",
        ConstVal::Int(c::CURLOPT_CA_CACHE_TIMEOUT),
    ),
    ("CURLOPT_QUICK_EXIT", ConstVal::Int(c::CURLOPT_QUICK_EXIT)),
    (
        "CURL_HTTP_VERSION_3ONLY",
        ConstVal::Int(c::CURL_HTTP_VERSION_3ONLY),
    ),
    ("CURLOPT_SAFE_UPLOAD", ConstVal::Int(c::CURLOPT_SAFE_UPLOAD)),
];
