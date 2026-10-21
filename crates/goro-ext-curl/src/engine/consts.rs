//! Constants from libcurl 8.14.1 include/curl/curl.h and multi.h (generated).
#![allow(non_upper_case_globals)]

/// CURLOPT_* option numbers
pub mod opt {
    pub const WRITEDATA: u32 = 10001;
    pub const URL: u32 = 10002;
    pub const PORT: u32 = 3;
    pub const PROXY: u32 = 10004;
    pub const USERPWD: u32 = 10005;
    pub const PROXYUSERPWD: u32 = 10006;
    pub const RANGE: u32 = 10007;
    pub const READDATA: u32 = 10009;
    pub const ERRORBUFFER: u32 = 10010;
    pub const WRITEFUNCTION: u32 = 20011;
    pub const READFUNCTION: u32 = 20012;
    pub const TIMEOUT: u32 = 13;
    pub const INFILESIZE: u32 = 14;
    pub const POSTFIELDS: u32 = 10015;
    pub const REFERER: u32 = 10016;
    pub const FTPPORT: u32 = 10017;
    pub const USERAGENT: u32 = 10018;
    pub const LOW_SPEED_LIMIT: u32 = 19;
    pub const LOW_SPEED_TIME: u32 = 20;
    pub const RESUME_FROM: u32 = 21;
    pub const COOKIE: u32 = 10022;
    pub const HTTPHEADER: u32 = 10023;
    pub const HTTPPOST: u32 = 10024;
    pub const SSLCERT: u32 = 10025;
    pub const KEYPASSWD: u32 = 10026;
    pub const CRLF: u32 = 27;
    pub const QUOTE: u32 = 10028;
    pub const HEADERDATA: u32 = 10029;
    pub const COOKIEFILE: u32 = 10031;
    pub const SSLVERSION: u32 = 32;
    pub const TIMECONDITION: u32 = 33;
    pub const TIMEVALUE: u32 = 34;
    pub const CUSTOMREQUEST: u32 = 10036;
    pub const STDERR: u32 = 10037;
    pub const POSTQUOTE: u32 = 10039;
    pub const VERBOSE: u32 = 41;
    pub const HEADER: u32 = 42;
    pub const NOPROGRESS: u32 = 43;
    pub const NOBODY: u32 = 44;
    pub const FAILONERROR: u32 = 45;
    pub const UPLOAD: u32 = 46;
    pub const POST: u32 = 47;
    pub const DIRLISTONLY: u32 = 48;
    pub const APPEND: u32 = 50;
    pub const NETRC: u32 = 51;
    pub const FOLLOWLOCATION: u32 = 52;
    pub const TRANSFERTEXT: u32 = 53;
    pub const PUT: u32 = 54;
    pub const PROGRESSFUNCTION: u32 = 20056;
    pub const XFERINFODATA: u32 = 10057;
    pub const AUTOREFERER: u32 = 58;
    pub const PROXYPORT: u32 = 59;
    pub const POSTFIELDSIZE: u32 = 60;
    pub const HTTPPROXYTUNNEL: u32 = 61;
    pub const INTERFACE: u32 = 10062;
    pub const KRBLEVEL: u32 = 10063;
    pub const SSL_VERIFYPEER: u32 = 64;
    pub const CAINFO: u32 = 10065;
    pub const MAXREDIRS: u32 = 68;
    pub const FILETIME: u32 = 69;
    pub const TELNETOPTIONS: u32 = 10070;
    pub const MAXCONNECTS: u32 = 71;
    pub const FRESH_CONNECT: u32 = 74;
    pub const FORBID_REUSE: u32 = 75;
    pub const RANDOM_FILE: u32 = 10076;
    pub const EGDSOCKET: u32 = 10077;
    pub const CONNECTTIMEOUT: u32 = 78;
    pub const HEADERFUNCTION: u32 = 20079;
    pub const HTTPGET: u32 = 80;
    pub const SSL_VERIFYHOST: u32 = 81;
    pub const COOKIEJAR: u32 = 10082;
    pub const SSL_CIPHER_LIST: u32 = 10083;
    pub const HTTP_VERSION: u32 = 84;
    pub const FTP_USE_EPSV: u32 = 85;
    pub const SSLCERTTYPE: u32 = 10086;
    pub const SSLKEY: u32 = 10087;
    pub const SSLKEYTYPE: u32 = 10088;
    pub const SSLENGINE: u32 = 10089;
    pub const SSLENGINE_DEFAULT: u32 = 90;
    pub const DNS_USE_GLOBAL_CACHE: u32 = 91;
    pub const DNS_CACHE_TIMEOUT: u32 = 92;
    pub const PREQUOTE: u32 = 10093;
    pub const DEBUGFUNCTION: u32 = 20094;
    pub const DEBUGDATA: u32 = 10095;
    pub const COOKIESESSION: u32 = 96;
    pub const CAPATH: u32 = 10097;
    pub const BUFFERSIZE: u32 = 98;
    pub const NOSIGNAL: u32 = 99;
    pub const SHARE: u32 = 10100;
    pub const PROXYTYPE: u32 = 101;
    pub const ACCEPT_ENCODING: u32 = 10102;
    pub const PRIVATE: u32 = 10103;
    pub const HTTP200ALIASES: u32 = 10104;
    pub const UNRESTRICTED_AUTH: u32 = 105;
    pub const FTP_USE_EPRT: u32 = 106;
    pub const HTTPAUTH: u32 = 107;
    pub const SSL_CTX_FUNCTION: u32 = 20108;
    pub const SSL_CTX_DATA: u32 = 10109;
    pub const FTP_CREATE_MISSING_DIRS: u32 = 110;
    pub const PROXYAUTH: u32 = 111;
    pub const SERVER_RESPONSE_TIMEOUT: u32 = 112;
    pub const IPRESOLVE: u32 = 113;
    pub const MAXFILESIZE: u32 = 114;
    pub const INFILESIZE_LARGE: u32 = 30115;
    pub const RESUME_FROM_LARGE: u32 = 30116;
    pub const MAXFILESIZE_LARGE: u32 = 30117;
    pub const NETRC_FILE: u32 = 10118;
    pub const USE_SSL: u32 = 119;
    pub const POSTFIELDSIZE_LARGE: u32 = 30120;
    pub const TCP_NODELAY: u32 = 121;
    pub const FTPSSLAUTH: u32 = 129;
    pub const IOCTLFUNCTION: u32 = 20130;
    pub const IOCTLDATA: u32 = 10131;
    pub const FTP_ACCOUNT: u32 = 10134;
    pub const COOKIELIST: u32 = 10135;
    pub const IGNORE_CONTENT_LENGTH: u32 = 136;
    pub const FTP_SKIP_PASV_IP: u32 = 137;
    pub const FTP_FILEMETHOD: u32 = 138;
    pub const LOCALPORT: u32 = 139;
    pub const LOCALPORTRANGE: u32 = 140;
    pub const CONNECT_ONLY: u32 = 141;
    pub const CONV_FROM_NETWORK_FUNCTION: u32 = 20142;
    pub const CONV_TO_NETWORK_FUNCTION: u32 = 20143;
    pub const CONV_FROM_UTF8_FUNCTION: u32 = 20144;
    pub const MAX_SEND_SPEED_LARGE: u32 = 30145;
    pub const MAX_RECV_SPEED_LARGE: u32 = 30146;
    pub const FTP_ALTERNATIVE_TO_USER: u32 = 10147;
    pub const SOCKOPTFUNCTION: u32 = 20148;
    pub const SOCKOPTDATA: u32 = 10149;
    pub const SSL_SESSIONID_CACHE: u32 = 150;
    pub const SSH_AUTH_TYPES: u32 = 151;
    pub const SSH_PUBLIC_KEYFILE: u32 = 10152;
    pub const SSH_PRIVATE_KEYFILE: u32 = 10153;
    pub const FTP_SSL_CCC: u32 = 154;
    pub const TIMEOUT_MS: u32 = 155;
    pub const CONNECTTIMEOUT_MS: u32 = 156;
    pub const HTTP_TRANSFER_DECODING: u32 = 157;
    pub const HTTP_CONTENT_DECODING: u32 = 158;
    pub const NEW_FILE_PERMS: u32 = 159;
    pub const NEW_DIRECTORY_PERMS: u32 = 160;
    pub const POSTREDIR: u32 = 161;
    pub const SSH_HOST_PUBLIC_KEY_MD5: u32 = 10162;
    pub const OPENSOCKETFUNCTION: u32 = 20163;
    pub const OPENSOCKETDATA: u32 = 10164;
    pub const COPYPOSTFIELDS: u32 = 10165;
    pub const PROXY_TRANSFER_MODE: u32 = 166;
    pub const SEEKFUNCTION: u32 = 20167;
    pub const SEEKDATA: u32 = 10168;
    pub const CRLFILE: u32 = 10169;
    pub const ISSUERCERT: u32 = 10170;
    pub const ADDRESS_SCOPE: u32 = 171;
    pub const CERTINFO: u32 = 172;
    pub const USERNAME: u32 = 10173;
    pub const PASSWORD: u32 = 10174;
    pub const PROXYUSERNAME: u32 = 10175;
    pub const PROXYPASSWORD: u32 = 10176;
    pub const NOPROXY: u32 = 10177;
    pub const TFTP_BLKSIZE: u32 = 178;
    pub const SOCKS5_GSSAPI_SERVICE: u32 = 10179;
    pub const SOCKS5_GSSAPI_NEC: u32 = 180;
    pub const PROTOCOLS: u32 = 181;
    pub const REDIR_PROTOCOLS: u32 = 182;
    pub const SSH_KNOWNHOSTS: u32 = 10183;
    pub const SSH_KEYFUNCTION: u32 = 20184;
    pub const SSH_KEYDATA: u32 = 10185;
    pub const MAIL_FROM: u32 = 10186;
    pub const MAIL_RCPT: u32 = 10187;
    pub const FTP_USE_PRET: u32 = 188;
    pub const RTSP_REQUEST: u32 = 189;
    pub const RTSP_SESSION_ID: u32 = 10190;
    pub const RTSP_STREAM_URI: u32 = 10191;
    pub const RTSP_TRANSPORT: u32 = 10192;
    pub const RTSP_CLIENT_CSEQ: u32 = 193;
    pub const RTSP_SERVER_CSEQ: u32 = 194;
    pub const INTERLEAVEDATA: u32 = 10195;
    pub const INTERLEAVEFUNCTION: u32 = 20196;
    pub const WILDCARDMATCH: u32 = 197;
    pub const CHUNK_BGN_FUNCTION: u32 = 20198;
    pub const CHUNK_END_FUNCTION: u32 = 20199;
    pub const FNMATCH_FUNCTION: u32 = 20200;
    pub const CHUNK_DATA: u32 = 10201;
    pub const FNMATCH_DATA: u32 = 10202;
    pub const RESOLVE: u32 = 10203;
    pub const TLSAUTH_USERNAME: u32 = 10204;
    pub const TLSAUTH_PASSWORD: u32 = 10205;
    pub const TLSAUTH_TYPE: u32 = 10206;
    pub const TRANSFER_ENCODING: u32 = 207;
    pub const CLOSESOCKETFUNCTION: u32 = 20208;
    pub const CLOSESOCKETDATA: u32 = 10209;
    pub const GSSAPI_DELEGATION: u32 = 210;
    pub const DNS_SERVERS: u32 = 10211;
    pub const ACCEPTTIMEOUT_MS: u32 = 212;
    pub const TCP_KEEPALIVE: u32 = 213;
    pub const TCP_KEEPIDLE: u32 = 214;
    pub const TCP_KEEPINTVL: u32 = 215;
    pub const SSL_OPTIONS: u32 = 216;
    pub const MAIL_AUTH: u32 = 10217;
    pub const SASL_IR: u32 = 218;
    pub const XFERINFOFUNCTION: u32 = 20219;
    pub const XOAUTH2_BEARER: u32 = 10220;
    pub const DNS_INTERFACE: u32 = 10221;
    pub const DNS_LOCAL_IP4: u32 = 10222;
    pub const DNS_LOCAL_IP6: u32 = 10223;
    pub const LOGIN_OPTIONS: u32 = 10224;
    pub const SSL_ENABLE_NPN: u32 = 225;
    pub const SSL_ENABLE_ALPN: u32 = 226;
    pub const EXPECT_100_TIMEOUT_MS: u32 = 227;
    pub const PROXYHEADER: u32 = 10228;
    pub const HEADEROPT: u32 = 229;
    pub const PINNEDPUBLICKEY: u32 = 10230;
    pub const UNIX_SOCKET_PATH: u32 = 10231;
    pub const SSL_VERIFYSTATUS: u32 = 232;
    pub const SSL_FALSESTART: u32 = 233;
    pub const PATH_AS_IS: u32 = 234;
    pub const PROXY_SERVICE_NAME: u32 = 10235;
    pub const SERVICE_NAME: u32 = 10236;
    pub const PIPEWAIT: u32 = 237;
    pub const DEFAULT_PROTOCOL: u32 = 10238;
    pub const STREAM_WEIGHT: u32 = 239;
    pub const STREAM_DEPENDS: u32 = 10240;
    pub const STREAM_DEPENDS_E: u32 = 10241;
    pub const TFTP_NO_OPTIONS: u32 = 242;
    pub const CONNECT_TO: u32 = 10243;
    pub const TCP_FASTOPEN: u32 = 244;
    pub const KEEP_SENDING_ON_ERROR: u32 = 245;
    pub const PROXY_CAINFO: u32 = 10246;
    pub const PROXY_CAPATH: u32 = 10247;
    pub const PROXY_SSL_VERIFYPEER: u32 = 248;
    pub const PROXY_SSL_VERIFYHOST: u32 = 249;
    pub const PROXY_SSLVERSION: u32 = 250;
    pub const PROXY_TLSAUTH_USERNAME: u32 = 10251;
    pub const PROXY_TLSAUTH_PASSWORD: u32 = 10252;
    pub const PROXY_TLSAUTH_TYPE: u32 = 10253;
    pub const PROXY_SSLCERT: u32 = 10254;
    pub const PROXY_SSLCERTTYPE: u32 = 10255;
    pub const PROXY_SSLKEY: u32 = 10256;
    pub const PROXY_SSLKEYTYPE: u32 = 10257;
    pub const PROXY_KEYPASSWD: u32 = 10258;
    pub const PROXY_SSL_CIPHER_LIST: u32 = 10259;
    pub const PROXY_CRLFILE: u32 = 10260;
    pub const PROXY_SSL_OPTIONS: u32 = 261;
    pub const PRE_PROXY: u32 = 10262;
    pub const PROXY_PINNEDPUBLICKEY: u32 = 10263;
    pub const ABSTRACT_UNIX_SOCKET: u32 = 10264;
    pub const SUPPRESS_CONNECT_HEADERS: u32 = 265;
    pub const REQUEST_TARGET: u32 = 10266;
    pub const SOCKS5_AUTH: u32 = 267;
    pub const SSH_COMPRESSION: u32 = 268;
    pub const MIMEPOST: u32 = 10269;
    pub const TIMEVALUE_LARGE: u32 = 30270;
    pub const HAPPY_EYEBALLS_TIMEOUT_MS: u32 = 271;
    pub const RESOLVER_START_FUNCTION: u32 = 20272;
    pub const RESOLVER_START_DATA: u32 = 10273;
    pub const HAPROXYPROTOCOL: u32 = 274;
    pub const DNS_SHUFFLE_ADDRESSES: u32 = 275;
    pub const TLS13_CIPHERS: u32 = 10276;
    pub const PROXY_TLS13_CIPHERS: u32 = 10277;
    pub const DISALLOW_USERNAME_IN_URL: u32 = 278;
    pub const DOH_URL: u32 = 10279;
    pub const UPLOAD_BUFFERSIZE: u32 = 280;
    pub const UPKEEP_INTERVAL_MS: u32 = 281;
    pub const CURLU: u32 = 10282;
    pub const TRAILERFUNCTION: u32 = 20283;
    pub const TRAILERDATA: u32 = 10284;
    pub const HTTP09_ALLOWED: u32 = 285;
    pub const ALTSVC_CTRL: u32 = 286;
    pub const ALTSVC: u32 = 10287;
    pub const MAXAGE_CONN: u32 = 288;
    pub const SASL_AUTHZID: u32 = 10289;
    pub const MAIL_RCPT_ALLOWFAILS: u32 = 290;
    pub const SSLCERT_BLOB: u32 = 40291;
    pub const SSLKEY_BLOB: u32 = 40292;
    pub const PROXY_SSLCERT_BLOB: u32 = 40293;
    pub const PROXY_SSLKEY_BLOB: u32 = 40294;
    pub const ISSUERCERT_BLOB: u32 = 40295;
    pub const PROXY_ISSUERCERT: u32 = 10296;
    pub const PROXY_ISSUERCERT_BLOB: u32 = 40297;
    pub const SSL_EC_CURVES: u32 = 10298;
    pub const HSTS_CTRL: u32 = 299;
    pub const HSTS: u32 = 10300;
    pub const HSTSREADFUNCTION: u32 = 20301;
    pub const HSTSREADDATA: u32 = 10302;
    pub const HSTSWRITEFUNCTION: u32 = 20303;
    pub const HSTSWRITEDATA: u32 = 10304;
    pub const AWS_SIGV4: u32 = 10305;
    pub const DOH_SSL_VERIFYPEER: u32 = 306;
    pub const DOH_SSL_VERIFYHOST: u32 = 307;
    pub const DOH_SSL_VERIFYSTATUS: u32 = 308;
    pub const CAINFO_BLOB: u32 = 40309;
    pub const PROXY_CAINFO_BLOB: u32 = 40310;
    pub const SSH_HOST_PUBLIC_KEY_SHA256: u32 = 10311;
    pub const PREREQFUNCTION: u32 = 20312;
    pub const PREREQDATA: u32 = 10313;
    pub const MAXLIFETIME_CONN: u32 = 314;
    pub const MIME_OPTIONS: u32 = 315;
    pub const SSH_HOSTKEYFUNCTION: u32 = 20316;
    pub const SSH_HOSTKEYDATA: u32 = 10317;
    pub const PROTOCOLS_STR: u32 = 10318;
    pub const REDIR_PROTOCOLS_STR: u32 = 10319;
    pub const WS_OPTIONS: u32 = 320;
    pub const CA_CACHE_TIMEOUT: u32 = 321;
    pub const QUICK_EXIT: u32 = 322;
    pub const HAPROXY_CLIENT_IP: u32 = 10323;
    pub const SERVER_RESPONSE_TIMEOUT_MS: u32 = 324;
    pub const ECH: u32 = 10325;
    pub const TCP_KEEPCNT: u32 = 326;
    pub const UPLOAD_FLAGS: u32 = 327;
    pub const SSL_SIGNATURE_ALGORITHMS: u32 = 10328;
}

