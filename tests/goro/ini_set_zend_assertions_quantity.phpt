--TEST--
ini_set('zend.assertions'): quantity parsing, -1 switch refused
--INI--
zend.assertions=1
--FILE--
<?php
var_dump(ini_set("zend.assertions", "-1"), ini_get("zend.assertions"));
var_dump(ini_set("zend.assertions", "0"), ini_get("zend.assertions"));
var_dump(ini_set("zend.assertions", "1M"), ini_get("zend.assertions"));
var_dump(ini_set("zend.assertions", "abc"), ini_get("zend.assertions"));
var_dump(ini_set("zend.assertions", "1"), ini_get("zend.assertions"));
--EXPECTF--
Warning: zend.assertions may be completely enabled or disabled only in php.ini in %s on line 2
bool(false)
string(1) "1"
string(1) "1"
string(1) "0"
string(1) "0"
string(2) "1M"

Warning: Invalid "zend.assertions" setting. Invalid quantity "abc": no valid leading digits, interpreting as "0" for backwards compatibility in %s on line 5
string(2) "1M"
string(3) "abc"
string(3) "abc"
string(1) "1"
