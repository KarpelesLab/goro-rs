--TEST--
ini_restore() keeps entries without on_modify (8.5), restores mbstring NULL entries silently; mbstring.regex_*_limit quantity warnings
--FILE--
<?php
var_dump(ini_get('highlight.string'));
var_dump(ini_set('highlight.string', '#123456'), ini_get('highlight.string'));
ini_restore('highlight.string');
var_dump(ini_get('highlight.string'));
var_dump(ini_set('highlight.string', 15), ini_get('highlight.string'));
ini_restore('highlight.string');
var_dump(ini_get('highlight.string'));
var_dump(ini_get('date.default_latitude'));
var_dump(ini_set('date.default_latitude', '5'), ini_get('date.default_latitude'));
ini_restore('date.default_latitude');
var_dump(ini_get('date.default_latitude'));
var_dump(ini_set('mbstring.internal_encoding', ''));
ini_restore('mbstring.internal_encoding');
var_dump(ini_set('mbstring.internal_encoding', 'UTF-8'));
ini_restore('mbstring.internal_encoding');
var_dump(ini_set('mbstring.regex_retry_limit', 'abc'), ini_set('mbstring.regex_retry_limit', '1e3'), ini_set('mbstring.regex_stack_limit', 'x'));
var_dump(ini_set('user_agent', 'x'));
ini_restore('user_agent');
var_dump(ini_get('user_agent'));
--EXPECTF--
string(7) "#DD0000"
string(7) "#DD0000"
string(7) "#123456"
string(7) "#123456"
string(7) "#123456"
string(2) "15"
string(2) "15"
string(7) "31.7667"
string(7) "31.7667"
string(1) "5"
string(1) "5"

Deprecated: ini_set(): Use of mbstring.internal_encoding is deprecated in %s on line 13
string(0) ""

Deprecated: ini_set(): Use of mbstring.internal_encoding is deprecated in %s on line 15
string(0) ""

Warning: Invalid "mbstring.regex_retry_limit" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 17

Warning: Invalid "mbstring.regex_retry_limit" setting. Invalid quantity "1e3": unknown multiplier "3", interpreting as "1" for backwards compatibility in %s on line 17

Warning: Invalid "mbstring.regex_stack_limit" setting. Invalid quantity "x": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 17
string(7) "1000000"
string(3) "abc"
string(6) "100000"
string(0) ""
string(0) ""
