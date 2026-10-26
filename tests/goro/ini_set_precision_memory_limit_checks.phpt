--TEST--
ini_set(): precision checks, memory_limit warning prefix, floats stored as strings, ini_get_all("standard")
--FILE--
<?php
var_dump(ini_set("memory_limit", "1x"));
var_dump(ini_set("memory_limit", "abc"));
var_dump(ini_set("memory_limit", "-1"));
var_dump(ini_set("serialize_precision", "-5"), ini_get("serialize_precision"));
var_dump(ini_set("precision", "-2"), ini_get("precision"));
var_dump(ini_set("precision", " 5x"), ini_get("precision"), 1/3);
var_dump(ini_set("precision", 1.5), ini_get("precision"), 2/3);
var_dump(ini_set("precision", 14));
var_dump(ini_set("serialize_precision", "-1"));
var_dump(array_keys(ini_get_all("standard")));
var_dump(ini_get_all("standard")["unserialize_max_depth"]);
--EXPECTF--

Warning: Invalid "memory_limit" setting. Invalid quantity "1x": unknown multiplier "x", interpreting as "1" for backwards compatibility in %s on line 2

Warning: Failed to set memory limit to 1 bytes (Current memory usage is 2097152 bytes) in %s on line 2
bool(false)

Warning: Invalid "memory_limit" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 3

Warning: Failed to set memory limit to 0 bytes (Current memory usage is 2097152 bytes) in %s on line 3
bool(false)
string(4) "128M"
bool(false)
string(2) "-1"
bool(false)
string(2) "14"
string(2) "14"
string(3) " 5x"
float(0.3333333333333333)
string(3) " 5x"
string(3) "1.5"
float(0.6666666666666666)
string(3) "1.5"
string(2) "-1"
array(14) {
  [0]=>
  string(13) "assert.active"
  [1]=>
  string(11) "assert.bail"
  [2]=>
  string(15) "assert.callback"
  [3]=>
  string(16) "assert.exception"
  [4]=>
  string(14) "assert.warning"
  [5]=>
  string(24) "auto_detect_line_endings"
  [6]=>
  string(22) "default_socket_timeout"
  [7]=>
  string(4) "from"
  [8]=>
  string(23) "session.trans_sid_hosts"
  [9]=>
  string(22) "session.trans_sid_tags"
  [10]=>
  string(21) "unserialize_max_depth"
  [11]=>
  string(18) "url_rewriter.hosts"
  [12]=>
  string(17) "url_rewriter.tags"
  [13]=>
  string(10) "user_agent"
}
array(3) {
  ["global_value"]=>
  string(4) "4096"
  ["local_value"]=>
  string(4) "4096"
  ["access"]=>
  int(7)
}