/// Option kinds (CURLOPTTYPE_*)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptKind {
    Long,
    Values,
    ObjectPoint,
    StringPoint,
    SlistPoint,
    CbPoint,
    FunctionPoint,
    OffT,
    Blob,
}

/// Every option libcurl 8.14.1 knows: (number, name, kind)
pub static OPTIONS: &[(u32, &str, OptKind)] = &[
    (10001, "WRITEDATA", OptKind::CbPoint),
    (10002, "URL", OptKind::StringPoint),
    (3, "PORT", OptKind::Long),
    (10004, "PROXY", OptKind::StringPoint),
    (10005, "USERPWD", OptKind::StringPoint),
    (10006, "PROXYUSERPWD", OptKind::StringPoint),
    (10007, "RANGE", OptKind::StringPoint),
    (10009, "READDATA", OptKind::CbPoint),
    (10010, "ERRORBUFFER", OptKind::ObjectPoint),
    (20011, "WRITEFUNCTION", OptKind::FunctionPoint),
    (20012, "READFUNCTION", OptKind::FunctionPoint),
    (13, "TIMEOUT", OptKind::Long),
    (14, "INFILESIZE", OptKind::Long),
    (10015, "POSTFIELDS", OptKind::ObjectPoint),
    (10016, "REFERER", OptKind::StringPoint),
    (10017, "FTPPORT", OptKind::StringPoint),
    (10018, "USERAGENT", OptKind::StringPoint),
    (19, "LOW_SPEED_LIMIT", OptKind::Long),
    (20, "LOW_SPEED_TIME", OptKind::Long),
    (21, "RESUME_FROM", OptKind::Long),
    (10022, "COOKIE", OptKind::StringPoint),
    (10023, "HTTPHEADER", OptKind::SlistPoint),
    (10024, "HTTPPOST", OptKind::ObjectPoint),
    (10025, "SSLCERT", OptKind::StringPoint),
    (10026, "KEYPASSWD", OptKind::StringPoint),
    (27, "CRLF", OptKind::Long),
    (10028, "QUOTE", OptKind::SlistPoint),
    (10029, "HEADERDATA", OptKind::CbPoint),
    (10031, "COOKIEFILE", OptKind::StringPoint),
    (32, "SSLVERSION", OptKind::Values),
    (33, "TIMECONDITION", OptKind::Values),
    (34, "TIMEVALUE", OptKind::Long),
    (10036, "CUSTOMREQUEST", OptKind::StringPoint),
    (10037, "STDERR", OptKind::ObjectPoint),
    (10039, "POSTQUOTE", OptKind::SlistPoint),
    (41, "VERBOSE", OptKind::Long),
    (42, "HEADER", OptKind::Long),
    (43, "NOPROGRESS", OptKind::Long),
    (44, "NOBODY", OptKind::Long),
    (45, "FAILONERROR", OptKind::Long),
    (46, "UPLOAD", OptKind::Long),
    (47, "POST", OptKind::Long),
    (48, "DIRLISTONLY", OptKind::Long),
    (50, "APPEND", OptKind::Long),
    (51, "NETRC", OptKind::Values),
    (52, "FOLLOWLOCATION", OptKind::Long),
    (53, "TRANSFERTEXT", OptKind::Long),
    (54, "PUT", OptKind::Long),
    (20056, "PROGRESSFUNCTION", OptKind::FunctionPoint),
    (10057, "XFERINFODATA", OptKind::CbPoint),
    (58, "AUTOREFERER", OptKind::Long),
    (59, "PROXYPORT", OptKind::Long),
    (60, "POSTFIELDSIZE", OptKind::Long),
    (61, "HTTPPROXYTUNNEL", OptKind::Long),
    (10062, "INTERFACE", OptKind::StringPoint),
    (10063, "KRBLEVEL", OptKind::StringPoint),
    (64, "SSL_VERIFYPEER", OptKind::Long),
    (10065, "CAINFO", OptKind::StringPoint),
    (68, "MAXREDIRS", OptKind::Long),
    (69, "FILETIME", OptKind::Long),
    (10070, "TELNETOPTIONS", OptKind::SlistPoint),
    (71, "MAXCONNECTS", OptKind::Long),
    (74, "FRESH_CONNECT", OptKind::Long),
    (75, "FORBID_REUSE", OptKind::Long),
    (10076, "RANDOM_FILE", OptKind::StringPoint),
    (10077, "EGDSOCKET", OptKind::StringPoint),
    (78, "CONNECTTIMEOUT", OptKind::Long),
    (20079, "HEADERFUNCTION", OptKind::FunctionPoint),
    (80, "HTTPGET", OptKind::Long),
    (81, "SSL_VERIFYHOST", OptKind::Long),
    (10082, "COOKIEJAR", OptKind::StringPoint),
    (10083, "SSL_CIPHER_LIST", OptKind::StringPoint),
    (84, "HTTP_VERSION", OptKind::Values),
    (85, "FTP_USE_EPSV", OptKind::Long),
    (10086, "SSLCERTTYPE", OptKind::StringPoint),
    (10087, "SSLKEY", OptKind::StringPoint),
    (10088, "SSLKEYTYPE", OptKind::StringPoint),
    (10089, "SSLENGINE", OptKind::StringPoint),
    (90, "SSLENGINE_DEFAULT", OptKind::Long),
    (91, "DNS_USE_GLOBAL_CACHE", OptKind::Long),
    (92, "DNS_CACHE_TIMEOUT", OptKind::Long),
    (10093, "PREQUOTE", OptKind::SlistPoint),
    (20094, "DEBUGFUNCTION", OptKind::FunctionPoint),
    (10095, "DEBUGDATA", OptKind::CbPoint),
    (96, "COOKIESESSION", OptKind::Long),
    (10097, "CAPATH", OptKind::StringPoint),
    (98, "BUFFERSIZE", OptKind::Long),
    (99, "NOSIGNAL", OptKind::Long),
    (10100, "SHARE", OptKind::ObjectPoint),
    (101, "PROXYTYPE", OptKind::Values),
    (10102, "ACCEPT_ENCODING", OptKind::StringPoint),
    (10103, "PRIVATE", OptKind::ObjectPoint),
    (10104, "HTTP200ALIASES", OptKind::SlistPoint),
    (105, "UNRESTRICTED_AUTH", OptKind::Long),
    (106, "FTP_USE_EPRT", OptKind::Long),
    (107, "HTTPAUTH", OptKind::Values),
    (20108, "SSL_CTX_FUNCTION", OptKind::FunctionPoint),
    (10109, "SSL_CTX_DATA", OptKind::CbPoint),
    (110, "FTP_CREATE_MISSING_DIRS", OptKind::Long),
    (111, "PROXYAUTH", OptKind::Values),
    (112, "SERVER_RESPONSE_TIMEOUT", OptKind::Long),
    (113, "IPRESOLVE", OptKind::Values),
    (114, "MAXFILESIZE", OptKind::Long),
    (30115, "INFILESIZE_LARGE", OptKind::OffT),
    (30116, "RESUME_FROM_LARGE", OptKind::OffT),
    (30117, "MAXFILESIZE_LARGE", OptKind::OffT),
    (10118, "NETRC_FILE", OptKind::StringPoint),
    (119, "USE_SSL", OptKind::Values),
    (30120, "POSTFIELDSIZE_LARGE", OptKind::OffT),
    (121, "TCP_NODELAY", OptKind::Long),
    (129, "FTPSSLAUTH", OptKind::Values),
    (20130, "IOCTLFUNCTION", OptKind::FunctionPoint),
    (10131, "IOCTLDATA", OptKind::CbPoint),
    (10134, "FTP_ACCOUNT", OptKind::StringPoint),
    (10135, "COOKIELIST", OptKind::StringPoint),
    (136, "IGNORE_CONTENT_LENGTH", OptKind::Long),
    (137, "FTP_SKIP_PASV_IP", OptKind::Long),
    (138, "FTP_FILEMETHOD", OptKind::Values),
    (139, "LOCALPORT", OptKind::Long),
    (140, "LOCALPORTRANGE", OptKind::Long),
    (141, "CONNECT_ONLY", OptKind::Long),
    (20142, "CONV_FROM_NETWORK_FUNCTION", OptKind::FunctionPoint),
    (20143, "CONV_TO_NETWORK_FUNCTION", OptKind::FunctionPoint),
    (20144, "CONV_FROM_UTF8_FUNCTION", OptKind::FunctionPoint),
    (30145, "MAX_SEND_SPEED_LARGE", OptKind::OffT),
    (30146, "MAX_RECV_SPEED_LARGE", OptKind::OffT),
    (10147, "FTP_ALTERNATIVE_TO_USER", OptKind::StringPoint),
    (20148, "SOCKOPTFUNCTION", OptKind::FunctionPoint),
    (10149, "SOCKOPTDATA", OptKind::CbPoint),
    (150, "SSL_SESSIONID_CACHE", OptKind::Long),
    (151, "SSH_AUTH_TYPES", OptKind::Values),
    (10152, "SSH_PUBLIC_KEYFILE", OptKind::StringPoint),
    (10153, "SSH_PRIVATE_KEYFILE", OptKind::StringPoint),
    (154, "FTP_SSL_CCC", OptKind::Long),
    (155, "TIMEOUT_MS", OptKind::Long),
    (156, "CONNECTTIMEOUT_MS", OptKind::Long),
    (157, "HTTP_TRANSFER_DECODING", OptKind::Long),
    (158, "HTTP_CONTENT_DECODING", OptKind::Long),
    (159, "NEW_FILE_PERMS", OptKind::Long),
    (160, "NEW_DIRECTORY_PERMS", OptKind::Long),
    (161, "POSTREDIR", OptKind::Values),
    (10162, "SSH_HOST_PUBLIC_KEY_MD5", OptKind::StringPoint),
    (20163, "OPENSOCKETFUNCTION", OptKind::FunctionPoint),
    (10164, "OPENSOCKETDATA", OptKind::CbPoint),
    (10165, "COPYPOSTFIELDS", OptKind::ObjectPoint),
    (166, "PROXY_TRANSFER_MODE", OptKind::Long),
    (20167, "SEEKFUNCTION", OptKind::FunctionPoint),
    (10168, "SEEKDATA", OptKind::CbPoint),
    (10169, "CRLFILE", OptKind::StringPoint),
    (10170, "ISSUERCERT", OptKind::StringPoint),
    (171, "ADDRESS_SCOPE", OptKind::Long),
    (172, "CERTINFO", OptKind::Long),
    (10173, "USERNAME", OptKind::StringPoint),
    (10174, "PASSWORD", OptKind::StringPoint),
    (10175, "PROXYUSERNAME", OptKind::StringPoint),
    (10176, "PROXYPASSWORD", OptKind::StringPoint),
    (10177, "NOPROXY", OptKind::StringPoint),
    (178, "TFTP_BLKSIZE", OptKind::Long),
    (10179, "SOCKS5_GSSAPI_SERVICE", OptKind::StringPoint),
    (180, "SOCKS5_GSSAPI_NEC", OptKind::Long),
    (181, "PROTOCOLS", OptKind::Long),
    (182, "REDIR_PROTOCOLS", OptKind::Long),
    (10183, "SSH_KNOWNHOSTS", OptKind::StringPoint),
    (20184, "SSH_KEYFUNCTION", OptKind::FunctionPoint),
    (10185, "SSH_KEYDATA", OptKind::CbPoint),
    (10186, "MAIL_FROM", OptKind::StringPoint),
    (10187, "MAIL_RCPT", OptKind::SlistPoint),
    (188, "FTP_USE_PRET", OptKind::Long),
    (189, "RTSP_REQUEST", OptKind::Values),
    (10190, "RTSP_SESSION_ID", OptKind::StringPoint),
    (10191, "RTSP_STREAM_URI", OptKind::StringPoint),
    (10192, "RTSP_TRANSPORT", OptKind::StringPoint),
    (193, "RTSP_CLIENT_CSEQ", OptKind::Long),
    (194, "RTSP_SERVER_CSEQ", OptKind::Long),
    (10195, "INTERLEAVEDATA", OptKind::CbPoint),
    (20196, "INTERLEAVEFUNCTION", OptKind::FunctionPoint),
    (197, "WILDCARDMATCH", OptKind::Long),
    (20198, "CHUNK_BGN_FUNCTION", OptKind::FunctionPoint),
    (20199, "CHUNK_END_FUNCTION", OptKind::FunctionPoint),
    (20200, "FNMATCH_FUNCTION", OptKind::FunctionPoint),
    (10201, "CHUNK_DATA", OptKind::CbPoint),
    (10202, "FNMATCH_DATA", OptKind::CbPoint),
    (10203, "RESOLVE", OptKind::SlistPoint),
    (10204, "TLSAUTH_USERNAME", OptKind::StringPoint),
    (10205, "TLSAUTH_PASSWORD", OptKind::StringPoint),
    (10206, "TLSAUTH_TYPE", OptKind::StringPoint),
    (207, "TRANSFER_ENCODING", OptKind::Long),
    (20208, "CLOSESOCKETFUNCTION", OptKind::FunctionPoint),
    (10209, "CLOSESOCKETDATA", OptKind::CbPoint),
    (210, "GSSAPI_DELEGATION", OptKind::Values),
    (10211, "DNS_SERVERS", OptKind::StringPoint),
    (212, "ACCEPTTIMEOUT_MS", OptKind::Long),
    (213, "TCP_KEEPALIVE", OptKind::Long),
    (214, "TCP_KEEPIDLE", OptKind::Long),
    (215, "TCP_KEEPINTVL", OptKind::Long),
    (216, "SSL_OPTIONS", OptKind::Values),
    (10217, "MAIL_AUTH", OptKind::StringPoint),
    (218, "SASL_IR", OptKind::Long),
    (20219, "XFERINFOFUNCTION", OptKind::FunctionPoint),
    (10220, "XOAUTH2_BEARER", OptKind::StringPoint),
    (10221, "DNS_INTERFACE", OptKind::StringPoint),
    (10222, "DNS_LOCAL_IP4", OptKind::StringPoint),
    (10223, "DNS_LOCAL_IP6", OptKind::StringPoint),
    (10224, "LOGIN_OPTIONS", OptKind::StringPoint),
    (225, "SSL_ENABLE_NPN", OptKind::Long),
    (226, "SSL_ENABLE_ALPN", OptKind::Long),
    (227, "EXPECT_100_TIMEOUT_MS", OptKind::Long),
    (10228, "PROXYHEADER", OptKind::SlistPoint),
    (229, "HEADEROPT", OptKind::Values),
    (10230, "PINNEDPUBLICKEY", OptKind::StringPoint),
    (10231, "UNIX_SOCKET_PATH", OptKind::StringPoint),
    (232, "SSL_VERIFYSTATUS", OptKind::Long),
    (233, "SSL_FALSESTART", OptKind::Long),
    (234, "PATH_AS_IS", OptKind::Long),
    (10235, "PROXY_SERVICE_NAME", OptKind::StringPoint),
    (10236, "SERVICE_NAME", OptKind::StringPoint),
    (237, "PIPEWAIT", OptKind::Long),
    (10238, "DEFAULT_PROTOCOL", OptKind::StringPoint),
    (239, "STREAM_WEIGHT", OptKind::Long),
    (10240, "STREAM_DEPENDS", OptKind::ObjectPoint),
    (10241, "STREAM_DEPENDS_E", OptKind::ObjectPoint),
    (242, "TFTP_NO_OPTIONS", OptKind::Long),
    (10243, "CONNECT_TO", OptKind::SlistPoint),
    (244, "TCP_FASTOPEN", OptKind::Long),
    (245, "KEEP_SENDING_ON_ERROR", OptKind::Long),
    (10246, "PROXY_CAINFO", OptKind::StringPoint),
    (10247, "PROXY_CAPATH", OptKind::StringPoint),
    (248, "PROXY_SSL_VERIFYPEER", OptKind::Long),
    (249, "PROXY_SSL_VERIFYHOST", OptKind::Long),
    (250, "PROXY_SSLVERSION", OptKind::Values),
    (10251, "PROXY_TLSAUTH_USERNAME", OptKind::StringPoint),
    (10252, "PROXY_TLSAUTH_PASSWORD", OptKind::StringPoint),
    (10253, "PROXY_TLSAUTH_TYPE", OptKind::StringPoint),
    (10254, "PROXY_SSLCERT", OptKind::StringPoint),
    (10255, "PROXY_SSLCERTTYPE", OptKind::StringPoint),
    (10256, "PROXY_SSLKEY", OptKind::StringPoint),
    (10257, "PROXY_SSLKEYTYPE", OptKind::StringPoint),
    (10258, "PROXY_KEYPASSWD", OptKind::StringPoint),
    (10259, "PROXY_SSL_CIPHER_LIST", OptKind::StringPoint),
    (10260, "PROXY_CRLFILE", OptKind::StringPoint),
    (261, "PROXY_SSL_OPTIONS", OptKind::Long),
    (10262, "PRE_PROXY", OptKind::StringPoint),
    (10263, "PROXY_PINNEDPUBLICKEY", OptKind::StringPoint),
    (10264, "ABSTRACT_UNIX_SOCKET", OptKind::StringPoint),
    (265, "SUPPRESS_CONNECT_HEADERS", OptKind::Long),
    (10266, "REQUEST_TARGET", OptKind::StringPoint),
    (267, "SOCKS5_AUTH", OptKind::Long),
    (268, "SSH_COMPRESSION", OptKind::Long),
    (10269, "MIMEPOST", OptKind::ObjectPoint),
    (30270, "TIMEVALUE_LARGE", OptKind::OffT),
    (271, "HAPPY_EYEBALLS_TIMEOUT_MS", OptKind::Long),
    (20272, "RESOLVER_START_FUNCTION", OptKind::FunctionPoint),
    (10273, "RESOLVER_START_DATA", OptKind::CbPoint),
    (274, "HAPROXYPROTOCOL", OptKind::Long),
    (275, "DNS_SHUFFLE_ADDRESSES", OptKind::Long),
    (10276, "TLS13_CIPHERS", OptKind::StringPoint),
    (10277, "PROXY_TLS13_CIPHERS", OptKind::StringPoint),
    (278, "DISALLOW_USERNAME_IN_URL", OptKind::Long),
    (10279, "DOH_URL", OptKind::StringPoint),
    (280, "UPLOAD_BUFFERSIZE", OptKind::Long),
    (281, "UPKEEP_INTERVAL_MS", OptKind::Long),
    (10282, "CURLU", OptKind::ObjectPoint),
    (20283, "TRAILERFUNCTION", OptKind::FunctionPoint),
    (10284, "TRAILERDATA", OptKind::CbPoint),
    (285, "HTTP09_ALLOWED", OptKind::Long),
    (286, "ALTSVC_CTRL", OptKind::Long),
    (10287, "ALTSVC", OptKind::StringPoint),
    (288, "MAXAGE_CONN", OptKind::Long),
    (10289, "SASL_AUTHZID", OptKind::StringPoint),
    (290, "MAIL_RCPT_ALLOWFAILS", OptKind::Long),
    (40291, "SSLCERT_BLOB", OptKind::Blob),
    (40292, "SSLKEY_BLOB", OptKind::Blob),
    (40293, "PROXY_SSLCERT_BLOB", OptKind::Blob),
    (40294, "PROXY_SSLKEY_BLOB", OptKind::Blob),
    (40295, "ISSUERCERT_BLOB", OptKind::Blob),
    (10296, "PROXY_ISSUERCERT", OptKind::StringPoint),
    (40297, "PROXY_ISSUERCERT_BLOB", OptKind::Blob),
    (10298, "SSL_EC_CURVES", OptKind::StringPoint),
    (299, "HSTS_CTRL", OptKind::Long),
    (10300, "HSTS", OptKind::StringPoint),
    (20301, "HSTSREADFUNCTION", OptKind::FunctionPoint),
    (10302, "HSTSREADDATA", OptKind::CbPoint),
    (20303, "HSTSWRITEFUNCTION", OptKind::FunctionPoint),
    (10304, "HSTSWRITEDATA", OptKind::CbPoint),
    (10305, "AWS_SIGV4", OptKind::StringPoint),
    (306, "DOH_SSL_VERIFYPEER", OptKind::Long),
    (307, "DOH_SSL_VERIFYHOST", OptKind::Long),
    (308, "DOH_SSL_VERIFYSTATUS", OptKind::Long),
    (40309, "CAINFO_BLOB", OptKind::Blob),
    (40310, "PROXY_CAINFO_BLOB", OptKind::Blob),
    (10311, "SSH_HOST_PUBLIC_KEY_SHA256", OptKind::StringPoint),
    (20312, "PREREQFUNCTION", OptKind::FunctionPoint),
    (10313, "PREREQDATA", OptKind::CbPoint),
    (314, "MAXLIFETIME_CONN", OptKind::Long),
    (315, "MIME_OPTIONS", OptKind::Long),
    (20316, "SSH_HOSTKEYFUNCTION", OptKind::FunctionPoint),
    (10317, "SSH_HOSTKEYDATA", OptKind::CbPoint),
    (10318, "PROTOCOLS_STR", OptKind::StringPoint),
    (10319, "REDIR_PROTOCOLS_STR", OptKind::StringPoint),
    (320, "WS_OPTIONS", OptKind::Long),
    (321, "CA_CACHE_TIMEOUT", OptKind::Long),
    (322, "QUICK_EXIT", OptKind::Long),
    (10323, "HAPROXY_CLIENT_IP", OptKind::StringPoint),
    (324, "SERVER_RESPONSE_TIMEOUT_MS", OptKind::Long),
    (10325, "ECH", OptKind::StringPoint),
    (326, "TCP_KEEPCNT", OptKind::Long),
    (327, "UPLOAD_FLAGS", OptKind::Long),
    (10328, "SSL_SIGNATURE_ALGORITHMS", OptKind::StringPoint),
];

