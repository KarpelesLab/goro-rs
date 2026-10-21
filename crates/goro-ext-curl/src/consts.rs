//! The constants ext/curl registers (curl_arginfo.h), with the values of
//! libcurl 8.14.1 (generated from the libcurl headers).

/// (name, value, deprecated)
pub static CONSTANTS: &[(&[u8], i64, bool)] = &[
    (b"CURLOPT_AUTOREFERER", 58, false),
    (b"CURLOPT_BINARYTRANSFER", 19914, true),
    (b"CURLOPT_BUFFERSIZE", 98, false),
    (b"CURLOPT_CAINFO", 10065, false),
    (b"CURLOPT_CAPATH", 10097, false),
    (b"CURLOPT_CONNECTTIMEOUT", 78, false),
    (b"CURLOPT_COOKIE", 10022, false),
    (b"CURLOPT_COOKIEFILE", 10031, false),
    (b"CURLOPT_COOKIEJAR", 10082, false),
    (b"CURLOPT_COOKIESESSION", 96, false),
    (b"CURLOPT_CRLF", 27, false),
    (b"CURLOPT_CUSTOMREQUEST", 10036, false),
    (b"CURLOPT_DNS_CACHE_TIMEOUT", 92, false),
    (b"CURLOPT_DNS_USE_GLOBAL_CACHE", 91, false),
    (b"CURLOPT_EGDSOCKET", 10077, false),
    (b"CURLOPT_ENCODING", 10102, false),
    (b"CURLOPT_FAILONERROR", 45, false),
    (b"CURLOPT_FILE", 10001, false),
    (b"CURLOPT_FILETIME", 69, false),
    (b"CURLOPT_FOLLOWLOCATION", 52, false),
    (b"CURLOPT_FORBID_REUSE", 75, false),
    (b"CURLOPT_FRESH_CONNECT", 74, false),
    (b"CURLOPT_FTPAPPEND", 50, false),
    (b"CURLOPT_FTPLISTONLY", 48, false),
    (b"CURLOPT_FTPPORT", 10017, false),
    (b"CURLOPT_FTP_USE_EPRT", 106, false),
    (b"CURLOPT_FTP_USE_EPSV", 85, false),
    (b"CURLOPT_HEADER", 42, false),
    (b"CURLOPT_HEADERFUNCTION", 20079, false),
    (b"CURLOPT_HTTP200ALIASES", 10104, false),
    (b"CURLOPT_HTTPGET", 80, false),
    (b"CURLOPT_HTTPHEADER", 10023, false),
    (b"CURLOPT_HTTPPROXYTUNNEL", 61, false),
    (b"CURLOPT_HTTP_VERSION", 84, false),
    (b"CURLOPT_INFILE", 10009, false),
    (b"CURLOPT_INFILESIZE", 14, false),
    (b"CURLOPT_INFILESIZE_LARGE", 30115, false),
    (b"CURLOPT_INTERFACE", 10062, false),
    (b"CURLOPT_KRB4LEVEL", 10063, false),
    (b"CURLOPT_LOW_SPEED_LIMIT", 19, false),
    (b"CURLOPT_LOW_SPEED_TIME", 20, false),
    (b"CURLOPT_MAXCONNECTS", 71, false),
    (b"CURLOPT_MAXREDIRS", 68, false),
    (b"CURLOPT_NETRC", 51, false),
    (b"CURLOPT_NOBODY", 44, false),
    (b"CURLOPT_NOPROGRESS", 43, false),
    (b"CURLOPT_NOSIGNAL", 99, false),
    (b"CURLOPT_PORT", 3, false),
    (b"CURLOPT_POST", 47, false),
    (b"CURLOPT_POSTFIELDS", 10015, false),
    (b"CURLOPT_POSTQUOTE", 10039, false),
    (b"CURLOPT_PREQUOTE", 10093, false),
    (b"CURLOPT_PRIVATE", 10103, false),
    (b"CURLOPT_PROGRESSFUNCTION", 20056, false),
    (b"CURLOPT_PROXY", 10004, false),
    (b"CURLOPT_PROXYPORT", 59, false),
    (b"CURLOPT_PROXYTYPE", 101, false),
    (b"CURLOPT_PROXYUSERPWD", 10006, false),
    (b"CURLOPT_PUT", 54, false),
    (b"CURLOPT_QUOTE", 10028, false),
    (b"CURLOPT_RANDOM_FILE", 10076, false),
    (b"CURLOPT_RANGE", 10007, false),
    (b"CURLOPT_READDATA", 10009, false),
    (b"CURLOPT_READFUNCTION", 20012, false),
    (b"CURLOPT_REFERER", 10016, false),
    (b"CURLOPT_RESUME_FROM", 21, false),
    (b"CURLOPT_RETURNTRANSFER", 19913, false),
    (b"CURLOPT_SHARE", 10100, false),
    (b"CURLOPT_SSLCERT", 10025, false),
    (b"CURLOPT_SSLCERTPASSWD", 10026, false),
    (b"CURLOPT_SSLCERTTYPE", 10086, false),
    (b"CURLOPT_SSLENGINE", 10089, false),
    (b"CURLOPT_SSLENGINE_DEFAULT", 90, false),
    (b"CURLOPT_SSLKEY", 10087, false),
    (b"CURLOPT_SSLKEYPASSWD", 10026, false),
    (b"CURLOPT_SSLKEYTYPE", 10088, false),
    (b"CURLOPT_SSLVERSION", 32, false),
    (b"CURLOPT_SSL_CIPHER_LIST", 10083, false),
    (b"CURLOPT_SSL_VERIFYHOST", 81, false),
    (b"CURLOPT_SSL_VERIFYPEER", 64, false),
    (b"CURLOPT_STDERR", 10037, false),
    (b"CURLOPT_TCP_KEEPCNT", 326, false),
    (b"CURLOPT_TELNETOPTIONS", 10070, false),
    (b"CURLOPT_TIMECONDITION", 33, false),
    (b"CURLOPT_TIMEOUT", 13, false),
    (b"CURLOPT_TIMEVALUE", 34, false),
    (b"CURLOPT_TRANSFERTEXT", 53, false),
    (b"CURLOPT_UNRESTRICTED_AUTH", 105, false),
    (b"CURLOPT_UPLOAD", 46, false),
    (b"CURLOPT_URL", 10002, false),
    (b"CURLOPT_USERAGENT", 10018, false),
    (b"CURLOPT_USERPWD", 10005, false),
    (b"CURLOPT_VERBOSE", 41, false),
    (b"CURLOPT_WRITEFUNCTION", 20011, false),
    (b"CURLOPT_WRITEHEADER", 10029, false),
    (b"CURLOPT_XFERINFOFUNCTION", 20219, false),
    (b"CURLOPT_DEBUGFUNCTION", 20094, false),
    (b"CURLFOLLOW_ALL", 1, false),
    (b"CURLFOLLOW_OBEYCODE", 2, false),
    (b"CURLFOLLOW_FIRSTONLY", 3, false),
    (b"CURLINFO_TEXT", 0, false),
    (b"CURLINFO_HEADER_IN", 1, false),
    (b"CURLINFO_DATA_IN", 3, false),
    (b"CURLINFO_DATA_OUT", 4, false),
    (b"CURLINFO_SSL_DATA_OUT", 6, false),
    (b"CURLINFO_SSL_DATA_IN", 5, false),
    (b"CURLE_ABORTED_BY_CALLBACK", 42, false),
    (b"CURLE_BAD_CALLING_ORDER", 44, false),
    (b"CURLE_BAD_CONTENT_ENCODING", 61, false),
    (b"CURLE_BAD_DOWNLOAD_RESUME", 36, false),
    (b"CURLE_BAD_FUNCTION_ARGUMENT", 43, false),
    (b"CURLE_BAD_PASSWORD_ENTERED", 46, false),
    (b"CURLE_COULDNT_CONNECT", 7, false),
    (b"CURLE_COULDNT_RESOLVE_HOST", 6, false),
    (b"CURLE_COULDNT_RESOLVE_PROXY", 5, false),
    (b"CURLE_FAILED_INIT", 2, false),
    (b"CURLE_FILE_COULDNT_READ_FILE", 37, false),
    (b"CURLE_FTP_ACCESS_DENIED", 9, false),
    (b"CURLE_FTP_BAD_DOWNLOAD_RESUME", 36, false),
    (b"CURLE_FTP_CANT_GET_HOST", 15, false),
    (b"CURLE_FTP_CANT_RECONNECT", 16, false),
    (b"CURLE_FTP_COULDNT_GET_SIZE", 32, false),
    (b"CURLE_FTP_COULDNT_RETR_FILE", 19, false),
    (b"CURLE_FTP_COULDNT_SET_ASCII", 29, false),
    (b"CURLE_FTP_COULDNT_SET_BINARY", 17, false),
    (b"CURLE_FTP_COULDNT_STOR_FILE", 25, false),
    (b"CURLE_FTP_COULDNT_USE_REST", 31, false),
    (b"CURLE_FTP_PARTIAL_FILE", 18, false),
    (b"CURLE_FTP_PORT_FAILED", 30, false),
    (b"CURLE_FTP_QUOTE_ERROR", 21, false),
    (b"CURLE_FTP_USER_PASSWORD_INCORRECT", 10, false),
    (b"CURLE_FTP_WEIRD_227_FORMAT", 14, false),
    (b"CURLE_FTP_WEIRD_PASS_REPLY", 11, false),
    (b"CURLE_FTP_WEIRD_PASV_REPLY", 13, false),
    (b"CURLE_FTP_WEIRD_SERVER_REPLY", 8, false),
    (b"CURLE_FTP_WEIRD_USER_REPLY", 12, false),
    (b"CURLE_FTP_WRITE_ERROR", 20, false),
    (b"CURLE_FUNCTION_NOT_FOUND", 41, false),
    (b"CURLE_GOT_NOTHING", 52, false),
    (b"CURLE_HTTP_NOT_FOUND", 22, false),
    (b"CURLE_HTTP_PORT_FAILED", 45, false),
    (b"CURLE_HTTP_POST_ERROR", 34, false),
    (b"CURLE_HTTP_RANGE_ERROR", 33, false),
    (b"CURLE_HTTP_RETURNED_ERROR", 22, false),
    (b"CURLE_LDAP_CANNOT_BIND", 38, false),
    (b"CURLE_LDAP_SEARCH_FAILED", 39, false),
    (b"CURLE_LIBRARY_NOT_FOUND", 40, false),
    (b"CURLE_MALFORMAT_USER", 24, false),
    (b"CURLE_OBSOLETE", 50, false),
    (b"CURLE_OK", 0, false),
    (b"CURLE_OPERATION_TIMEDOUT", 28, false),
    (b"CURLE_OPERATION_TIMEOUTED", 28, false),
    (b"CURLE_OUT_OF_MEMORY", 27, false),
    (b"CURLE_PARTIAL_FILE", 18, false),
    (b"CURLE_READ_ERROR", 26, false),
    (b"CURLE_RECV_ERROR", 56, false),
    (b"CURLE_SEND_ERROR", 55, false),
    (b"CURLE_SHARE_IN_USE", 57, false),
    (b"CURLE_SSL_CACERT", 60, false),
    (b"CURLE_SSL_CERTPROBLEM", 58, false),
    (b"CURLE_SSL_CIPHER", 59, false),
    (b"CURLE_SSL_CONNECT_ERROR", 35, false),
    (b"CURLE_SSL_ENGINE_NOTFOUND", 53, false),
    (b"CURLE_SSL_ENGINE_SETFAILED", 54, false),
    (b"CURLE_SSL_PEER_CERTIFICATE", 60, false),
    (b"CURLE_SSL_PINNEDPUBKEYNOTMATCH", 90, false),
    (b"CURLE_TELNET_OPTION_SYNTAX", 49, false),
    (b"CURLE_TOO_MANY_REDIRECTS", 47, false),
    (b"CURLE_UNKNOWN_TELNET_OPTION", 48, false),
    (b"CURLE_UNSUPPORTED_PROTOCOL", 1, false),
    (b"CURLE_URL_MALFORMAT", 3, false),
    (b"CURLE_URL_MALFORMAT_USER", 4, false),
    (b"CURLE_WRITE_ERROR", 23, false),
    (b"CURLINFO_CONNECT_TIME", 3145733, false),
    (b"CURLINFO_CONTENT_LENGTH_DOWNLOAD", 3145743, false),
    (b"CURLINFO_CONTENT_LENGTH_UPLOAD", 3145744, false),
    (b"CURLINFO_CONTENT_TYPE", 1048594, false),
    (b"CURLINFO_EFFECTIVE_URL", 1048577, false),
    (b"CURLINFO_FILETIME", 2097166, false),
    (b"CURLINFO_HEADER_OUT", 2, false),
    (b"CURLINFO_HEADER_SIZE", 2097163, false),
    (b"CURLINFO_HTTP_CODE", 2097154, false),
    (b"CURLINFO_LASTONE", 71, false),
    (b"CURLINFO_NAMELOOKUP_TIME", 3145732, false),
    (b"CURLINFO_PRETRANSFER_TIME", 3145734, false),
    (b"CURLINFO_PRIVATE", 1048597, false),
    (b"CURLINFO_REDIRECT_COUNT", 2097172, false),
    (b"CURLINFO_REDIRECT_TIME", 3145747, false),
    (b"CURLINFO_REQUEST_SIZE", 2097164, false),
    (b"CURLINFO_SIZE_DOWNLOAD", 3145736, false),
    (b"CURLINFO_SIZE_UPLOAD", 3145735, false),
    (b"CURLINFO_SPEED_DOWNLOAD", 3145737, false),
    (b"CURLINFO_SPEED_UPLOAD", 3145738, false),
    (b"CURLINFO_SSL_VERIFYRESULT", 2097165, false),
    (b"CURLINFO_STARTTRANSFER_TIME", 3145745, false),
    (b"CURLINFO_TOTAL_TIME", 3145731, false),
    (b"CURLINFO_EFFECTIVE_METHOD", 1048634, false),
    (b"CURLINFO_CAPATH", 1048638, false),
    (b"CURLINFO_CAINFO", 1048637, false),
    (b"CURLINFO_HTTPAUTH_USED", 2097221, false),
    (b"CURLINFO_PROXYAUTH_USED", 2097222, false),
    (b"CURLMSG_DONE", 1, false),
    (b"CURLVERSION_NOW", 11, false),
    (b"CURLM_BAD_EASY_HANDLE", 2, false),
    (b"CURLM_BAD_HANDLE", 1, false),
    (b"CURLM_CALL_MULTI_PERFORM", -1, false),
    (b"CURLM_INTERNAL_ERROR", 4, false),
    (b"CURLM_OK", 0, false),
    (b"CURLM_OUT_OF_MEMORY", 3, false),
    (b"CURLM_ADDED_ALREADY", 7, false),
    (b"CURLPROXY_HTTP", 0, false),
    (b"CURLPROXY_SOCKS4", 4, false),
    (b"CURLPROXY_SOCKS5", 5, false),
    (b"CURLSHOPT_NONE", 0, false),
    (b"CURLSHOPT_SHARE", 1, false),
    (b"CURLSHOPT_UNSHARE", 2, false),
    (b"CURL_HTTP_VERSION_1_0", 1, false),
    (b"CURL_HTTP_VERSION_1_1", 2, false),
    (b"CURL_HTTP_VERSION_NONE", 0, false),
    (b"CURL_LOCK_DATA_COOKIE", 2, false),
    (b"CURL_LOCK_DATA_DNS", 3, false),
    (b"CURL_LOCK_DATA_SSL_SESSION", 4, false),
    (b"CURL_NETRC_IGNORED", 0, false),
    (b"CURL_NETRC_OPTIONAL", 1, false),
    (b"CURL_NETRC_REQUIRED", 2, false),
    (b"CURL_SSLVERSION_DEFAULT", 0, false),
    (b"CURL_SSLVERSION_SSLv2", 2, false),
    (b"CURL_SSLVERSION_SSLv3", 3, false),
    (b"CURL_SSLVERSION_TLSv1", 1, false),
    (b"CURL_TIMECOND_IFMODSINCE", 1, false),
    (b"CURL_TIMECOND_IFUNMODSINCE", 2, false),
    (b"CURL_TIMECOND_LASTMOD", 3, false),
    (b"CURL_TIMECOND_NONE", 0, false),
    (b"CURL_VERSION_ASYNCHDNS", 128, false),
    (b"CURL_VERSION_CONV", 4096, false),
    (b"CURL_VERSION_DEBUG", 64, false),
    (b"CURL_VERSION_GSSNEGOTIATE", 32, false),
    (b"CURL_VERSION_IDN", 1024, false),
    (b"CURL_VERSION_IPV6", 1, false),
    (b"CURL_VERSION_KERBEROS4", 2, false),
    (b"CURL_VERSION_LARGEFILE", 512, false),
    (b"CURL_VERSION_LIBZ", 8, false),
    (b"CURL_VERSION_NTLM", 16, false),
    (b"CURL_VERSION_SPNEGO", 256, false),
    (b"CURL_VERSION_SSL", 4, false),
    (b"CURL_VERSION_SSPI", 2048, false),
    (b"CURLOPT_HTTPAUTH", 107, false),
    (b"CURLAUTH_ANY", 4294967279, false),
    (b"CURLAUTH_ANYSAFE", 4294967278, false),
    (b"CURLAUTH_BASIC", 1, false),
    (b"CURLAUTH_DIGEST", 2, false),
    (b"CURLAUTH_GSSNEGOTIATE", 4, false),
    (b"CURLAUTH_NONE", 0, false),
    (b"CURLAUTH_NTLM", 8, false),
    (b"CURLINFO_HTTP_CONNECTCODE", 2097174, false),
    (b"CURLOPT_FTP_CREATE_MISSING_DIRS", 110, false),
    (b"CURLOPT_PROXYAUTH", 111, false),
    (b"CURLE_FILESIZE_EXCEEDED", 63, false),
    (b"CURLE_LDAP_INVALID_URL", 62, false),
    (b"CURLINFO_HTTPAUTH_AVAIL", 2097175, false),
    (b"CURLINFO_RESPONSE_CODE", 2097154, false),
    (b"CURLINFO_PROXYAUTH_AVAIL", 2097176, false),
    (b"CURLOPT_FTP_RESPONSE_TIMEOUT", 112, false),
    (b"CURLOPT_SERVER_RESPONSE_TIMEOUT", 112, false),
    (b"CURLOPT_IPRESOLVE", 113, false),
    (b"CURLOPT_MAXFILESIZE", 114, false),
    (b"CURL_IPRESOLVE_V4", 1, false),
    (b"CURL_IPRESOLVE_V6", 2, false),
    (b"CURL_IPRESOLVE_WHATEVER", 0, false),
    (b"CURLE_FTP_SSL_FAILED", 64, false),
    (b"CURLFTPSSL_ALL", 3, false),
    (b"CURLFTPSSL_CONTROL", 2, false),
    (b"CURLFTPSSL_NONE", 0, false),
    (b"CURLFTPSSL_TRY", 1, false),
    (b"CURLOPT_FTP_SSL", 119, false),
    (b"CURLOPT_NETRC_FILE", 10118, false),
    (b"CURLOPT_MAXFILESIZE_LARGE", 30117, false),
    (b"CURLOPT_TCP_NODELAY", 121, false),
    (b"CURLFTPAUTH_DEFAULT", 0, false),
    (b"CURLFTPAUTH_SSL", 1, false),
    (b"CURLFTPAUTH_TLS", 2, false),
    (b"CURLOPT_FTPSSLAUTH", 129, false),
    (b"CURLOPT_FTP_ACCOUNT", 10134, false),
    (b"CURLINFO_OS_ERRNO", 2097177, false),
    (b"CURLINFO_NUM_CONNECTS", 2097178, false),
    (b"CURLINFO_SSL_ENGINES", 4194331, false),
    (b"CURLINFO_COOKIELIST", 4194332, false),
    (b"CURLOPT_COOKIELIST", 10135, false),
    (b"CURLOPT_IGNORE_CONTENT_LENGTH", 136, false),
    (b"CURLOPT_FTP_SKIP_PASV_IP", 137, false),
    (b"CURLOPT_FTP_FILEMETHOD", 138, false),
    (b"CURLOPT_CONNECT_ONLY", 141, false),
    (b"CURLOPT_LOCALPORT", 139, false),
    (b"CURLOPT_LOCALPORTRANGE", 140, false),
    (b"CURLFTPMETHOD_DEFAULT", 0, false),
    (b"CURLFTPMETHOD_MULTICWD", 1, false),
    (b"CURLFTPMETHOD_NOCWD", 2, false),
    (b"CURLFTPMETHOD_SINGLECWD", 3, false),
    (b"CURLINFO_FTP_ENTRY_PATH", 1048606, false),
    (b"CURLOPT_FTP_ALTERNATIVE_TO_USER", 10147, false),
    (b"CURLOPT_MAX_RECV_SPEED_LARGE", 30146, false),
    (b"CURLOPT_MAX_SEND_SPEED_LARGE", 30145, false),
    (b"CURLE_SSL_CACERT_BADFILE", 77, false),
    (b"CURLOPT_SSL_SESSIONID_CACHE", 150, false),
    (b"CURLMOPT_PIPELINING", 3, false),
    (b"CURLE_SSH", 79, false),
    (b"CURLOPT_FTP_SSL_CCC", 154, false),
    (b"CURLOPT_SSH_AUTH_TYPES", 151, false),
    (b"CURLOPT_SSH_PRIVATE_KEYFILE", 10153, false),
    (b"CURLOPT_SSH_PUBLIC_KEYFILE", 10152, false),
    (b"CURLFTPSSL_CCC_ACTIVE", 2, false),
    (b"CURLFTPSSL_CCC_NONE", 0, false),
    (b"CURLFTPSSL_CCC_PASSIVE", 1, false),
    (b"CURLOPT_CONNECTTIMEOUT_MS", 156, false),
    (b"CURLOPT_HTTP_CONTENT_DECODING", 158, false),
    (b"CURLOPT_HTTP_TRANSFER_DECODING", 157, false),
    (b"CURLOPT_TIMEOUT_MS", 155, false),
    (b"CURLMOPT_MAXCONNECTS", 6, false),
    (b"CURLOPT_KRBLEVEL", 10063, false),
    (b"CURLOPT_NEW_DIRECTORY_PERMS", 160, false),
    (b"CURLOPT_NEW_FILE_PERMS", 159, false),
    (b"CURLOPT_APPEND", 50, false),
    (b"CURLOPT_DIRLISTONLY", 48, false),
    (b"CURLOPT_USE_SSL", 119, false),
    (b"CURLUSESSL_ALL", 3, false),
    (b"CURLUSESSL_CONTROL", 2, false),
    (b"CURLUSESSL_NONE", 0, false),
    (b"CURLUSESSL_TRY", 1, false),
    (b"CURLOPT_SSH_HOST_PUBLIC_KEY_MD5", 10162, false),
    (b"CURLOPT_PROXY_TRANSFER_MODE", 166, false),
    (b"CURLPAUSE_ALL", 5, false),
    (b"CURLPAUSE_CONT", 0, false),
    (b"CURLPAUSE_RECV", 1, false),
    (b"CURLPAUSE_RECV_CONT", 0, false),
    (b"CURLPAUSE_SEND", 4, false),
    (b"CURLPAUSE_SEND_CONT", 0, false),
    (b"CURL_READFUNC_PAUSE", 268435457, false),
    (b"CURL_WRITEFUNC_PAUSE", 268435457, false),
    (b"CURLPROXY_SOCKS4A", 6, false),
    (b"CURLPROXY_SOCKS5_HOSTNAME", 7, false),
    (b"CURLINFO_REDIRECT_URL", 1048607, false),
    (b"CURLINFO_APPCONNECT_TIME", 3145761, false),
    (b"CURLINFO_PRIMARY_IP", 1048608, false),
    (b"CURLOPT_ADDRESS_SCOPE", 171, false),
    (b"CURLOPT_CRLFILE", 10169, false),
    (b"CURLOPT_ISSUERCERT", 10170, false),
    (b"CURLOPT_KEYPASSWD", 10026, false),
    (b"CURLSSH_AUTH_ANY", 4294967295, false),
    (b"CURLSSH_AUTH_DEFAULT", 4294967295, false),
    (b"CURLSSH_AUTH_HOST", 4, false),
    (b"CURLSSH_AUTH_KEYBOARD", 8, false),
    (b"CURLSSH_AUTH_NONE", 0, false),
    (b"CURLSSH_AUTH_PASSWORD", 2, false),
    (b"CURLSSH_AUTH_PUBLICKEY", 1, false),
    (b"CURLINFO_CERTINFO", 4194338, false),
    (b"CURLOPT_CERTINFO", 172, false),
    (b"CURLOPT_PASSWORD", 10174, false),
    (b"CURLOPT_POSTREDIR", 161, false),
    (b"CURLOPT_PROXYPASSWORD", 10176, false),
    (b"CURLOPT_PROXYUSERNAME", 10175, false),
    (b"CURLOPT_USERNAME", 10173, false),
    (b"CURL_REDIR_POST_301", 1, false),
    (b"CURL_REDIR_POST_302", 2, false),
    (b"CURL_REDIR_POST_ALL", 7, false),
    (b"CURLAUTH_DIGEST_IE", 16, false),
    (b"CURLINFO_CONDITION_UNMET", 2097187, false),
    (b"CURLOPT_NOPROXY", 10177, false),
    (b"CURLOPT_PROTOCOLS", 181, false),
    (b"CURLOPT_REDIR_PROTOCOLS", 182, false),
    (b"CURLOPT_SOCKS5_GSSAPI_NEC", 180, false),
    (b"CURLOPT_SOCKS5_GSSAPI_SERVICE", 10179, false),
    (b"CURLOPT_TFTP_BLKSIZE", 178, false),
    (b"CURLPROTO_ALL", 4294967295, false),
    (b"CURLPROTO_DICT", 512, false),
    (b"CURLPROTO_FILE", 1024, false),
    (b"CURLPROTO_FTP", 4, false),
    (b"CURLPROTO_FTPS", 8, false),
    (b"CURLPROTO_HTTP", 1, false),
    (b"CURLPROTO_HTTPS", 2, false),
    (b"CURLPROTO_LDAP", 128, false),
    (b"CURLPROTO_LDAPS", 256, false),
    (b"CURLPROTO_SCP", 16, false),
    (b"CURLPROTO_SFTP", 32, false),
    (b"CURLPROTO_TELNET", 64, false),
    (b"CURLPROTO_TFTP", 2048, false),
    (b"CURLPROXY_HTTP_1_0", 1, false),
    (b"CURLFTP_CREATE_DIR", 1, false),
    (b"CURLFTP_CREATE_DIR_NONE", 0, false),
    (b"CURLFTP_CREATE_DIR_RETRY", 2, false),
    (b"CURL_VERSION_CURLDEBUG", 8192, false),
    (b"CURLOPT_SSH_KNOWNHOSTS", 10183, false),
    (b"CURLKHMATCH_OK", 0, false),
    (b"CURLKHMATCH_MISMATCH", 1, false),
    (b"CURLKHMATCH_MISSING", 2, false),
    (b"CURLKHMATCH_LAST", 3, false),
    (b"CURLINFO_RTSP_CLIENT_CSEQ", 2097189, false),
    (b"CURLINFO_RTSP_CSEQ_RECV", 2097191, false),
    (b"CURLINFO_RTSP_SERVER_CSEQ", 2097190, false),
    (b"CURLINFO_RTSP_SESSION_ID", 1048612, false),
    (b"CURLOPT_FTP_USE_PRET", 188, false),
    (b"CURLOPT_MAIL_FROM", 10186, false),
    (b"CURLOPT_MAIL_RCPT", 10187, false),
    (b"CURLOPT_RTSP_CLIENT_CSEQ", 193, false),
    (b"CURLOPT_RTSP_REQUEST", 189, false),
    (b"CURLOPT_RTSP_SERVER_CSEQ", 194, false),
    (b"CURLOPT_RTSP_SESSION_ID", 10190, false),
    (b"CURLOPT_RTSP_STREAM_URI", 10191, false),
    (b"CURLOPT_RTSP_TRANSPORT", 10192, false),
    (b"CURLPROTO_IMAP", 4096, false),
    (b"CURLPROTO_IMAPS", 8192, false),
    (b"CURLPROTO_POP3", 16384, false),
    (b"CURLPROTO_POP3S", 32768, false),
    (b"CURLPROTO_RTSP", 262144, false),
    (b"CURLPROTO_SMTP", 65536, false),
    (b"CURLPROTO_SMTPS", 131072, false),
    (b"CURL_RTSPREQ_ANNOUNCE", 3, false),
    (b"CURL_RTSPREQ_DESCRIBE", 2, false),
    (b"CURL_RTSPREQ_GET_PARAMETER", 8, false),
    (b"CURL_RTSPREQ_OPTIONS", 1, false),
    (b"CURL_RTSPREQ_PAUSE", 6, false),
    (b"CURL_RTSPREQ_PLAY", 5, false),
    (b"CURL_RTSPREQ_RECEIVE", 11, false),
    (b"CURL_RTSPREQ_RECORD", 10, false),
    (b"CURL_RTSPREQ_SET_PARAMETER", 9, false),
    (b"CURL_RTSPREQ_SETUP", 4, false),
    (b"CURL_RTSPREQ_TEARDOWN", 7, false),
    (b"CURLINFO_LOCAL_IP", 1048617, false),
    (b"CURLINFO_LOCAL_PORT", 2097194, false),
    (b"CURLINFO_PRIMARY_PORT", 2097192, false),
    (b"CURLOPT_FNMATCH_FUNCTION", 20200, false),
    (b"CURLOPT_WILDCARDMATCH", 197, false),
    (b"CURLPROTO_RTMP", 524288, false),
    (b"CURLPROTO_RTMPE", 2097152, false),
    (b"CURLPROTO_RTMPS", 8388608, false),
    (b"CURLPROTO_RTMPT", 1048576, false),
    (b"CURLPROTO_RTMPTE", 4194304, false),
    (b"CURLPROTO_RTMPTS", 16777216, false),
    (b"CURL_FNMATCHFUNC_FAIL", 2, false),
    (b"CURL_FNMATCHFUNC_MATCH", 0, false),
    (b"CURL_FNMATCHFUNC_NOMATCH", 1, false),
    (b"CURLPROTO_GOPHER", 33554432, false),
    (b"CURLAUTH_ONLY", 2147483648, false),
    (b"CURLOPT_RESOLVE", 10203, false),
    (b"CURLOPT_TLSAUTH_PASSWORD", 10205, false),
    (b"CURLOPT_TLSAUTH_TYPE", 10206, false),
    (b"CURLOPT_TLSAUTH_USERNAME", 10204, false),
    (b"CURL_TLSAUTH_SRP", 1, false),
    (b"CURL_VERSION_TLSAUTH_SRP", 16384, false),
    (b"CURLOPT_ACCEPT_ENCODING", 10102, false),
    (b"CURLOPT_TRANSFER_ENCODING", 207, false),
    (b"CURLAUTH_NTLM_WB", 32, false),
    (b"CURLGSSAPI_DELEGATION_FLAG", 2, false),
    (b"CURLGSSAPI_DELEGATION_POLICY_FLAG", 1, false),
    (b"CURLOPT_GSSAPI_DELEGATION", 210, false),
    (b"CURL_VERSION_NTLM_WB", 32768, false),
    (b"CURLOPT_ACCEPTTIMEOUT_MS", 212, false),
    (b"CURLOPT_DNS_SERVERS", 10211, false),
    (b"CURLOPT_MAIL_AUTH", 10217, false),
    (b"CURLOPT_SSL_OPTIONS", 216, false),
    (b"CURLOPT_TCP_KEEPALIVE", 213, false),
    (b"CURLOPT_TCP_KEEPIDLE", 214, false),
    (b"CURLOPT_TCP_KEEPINTVL", 215, false),
    (b"CURLSSLOPT_ALLOW_BEAST", 1, false),
    (b"CURL_REDIR_POST_303", 4, false),
    (b"CURLSSH_AUTH_AGENT", 16, false),
    (b"CURLMOPT_CHUNK_LENGTH_PENALTY_SIZE", 30010, false),
    (b"CURLMOPT_CONTENT_LENGTH_PENALTY_SIZE", 30009, false),
    (b"CURLMOPT_MAX_HOST_CONNECTIONS", 7, false),
    (b"CURLMOPT_MAX_PIPELINE_LENGTH", 8, false),
    (b"CURLMOPT_MAX_TOTAL_CONNECTIONS", 13, false),
    (b"CURLOPT_SASL_IR", 218, false),
    (b"CURLOPT_DNS_INTERFACE", 10221, false),
    (b"CURLOPT_DNS_LOCAL_IP4", 10222, false),
    (b"CURLOPT_DNS_LOCAL_IP6", 10223, false),
    (b"CURLOPT_XOAUTH2_BEARER", 10220, false),
    (b"CURL_HTTP_VERSION_2_0", 3, false),
    (b"CURL_VERSION_HTTP2", 65536, false),
    (b"CURLOPT_LOGIN_OPTIONS", 10224, false),
    (b"CURL_SSLVERSION_TLSv1_0", 4, false),
    (b"CURL_SSLVERSION_TLSv1_1", 5, false),
    (b"CURL_SSLVERSION_TLSv1_2", 6, false),
    (b"CURLOPT_EXPECT_100_TIMEOUT_MS", 227, false),
    (b"CURLOPT_SSL_ENABLE_ALPN", 226, false),
    (b"CURLOPT_SSL_ENABLE_NPN", 225, false),
    (b"CURLHEADER_SEPARATE", 1, false),
    (b"CURLHEADER_UNIFIED", 0, false),
    (b"CURLOPT_HEADEROPT", 229, false),
    (b"CURLOPT_PROXYHEADER", 10228, false),
    (b"CURLAUTH_NEGOTIATE", 4, false),
    (b"CURL_VERSION_GSSAPI", 131072, false),
    (b"CURLOPT_PINNEDPUBLICKEY", 10230, false),
    (b"CURLOPT_UNIX_SOCKET_PATH", 10231, false),
    (b"CURLPROTO_SMB", 67108864, false),
    (b"CURLPROTO_SMBS", 134217728, false),
    (b"CURL_VERSION_KERBEROS5", 262144, false),
    (b"CURL_VERSION_UNIX_SOCKETS", 524288, false),
    (b"CURLOPT_SSL_VERIFYSTATUS", 232, false),
    (b"CURLOPT_PATH_AS_IS", 234, false),
    (b"CURLOPT_SSL_FALSESTART", 233, false),
    (b"CURL_HTTP_VERSION_2", 3, false),
    (b"CURLOPT_PIPEWAIT", 237, false),
    (b"CURLOPT_PROXY_SERVICE_NAME", 10235, false),
    (b"CURLOPT_SERVICE_NAME", 10236, false),
    (b"CURLPIPE_NOTHING", 0, false),
    (b"CURLPIPE_HTTP1", 1, false),
    (b"CURLPIPE_MULTIPLEX", 2, false),
    (b"CURLSSLOPT_NO_REVOKE", 2, false),
    (b"CURLOPT_DEFAULT_PROTOCOL", 10238, false),
    (b"CURLOPT_STREAM_WEIGHT", 239, false),
    (b"CURLMOPT_PUSHFUNCTION", 20014, false),
    (b"CURL_PUSH_OK", 0, false),
    (b"CURL_PUSH_DENY", 1, false),
    (b"CURL_HTTP_VERSION_2TLS", 4, false),
    (b"CURL_VERSION_PSL", 1048576, false),
    (b"CURLOPT_TFTP_NO_OPTIONS", 242, false),
    (b"CURL_HTTP_VERSION_2_PRIOR_KNOWLEDGE", 5, false),
    (b"CURLOPT_CONNECT_TO", 10243, false),
    (b"CURLOPT_TCP_FASTOPEN", 244, false),
    (b"CURLINFO_HTTP_VERSION", 2097198, false),
    (b"CURLE_WEIRD_SERVER_REPLY", 8, false),
    (b"CURLOPT_KEEP_SENDING_ON_ERROR", 245, false),
    (b"CURL_SSLVERSION_TLSv1_3", 7, false),
    (b"CURL_VERSION_HTTPS_PROXY", 2097152, false),
    (b"CURLINFO_PROTOCOL", 2097200, false),
    (b"CURLINFO_PROXY_SSL_VERIFYRESULT", 2097199, false),
    (b"CURLINFO_SCHEME", 1048625, false),
    (b"CURLOPT_PRE_PROXY", 10262, false),
    (b"CURLOPT_PROXY_CAINFO", 10246, false),
    (b"CURLOPT_PROXY_CAPATH", 10247, false),
    (b"CURLOPT_PROXY_CRLFILE", 10260, false),
    (b"CURLOPT_PROXY_KEYPASSWD", 10258, false),
    (b"CURLOPT_PROXY_PINNEDPUBLICKEY", 10263, false),
    (b"CURLOPT_PROXY_SSL_CIPHER_LIST", 10259, false),
    (b"CURLOPT_PROXY_SSL_OPTIONS", 261, false),
    (b"CURLOPT_PROXY_SSL_VERIFYHOST", 249, false),
    (b"CURLOPT_PROXY_SSL_VERIFYPEER", 248, false),
    (b"CURLOPT_PROXY_SSLCERT", 10254, false),
    (b"CURLOPT_PROXY_SSLCERTTYPE", 10255, false),
    (b"CURLOPT_PROXY_SSLKEY", 10256, false),
    (b"CURLOPT_PROXY_SSLKEYTYPE", 10257, false),
    (b"CURLOPT_PROXY_SSLVERSION", 250, false),
    (b"CURLOPT_PROXY_TLSAUTH_PASSWORD", 10252, false),
    (b"CURLOPT_PROXY_TLSAUTH_TYPE", 10253, false),
    (b"CURLOPT_PROXY_TLSAUTH_USERNAME", 10251, false),
    (b"CURLPROXY_HTTPS", 2, false),
    (b"CURL_MAX_READ_SIZE", 10485760, false),
    (b"CURLOPT_ABSTRACT_UNIX_SOCKET", 10264, false),
    (b"CURL_SSLVERSION_MAX_DEFAULT", 65536, false),
    (b"CURL_SSLVERSION_MAX_NONE", 0, false),
    (b"CURL_SSLVERSION_MAX_TLSv1_0", 262144, false),
    (b"CURL_SSLVERSION_MAX_TLSv1_1", 327680, false),
    (b"CURL_SSLVERSION_MAX_TLSv1_2", 393216, false),
    (b"CURL_SSLVERSION_MAX_TLSv1_3", 458752, false),
    (b"CURLOPT_SUPPRESS_CONNECT_HEADERS", 265, false),
    (b"CURLAUTH_GSSAPI", 4, false),
    (b"CURLINFO_CONTENT_LENGTH_DOWNLOAD_T", 6291471, false),
    (b"CURLINFO_CONTENT_LENGTH_UPLOAD_T", 6291472, false),
    (b"CURLINFO_SIZE_DOWNLOAD_T", 6291464, false),
    (b"CURLINFO_SIZE_UPLOAD_T", 6291463, false),
    (b"CURLINFO_SPEED_DOWNLOAD_T", 6291465, false),
    (b"CURLINFO_SPEED_UPLOAD_T", 6291466, false),
    (b"CURLOPT_REQUEST_TARGET", 10266, false),
    (b"CURLOPT_SOCKS5_AUTH", 267, false),
    (b"CURLOPT_SSH_COMPRESSION", 268, false),
    (b"CURL_VERSION_MULTI_SSL", 4194304, false),
    (b"CURL_VERSION_BROTLI", 8388608, false),
    (b"CURL_LOCK_DATA_CONNECT", 5, false),
    (b"CURLSSH_AUTH_GSSAPI", 32, false),
    (b"CURLINFO_FILETIME_T", 6291470, false),
    (b"CURLOPT_HAPPY_EYEBALLS_TIMEOUT_MS", 271, false),
    (b"CURLOPT_TIMEVALUE_LARGE", 30270, false),
    (b"CURLOPT_DNS_SHUFFLE_ADDRESSES", 275, false),
    (b"CURLOPT_HAPROXYPROTOCOL", 274, false),
    (b"CURL_LOCK_DATA_PSL", 6, false),
    (b"CURLAUTH_BEARER", 64, false),
    (b"CURLINFO_QUEUE_TIME_T", 6291521, false),
    (b"CURLINFO_APPCONNECT_TIME_T", 6291512, false),
    (b"CURLINFO_CONNECT_TIME_T", 6291508, false),
    (b"CURLINFO_NAMELOOKUP_TIME_T", 6291507, false),
    (b"CURLINFO_PRETRANSFER_TIME_T", 6291509, false),
    (b"CURLINFO_REDIRECT_TIME_T", 6291511, false),
    (b"CURLINFO_STARTTRANSFER_TIME_T", 6291510, false),
    (b"CURLINFO_TOTAL_TIME_T", 6291506, false),
    (b"CURLINFO_USED_PROXY", 2097218, false),
    (b"CURLINFO_POSTTRANSFER_TIME_T", 6291523, false),
    (b"CURLINFO_CONN_ID", 6291520, false),
    (b"CURLOPT_DISALLOW_USERNAME_IN_URL", 278, false),
    (b"CURLOPT_PROXY_TLS13_CIPHERS", 10277, false),
    (b"CURLOPT_TLS13_CIPHERS", 10276, false),
    (b"CURLOPT_DOH_URL", 10279, false),
    (b"CURLOPT_UPKEEP_INTERVAL_MS", 281, false),
    (b"CURLOPT_UPLOAD_BUFFERSIZE", 280, false),
    (b"CURLOPT_HTTP09_ALLOWED", 285, false),
    (b"CURLALTSVC_H1", 8, false),
    (b"CURLALTSVC_H2", 16, false),
    (b"CURLALTSVC_H3", 32, false),
    (b"CURLALTSVC_READONLYFILE", 4, false),
    (b"CURLOPT_ALTSVC", 10287, false),
    (b"CURLOPT_ALTSVC_CTRL", 286, false),
    (b"CURL_VERSION_ALTSVC", 16777216, false),
    (b"CURLOPT_MAXAGE_CONN", 288, false),
    (b"CURLOPT_SASL_AUTHZID", 10289, false),
    (b"CURL_VERSION_HTTP3", 33554432, false),
    (b"CURLINFO_RETRY_AFTER", 6291513, false),
    (b"CURL_HTTP_VERSION_3", 30, false),
    (b"CURLMOPT_MAX_CONCURRENT_STREAMS", 16, false),
    (b"CURLSSLOPT_NO_PARTIALCHAIN", 4, false),
    (b"CURLOPT_MAIL_RCPT_ALLLOWFAILS", 290, false),
    (b"CURLSSLOPT_REVOKE_BEST_EFFORT", 8, false),
    (b"CURLOPT_ISSUERCERT_BLOB", 40295, false),
    (b"CURLOPT_PROXY_ISSUERCERT", 10296, false),
    (b"CURLOPT_PROXY_ISSUERCERT_BLOB", 40297, false),
    (b"CURLOPT_PROXY_SSLCERT_BLOB", 40293, false),
    (b"CURLOPT_PROXY_SSLKEY_BLOB", 40294, false),
    (b"CURLOPT_SSLCERT_BLOB", 40291, false),
    (b"CURLOPT_SSLKEY_BLOB", 40292, false),
    (b"CURLPROTO_MQTT", 268435456, false),
    (b"CURLSSLOPT_NATIVE_CA", 16, false),
    (b"CURL_VERSION_UNICODE", 134217728, false),
    (b"CURL_VERSION_ZSTD", 67108864, false),
    (b"CURLE_PROXY", 97, false),
    (b"CURLINFO_PROXY_ERROR", 2097211, false),
    (b"CURLOPT_SSL_EC_CURVES", 10298, false),
    (b"CURLOPT_SSL_SIGNATURE_ALGORITHMS", 10328, false),
    (b"CURLPX_BAD_ADDRESS_TYPE", 1, false),
    (b"CURLPX_BAD_VERSION", 2, false),
    (b"CURLPX_CLOSED", 3, false),
    (b"CURLPX_GSSAPI", 4, false),
    (b"CURLPX_GSSAPI_PERMSG", 5, false),
    (b"CURLPX_GSSAPI_PROTECTION", 6, false),
    (b"CURLPX_IDENTD", 7, false),
    (b"CURLPX_IDENTD_DIFFER", 8, false),
    (b"CURLPX_LONG_HOSTNAME", 9, false),
    (b"CURLPX_LONG_PASSWD", 10, false),
    (b"CURLPX_LONG_USER", 11, false),
    (b"CURLPX_NO_AUTH", 12, false),
    (b"CURLPX_OK", 0, false),
    (b"CURLPX_RECV_ADDRESS", 13, false),
    (b"CURLPX_RECV_AUTH", 14, false),
    (b"CURLPX_RECV_CONNECT", 15, false),
    (b"CURLPX_RECV_REQACK", 16, false),
    (b"CURLPX_REPLY_ADDRESS_TYPE_NOT_SUPPORTED", 17, false),
    (b"CURLPX_REPLY_COMMAND_NOT_SUPPORTED", 18, false),
    (b"CURLPX_REPLY_CONNECTION_REFUSED", 19, false),
    (b"CURLPX_REPLY_GENERAL_SERVER_FAILURE", 20, false),
    (b"CURLPX_REPLY_HOST_UNREACHABLE", 21, false),
    (b"CURLPX_REPLY_NETWORK_UNREACHABLE", 22, false),
    (b"CURLPX_REPLY_NOT_ALLOWED", 23, false),
    (b"CURLPX_REPLY_TTL_EXPIRED", 24, false),
    (b"CURLPX_REPLY_UNASSIGNED", 25, false),
    (b"CURLPX_REQUEST_FAILED", 26, false),
    (b"CURLPX_RESOLVE_HOST", 27, false),
    (b"CURLPX_SEND_AUTH", 28, false),
    (b"CURLPX_SEND_CONNECT", 29, false),
    (b"CURLPX_SEND_REQUEST", 30, false),
    (b"CURLPX_UNKNOWN_FAIL", 31, false),
    (b"CURLPX_UNKNOWN_MODE", 32, false),
    (b"CURLPX_USER_REJECTED", 33, false),
    (b"CURLHSTS_ENABLE", 1, false),
    (b"CURLHSTS_READONLYFILE", 2, false),
    (b"CURLOPT_HSTS", 10300, false),
    (b"CURLOPT_HSTS_CTRL", 299, false),
    (b"CURL_VERSION_HSTS", 268435456, false),
    (b"CURLAUTH_AWS_SIGV4", 128, false),
    (b"CURLOPT_AWS_SIGV4", 10305, false),
    (b"CURLINFO_REFERER", 1048636, false),
    (b"CURLOPT_DOH_SSL_VERIFYHOST", 307, false),
    (b"CURLOPT_DOH_SSL_VERIFYPEER", 306, false),
    (b"CURLOPT_DOH_SSL_VERIFYSTATUS", 308, false),
    (b"CURL_VERSION_GSASL", 536870912, false),
    (b"CURLOPT_CAINFO_BLOB", 40309, false),
    (b"CURLOPT_PROXY_CAINFO_BLOB", 40310, false),
    (b"CURLSSLOPT_AUTO_CLIENT_CERT", 32, false),
    (b"CURLOPT_MAXLIFETIME_CONN", 314, false),
    (b"CURLOPT_SSH_HOST_PUBLIC_KEY_SHA256", 10311, false),
    (b"CURLOPT_PREREQFUNCTION", 20312, false),
    (b"CURL_PREREQFUNC_OK", 0, false),
    (b"CURL_PREREQFUNC_ABORT", 1, false),
    (b"CURLOPT_MIME_OPTIONS", 315, false),
    (b"CURLMIMEOPT_FORMESCAPE", 1, false),
    (b"CURLOPT_SSH_HOSTKEYFUNCTION", 20316, false),
    (b"CURLOPT_PROTOCOLS_STR", 10318, false),
    (b"CURLOPT_REDIR_PROTOCOLS_STR", 10319, false),
    (b"CURLOPT_WS_OPTIONS", 320, false),
    (b"CURLWS_RAW_MODE", 1, false),
    (b"CURLOPT_CA_CACHE_TIMEOUT", 321, false),
    (b"CURLOPT_QUICK_EXIT", 322, false),
    (b"CURL_HTTP_VERSION_3ONLY", 31, false),
    (b"CURLOPT_SAFE_UPLOAD", -1, false),
];

