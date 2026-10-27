--TEST--
A list()/[...] destructuring assignment of $http_response_header makes later reads not deprecated; a foreach value fetch is a read
--FILE--
<?php
function a() { [$c, $http_response_header] = [1, ['x']]; var_dump($http_response_header); }
function b() { list($c, list($http_response_header)) = [1, [['y']]]; var_dump(isset($http_response_header), $http_response_header); }
function c() { foreach ([[1]] as $http_response_header) { var_dump($http_response_header); } }
function d() { [, ['k' => $http_response_header]] = [0, ['k' => 2]]; return $http_response_header; }
function e() { return $http_response_header ?? 'none'; }
function f() { $q = $http_response_header; return $q; }
a(); b(); c(); var_dump(d(), e(), @f());

?>
--EXPECTF--

Deprecated: The predefined locally scoped $http_response_header variable is deprecated, call http_get_last_response_headers() instead in %s on line 4

Deprecated: The predefined locally scoped $http_response_header variable is deprecated, call http_get_last_response_headers() instead in %s on line 4

Deprecated: The predefined locally scoped $http_response_header variable is deprecated, call http_get_last_response_headers() instead in %s on line 7
array(1) {
  [0]=>
  string(1) "x"
}
bool(true)
array(1) {
  [0]=>
  string(1) "y"
}
array(1) {
  [0]=>
  int(1)
}
int(2)
string(4) "none"
NULL
