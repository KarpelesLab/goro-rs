--TEST--
Core constants: ReflectionExtension::getConstants(), get_defined_constants(true), INI entries are no constants
--FILE--
<?php
$c = (new ReflectionExtension('Core'))->getConstants();
echo implode(" ", array_keys($c)), "\n";
$all = get_defined_constants(true);
var_dump(array_keys($all['Core']) === array_keys($c));
var_dump(PHP_ZTS, PHP_DEBUG, ZEND_VM_KIND, PHP_CLI_PROCESS_TITLE, INFO_ALL, CREDITS_ALL);
var_dump(GLOB_MARK, GLOB_NOSORT, GLOB_NOCHECK, GLOB_NOESCAPE, GLOB_ERR, GLOB_BRACE, GLOB_ONLYDIR, GLOB_AVAILABLE_FLAGS);
var_dump(LOG_MAIL, LOG_LOCAL7, LOG_NOWAIT, STREAM_PF_INET6, STREAM_SOCK_SEQPACKET, STREAM_SOCK_RDM, FILE_NO_DEFAULT_CONTEXT, STREAM_IGNORE_URL, STREAM_MUST_SEEK);
foreach (['PHP_FLOAT_INF', 'PHP_FLOAT_NAN', 'ARRAY_UNIQUE_REGULAR', 'PHP_ROUND_CEILING', 'CTYPE_ALPHA', 'ARRAY_FILTER_USE_VALUE', 'precision', 'memory_limit', 'SMTP', 'error_reporting', 'date.timezone'] as $n) {
    var_dump(defined($n), array_key_exists($n, get_defined_constants()));
}
try { echo precision; } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { echo constant('include_path'); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(ini_get('precision'));
var_dump(glob(__DIR__ . '/*', 1 << 20));
var_dump(glob(str_repeat('a', 5000)));
--EXPECTF--
E_ERROR E_WARNING E_PARSE E_NOTICE E_CORE_ERROR E_CORE_WARNING E_COMPILE_ERROR E_COMPILE_WARNING E_USER_ERROR E_USER_WARNING E_USER_NOTICE E_STRICT E_RECOVERABLE_ERROR E_DEPRECATED E_USER_DEPRECATED E_ALL DEBUG_BACKTRACE_PROVIDE_OBJECT DEBUG_BACKTRACE_IGNORE_ARGS ZEND_THREAD_SAFE ZEND_DEBUG_BUILD ZEND_VM_KIND TRUE FALSE NULL PHP_VERSION PHP_MAJOR_VERSION PHP_MINOR_VERSION PHP_RELEASE_VERSION PHP_EXTRA_VERSION PHP_VERSION_ID PHP_BUILD_DATE PHP_ZTS PHP_DEBUG PHP_OS PHP_OS_FAMILY DEFAULT_INCLUDE_PATH PEAR_INSTALL_DIR PEAR_EXTENSION_DIR PHP_EXTENSION_DIR PHP_PREFIX PHP_BINDIR PHP_SBINDIR PHP_MANDIR PHP_LIBDIR PHP_DATADIR PHP_SYSCONFDIR PHP_LOCALSTATEDIR PHP_CONFIG_FILE_PATH PHP_CONFIG_FILE_SCAN_DIR PHP_SHLIB_SUFFIX PHP_EOL PHP_MAXPATHLEN PHP_INT_MAX PHP_INT_MIN PHP_INT_SIZE PHP_FD_SETSIZE PHP_FLOAT_DIG PHP_FLOAT_EPSILON PHP_FLOAT_MAX PHP_FLOAT_MIN PHP_OUTPUT_HANDLER_START PHP_OUTPUT_HANDLER_WRITE PHP_OUTPUT_HANDLER_FLUSH PHP_OUTPUT_HANDLER_CLEAN PHP_OUTPUT_HANDLER_FINAL PHP_OUTPUT_HANDLER_CONT PHP_OUTPUT_HANDLER_END PHP_OUTPUT_HANDLER_CLEANABLE PHP_OUTPUT_HANDLER_FLUSHABLE PHP_OUTPUT_HANDLER_REMOVABLE PHP_OUTPUT_HANDLER_STDFLAGS PHP_OUTPUT_HANDLER_STARTED PHP_OUTPUT_HANDLER_DISABLED PHP_OUTPUT_HANDLER_PROCESSED UPLOAD_ERR_OK UPLOAD_ERR_INI_SIZE UPLOAD_ERR_FORM_SIZE UPLOAD_ERR_PARTIAL UPLOAD_ERR_NO_FILE UPLOAD_ERR_NO_TMP_DIR UPLOAD_ERR_CANT_WRITE UPLOAD_ERR_EXTENSION PHP_SAPI PHP_BINARY PHP_CLI_PROCESS_TITLE STDIN STDOUT STDERR
bool(true)
bool(false)
bool(false)
string(19) "ZEND_VM_KIND_HYBRID"
bool(true)
int(4294967295)
int(4294967295)
int(8)
int(32)
int(16)
int(4096)
int(4)
int(128)
int(1073741824)
int(1073746108)
int(16)
int(184)
int(16)
int(10)
int(5)
int(4)
int(16)
int(2)
int(16)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
Undefined constant "precision"
Undefined constant "include_path"
string(2) "14"

Warning: glob(): At least one of the passed flags is invalid or not supported on this platform in %s on line 15
bool(false)

Warning: glob(): Pattern exceeds the maximum allowed length of 4096 characters in %s on line 16
bool(false)
