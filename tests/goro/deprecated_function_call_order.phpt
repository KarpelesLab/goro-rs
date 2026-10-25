--TEST--
Deprecated internal functions: the deprecation from the call (no frame of the function), then the argument checks; a throwing handler skips the function
--FILE--
<?php
$calls = [
    'assert_options' => [[], [[]], [ASSERT_ACTIVE, 1, 2], [null]],
    'curl_close' => [[], [1]],
    'curl_share_close' => [[], [1]],
    'date_sunrise' => [[], [[]], [0, 9]],
    'date_sunset' => [[]],
    'finfo_close' => [[], [1]],
    'strftime' => [[], [[]], ['%Y', 'x']],
    'gmstrftime' => [[]],
    'lcg_value' => [[1]],
    'libxml_disable_entity_loader' => [[[]], [null], [1, 2]],
    'openssl_free_key' => [[], [1]],
    'openssl_pkey_free' => [[null]],
    'openssl_x509_free' => [[], ['x']],
    'socket_set_timeout' => [[], [STDIN], [1, 1], [STDIN, 'x']],
    'xml_parser_free' => [[], [xml_parser_create(), 1]],
    'xml_set_object' => [[], [xml_parser_create()], [xml_parser_create(), 1]],
];
foreach ($calls as $f => $list) {
    foreach ($list as $args) {
        try { var_dump($f(...$args)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    }
}
var_dump(ASSERT_ACTIVE === 1);

function frames() {
    foreach (debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS) as $f) {
        echo "  ", $f['function'], isset($f['line']) ? " @" . $f['line'] : "", "\n";
    }
}
function w($f, $a) { return $f(...$a); }
$good = [
    'assert_options' => [ASSERT_WARNING],
    'curl_close' => [curl_init()],
    'date_sunrise' => [0],
    'finfo_close' => [finfo_open()],
    'strftime' => ['%Y', 0],
    'lcg_value' => [],
    'libxml_disable_entity_loader' => [false],
    'openssl_free_key' => [openssl_pkey_new(['private_key_bits' => 1024])],
    'socket_set_timeout' => [STDIN, 1],
    'xml_parser_free' => [xml_parser_create()],
    'xml_set_object' => [xml_parser_create(), new stdClass],
];
// The deprecation comes from the caller's code: no frame for the function.
set_error_handler(function ($n, $s) { echo "$s\n"; frames(); return true; });
foreach ($good as $f => $a) {
    $r = w($f, $a);
    echo gettype($r), "\n";
}
// A throwing handler: the function is not run.
set_error_handler(function ($n, $s) { throw new Exception($s); });
foreach ($good as $f => $a) {
    try { w($f, $a); echo "ran\n"; } catch (Exception $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
}
try { libxml_disable_entity_loader(true); } catch (Exception $e) { echo $e->getMessage(), "\n"; }
set_error_handler(fn() => true);
var_dump(libxml_disable_entity_loader(false));
--EXPECTF--

Deprecated: Constant ASSERT_ACTIVE is deprecated since 8.3, as assert_options() is deprecated in %s on line 3

Deprecated: Function assert_options() is deprecated since 8.3 in %s on line 22
ArgumentCountError: assert_options() expects at least 1 argument, 0 given

Deprecated: Function assert_options() is deprecated since 8.3 in %s on line 22
TypeError: assert_options(): Argument #1 ($option) must be of type int, array given

Deprecated: Function assert_options() is deprecated since 8.3 in %s on line 22
ArgumentCountError: assert_options() expects at most 2 arguments, 3 given

Deprecated: Function assert_options() is deprecated since 8.3 in %s on line 22

Deprecated: assert_options(): Passing null to parameter #1 ($option) of type int is deprecated in %s on line 22
ValueError: assert_options(): Argument #1 ($option) must be an ASSERT_* constant

Deprecated: Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0 in %s on line 22
ArgumentCountError: curl_close() expects exactly 1 argument, 0 given

Deprecated: Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0 in %s on line 22
TypeError: curl_close(): Argument #1 ($handle) must be of type CurlHandle, int given

Deprecated: Function curl_share_close() is deprecated since 8.5, as it has no effect since PHP 8.0 in %s on line 22
ArgumentCountError: curl_share_close() expects exactly 1 argument, 0 given

Deprecated: Function curl_share_close() is deprecated since 8.5, as it has no effect since PHP 8.0 in %s on line 22
TypeError: curl_share_close(): Argument #1 ($share_handle) must be of type CurlShareHandle, int given

Deprecated: Function date_sunrise() is deprecated since 8.1, use date_sun_info() instead in %s on line 22
ArgumentCountError: date_sunrise() expects at least 1 argument, 0 given

Deprecated: Function date_sunrise() is deprecated since 8.1, use date_sun_info() instead in %s on line 22
TypeError: date_sunrise(): Argument #1 ($timestamp) must be of type int, array given

Deprecated: Function date_sunrise() is deprecated since 8.1, use date_sun_info() instead in %s on line 22
ValueError: date_sunrise(): Argument #2 ($returnFormat) must be one of SUNFUNCS_RET_TIMESTAMP, SUNFUNCS_RET_STRING, or SUNFUNCS_RET_DOUBLE

Deprecated: Function date_sunset() is deprecated since 8.1, use date_sun_info() instead in %s on line 22
ArgumentCountError: date_sunset() expects at least 1 argument, 0 given

Deprecated: Function finfo_close() is deprecated since 8.5, as finfo objects are freed automatically in %s on line 22
ArgumentCountError: finfo_close() expects exactly 1 argument, 0 given

Deprecated: Function finfo_close() is deprecated since 8.5, as finfo objects are freed automatically in %s on line 22
TypeError: finfo_close(): Argument #1 ($finfo) must be of type finfo, int given

Deprecated: Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead in %s on line 22
ArgumentCountError: strftime() expects at least 1 argument, 0 given

Deprecated: Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead in %s on line 22
TypeError: strftime(): Argument #1 ($format) must be of type string, array given

Deprecated: Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead in %s on line 22
TypeError: strftime(): Argument #2 ($timestamp) must be of type ?int, string given

Deprecated: Function gmstrftime() is deprecated since 8.1, use IntlDateFormatter::format() instead in %s on line 22
ArgumentCountError: gmstrftime() expects at least 1 argument, 0 given

Deprecated: Function lcg_value() is deprecated since 8.4, use \Random\Randomizer::getFloat() instead in %s on line 22
ArgumentCountError: lcg_value() expects exactly 0 arguments, 1 given

Deprecated: Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default in %s on line 22
TypeError: libxml_disable_entity_loader(): Argument #1 ($disable) must be of type bool, array given

Deprecated: Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default in %s on line 22

Deprecated: libxml_disable_entity_loader(): Passing null to parameter #1 ($disable) of type bool is deprecated in %s on line 22
bool(false)

Deprecated: Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default in %s on line 22
ArgumentCountError: libxml_disable_entity_loader() expects at most 1 argument, 2 given

Deprecated: Function openssl_free_key() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically in %s on line 22
ArgumentCountError: openssl_free_key() expects exactly 1 argument, 0 given

Deprecated: Function openssl_free_key() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically in %s on line 22
TypeError: openssl_free_key(): Argument #1 ($key) must be of type OpenSSLAsymmetricKey, int given

Deprecated: Function openssl_pkey_free() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically in %s on line 22
TypeError: openssl_pkey_free(): Argument #1 ($key) must be of type OpenSSLAsymmetricKey, null given

Deprecated: Function openssl_x509_free() is deprecated since 8.0, as OpenSSLCertificate objects are freed automatically in %s on line 22
ArgumentCountError: openssl_x509_free() expects exactly 1 argument, 0 given

Deprecated: Function openssl_x509_free() is deprecated since 8.0, as OpenSSLCertificate objects are freed automatically in %s on line 22
TypeError: openssl_x509_free(): Argument #1 ($certificate) must be of type OpenSSLCertificate, string given

Deprecated: Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead in %s on line 22
ArgumentCountError: socket_set_timeout() expects at least 2 arguments, 0 given

Deprecated: Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead in %s on line 22
ArgumentCountError: socket_set_timeout() expects at least 2 arguments, 1 given

Deprecated: Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead in %s on line 22
TypeError: socket_set_timeout(): Argument #1 ($stream) must be of type resource, int given

Deprecated: Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead in %s on line 22
TypeError: socket_set_timeout(): Argument #2 ($seconds) must be of type int, string given

Deprecated: Function xml_parser_free() is deprecated since 8.5, as it has no effect since PHP 8.0 in %s on line 22
ArgumentCountError: xml_parser_free() expects exactly 1 argument, 0 given

Deprecated: Function xml_parser_free() is deprecated since 8.5, as it has no effect since PHP 8.0 in %s on line 22
ArgumentCountError: xml_parser_free() expects exactly 1 argument, 2 given

Deprecated: Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions in %s on line 22
ArgumentCountError: xml_set_object() expects exactly 2 arguments, 0 given

Deprecated: Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions in %s on line 22
ArgumentCountError: xml_set_object() expects exactly 2 arguments, 1 given

Deprecated: Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions in %s on line 22
TypeError: xml_set_object(): Argument #2 ($object) must be of type object, int given

Deprecated: Constant ASSERT_ACTIVE is deprecated since 8.3, as assert_options() is deprecated in %s on line 25
bool(true)

Deprecated: Constant ASSERT_WARNING is deprecated since 8.3, as assert_options() is deprecated in %s on line 34
Function assert_options() is deprecated since 8.3
  frames @47
  {closure:%s:47} @32
  w @49
integer
Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0
  frames @47
  {closure:%s:47} @32
  w @49
NULL
Function date_sunrise() is deprecated since 8.1, use date_sun_info() instead
  frames @47
  {closure:%s:47} @32
  w @49
string
Function finfo_close() is deprecated since 8.5, as finfo objects are freed automatically
  frames @47
  {closure:%s:47} @32
  w @49
boolean
Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead
  frames @47
  {closure:%s:47} @32
  w @49
string
Function lcg_value() is deprecated since 8.4, use \Random\Randomizer::getFloat() instead
  frames @47
  {closure:%s:47} @32
  w @49
double
Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default
  frames @47
  {closure:%s:47} @32
  w @49
boolean
Function openssl_free_key() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically
  frames @47
  {closure:%s:47} @32
  w @49
NULL
Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead
  frames @47
  {closure:%s:47} @32
  w @49
boolean
Function xml_parser_free() is deprecated since 8.5, as it has no effect since PHP 8.0
  frames @47
  {closure:%s:47} @32
  w @49
boolean
Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions
  frames @47
  {closure:%s:47} @32
  w @49
boolean
Function assert_options() is deprecated since 8.3 @53
Function curl_close() is deprecated since 8.5, as it has no effect since PHP 8.0 @53
Function date_sunrise() is deprecated since 8.1, use date_sun_info() instead @53
Function finfo_close() is deprecated since 8.5, as finfo objects are freed automatically @53
Function strftime() is deprecated since 8.1, use IntlDateFormatter::format() instead @53
Function lcg_value() is deprecated since 8.4, use \Random\Randomizer::getFloat() instead @53
Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default @53
Function openssl_free_key() is deprecated since 8.0, as OpenSSLAsymmetricKey objects are freed automatically @53
Function socket_set_timeout() is deprecated since 8.5, use stream_set_timeout() instead @53
Function xml_parser_free() is deprecated since 8.5, as it has no effect since PHP 8.0 @53
Function xml_set_object() is deprecated since 8.4, provide a proper method callable to xml_set_*_handler() functions @53
Function libxml_disable_entity_loader() is deprecated since 8.0, as external entity loading is disabled by default
bool(false)
