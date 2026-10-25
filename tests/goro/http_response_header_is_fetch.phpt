--TEST--
$http_response_header fetched for isset() / empty() / ?? is not deprecated
--FILE--
<?php
function a() { return isset($http_response_header); }
function b() { return empty($http_response_header); }
function c() { return $http_response_header ?? 'none'; }
function d() { return isset($http_response_header[0]); }
function e() { return $http_response_header[0] ?? 'none'; }
function f() { return $http_response_header; }
var_dump(a(), b(), c(), d(), e());
?>
--EXPECTF--
Deprecated: The predefined locally scoped $http_response_header variable is deprecated, call http_get_last_response_headers() instead in %s on line 7
bool(false)
bool(true)
string(4) "none"
bool(false)
string(4) "none"