/// CURLMOPT_* option numbers
pub mod mopt {
    pub const SOCKETFUNCTION: u32 = 20001;
    pub const SOCKETDATA: u32 = 10002;
    pub const PIPELINING: u32 = 3;
    pub const TIMERFUNCTION: u32 = 20004;
    pub const TIMERDATA: u32 = 10005;
    pub const MAXCONNECTS: u32 = 6;
    pub const MAX_HOST_CONNECTIONS: u32 = 7;
    pub const MAX_PIPELINE_LENGTH: u32 = 8;
    pub const CONTENT_LENGTH_PENALTY_SIZE: u32 = 30009;
    pub const CHUNK_LENGTH_PENALTY_SIZE: u32 = 30010;
    pub const PIPELINING_SITE_BL: u32 = 10011;
    pub const PIPELINING_SERVER_BL: u32 = 10012;
    pub const MAX_TOTAL_CONNECTIONS: u32 = 13;
    pub const PUSHFUNCTION: u32 = 20014;
    pub const PUSHDATA: u32 = 10015;
    pub const MAX_CONCURRENT_STREAMS: u32 = 16;
}

/// CURLINFO_* numbers and curl_infotype values
pub mod info {
    pub const TEXT: u32 = 0;
    pub const HEADER_IN: u32 = 1;
    pub const HEADER_OUT: u32 = 2;
    pub const DATA_IN: u32 = 3;
    pub const DATA_OUT: u32 = 4;
    pub const SSL_DATA_IN: u32 = 5;
    pub const SSL_DATA_OUT: u32 = 6;
    pub const END: u32 = 7;
    pub const NONE: u32 = 0;
    pub const EFFECTIVE_URL: u32 = 1048577;
    pub const RESPONSE_CODE: u32 = 2097154;
    pub const TOTAL_TIME: u32 = 3145731;
    pub const NAMELOOKUP_TIME: u32 = 3145732;
    pub const CONNECT_TIME: u32 = 3145733;
    pub const PRETRANSFER_TIME: u32 = 3145734;
    pub const SIZE_UPLOAD: u32 = 3145735;
    pub const SIZE_UPLOAD_T: u32 = 6291463;
    pub const SIZE_DOWNLOAD: u32 = 3145736;
    pub const SIZE_DOWNLOAD_T: u32 = 6291464;
    pub const SPEED_DOWNLOAD: u32 = 3145737;
    pub const SPEED_DOWNLOAD_T: u32 = 6291465;
    pub const SPEED_UPLOAD: u32 = 3145738;
    pub const SPEED_UPLOAD_T: u32 = 6291466;
    pub const HEADER_SIZE: u32 = 2097163;
    pub const REQUEST_SIZE: u32 = 2097164;
    pub const SSL_VERIFYRESULT: u32 = 2097165;
    pub const FILETIME: u32 = 2097166;
    pub const FILETIME_T: u32 = 6291470;
    pub const CONTENT_LENGTH_DOWNLOAD: u32 = 3145743;
    pub const CONTENT_LENGTH_DOWNLOAD_T: u32 = 6291471;
    pub const CONTENT_LENGTH_UPLOAD: u32 = 3145744;
    pub const CONTENT_LENGTH_UPLOAD_T: u32 = 6291472;
    pub const STARTTRANSFER_TIME: u32 = 3145745;
    pub const CONTENT_TYPE: u32 = 1048594;
    pub const REDIRECT_TIME: u32 = 3145747;
    pub const REDIRECT_COUNT: u32 = 2097172;
    pub const PRIVATE: u32 = 1048597;
    pub const HTTP_CONNECTCODE: u32 = 2097174;
    pub const HTTPAUTH_AVAIL: u32 = 2097175;
    pub const PROXYAUTH_AVAIL: u32 = 2097176;
    pub const OS_ERRNO: u32 = 2097177;
    pub const NUM_CONNECTS: u32 = 2097178;
    pub const SSL_ENGINES: u32 = 4194331;
    pub const COOKIELIST: u32 = 4194332;
    pub const LASTSOCKET: u32 = 2097181;
    pub const FTP_ENTRY_PATH: u32 = 1048606;
    pub const REDIRECT_URL: u32 = 1048607;
    pub const PRIMARY_IP: u32 = 1048608;
    pub const APPCONNECT_TIME: u32 = 3145761;
    pub const CERTINFO: u32 = 4194338;
    pub const CONDITION_UNMET: u32 = 2097187;
    pub const RTSP_SESSION_ID: u32 = 1048612;
    pub const RTSP_CLIENT_CSEQ: u32 = 2097189;
    pub const RTSP_SERVER_CSEQ: u32 = 2097190;
    pub const RTSP_CSEQ_RECV: u32 = 2097191;
    pub const PRIMARY_PORT: u32 = 2097192;
    pub const LOCAL_IP: u32 = 1048617;
    pub const LOCAL_PORT: u32 = 2097194;
    pub const TLS_SESSION: u32 = 4194347;
    pub const ACTIVESOCKET: u32 = 5242924;
    pub const TLS_SSL_PTR: u32 = 4194349;
    pub const HTTP_VERSION: u32 = 2097198;
    pub const PROXY_SSL_VERIFYRESULT: u32 = 2097199;
    pub const PROTOCOL: u32 = 2097200;
    pub const SCHEME: u32 = 1048625;
    pub const TOTAL_TIME_T: u32 = 6291506;
    pub const NAMELOOKUP_TIME_T: u32 = 6291507;
    pub const CONNECT_TIME_T: u32 = 6291508;
    pub const PRETRANSFER_TIME_T: u32 = 6291509;
    pub const STARTTRANSFER_TIME_T: u32 = 6291510;
    pub const REDIRECT_TIME_T: u32 = 6291511;
    pub const APPCONNECT_TIME_T: u32 = 6291512;
    pub const RETRY_AFTER: u32 = 6291513;
    pub const EFFECTIVE_METHOD: u32 = 1048634;
    pub const PROXY_ERROR: u32 = 2097211;
    pub const REFERER: u32 = 1048636;
    pub const CAINFO: u32 = 1048637;
    pub const CAPATH: u32 = 1048638;
    pub const XFER_ID: u32 = 6291519;
    pub const CONN_ID: u32 = 6291520;
    pub const QUEUE_TIME_T: u32 = 6291521;
    pub const USED_PROXY: u32 = 2097218;
    pub const POSTTRANSFER_TIME_T: u32 = 6291523;
    pub const EARLYDATA_SENT_T: u32 = 6291524;
    pub const HTTPAUTH_USED: u32 = 2097221;
    pub const PROXYAUTH_USED: u32 = 2097222;
    pub const LASTONE: u32 = 70;
    pub const STRING: u32 = 1048576;
    pub const LONG: u32 = 2097152;
    pub const DOUBLE: u32 = 3145728;
    pub const SLIST: u32 = 4194304;
    pub const PTR: u32 = 4194304;
    pub const SOCKET: u32 = 5242880;
    pub const OFF_T: u32 = 6291456;
    pub const MASK: u32 = 1048575;
    pub const TYPEMASK: u32 = 15728640;
    pub const HTTP_CODE: u32 = 2097154;
}

