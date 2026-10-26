--TEST--
OnUpdateLong INI entries (unserialize_max_depth, default_socket_timeout, ...) warn about a malformed quantity
--FILE--
<?php
var_dump(ini_set('unserialize_max_depth', 'abc'), ini_get('unserialize_max_depth'));
var_dump(unserialize('a:1:{i:0;a:1:{i:0;i:1;}}'));
var_dump(ini_set('unserialize_max_depth', '3x'), ini_get('unserialize_max_depth'));
var_dump(ini_set('unserialize_max_depth', '1k'), ini_get('unserialize_max_depth'));
var_dump(ini_set('default_socket_timeout', '5 seconds'));
var_dump(ini_set('error_log_mode', '0644'));
--EXPECTF--

Warning: Invalid "unserialize_max_depth" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line %d
string(4) "4096"
string(3) "abc"
array(1) {
  [0]=>
  array(1) {
    [0]=>
    int(1)
  }
}

Warning: Invalid "unserialize_max_depth" setting. Invalid quantity "3x": unknown multiplier "x", interpreting as "3" for backwards compatibility in %s on line %d
string(3) "abc"
string(2) "3x"
string(2) "3x"
string(2) "1k"

Warning: Invalid "default_socket_timeout" setting. Invalid quantity "5 seconds": unknown multiplier "s", interpreting as "5 " for backwards compatibility in %s on line %d
string(2) "60"
string(4) "0644"