/// The same values as Rust constants, named like the C ones.
#[allow(dead_code, non_upper_case_globals)]
pub mod c {
    pub const CURLOPT_AUTOREFERER: i64 = 58;
    pub const CURLOPT_BINARYTRANSFER: i64 = 19914;
    pub const CURLOPT_BUFFERSIZE: i64 = 98;
    pub const CURLOPT_CAINFO: i64 = 10065;
    pub const CURLOPT_CAPATH: i64 = 10097;
    pub const CURLOPT_CONNECTTIMEOUT: i64 = 78;
    pub const CURLOPT_COOKIE: i64 = 10022;
    pub const CURLOPT_COOKIEFILE: i64 = 10031;
    pub const CURLOPT_COOKIEJAR: i64 = 10082;
    pub const CURLOPT_COOKIESESSION: i64 = 96;
    pub const CURLOPT_CRLF: i64 = 27;
    pub const CURLOPT_CUSTOMREQUEST: i64 = 10036;
    pub const CURLOPT_DNS_CACHE_TIMEOUT: i64 = 92;
    pub const CURLOPT_DNS_USE_GLOBAL_CACHE: i64 = 91;
    pub const CURLOPT_EGDSOCKET: i64 = 10077;
    pub const CURLOPT_ENCODING: i64 = 10102;
    pub const CURLOPT_FAILONERROR: i64 = 45;
    pub const CURLOPT_FILE: i64 = 10001;
    pub const CURLOPT_FILETIME: i64 = 69;
    pub const CURLOPT_FOLLOWLOCATION: i64 = 52;
    pub const CURLOPT_FORBID_REUSE: i64 = 75;
    pub const CURLOPT_FRESH_CONNECT: i64 = 74;
    pub const CURLOPT_FTPAPPEND: i64 = 50;
    pub const CURLOPT_FTPLISTONLY: i64 = 48;
    pub const CURLOPT_FTPPORT: i64 = 10017;
    pub const CURLOPT_FTP_USE_EPRT: i64 = 106;
    pub const CURLOPT_FTP_USE_EPSV: i64 = 85;
    pub const CURLOPT_HEADER: i64 = 42;
    pub const CURLOPT_HEADERFUNCTION: i64 = 20079;
    pub const CURLOPT_HTTP200ALIASES: i64 = 10104;
    pub const CURLOPT_HTTPGET: i64 = 80;
    pub const CURLOPT_HTTPHEADER: i64 = 10023;
    pub const CURLOPT_HTTPPROXYTUNNEL: i64 = 61;
    pub const CURLOPT_HTTP_VERSION: i64 = 84;
    pub const CURLOPT_INFILE: i64 = 10009;
    pub const CURLOPT_INFILESIZE: i64 = 14;
    pub const CURLOPT_INFILESIZE_LARGE: i64 = 30115;
    pub const CURLOPT_INTERFACE: i64 = 10062;
    pub const CURLOPT_KRB4LEVEL: i64 = 10063;
    pub const CURLOPT_LOW_SPEED_LIMIT: i64 = 19;
    pub const CURLOPT_LOW_SPEED_TIME: i64 = 20;
    pub const CURLOPT_MAXCONNECTS: i64 = 71;
    pub const CURLOPT_MAXREDIRS: i64 = 68;
    pub const CURLOPT_NETRC: i64 = 51;
    pub const CURLOPT_NOBODY: i64 = 44;
    pub const CURLOPT_NOPROGRESS: i64 = 43;
    pub const CURLOPT_NOSIGNAL: i64 = 99;
    pub const CURLOPT_PORT: i64 = 3;
    pub const CURLOPT_POST: i64 = 47;
    pub const CURLOPT_POSTFIELDS: i64 = 10015;
    pub const CURLOPT_POSTQUOTE: i64 = 10039;
    pub const CURLOPT_PREQUOTE: i64 = 10093;
    pub const CURLOPT_PRIVATE: i64 = 10103;
    pub const CURLOPT_PROGRESSFUNCTION: i64 = 20056;
    pub const CURLOPT_PROXY: i64 = 10004;
    pub const CURLOPT_PROXYPORT: i64 = 59;
    pub const CURLOPT_PROXYTYPE: i64 = 101;
    pub const CURLOPT_PROXYUSERPWD: i64 = 10006;
    pub const CURLOPT_PUT: i64 = 54;
    pub const CURLOPT_QUOTE: i64 = 10028;
    pub const CURLOPT_RANDOM_FILE: i64 = 10076;
    pub const CURLOPT_RANGE: i64 = 10007;
    pub const CURLOPT_READDATA: i64 = 10009;
    pub const CURLOPT_READFUNCTION: i64 = 20012;
    pub const CURLOPT_REFERER: i64 = 10016;
    pub const CURLOPT_RESUME_FROM: i64 = 21;
    pub const CURLOPT_RETURNTRANSFER: i64 = 19913;
    pub const CURLOPT_SHARE: i64 = 10100;
    pub const CURLOPT_SSLCERT: i64 = 10025;
    pub const CURLOPT_SSLCERTPASSWD: i64 = 10026;
    pub const CURLOPT_SSLCERTTYPE: i64 = 10086;
    pub const CURLOPT_SSLENGINE: i64 = 10089;
    pub const CURLOPT_SSLENGINE_DEFAULT: i64 = 90;
    pub const CURLOPT_SSLKEY: i64 = 10087;
    pub const CURLOPT_SSLKEYPASSWD: i64 = 10026;
    pub const CURLOPT_SSLKEYTYPE: i64 = 10088;
    pub const CURLOPT_SSLVERSION: i64 = 32;
    pub const CURLOPT_SSL_CIPHER_LIST: i64 = 10083;
    pub const CURLOPT_SSL_VERIFYHOST: i64 = 81;
    pub const CURLOPT_SSL_VERIFYPEER: i64 = 64;
    pub const CURLOPT_STDERR: i64 = 10037;
    pub const CURLOPT_TCP_KEEPCNT: i64 = 326;
    pub const CURLOPT_TELNETOPTIONS: i64 = 10070;
    pub const CURLOPT_TIMECONDITION: i64 = 33;
    pub const CURLOPT_TIMEOUT: i64 = 13;
    pub const CURLOPT_TIMEVALUE: i64 = 34;
    pub const CURLOPT_TRANSFERTEXT: i64 = 53;
    pub const CURLOPT_UNRESTRICTED_AUTH: i64 = 105;
    pub const CURLOPT_UPLOAD: i64 = 46;
    pub const CURLOPT_URL: i64 = 10002;
    pub const CURLOPT_USERAGENT: i64 = 10018;
    pub const CURLOPT_USERPWD: i64 = 10005;
    pub const CURLOPT_VERBOSE: i64 = 41;
    pub const CURLOPT_WRITEFUNCTION: i64 = 20011;
    pub const CURLOPT_WRITEHEADER: i64 = 10029;
    pub const CURLOPT_XFERINFOFUNCTION: i64 = 20219;
    pub const CURLOPT_DEBUGFUNCTION: i64 = 20094;
    pub const CURLFOLLOW_ALL: i64 = 1;
    pub const CURLFOLLOW_OBEYCODE: i64 = 2;
    pub const CURLFOLLOW_FIRSTONLY: i64 = 3;
    pub const CURLINFO_TEXT: i64 = 0;
    pub const CURLINFO_HEADER_IN: i64 = 1;
    pub const CURLINFO_DATA_IN: i64 = 3;
    pub const CURLINFO_DATA_OUT: i64 = 4;
    pub const CURLINFO_SSL_DATA_OUT: i64 = 6;
    pub const CURLINFO_SSL_DATA_IN: i64 = 5;
    pub const CURLE_ABORTED_BY_CALLBACK: i64 = 42;
    pub const CURLE_BAD_CALLING_ORDER: i64 = 44;
    pub const CURLE_BAD_CONTENT_ENCODING: i64 = 61;
    pub const CURLE_BAD_DOWNLOAD_RESUME: i64 = 36;
    pub const CURLE_BAD_FUNCTION_ARGUMENT: i64 = 43;
    pub const CURLE_BAD_PASSWORD_ENTERED: i64 = 46;
    pub const CURLE_COULDNT_CONNECT: i64 = 7;
    pub const CURLE_COULDNT_RESOLVE_HOST: i64 = 6;
    pub const CURLE_COULDNT_RESOLVE_PROXY: i64 = 5;
    pub const CURLE_FAILED_INIT: i64 = 2;
    pub const CURLE_FILE_COULDNT_READ_FILE: i64 = 37;
    pub const CURLE_FTP_ACCESS_DENIED: i64 = 9;
    pub const CURLE_FTP_BAD_DOWNLOAD_RESUME: i64 = 36;
    pub const CURLE_FTP_CANT_GET_HOST: i64 = 15;
    pub const CURLE_FTP_CANT_RECONNECT: i64 = 16;
    pub const CURLE_FTP_COULDNT_GET_SIZE: i64 = 32;
    pub const CURLE_FTP_COULDNT_RETR_FILE: i64 = 19;
    pub const CURLE_FTP_COULDNT_SET_ASCII: i64 = 29;
    pub const CURLE_FTP_COULDNT_SET_BINARY: i64 = 17;
    pub const CURLE_FTP_COULDNT_STOR_FILE: i64 = 25;
    pub const CURLE_FTP_COULDNT_USE_REST: i64 = 31;
    pub const CURLE_FTP_PARTIAL_FILE: i64 = 18;
    pub const CURLE_FTP_PORT_FAILED: i64 = 30;
    pub const CURLE_FTP_QUOTE_ERROR: i64 = 21;
    pub const CURLE_FTP_USER_PASSWORD_INCORRECT: i64 = 10;
    pub const CURLE_FTP_WEIRD_227_FORMAT: i64 = 14;
    pub const CURLE_FTP_WEIRD_PASS_REPLY: i64 = 11;
    pub const CURLE_FTP_WEIRD_PASV_REPLY: i64 = 13;
    pub const CURLE_FTP_WEIRD_SERVER_REPLY: i64 = 8;
    pub const CURLE_FTP_WEIRD_USER_REPLY: i64 = 12;
    pub const CURLE_FTP_WRITE_ERROR: i64 = 20;
    pub const CURLE_FUNCTION_NOT_FOUND: i64 = 41;
    pub const CURLE_GOT_NOTHING: i64 = 52;
    pub const CURLE_HTTP_NOT_FOUND: i64 = 22;
    pub const CURLE_HTTP_PORT_FAILED: i64 = 45;
    pub const CURLE_HTTP_POST_ERROR: i64 = 34;
    pub const CURLE_HTTP_RANGE_ERROR: i64 = 33;
    pub const CURLE_HTTP_RETURNED_ERROR: i64 = 22;
    pub const CURLE_LDAP_CANNOT_BIND: i64 = 38;
    pub const CURLE_LDAP_SEARCH_FAILED: i64 = 39;
    pub const CURLE_LIBRARY_NOT_FOUND: i64 = 40;
    pub const CURLE_MALFORMAT_USER: i64 = 24;
    pub const CURLE_OBSOLETE: i64 = 50;
    pub const CURLE_OK: i64 = 0;
    pub const CURLE_OPERATION_TIMEDOUT: i64 = 28;
    pub const CURLE_OPERATION_TIMEOUTED: i64 = 28;
    pub const CURLE_OUT_OF_MEMORY: i64 = 27;
    pub const CURLE_PARTIAL_FILE: i64 = 18;
    pub const CURLE_READ_ERROR: i64 = 26;
    pub const CURLE_RECV_ERROR: i64 = 56;
    pub const CURLE_SEND_ERROR: i64 = 55;
    pub const CURLE_SHARE_IN_USE: i64 = 57;
    pub const CURLE_SSL_CACERT: i64 = 60;
    pub const CURLE_SSL_CERTPROBLEM: i64 = 58;
    pub const CURLE_SSL_CIPHER: i64 = 59;
    pub const CURLE_SSL_CONNECT_ERROR: i64 = 35;
    pub const CURLE_SSL_ENGINE_NOTFOUND: i64 = 53;
    pub const CURLE_SSL_ENGINE_SETFAILED: i64 = 54;
    pub const CURLE_SSL_PEER_CERTIFICATE: i64 = 60;
    pub const CURLE_SSL_PINNEDPUBKEYNOTMATCH: i64 = 90;
    pub const CURLE_TELNET_OPTION_SYNTAX: i64 = 49;
    pub const CURLE_TOO_MANY_REDIRECTS: i64 = 47;
    pub const CURLE_UNKNOWN_TELNET_OPTION: i64 = 48;
    pub const CURLE_UNSUPPORTED_PROTOCOL: i64 = 1;
    pub const CURLE_URL_MALFORMAT: i64 = 3;
    pub const CURLE_URL_MALFORMAT_USER: i64 = 4;
    pub const CURLE_WRITE_ERROR: i64 = 23;
    pub const CURLINFO_CONNECT_TIME: i64 = 3145733;
    pub const CURLINFO_CONTENT_LENGTH_DOWNLOAD: i64 = 3145743;
    pub const CURLINFO_CONTENT_LENGTH_UPLOAD: i64 = 3145744;
    pub const CURLINFO_CONTENT_TYPE: i64 = 1048594;
    pub const CURLINFO_EFFECTIVE_URL: i64 = 1048577;
    pub const CURLINFO_FILETIME: i64 = 2097166;
    pub const CURLINFO_HEADER_OUT: i64 = 2;
    pub const CURLINFO_HEADER_SIZE: i64 = 2097163;
    pub const CURLINFO_HTTP_CODE: i64 = 2097154;
    pub const CURLINFO_LASTONE: i64 = 71;
    pub const CURLINFO_NAMELOOKUP_TIME: i64 = 3145732;
    pub const CURLINFO_PRETRANSFER_TIME: i64 = 3145734;
    pub const CURLINFO_PRIVATE: i64 = 1048597;
    pub const CURLINFO_REDIRECT_COUNT: i64 = 2097172;
    pub const CURLINFO_REDIRECT_TIME: i64 = 3145747;
    pub const CURLINFO_REQUEST_SIZE: i64 = 2097164;
    pub const CURLINFO_SIZE_DOWNLOAD: i64 = 3145736;
    pub const CURLINFO_SIZE_UPLOAD: i64 = 3145735;
    pub const CURLINFO_SPEED_DOWNLOAD: i64 = 3145737;
    pub const CURLINFO_SPEED_UPLOAD: i64 = 3145738;
    pub const CURLINFO_SSL_VERIFYRESULT: i64 = 2097165;
    pub const CURLINFO_STARTTRANSFER_TIME: i64 = 3145745;
    pub const CURLINFO_TOTAL_TIME: i64 = 3145731;
    pub const CURLINFO_EFFECTIVE_METHOD: i64 = 1048634;
    pub const CURLINFO_CAPATH: i64 = 1048638;
    pub const CURLINFO_CAINFO: i64 = 1048637;
    pub const CURLINFO_HTTPAUTH_USED: i64 = 2097221;
    pub const CURLINFO_PROXYAUTH_USED: i64 = 2097222;
    pub const CURLMSG_DONE: i64 = 1;
    pub const CURLVERSION_NOW: i64 = 11;
    pub const CURLM_BAD_EASY_HANDLE: i64 = 2;
    pub const CURLM_BAD_HANDLE: i64 = 1;
    pub const CURLM_CALL_MULTI_PERFORM: i64 = -1;
    pub const CURLM_INTERNAL_ERROR: i64 = 4;
    pub const CURLM_OK: i64 = 0;
    pub const CURLM_OUT_OF_MEMORY: i64 = 3;
    pub const CURLM_ADDED_ALREADY: i64 = 7;
    pub const CURLPROXY_HTTP: i64 = 0;
    pub const CURLPROXY_SOCKS4: i64 = 4;
    pub const CURLPROXY_SOCKS5: i64 = 5;
    pub const CURLSHOPT_NONE: i64 = 0;
    pub const CURLSHOPT_SHARE: i64 = 1;
    pub const CURLSHOPT_UNSHARE: i64 = 2;
    pub const CURL_HTTP_VERSION_1_0: i64 = 1;
    pub const CURL_HTTP_VERSION_1_1: i64 = 2;
    pub const CURL_HTTP_VERSION_NONE: i64 = 0;
    pub const CURL_LOCK_DATA_COOKIE: i64 = 2;
    pub const CURL_LOCK_DATA_DNS: i64 = 3;
    pub const CURL_LOCK_DATA_SSL_SESSION: i64 = 4;
    pub const CURL_NETRC_IGNORED: i64 = 0;
    pub const CURL_NETRC_OPTIONAL: i64 = 1;
    pub const CURL_NETRC_REQUIRED: i64 = 2;
    pub const CURL_SSLVERSION_DEFAULT: i64 = 0;
    pub const CURL_SSLVERSION_SSLv2: i64 = 2;
    pub const CURL_SSLVERSION_SSLv3: i64 = 3;
    pub const CURL_SSLVERSION_TLSv1: i64 = 1;
    pub const CURL_TIMECOND_IFMODSINCE: i64 = 1;
    pub const CURL_TIMECOND_IFUNMODSINCE: i64 = 2;
    pub const CURL_TIMECOND_LASTMOD: i64 = 3;
    pub const CURL_TIMECOND_NONE: i64 = 0;
    pub const CURL_VERSION_ASYNCHDNS: i64 = 128;
    pub const CURL_VERSION_CONV: i64 = 4096;
    pub const CURL_VERSION_DEBUG: i64 = 64;
    pub const CURL_VERSION_GSSNEGOTIATE: i64 = 32;
    pub const CURL_VERSION_IDN: i64 = 1024;
    pub const CURL_VERSION_IPV6: i64 = 1;
    pub const CURL_VERSION_KERBEROS4: i64 = 2;
    pub const CURL_VERSION_LARGEFILE: i64 = 512;
    pub const CURL_VERSION_LIBZ: i64 = 8;
    pub const CURL_VERSION_NTLM: i64 = 16;
    pub const CURL_VERSION_SPNEGO: i64 = 256;
    pub const CURL_VERSION_SSL: i64 = 4;
    pub const CURL_VERSION_SSPI: i64 = 2048;
    pub const CURLOPT_HTTPAUTH: i64 = 107;
    pub const CURLAUTH_ANY: i64 = 4294967279;
    pub const CURLAUTH_ANYSAFE: i64 = 4294967278;
    pub const CURLAUTH_BASIC: i64 = 1;
    pub const CURLAUTH_DIGEST: i64 = 2;
    pub const CURLAUTH_GSSNEGOTIATE: i64 = 4;
    pub const CURLAUTH_NONE: i64 = 0;
    pub const CURLAUTH_NTLM: i64 = 8;
    pub const CURLINFO_HTTP_CONNECTCODE: i64 = 2097174;
    pub const CURLOPT_FTP_CREATE_MISSING_DIRS: i64 = 110;
    pub const CURLOPT_PROXYAUTH: i64 = 111;
    pub const CURLE_FILESIZE_EXCEEDED: i64 = 63;
    pub const CURLE_LDAP_INVALID_URL: i64 = 62;
    pub const CURLINFO_HTTPAUTH_AVAIL: i64 = 2097175;
    pub const CURLINFO_RESPONSE_CODE: i64 = 2097154;
    pub const CURLINFO_PROXYAUTH_AVAIL: i64 = 2097176;
    pub const CURLOPT_FTP_RESPONSE_TIMEOUT: i64 = 112;
    pub const CURLOPT_SERVER_RESPONSE_TIMEOUT: i64 = 112;
    pub const CURLOPT_IPRESOLVE: i64 = 113;
    pub const CURLOPT_MAXFILESIZE: i64 = 114;
    pub const CURL_IPRESOLVE_V4: i64 = 1;
    pub const CURL_IPRESOLVE_V6: i64 = 2;
    pub const CURL_IPRESOLVE_WHATEVER: i64 = 0;
    pub const CURLE_FTP_SSL_FAILED: i64 = 64;
    pub const CURLFTPSSL_ALL: i64 = 3;
    pub const CURLFTPSSL_CONTROL: i64 = 2;
    pub const CURLFTPSSL_NONE: i64 = 0;
    pub const CURLFTPSSL_TRY: i64 = 1;
    pub const CURLOPT_FTP_SSL: i64 = 119;
    pub const CURLOPT_NETRC_FILE: i64 = 10118;
    pub const CURLOPT_MAXFILESIZE_LARGE: i64 = 30117;
    pub const CURLOPT_TCP_NODELAY: i64 = 121;
    pub const CURLFTPAUTH_DEFAULT: i64 = 0;
    pub const CURLFTPAUTH_SSL: i64 = 1;
    pub const CURLFTPAUTH_TLS: i64 = 2;
    pub const CURLOPT_FTPSSLAUTH: i64 = 129;
    pub const CURLOPT_FTP_ACCOUNT: i64 = 10134;
    pub const CURLINFO_OS_ERRNO: i64 = 2097177;
    pub const CURLINFO_NUM_CONNECTS: i64 = 2097178;
    pub const CURLINFO_SSL_ENGINES: i64 = 4194331;
    pub const CURLINFO_COOKIELIST: i64 = 4194332;
    pub const CURLOPT_COOKIELIST: i64 = 10135;
    pub const CURLOPT_IGNORE_CONTENT_LENGTH: i64 = 136;
    pub const CURLOPT_FTP_SKIP_PASV_IP: i64 = 137;
    pub const CURLOPT_FTP_FILEMETHOD: i64 = 138;
    pub const CURLOPT_CONNECT_ONLY: i64 = 141;
    pub const CURLOPT_LOCALPORT: i64 = 139;
    pub const CURLOPT_LOCALPORTRANGE: i64 = 140;
    pub const CURLFTPMETHOD_DEFAULT: i64 = 0;
    pub const CURLFTPMETHOD_MULTICWD: i64 = 1;
    pub const CURLFTPMETHOD_NOCWD: i64 = 2;
    pub const CURLFTPMETHOD_SINGLECWD: i64 = 3;
    pub const CURLINFO_FTP_ENTRY_PATH: i64 = 1048606;
    pub const CURLOPT_FTP_ALTERNATIVE_TO_USER: i64 = 10147;
    pub const CURLOPT_MAX_RECV_SPEED_LARGE: i64 = 30146;
    pub const CURLOPT_MAX_SEND_SPEED_LARGE: i64 = 30145;
    pub const CURLE_SSL_CACERT_BADFILE: i64 = 77;
    pub const CURLOPT_SSL_SESSIONID_CACHE: i64 = 150;
    pub const CURLMOPT_PIPELINING: i64 = 3;
    pub const CURLE_SSH: i64 = 79;
    pub const CURLOPT_FTP_SSL_CCC: i64 = 154;
    pub const CURLOPT_SSH_AUTH_TYPES: i64 = 151;
    pub const CURLOPT_SSH_PRIVATE_KEYFILE: i64 = 10153;
    pub const CURLOPT_SSH_PUBLIC_KEYFILE: i64 = 10152;
    pub const CURLFTPSSL_CCC_ACTIVE: i64 = 2;
    pub const CURLFTPSSL_CCC_NONE: i64 = 0;
    pub const CURLFTPSSL_CCC_PASSIVE: i64 = 1;
    pub const CURLOPT_CONNECTTIMEOUT_MS: i64 = 156;
    pub const CURLOPT_HTTP_CONTENT_DECODING: i64 = 158;
    pub const CURLOPT_HTTP_TRANSFER_DECODING: i64 = 157;
    pub const CURLOPT_TIMEOUT_MS: i64 = 155;
    pub const CURLMOPT_MAXCONNECTS: i64 = 6;
    pub const CURLOPT_KRBLEVEL: i64 = 10063;
    pub const CURLOPT_NEW_DIRECTORY_PERMS: i64 = 160;
    pub const CURLOPT_NEW_FILE_PERMS: i64 = 159;
    pub const CURLOPT_APPEND: i64 = 50;
    pub const CURLOPT_DIRLISTONLY: i64 = 48;
    pub const CURLOPT_USE_SSL: i64 = 119;
    pub const CURLUSESSL_ALL: i64 = 3;
    pub const CURLUSESSL_CONTROL: i64 = 2;
    pub const CURLUSESSL_NONE: i64 = 0;
    pub const CURLUSESSL_TRY: i64 = 1;
    pub const CURLOPT_SSH_HOST_PUBLIC_KEY_MD5: i64 = 10162;
    pub const CURLOPT_PROXY_TRANSFER_MODE: i64 = 166;
    pub const CURLPAUSE_ALL: i64 = 5;
    pub const CURLPAUSE_CONT: i64 = 0;
    pub const CURLPAUSE_RECV: i64 = 1;
    pub const CURLPAUSE_RECV_CONT: i64 = 0;
    pub const CURLPAUSE_SEND: i64 = 4;
    pub const CURLPAUSE_SEND_CONT: i64 = 0;
    pub const CURL_READFUNC_PAUSE: i64 = 268435457;
    pub const CURL_WRITEFUNC_PAUSE: i64 = 268435457;
    pub const CURLPROXY_SOCKS4A: i64 = 6;
    pub const CURLPROXY_SOCKS5_HOSTNAME: i64 = 7;
    pub const CURLINFO_REDIRECT_URL: i64 = 1048607;
    pub const CURLINFO_APPCONNECT_TIME: i64 = 3145761;
    pub const CURLINFO_PRIMARY_IP: i64 = 1048608;
    pub const CURLOPT_ADDRESS_SCOPE: i64 = 171;
    pub const CURLOPT_CRLFILE: i64 = 10169;
    pub const CURLOPT_ISSUERCERT: i64 = 10170;
    pub const CURLOPT_KEYPASSWD: i64 = 10026;
    pub const CURLSSH_AUTH_ANY: i64 = 4294967295;
    pub const CURLSSH_AUTH_DEFAULT: i64 = 4294967295;
    pub const CURLSSH_AUTH_HOST: i64 = 4;
    pub const CURLSSH_AUTH_KEYBOARD: i64 = 8;
    pub const CURLSSH_AUTH_NONE: i64 = 0;
    pub const CURLSSH_AUTH_PASSWORD: i64 = 2;
    pub const CURLSSH_AUTH_PUBLICKEY: i64 = 1;
    pub const CURLINFO_CERTINFO: i64 = 4194338;
    pub const CURLOPT_CERTINFO: i64 = 172;
    pub const CURLOPT_PASSWORD: i64 = 10174;
    pub const CURLOPT_POSTREDIR: i64 = 161;
    pub const CURLOPT_PROXYPASSWORD: i64 = 10176;
    pub const CURLOPT_PROXYUSERNAME: i64 = 10175;
    pub const CURLOPT_USERNAME: i64 = 10173;
    pub const CURL_REDIR_POST_301: i64 = 1;
    pub const CURL_REDIR_POST_302: i64 = 2;
    pub const CURL_REDIR_POST_ALL: i64 = 7;
    pub const CURLAUTH_DIGEST_IE: i64 = 16;
    pub const CURLINFO_CONDITION_UNMET: i64 = 2097187;
    pub const CURLOPT_NOPROXY: i64 = 10177;
    pub const CURLOPT_PROTOCOLS: i64 = 181;
    pub const CURLOPT_REDIR_PROTOCOLS: i64 = 182;
    pub const CURLOPT_SOCKS5_GSSAPI_NEC: i64 = 180;
    pub const CURLOPT_SOCKS5_GSSAPI_SERVICE: i64 = 10179;
    pub const CURLOPT_TFTP_BLKSIZE: i64 = 178;
    pub const CURLPROTO_ALL: i64 = 4294967295;
    pub const CURLPROTO_DICT: i64 = 512;
    pub const CURLPROTO_FILE: i64 = 1024;
    pub const CURLPROTO_FTP: i64 = 4;
    pub const CURLPROTO_FTPS: i64 = 8;
    pub const CURLPROTO_HTTP: i64 = 1;
    pub const CURLPROTO_HTTPS: i64 = 2;
    pub const CURLPROTO_LDAP: i64 = 128;
    pub const CURLPROTO_LDAPS: i64 = 256;
    pub const CURLPROTO_SCP: i64 = 16;
    pub const CURLPROTO_SFTP: i64 = 32;
    pub const CURLPROTO_TELNET: i64 = 64;
    pub const CURLPROTO_TFTP: i64 = 2048;
    pub const CURLPROXY_HTTP_1_0: i64 = 1;
    pub const CURLFTP_CREATE_DIR: i64 = 1;
    pub const CURLFTP_CREATE_DIR_NONE: i64 = 0;
    pub const CURLFTP_CREATE_DIR_RETRY: i64 = 2;
    pub const CURL_VERSION_CURLDEBUG: i64 = 8192;
    pub const CURLOPT_SSH_KNOWNHOSTS: i64 = 10183;
    pub const CURLKHMATCH_OK: i64 = 0;
    pub const CURLKHMATCH_MISMATCH: i64 = 1;
    pub const CURLKHMATCH_MISSING: i64 = 2;
    pub const CURLKHMATCH_LAST: i64 = 3;
    pub const CURLINFO_RTSP_CLIENT_CSEQ: i64 = 2097189;
    pub const CURLINFO_RTSP_CSEQ_RECV: i64 = 2097191;
    pub const CURLINFO_RTSP_SERVER_CSEQ: i64 = 2097190;
    pub const CURLINFO_RTSP_SESSION_ID: i64 = 1048612;
    pub const CURLOPT_FTP_USE_PRET: i64 = 188;
    pub const CURLOPT_MAIL_FROM: i64 = 10186;
    pub const CURLOPT_MAIL_RCPT: i64 = 10187;
    pub const CURLOPT_RTSP_CLIENT_CSEQ: i64 = 193;
    pub const CURLOPT_RTSP_REQUEST: i64 = 189;
    pub const CURLOPT_RTSP_SERVER_CSEQ: i64 = 194;
    pub const CURLOPT_RTSP_SESSION_ID: i64 = 10190;
    pub const CURLOPT_RTSP_STREAM_URI: i64 = 10191;
    pub const CURLOPT_RTSP_TRANSPORT: i64 = 10192;
    pub const CURLPROTO_IMAP: i64 = 4096;
    pub const CURLPROTO_IMAPS: i64 = 8192;
    pub const CURLPROTO_POP3: i64 = 16384;
    pub const CURLPROTO_POP3S: i64 = 32768;
    pub const CURLPROTO_RTSP: i64 = 262144;
    pub const CURLPROTO_SMTP: i64 = 65536;
    pub const CURLPROTO_SMTPS: i64 = 131072;
    pub const CURL_RTSPREQ_ANNOUNCE: i64 = 3;
    pub const CURL_RTSPREQ_DESCRIBE: i64 = 2;
    pub const CURL_RTSPREQ_GET_PARAMETER: i64 = 8;
    pub const CURL_RTSPREQ_OPTIONS: i64 = 1;
    pub const CURL_RTSPREQ_PAUSE: i64 = 6;
    pub const CURL_RTSPREQ_PLAY: i64 = 5;
    pub const CURL_RTSPREQ_RECEIVE: i64 = 11;
    pub const CURL_RTSPREQ_RECORD: i64 = 10;
    pub const CURL_RTSPREQ_SET_PARAMETER: i64 = 9;
    pub const CURL_RTSPREQ_SETUP: i64 = 4;
    pub const CURL_RTSPREQ_TEARDOWN: i64 = 7;
    pub const CURLINFO_LOCAL_IP: i64 = 1048617;
    pub const CURLINFO_LOCAL_PORT: i64 = 2097194;
    pub const CURLINFO_PRIMARY_PORT: i64 = 2097192;
    pub const CURLOPT_FNMATCH_FUNCTION: i64 = 20200;
    pub const CURLOPT_WILDCARDMATCH: i64 = 197;
    pub const CURLPROTO_RTMP: i64 = 524288;
    pub const CURLPROTO_RTMPE: i64 = 2097152;
    pub const CURLPROTO_RTMPS: i64 = 8388608;
    pub const CURLPROTO_RTMPT: i64 = 1048576;
    pub const CURLPROTO_RTMPTE: i64 = 4194304;
    pub const CURLPROTO_RTMPTS: i64 = 16777216;
    pub const CURL_FNMATCHFUNC_FAIL: i64 = 2;
    pub const CURL_FNMATCHFUNC_MATCH: i64 = 0;
    pub const CURL_FNMATCHFUNC_NOMATCH: i64 = 1;
    pub const CURLPROTO_GOPHER: i64 = 33554432;
    pub const CURLAUTH_ONLY: i64 = 2147483648;
    pub const CURLOPT_RESOLVE: i64 = 10203;
    pub const CURLOPT_TLSAUTH_PASSWORD: i64 = 10205;
    pub const CURLOPT_TLSAUTH_TYPE: i64 = 10206;
    pub const CURLOPT_TLSAUTH_USERNAME: i64 = 10204;
    pub const CURL_TLSAUTH_SRP: i64 = 1;
    pub const CURL_VERSION_TLSAUTH_SRP: i64 = 16384;
    pub const CURLOPT_ACCEPT_ENCODING: i64 = 10102;
    pub const CURLOPT_TRANSFER_ENCODING: i64 = 207;
    pub const CURLAUTH_NTLM_WB: i64 = 32;
    pub const CURLGSSAPI_DELEGATION_FLAG: i64 = 2;
    pub const CURLGSSAPI_DELEGATION_POLICY_FLAG: i64 = 1;
    pub const CURLOPT_GSSAPI_DELEGATION: i64 = 210;
    pub const CURL_VERSION_NTLM_WB: i64 = 32768;
    pub const CURLOPT_ACCEPTTIMEOUT_MS: i64 = 212;
    pub const CURLOPT_DNS_SERVERS: i64 = 10211;
    pub const CURLOPT_MAIL_AUTH: i64 = 10217;
    pub const CURLOPT_SSL_OPTIONS: i64 = 216;
    pub const CURLOPT_TCP_KEEPALIVE: i64 = 213;
    pub const CURLOPT_TCP_KEEPIDLE: i64 = 214;
    pub const CURLOPT_TCP_KEEPINTVL: i64 = 215;
    pub const CURLSSLOPT_ALLOW_BEAST: i64 = 1;
    pub const CURL_REDIR_POST_303: i64 = 4;
    pub const CURLSSH_AUTH_AGENT: i64 = 16;
    pub const CURLMOPT_CHUNK_LENGTH_PENALTY_SIZE: i64 = 30010;
    pub const CURLMOPT_CONTENT_LENGTH_PENALTY_SIZE: i64 = 30009;
    pub const CURLMOPT_MAX_HOST_CONNECTIONS: i64 = 7;
    pub const CURLMOPT_MAX_PIPELINE_LENGTH: i64 = 8;
    pub const CURLMOPT_MAX_TOTAL_CONNECTIONS: i64 = 13;
    pub const CURLOPT_SASL_IR: i64 = 218;
    pub const CURLOPT_DNS_INTERFACE: i64 = 10221;
    pub const CURLOPT_DNS_LOCAL_IP4: i64 = 10222;
    pub const CURLOPT_DNS_LOCAL_IP6: i64 = 10223;
    pub const CURLOPT_XOAUTH2_BEARER: i64 = 10220;
    pub const CURL_HTTP_VERSION_2_0: i64 = 3;
    pub const CURL_VERSION_HTTP2: i64 = 65536;
    pub const CURLOPT_LOGIN_OPTIONS: i64 = 10224;
    pub const CURL_SSLVERSION_TLSv1_0: i64 = 4;
    pub const CURL_SSLVERSION_TLSv1_1: i64 = 5;
    pub const CURL_SSLVERSION_TLSv1_2: i64 = 6;
    pub const CURLOPT_EXPECT_100_TIMEOUT_MS: i64 = 227;
    pub const CURLOPT_SSL_ENABLE_ALPN: i64 = 226;
    pub const CURLOPT_SSL_ENABLE_NPN: i64 = 225;
    pub const CURLHEADER_SEPARATE: i64 = 1;
    pub const CURLHEADER_UNIFIED: i64 = 0;
    pub const CURLOPT_HEADEROPT: i64 = 229;
    pub const CURLOPT_PROXYHEADER: i64 = 10228;
    pub const CURLAUTH_NEGOTIATE: i64 = 4;
    pub const CURL_VERSION_GSSAPI: i64 = 131072;
    pub const CURLOPT_PINNEDPUBLICKEY: i64 = 10230;
    pub const CURLOPT_UNIX_SOCKET_PATH: i64 = 10231;
    pub const CURLPROTO_SMB: i64 = 67108864;
    pub const CURLPROTO_SMBS: i64 = 134217728;
    pub const CURL_VERSION_KERBEROS5: i64 = 262144;
    pub const CURL_VERSION_UNIX_SOCKETS: i64 = 524288;
    pub const CURLOPT_SSL_VERIFYSTATUS: i64 = 232;
    pub const CURLOPT_PATH_AS_IS: i64 = 234;
    pub const CURLOPT_SSL_FALSESTART: i64 = 233;
    pub const CURL_HTTP_VERSION_2: i64 = 3;
    pub const CURLOPT_PIPEWAIT: i64 = 237;
    pub const CURLOPT_PROXY_SERVICE_NAME: i64 = 10235;
    pub const CURLOPT_SERVICE_NAME: i64 = 10236;
    pub const CURLPIPE_NOTHING: i64 = 0;
    pub const CURLPIPE_HTTP1: i64 = 1;
    pub const CURLPIPE_MULTIPLEX: i64 = 2;
    pub const CURLSSLOPT_NO_REVOKE: i64 = 2;
    pub const CURLOPT_DEFAULT_PROTOCOL: i64 = 10238;
    pub const CURLOPT_STREAM_WEIGHT: i64 = 239;
    pub const CURLMOPT_PUSHFUNCTION: i64 = 20014;
    pub const CURL_PUSH_OK: i64 = 0;
    pub const CURL_PUSH_DENY: i64 = 1;
    pub const CURL_HTTP_VERSION_2TLS: i64 = 4;
    pub const CURL_VERSION_PSL: i64 = 1048576;
    pub const CURLOPT_TFTP_NO_OPTIONS: i64 = 242;
    pub const CURL_HTTP_VERSION_2_PRIOR_KNOWLEDGE: i64 = 5;
    pub const CURLOPT_CONNECT_TO: i64 = 10243;
    pub const CURLOPT_TCP_FASTOPEN: i64 = 244;
    pub const CURLINFO_HTTP_VERSION: i64 = 2097198;
    pub const CURLE_WEIRD_SERVER_REPLY: i64 = 8;
    pub const CURLOPT_KEEP_SENDING_ON_ERROR: i64 = 245;
    pub const CURL_SSLVERSION_TLSv1_3: i64 = 7;
    pub const CURL_VERSION_HTTPS_PROXY: i64 = 2097152;
    pub const CURLINFO_PROTOCOL: i64 = 2097200;
    pub const CURLINFO_PROXY_SSL_VERIFYRESULT: i64 = 2097199;
    pub const CURLINFO_SCHEME: i64 = 1048625;
    pub const CURLOPT_PRE_PROXY: i64 = 10262;
    pub const CURLOPT_PROXY_CAINFO: i64 = 10246;
    pub const CURLOPT_PROXY_CAPATH: i64 = 10247;
    pub const CURLOPT_PROXY_CRLFILE: i64 = 10260;
    pub const CURLOPT_PROXY_KEYPASSWD: i64 = 10258;
    pub const CURLOPT_PROXY_PINNEDPUBLICKEY: i64 = 10263;
    pub const CURLOPT_PROXY_SSL_CIPHER_LIST: i64 = 10259;
    pub const CURLOPT_PROXY_SSL_OPTIONS: i64 = 261;
    pub const CURLOPT_PROXY_SSL_VERIFYHOST: i64 = 249;
    pub const CURLOPT_PROXY_SSL_VERIFYPEER: i64 = 248;
    pub const CURLOPT_PROXY_SSLCERT: i64 = 10254;
    pub const CURLOPT_PROXY_SSLCERTTYPE: i64 = 10255;
    pub const CURLOPT_PROXY_SSLKEY: i64 = 10256;
    pub const CURLOPT_PROXY_SSLKEYTYPE: i64 = 10257;
    pub const CURLOPT_PROXY_SSLVERSION: i64 = 250;
    pub const CURLOPT_PROXY_TLSAUTH_PASSWORD: i64 = 10252;
    pub const CURLOPT_PROXY_TLSAUTH_TYPE: i64 = 10253;
    pub const CURLOPT_PROXY_TLSAUTH_USERNAME: i64 = 10251;
    pub const CURLPROXY_HTTPS: i64 = 2;
    pub const CURL_MAX_READ_SIZE: i64 = 10485760;
    pub const CURLOPT_ABSTRACT_UNIX_SOCKET: i64 = 10264;
    pub const CURL_SSLVERSION_MAX_DEFAULT: i64 = 65536;
    pub const CURL_SSLVERSION_MAX_NONE: i64 = 0;
    pub const CURL_SSLVERSION_MAX_TLSv1_0: i64 = 262144;
    pub const CURL_SSLVERSION_MAX_TLSv1_1: i64 = 327680;
    pub const CURL_SSLVERSION_MAX_TLSv1_2: i64 = 393216;
    pub const CURL_SSLVERSION_MAX_TLSv1_3: i64 = 458752;
    pub const CURLOPT_SUPPRESS_CONNECT_HEADERS: i64 = 265;
    pub const CURLAUTH_GSSAPI: i64 = 4;
    pub const CURLINFO_CONTENT_LENGTH_DOWNLOAD_T: i64 = 6291471;
    pub const CURLINFO_CONTENT_LENGTH_UPLOAD_T: i64 = 6291472;
    pub const CURLINFO_SIZE_DOWNLOAD_T: i64 = 6291464;
    pub const CURLINFO_SIZE_UPLOAD_T: i64 = 6291463;
    pub const CURLINFO_SPEED_DOWNLOAD_T: i64 = 6291465;
    pub const CURLINFO_SPEED_UPLOAD_T: i64 = 6291466;
    pub const CURLOPT_REQUEST_TARGET: i64 = 10266;
    pub const CURLOPT_SOCKS5_AUTH: i64 = 267;
    pub const CURLOPT_SSH_COMPRESSION: i64 = 268;
    pub const CURL_VERSION_MULTI_SSL: i64 = 4194304;
    pub const CURL_VERSION_BROTLI: i64 = 8388608;
    pub const CURL_LOCK_DATA_CONNECT: i64 = 5;
    pub const CURLSSH_AUTH_GSSAPI: i64 = 32;
    pub const CURLINFO_FILETIME_T: i64 = 6291470;
    pub const CURLOPT_HAPPY_EYEBALLS_TIMEOUT_MS: i64 = 271;
    pub const CURLOPT_TIMEVALUE_LARGE: i64 = 30270;
    pub const CURLOPT_DNS_SHUFFLE_ADDRESSES: i64 = 275;
    pub const CURLOPT_HAPROXYPROTOCOL: i64 = 274;
    pub const CURL_LOCK_DATA_PSL: i64 = 6;
    pub const CURLAUTH_BEARER: i64 = 64;
    pub const CURLINFO_QUEUE_TIME_T: i64 = 6291521;
    pub const CURLINFO_APPCONNECT_TIME_T: i64 = 6291512;
    pub const CURLINFO_CONNECT_TIME_T: i64 = 6291508;
    pub const CURLINFO_NAMELOOKUP_TIME_T: i64 = 6291507;
    pub const CURLINFO_PRETRANSFER_TIME_T: i64 = 6291509;
    pub const CURLINFO_REDIRECT_TIME_T: i64 = 6291511;
    pub const CURLINFO_STARTTRANSFER_TIME_T: i64 = 6291510;
    pub const CURLINFO_TOTAL_TIME_T: i64 = 6291506;
    pub const CURLINFO_USED_PROXY: i64 = 2097218;
    pub const CURLINFO_POSTTRANSFER_TIME_T: i64 = 6291523;
    pub const CURLINFO_CONN_ID: i64 = 6291520;
    pub const CURLOPT_DISALLOW_USERNAME_IN_URL: i64 = 278;
    pub const CURLOPT_PROXY_TLS13_CIPHERS: i64 = 10277;
    pub const CURLOPT_TLS13_CIPHERS: i64 = 10276;
    pub const CURLOPT_DOH_URL: i64 = 10279;
    pub const CURLOPT_UPKEEP_INTERVAL_MS: i64 = 281;
    pub const CURLOPT_UPLOAD_BUFFERSIZE: i64 = 280;
    pub const CURLOPT_HTTP09_ALLOWED: i64 = 285;
    pub const CURLALTSVC_H1: i64 = 8;
    pub const CURLALTSVC_H2: i64 = 16;
    pub const CURLALTSVC_H3: i64 = 32;
    pub const CURLALTSVC_READONLYFILE: i64 = 4;
    pub const CURLOPT_ALTSVC: i64 = 10287;
    pub const CURLOPT_ALTSVC_CTRL: i64 = 286;
    pub const CURL_VERSION_ALTSVC: i64 = 16777216;
    pub const CURLOPT_MAXAGE_CONN: i64 = 288;
    pub const CURLOPT_SASL_AUTHZID: i64 = 10289;
    pub const CURL_VERSION_HTTP3: i64 = 33554432;
    pub const CURLINFO_RETRY_AFTER: i64 = 6291513;
    pub const CURL_HTTP_VERSION_3: i64 = 30;
    pub const CURLMOPT_MAX_CONCURRENT_STREAMS: i64 = 16;
    pub const CURLSSLOPT_NO_PARTIALCHAIN: i64 = 4;
    pub const CURLOPT_MAIL_RCPT_ALLLOWFAILS: i64 = 290;
    pub const CURLSSLOPT_REVOKE_BEST_EFFORT: i64 = 8;
    pub const CURLOPT_ISSUERCERT_BLOB: i64 = 40295;
    pub const CURLOPT_PROXY_ISSUERCERT: i64 = 10296;
    pub const CURLOPT_PROXY_ISSUERCERT_BLOB: i64 = 40297;
    pub const CURLOPT_PROXY_SSLCERT_BLOB: i64 = 40293;
    pub const CURLOPT_PROXY_SSLKEY_BLOB: i64 = 40294;
    pub const CURLOPT_SSLCERT_BLOB: i64 = 40291;
    pub const CURLOPT_SSLKEY_BLOB: i64 = 40292;
    pub const CURLPROTO_MQTT: i64 = 268435456;
    pub const CURLSSLOPT_NATIVE_CA: i64 = 16;
    pub const CURL_VERSION_UNICODE: i64 = 134217728;
    pub const CURL_VERSION_ZSTD: i64 = 67108864;
    pub const CURLE_PROXY: i64 = 97;
    pub const CURLINFO_PROXY_ERROR: i64 = 2097211;
    pub const CURLOPT_SSL_EC_CURVES: i64 = 10298;
    pub const CURLOPT_SSL_SIGNATURE_ALGORITHMS: i64 = 10328;
    pub const CURLPX_BAD_ADDRESS_TYPE: i64 = 1;
    pub const CURLPX_BAD_VERSION: i64 = 2;
    pub const CURLPX_CLOSED: i64 = 3;
    pub const CURLPX_GSSAPI: i64 = 4;
    pub const CURLPX_GSSAPI_PERMSG: i64 = 5;
    pub const CURLPX_GSSAPI_PROTECTION: i64 = 6;
    pub const CURLPX_IDENTD: i64 = 7;
    pub const CURLPX_IDENTD_DIFFER: i64 = 8;
    pub const CURLPX_LONG_HOSTNAME: i64 = 9;
    pub const CURLPX_LONG_PASSWD: i64 = 10;
    pub const CURLPX_LONG_USER: i64 = 11;
    pub const CURLPX_NO_AUTH: i64 = 12;
    pub const CURLPX_OK: i64 = 0;
    pub const CURLPX_RECV_ADDRESS: i64 = 13;
    pub const CURLPX_RECV_AUTH: i64 = 14;
    pub const CURLPX_RECV_CONNECT: i64 = 15;
    pub const CURLPX_RECV_REQACK: i64 = 16;
    pub const CURLPX_REPLY_ADDRESS_TYPE_NOT_SUPPORTED: i64 = 17;
    pub const CURLPX_REPLY_COMMAND_NOT_SUPPORTED: i64 = 18;
    pub const CURLPX_REPLY_CONNECTION_REFUSED: i64 = 19;
    pub const CURLPX_REPLY_GENERAL_SERVER_FAILURE: i64 = 20;
    pub const CURLPX_REPLY_HOST_UNREACHABLE: i64 = 21;
    pub const CURLPX_REPLY_NETWORK_UNREACHABLE: i64 = 22;
    pub const CURLPX_REPLY_NOT_ALLOWED: i64 = 23;
    pub const CURLPX_REPLY_TTL_EXPIRED: i64 = 24;
    pub const CURLPX_REPLY_UNASSIGNED: i64 = 25;
    pub const CURLPX_REQUEST_FAILED: i64 = 26;
    pub const CURLPX_RESOLVE_HOST: i64 = 27;
    pub const CURLPX_SEND_AUTH: i64 = 28;
    pub const CURLPX_SEND_CONNECT: i64 = 29;
    pub const CURLPX_SEND_REQUEST: i64 = 30;
    pub const CURLPX_UNKNOWN_FAIL: i64 = 31;
    pub const CURLPX_UNKNOWN_MODE: i64 = 32;
    pub const CURLPX_USER_REJECTED: i64 = 33;
    pub const CURLHSTS_ENABLE: i64 = 1;
    pub const CURLHSTS_READONLYFILE: i64 = 2;
    pub const CURLOPT_HSTS: i64 = 10300;
    pub const CURLOPT_HSTS_CTRL: i64 = 299;
    pub const CURL_VERSION_HSTS: i64 = 268435456;
    pub const CURLAUTH_AWS_SIGV4: i64 = 128;
    pub const CURLOPT_AWS_SIGV4: i64 = 10305;
    pub const CURLINFO_REFERER: i64 = 1048636;
    pub const CURLOPT_DOH_SSL_VERIFYHOST: i64 = 307;
    pub const CURLOPT_DOH_SSL_VERIFYPEER: i64 = 306;
    pub const CURLOPT_DOH_SSL_VERIFYSTATUS: i64 = 308;
    pub const CURL_VERSION_GSASL: i64 = 536870912;
    pub const CURLOPT_CAINFO_BLOB: i64 = 40309;
    pub const CURLOPT_PROXY_CAINFO_BLOB: i64 = 40310;
    pub const CURLSSLOPT_AUTO_CLIENT_CERT: i64 = 32;
    pub const CURLOPT_MAXLIFETIME_CONN: i64 = 314;
    pub const CURLOPT_SSH_HOST_PUBLIC_KEY_SHA256: i64 = 10311;
    pub const CURLOPT_PREREQFUNCTION: i64 = 20312;
    pub const CURL_PREREQFUNC_OK: i64 = 0;
    pub const CURL_PREREQFUNC_ABORT: i64 = 1;
    pub const CURLOPT_MIME_OPTIONS: i64 = 315;
    pub const CURLMIMEOPT_FORMESCAPE: i64 = 1;
    pub const CURLOPT_SSH_HOSTKEYFUNCTION: i64 = 20316;
    pub const CURLOPT_PROTOCOLS_STR: i64 = 10318;
    pub const CURLOPT_REDIR_PROTOCOLS_STR: i64 = 10319;
    pub const CURLOPT_WS_OPTIONS: i64 = 320;
    pub const CURLWS_RAW_MODE: i64 = 1;
    pub const CURLOPT_CA_CACHE_TIMEOUT: i64 = 321;
    pub const CURLOPT_QUICK_EXIT: i64 = 322;
    pub const CURL_HTTP_VERSION_3ONLY: i64 = 31;
    pub const CURLOPT_SAFE_UPLOAD: i64 = -1;
}