/// CURLcode values
pub mod code {
    pub const OK: u32 = 0;
    pub const UNSUPPORTED_PROTOCOL: u32 = 1;
    pub const FAILED_INIT: u32 = 2;
    pub const URL_MALFORMAT: u32 = 3;
    pub const NOT_BUILT_IN: u32 = 4;
    pub const COULDNT_RESOLVE_PROXY: u32 = 5;
    pub const COULDNT_RESOLVE_HOST: u32 = 6;
    pub const COULDNT_CONNECT: u32 = 7;
    pub const WEIRD_SERVER_REPLY: u32 = 8;
    pub const REMOTE_ACCESS_DENIED: u32 = 9;
    pub const FTP_ACCEPT_FAILED: u32 = 10;
    pub const FTP_WEIRD_PASS_REPLY: u32 = 11;
    pub const FTP_ACCEPT_TIMEOUT: u32 = 12;
    pub const FTP_WEIRD_PASV_REPLY: u32 = 13;
    pub const FTP_WEIRD_227_FORMAT: u32 = 14;
    pub const FTP_CANT_GET_HOST: u32 = 15;
    pub const HTTP2: u32 = 16;
    pub const FTP_COULDNT_SET_TYPE: u32 = 17;
    pub const PARTIAL_FILE: u32 = 18;
    pub const FTP_COULDNT_RETR_FILE: u32 = 19;
    pub const OBSOLETE20: u32 = 20;
    pub const QUOTE_ERROR: u32 = 21;
    pub const HTTP_RETURNED_ERROR: u32 = 22;
    pub const WRITE_ERROR: u32 = 23;
    pub const OBSOLETE24: u32 = 24;
    pub const UPLOAD_FAILED: u32 = 25;
    pub const READ_ERROR: u32 = 26;
    pub const OUT_OF_MEMORY: u32 = 27;
    pub const OPERATION_TIMEDOUT: u32 = 28;
    pub const OBSOLETE29: u32 = 29;
    pub const FTP_PORT_FAILED: u32 = 30;
    pub const FTP_COULDNT_USE_REST: u32 = 31;
    pub const OBSOLETE32: u32 = 32;
    pub const RANGE_ERROR: u32 = 33;
    pub const OBSOLETE34: u32 = 34;
    pub const SSL_CONNECT_ERROR: u32 = 35;
    pub const BAD_DOWNLOAD_RESUME: u32 = 36;
    pub const FILE_COULDNT_READ_FILE: u32 = 37;
    pub const LDAP_CANNOT_BIND: u32 = 38;
    pub const LDAP_SEARCH_FAILED: u32 = 39;
    pub const OBSOLETE40: u32 = 40;
    pub const OBSOLETE41: u32 = 41;
    pub const ABORTED_BY_CALLBACK: u32 = 42;
    pub const BAD_FUNCTION_ARGUMENT: u32 = 43;
    pub const OBSOLETE44: u32 = 44;
    pub const INTERFACE_FAILED: u32 = 45;
    pub const OBSOLETE46: u32 = 46;
    pub const TOO_MANY_REDIRECTS: u32 = 47;
    pub const UNKNOWN_OPTION: u32 = 48;
    pub const SETOPT_OPTION_SYNTAX: u32 = 49;
    pub const OBSOLETE50: u32 = 50;
    pub const OBSOLETE51: u32 = 51;
    pub const GOT_NOTHING: u32 = 52;
    pub const SSL_ENGINE_NOTFOUND: u32 = 53;
    pub const SSL_ENGINE_SETFAILED: u32 = 54;
    pub const SEND_ERROR: u32 = 55;
    pub const RECV_ERROR: u32 = 56;
    pub const OBSOLETE57: u32 = 57;
    pub const SSL_CERTPROBLEM: u32 = 58;
    pub const SSL_CIPHER: u32 = 59;
    pub const PEER_FAILED_VERIFICATION: u32 = 60;
    pub const BAD_CONTENT_ENCODING: u32 = 61;
    pub const OBSOLETE62: u32 = 62;
    pub const FILESIZE_EXCEEDED: u32 = 63;
    pub const USE_SSL_FAILED: u32 = 64;
    pub const SEND_FAIL_REWIND: u32 = 65;
    pub const SSL_ENGINE_INITFAILED: u32 = 66;
    pub const LOGIN_DENIED: u32 = 67;
    pub const TFTP_NOTFOUND: u32 = 68;
    pub const TFTP_PERM: u32 = 69;
    pub const REMOTE_DISK_FULL: u32 = 70;
    pub const TFTP_ILLEGAL: u32 = 71;
    pub const TFTP_UNKNOWNID: u32 = 72;
    pub const REMOTE_FILE_EXISTS: u32 = 73;
    pub const TFTP_NOSUCHUSER: u32 = 74;
    pub const OBSOLETE75: u32 = 75;
    pub const OBSOLETE76: u32 = 76;
    pub const SSL_CACERT_BADFILE: u32 = 77;
    pub const REMOTE_FILE_NOT_FOUND: u32 = 78;
    pub const SSH: u32 = 79;
    pub const SSL_SHUTDOWN_FAILED: u32 = 80;
    pub const AGAIN: u32 = 81;
    pub const SSL_CRL_BADFILE: u32 = 82;
    pub const SSL_ISSUER_ERROR: u32 = 83;
    pub const FTP_PRET_FAILED: u32 = 84;
    pub const RTSP_CSEQ_ERROR: u32 = 85;
    pub const RTSP_SESSION_ERROR: u32 = 86;
    pub const FTP_BAD_FILE_LIST: u32 = 87;
    pub const CHUNK_FAILED: u32 = 88;
    pub const NO_CONNECTION_AVAILABLE: u32 = 89;
    pub const SSL_PINNEDPUBKEYNOTMATCH: u32 = 90;
    pub const SSL_INVALIDCERTSTATUS: u32 = 91;
    pub const HTTP2_STREAM: u32 = 92;
    pub const RECURSIVE_API_CALL: u32 = 93;
    pub const AUTH_ERROR: u32 = 94;
    pub const HTTP3: u32 = 95;
    pub const QUIC_CONNECT_ERROR: u32 = 96;
    pub const PROXY: u32 = 97;
    pub const SSL_CLIENTCERT: u32 = 98;
    pub const UNRECOVERABLE_POLL: u32 = 99;
    pub const TOO_LARGE: u32 = 100;
    pub const ECH_REQUIRED: u32 = 101;
    pub const RESERVED115: u32 = 115;
    pub const RESERVED116: u32 = 116;
    pub const RESERVED117: u32 = 117;
    pub const RESERVED118: u32 = 118;
    pub const RESERVED119: u32 = 119;
    pub const RESERVED120: u32 = 120;
    pub const RESERVED121: u32 = 121;
    pub const RESERVED122: u32 = 122;
    pub const RESERVED123: u32 = 123;
    pub const RESERVED124: u32 = 124;
    pub const RESERVED125: u32 = 125;
    pub const RESERVED126: u32 = 126;
    pub const FUNCTION_NOT_FOUND: u32 = 41;
    pub const HTTP_POST_ERROR: u32 = 34;
    pub const OBSOLETE16: u32 = 16;
    pub const OBSOLETE10: u32 = 10;
    pub const OBSOLETE12: u32 = 12;
    pub const FTP_WEIRD_SERVER_REPLY: u32 = 8;
    pub const SSL_CACERT: u32 = 60;
    pub const UNKNOWN_TELNET_OPTION: u32 = 48;
    pub const TELNET_OPTION_SYNTAX: u32 = 49;
    pub const SSL_PEER_CERTIFICATE: u32 = 60;
    pub const OBSOLETE: u32 = 50;
    pub const BAD_PASSWORD_ENTERED: u32 = 46;
    pub const BAD_CALLING_ORDER: u32 = 44;
    pub const FTP_USER_PASSWORD_INCORRECT: u32 = 10;
    pub const FTP_CANT_RECONNECT: u32 = 16;
    pub const FTP_COULDNT_GET_SIZE: u32 = 32;
    pub const FTP_COULDNT_SET_ASCII: u32 = 29;
    pub const FTP_WEIRD_USER_REPLY: u32 = 12;
    pub const FTP_WRITE_ERROR: u32 = 20;
    pub const LIBRARY_NOT_FOUND: u32 = 40;
    pub const MALFORMAT_USER: u32 = 24;
    pub const SHARE_IN_USE: u32 = 57;
    pub const URL_MALFORMAT_USER: u32 = 4;
    pub const FTP_ACCESS_DENIED: u32 = 9;
    pub const FTP_COULDNT_SET_BINARY: u32 = 17;
    pub const FTP_QUOTE_ERROR: u32 = 21;
    pub const TFTP_DISKFULL: u32 = 70;
    pub const TFTP_EXISTS: u32 = 73;
    pub const HTTP_RANGE_ERROR: u32 = 33;
    pub const FTP_SSL_FAILED: u32 = 64;
    pub const OPERATION_TIMEOUTED: u32 = 28;
    pub const HTTP_NOT_FOUND: u32 = 22;
    pub const HTTP_PORT_FAILED: u32 = 45;
    pub const FTP_COULDNT_STOR_FILE: u32 = 25;
    pub const FTP_PARTIAL_FILE: u32 = 18;
    pub const FTP_BAD_DOWNLOAD_RESUME: u32 = 36;
    pub const LDAP_INVALID_URL: u32 = 62;
    pub const CONV_REQD: u32 = 76;
    pub const CONV_FAILED: u32 = 75;
    pub const ALREADY_COMPLETE: u32 = 99999;
}

