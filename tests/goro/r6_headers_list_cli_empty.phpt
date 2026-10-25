--TEST--
CLI: headers_list() stays empty (the CLI header handler keeps nothing)
--FILE--
<?php
header('X-A: b');
setcookie('c', 'd');
header('Content-Type: text/plain');
var_dump(headers_list(), headers_sent());
echo "x\n";
var_dump(headers_list(), headers_sent(), http_response_code());
header_remove('X-A');
var_dump(headers_list());
--EXPECTF--
array(0) {
}
bool(false)
x
array(0) {
}
bool(true)
bool(false)

Warning: Cannot modify header information - headers already sent by (output started at %s:5) in %s on line 8
array(0) {
}