/// CURLMcode values
pub mod mcode {
    pub const CALL_MULTI_PERFORM: i32 = -1;
    pub const OK: i32 = 0;
    pub const BAD_HANDLE: i32 = 1;
    pub const BAD_EASY_HANDLE: i32 = 2;
    pub const OUT_OF_MEMORY: i32 = 3;
    pub const INTERNAL_ERROR: i32 = 4;
    pub const BAD_SOCKET: i32 = 5;
    pub const UNKNOWN_OPTION: i32 = 6;
    pub const ADDED_ALREADY: i32 = 7;
    pub const RECURSIVE_API_CALL: i32 = 8;
    pub const WAKEUP_FAILURE: i32 = 9;
    pub const BAD_FUNCTION_ARGUMENT: i32 = 10;
    pub const ABORTED_BY_CALLBACK: i32 = 11;
    pub const UNRECOVERABLE_POLL: i32 = 12;
    pub const LAST: i32 = 13;
    pub const CALL_MULTI_SOCKET: i32 = -1;
}

/// CURLSHcode values
pub mod shcode {
    pub const OK: u32 = 0;
    pub const BAD_OPTION: u32 = 1;
    pub const IN_USE: u32 = 2;
    pub const INVALID: u32 = 3;
    pub const NOMEM: u32 = 4;
    pub const NOT_BUILT_IN: u32 = 5;
    pub const LAST: u32 = 6;
}

/// CURLSHoption values
pub mod shopt {
    pub const NONE: u32 = 0;
    pub const SHARE: u32 = 1;
    pub const UNSHARE: u32 = 2;
    pub const LOCKFUNC: u32 = 3;
    pub const UNLOCKFUNC: u32 = 4;
    pub const USERDATA: u32 = 5;
    pub const LAST: u32 = 6;
}

/// curl_lock_data values
pub mod lock {
    pub const NONE: u32 = 0;
    pub const SHARE: u32 = 1;
    pub const COOKIE: u32 = 2;
    pub const DNS: u32 = 3;
    pub const SSL_SESSION: u32 = 4;
    pub const CONNECT: u32 = 5;
    pub const PSL: u32 = 6;
    pub const HSTS: u32 = 7;
    pub const LAST: u32 = 8;
}

/// CURLAUTH_* bits
pub mod auth {
    pub const NONE: u64 = 0;
    pub const BASIC: u64 = 1;
    pub const DIGEST: u64 = 2;
    pub const NEGOTIATE: u64 = 4;
    pub const GSSNEGOTIATE: u64 = 4;
    pub const GSSAPI: u64 = 4;
    pub const NTLM: u64 = 8;
    pub const DIGEST_IE: u64 = 16;
    pub const NTLM_WB: u64 = 32;
    pub const BEARER: u64 = 64;
    pub const AWS_SIGV4: u64 = 128;
    pub const ONLY: u64 = 2147483648;
}

/// CURLPROTO_* bits
pub mod proto {
    pub const HTTP: i64 = 1;
    pub const HTTPS: i64 = 2;
    pub const FTP: i64 = 4;
    pub const FTPS: i64 = 8;
    pub const SCP: i64 = 16;
    pub const SFTP: i64 = 32;
    pub const TELNET: i64 = 64;
    pub const LDAP: i64 = 128;
    pub const LDAPS: i64 = 256;
    pub const DICT: i64 = 512;
    pub const FILE: i64 = 1024;
    pub const TFTP: i64 = 2048;
    pub const IMAP: i64 = 4096;
    pub const IMAPS: i64 = 8192;
    pub const POP3: i64 = 16384;
    pub const POP3S: i64 = 32768;
    pub const SMTP: i64 = 65536;
    pub const SMTPS: i64 = 131072;
    pub const RTSP: i64 = 262144;
    pub const RTMP: i64 = 524288;
    pub const RTMPT: i64 = 1048576;
    pub const RTMPE: i64 = 2097152;
    pub const RTMPTE: i64 = 4194304;
    pub const RTMPS: i64 = 8388608;
    pub const RTMPTS: i64 = 16777216;
    pub const GOPHER: i64 = 33554432;
    pub const SMB: i64 = 67108864;
    pub const SMBS: i64 = 134217728;
    pub const MQTT: i64 = 268435456;
    pub const GOPHERS: i64 = 536870912;
    pub const ALL: i64 = -1;
}

/// CURL_VERSION_* feature bits
pub mod feature {
    pub const IPV6: i64 = 1;
    pub const KERBEROS4: i64 = 2;
    pub const SSL: i64 = 4;
    pub const LIBZ: i64 = 8;
    pub const NTLM: i64 = 16;
    pub const GSSNEGOTIATE: i64 = 32;
    pub const DEBUG: i64 = 64;
    pub const ASYNCHDNS: i64 = 128;
    pub const SPNEGO: i64 = 256;
    pub const LARGEFILE: i64 = 512;
    pub const IDN: i64 = 1024;
    pub const SSPI: i64 = 2048;
    pub const CONV: i64 = 4096;
    pub const CURLDEBUG: i64 = 8192;
    pub const TLSAUTH_SRP: i64 = 16384;
    pub const NTLM_WB: i64 = 32768;
    pub const HTTP2: i64 = 65536;
    pub const GSSAPI: i64 = 131072;
    pub const KERBEROS5: i64 = 262144;
    pub const UNIX_SOCKETS: i64 = 524288;
    pub const PSL: i64 = 1048576;
    pub const HTTPS_PROXY: i64 = 2097152;
    pub const MULTI_SSL: i64 = 4194304;
    pub const BROTLI: i64 = 8388608;
    pub const ALTSVC: i64 = 16777216;
    pub const HTTP3: i64 = 33554432;
    pub const ZSTD: i64 = 67108864;
    pub const UNICODE: i64 = 134217728;
    pub const HSTS: i64 = 268435456;
    pub const GSASL: i64 = 536870912;
    pub const THREADSAFE: i64 = 1073741824;
}

/// CURLSSLOPT_* bits
pub mod sslopt {
    pub const ALLOW_BEAST: i64 = 1;
    pub const NO_REVOKE: i64 = 2;
    pub const NO_PARTIALCHAIN: i64 = 4;
    pub const REVOKE_BEST_EFFORT: i64 = 8;
    pub const NATIVE_CA: i64 = 16;
    pub const AUTO_CLIENT_CERT: i64 = 32;
    pub const EARLYDATA: i64 = 64;
}

/// CURLPAUSE_* bits
pub mod pause {
    pub const RECV: i64 = 1;
    pub const RECV_CONT: i64 = 0;
    pub const SEND: i64 = 4;
    pub const SEND_CONT: i64 = 0;
    pub const ALL: i64 = 5;
    pub const CONT: i64 = 0;
}

/// CURL_REDIR_* bits
pub mod redir {
    pub const GET_ALL: i64 = 0;
    pub const POST_301: i64 = 1;
    pub const POST_302: i64 = 2;
    pub const POST_303: i64 = 4;
    pub const POST_ALL: i64 = 7;
}

/// CURL_HTTP_VERSION_* values
pub mod httpver {
    pub const NONE: i64 = 0;
    pub const _1_0: i64 = 1;
    pub const _1_1: i64 = 2;
    pub const _2_0: i64 = 3;
    pub const _2TLS: i64 = 4;
    pub const _2_PRIOR_KNOWLEDGE: i64 = 5;
    pub const _3: i64 = 30;
    pub const _3ONLY: i64 = 31;
    pub const LAST: i64 = 32;
    pub const _2: i64 = 3;
}

/// CURL_SSLVERSION_* values
pub mod sslver {
    pub const DEFAULT: i64 = 0;
    pub const TLSv1: i64 = 1;
    pub const SSLv2: i64 = 2;
    pub const SSLv3: i64 = 3;
    pub const TLSv1_0: i64 = 4;
    pub const TLSv1_1: i64 = 5;
    pub const TLSv1_2: i64 = 6;
    pub const TLSv1_3: i64 = 7;
    pub const LAST: i64 = 8;
    pub const MAX_NONE: i64 = 0;
    pub const MAX_DEFAULT: i64 = 65536;
    pub const MAX_TLSv1_0: i64 = 262144;
    pub const MAX_TLSv1_1: i64 = 327680;
    pub const MAX_TLSv1_2: i64 = 393216;
    pub const MAX_TLSv1_3: i64 = 458752;
    pub const MAX_LAST: i64 = 524288;
}

/// curl_proxytype values
pub mod proxytype {
    pub const HTTP: i64 = 0;
    pub const HTTP_1_0: i64 = 1;
    pub const HTTPS: i64 = 2;
    pub const HTTPS2: i64 = 3;
    pub const SOCKS4: i64 = 4;
    pub const SOCKS5: i64 = 5;
    pub const SOCKS4A: i64 = 6;
    pub const SOCKS5_HOSTNAME: i64 = 7;
}

/// CURL_IPRESOLVE_* values
pub mod ipresolve {
    pub const WHATEVER: i64 = 0;
    pub const V4: i64 = 1;
    pub const V6: i64 = 2;
}

/// curl_usessl values
pub mod usessl {
    pub const LAST: i64 = 4;
    pub const NONE: i64 = 0;
    pub const TRY: i64 = 1;
    pub const CONTROL: i64 = 2;
    pub const ALL: i64 = 3;
}

/// CURL_NETRC_* values
pub mod netrc {
    pub const IGNORED: i64 = 0;
    pub const OPTIONAL: i64 = 1;
    pub const REQUIRED: i64 = 2;
}

/// curl_TimeCond values
pub mod timecond {
    pub const LAST: i64 = 4;
    pub const NONE: i64 = 0;
    pub const IFMODSINCE: i64 = 1;
    pub const IFUNMODSINCE: i64 = 2;
    pub const LASTMOD: i64 = 3;
}

/// CURLHEADER_* values
pub mod header {
    pub const UNIFIED: i64 = 0;
    pub const SEPARATE: i64 = 1;
}

/// CURLFOLLOW_* values
pub mod follow {
    pub const ALL: i64 = 1;
    pub const OBEYCODE: i64 = 2;
    pub const FIRSTONLY: i64 = 3;
}

/// CURLMSG values
pub mod msg {
    pub const NONE: i32 = 0;
    pub const DONE: i32 = 1;
    pub const LAST: i32 = 2;
}

/// CURLPIPE_* values
pub mod pipe {
    pub const NOTHING: i64 = 0;
    pub const HTTP1: i64 = 1;
    pub const MULTIPLEX: i64 = 2;
}

/// CURLproxycode values
pub mod proxyerr {
    pub const OK: i64 = 0;
    pub const BAD_ADDRESS_TYPE: i64 = 1;
    pub const BAD_VERSION: i64 = 2;
    pub const CLOSED: i64 = 3;
    pub const GSSAPI: i64 = 4;
    pub const GSSAPI_PERMSG: i64 = 5;
    pub const GSSAPI_PROTECTION: i64 = 6;
    pub const IDENTD: i64 = 7;
    pub const IDENTD_DIFFER: i64 = 8;
    pub const LONG_HOSTNAME: i64 = 9;
    pub const LONG_PASSWD: i64 = 10;
    pub const LONG_USER: i64 = 11;
    pub const NO_AUTH: i64 = 12;
    pub const RECV_ADDRESS: i64 = 13;
    pub const RECV_AUTH: i64 = 14;
    pub const RECV_CONNECT: i64 = 15;
    pub const RECV_REQACK: i64 = 16;
    pub const REPLY_ADDRESS_TYPE_NOT_SUPPORTED: i64 = 17;
    pub const REPLY_COMMAND_NOT_SUPPORTED: i64 = 18;
    pub const REPLY_CONNECTION_REFUSED: i64 = 19;
    pub const REPLY_GENERAL_SERVER_FAILURE: i64 = 20;
    pub const REPLY_HOST_UNREACHABLE: i64 = 21;
    pub const REPLY_NETWORK_UNREACHABLE: i64 = 22;
    pub const REPLY_NOT_ALLOWED: i64 = 23;
    pub const REPLY_TTL_EXPIRED: i64 = 24;
    pub const REPLY_UNASSIGNED: i64 = 25;
    pub const REQUEST_FAILED: i64 = 26;
    pub const RESOLVE_HOST: i64 = 27;
    pub const SEND_AUTH: i64 = 28;
    pub const SEND_CONNECT: i64 = 29;
    pub const SEND_REQUEST: i64 = 30;
    pub const UNKNOWN_FAIL: i64 = 31;
    pub const UNKNOWN_MODE: i64 = 32;
    pub const USER_REJECTED: i64 = 33;
    pub const LAST: i64 = 34;
}

/// other CURL_* defines
pub mod misc {
    pub const SOCKET_BAD: i64 = -1;
    pub const HTTPPOST_FILENAME: i64 = 1;
    pub const HTTPPOST_READFILE: i64 = 2;
    pub const HTTPPOST_PTRNAME: i64 = 4;
    pub const HTTPPOST_PTRCONTENTS: i64 = 8;
    pub const HTTPPOST_BUFFER: i64 = 16;
    pub const HTTPPOST_PTRBUFFER: i64 = 32;
    pub const HTTPPOST_CALLBACK: i64 = 64;
    pub const HTTPPOST_LARGE: i64 = 128;
    pub const PROGRESSFUNC_CONTINUE: i64 = 268435457;
    pub const MAX_READ_SIZE: i64 = 10485760;
    pub const MAX_WRITE_SIZE: i64 = 16384;
    pub const MAX_HTTP_HEADER: i64 = 102400;
    pub const WRITEFUNC_PAUSE: i64 = 268435457;
    pub const WRITEFUNC_ERROR: i64 = 4294967295;
    pub const CHUNK_BGN_FUNC_OK: i64 = 0;
    pub const CHUNK_BGN_FUNC_FAIL: i64 = 1;
    pub const CHUNK_BGN_FUNC_SKIP: i64 = 2;
    pub const CHUNK_END_FUNC_OK: i64 = 0;
    pub const CHUNK_END_FUNC_FAIL: i64 = 1;
    pub const FNMATCHFUNC_MATCH: i64 = 0;
    pub const FNMATCHFUNC_NOMATCH: i64 = 1;
    pub const FNMATCHFUNC_FAIL: i64 = 2;
    pub const SEEKFUNC_OK: i64 = 0;
    pub const SEEKFUNC_FAIL: i64 = 1;
    pub const SEEKFUNC_CANTSEEK: i64 = 2;
    pub const READFUNC_ABORT: i64 = 268435456;
    pub const READFUNC_PAUSE: i64 = 268435457;
    pub const TRAILERFUNC_OK: i64 = 0;
    pub const TRAILERFUNC_ABORT: i64 = 1;
    pub const PREREQFUNC_OK: i64 = 0;
    pub const PREREQFUNC_ABORT: i64 = 1;
    pub const ERROR_SIZE: i64 = 256;
    pub const HET_DEFAULT: i64 = 200;
    pub const UPKEEP_INTERVAL_DEFAULT: i64 = 60000;
    pub const RTSPREQ_NONE: i64 = 0;
    pub const RTSPREQ_OPTIONS: i64 = 1;
    pub const RTSPREQ_DESCRIBE: i64 = 2;
    pub const RTSPREQ_ANNOUNCE: i64 = 3;
    pub const RTSPREQ_SETUP: i64 = 4;
    pub const RTSPREQ_PLAY: i64 = 5;
    pub const RTSPREQ_PAUSE: i64 = 6;
    pub const RTSPREQ_TEARDOWN: i64 = 7;
    pub const RTSPREQ_GET_PARAMETER: i64 = 8;
    pub const RTSPREQ_SET_PARAMETER: i64 = 9;
    pub const RTSPREQ_RECORD: i64 = 10;
    pub const RTSPREQ_RECEIVE: i64 = 11;
    pub const RTSPREQ_LAST: i64 = 12;
    pub const TLSAUTH_NONE: i64 = 0;
    pub const TLSAUTH_SRP: i64 = 1;
    pub const ZERO_TERMINATED: i64 = -1;
    pub const GLOBAL_SSL: i64 = 1;
    pub const GLOBAL_WIN32: i64 = 2;
    pub const GLOBAL_ALL: i64 = 3;
    pub const GLOBAL_NOTHING: i64 = 0;
    pub const GLOBAL_DEFAULT: i64 = 3;
    pub const GLOBAL_ACK_EINTR: i64 = 4;
    pub const POLL_NONE: i64 = 0;
    pub const POLL_IN: i64 = 1;
    pub const POLL_OUT: i64 = 2;
    pub const POLL_INOUT: i64 = 3;
    pub const POLL_REMOVE: i64 = 4;
    pub const SOCKET_TIMEOUT: i64 = -1;
    pub const CSELECT_IN: i64 = 1;
    pub const CSELECT_OUT: i64 = 2;
    pub const CSELECT_ERR: i64 = 4;
    pub const PUSH_OK: i64 = 0;
    pub const PUSH_DENY: i64 = 1;
    pub const PUSH_ERROROUT: i64 = 2;
}
