--TEST--
ini_set('error_reporting', ...) parses the value with atoi() (no constant expressions)
--FILE--
<?php
error_reporting(E_ALL);
var_dump(ini_set('error_reporting', 'E_ALL & ~E_DEPRECATED'), error_reporting(), ini_get('error_reporting'));
var_dump(ini_set('error_reporting', '32767'), error_reporting());
var_dump(ini_set('error_reporting', '0x10'), error_reporting());
var_dump(ini_set('error_reporting', ' 12abc'), error_reporting());
var_dump(ini_set('error_reporting', '-1'), error_reporting());
var_dump(ini_set('error_reporting', '1e3'), error_reporting());
var_dump(ini_set('error_reporting', ''), error_reporting());
var_dump(ini_set('error_reporting', 'E_ALL'), error_reporting());

?>
--EXPECTF--
string(5) "30719"
int(0)
string(21) "E_ALL & ~E_DEPRECATED"
string(21) "E_ALL & ~E_DEPRECATED"
int(32767)
string(5) "32767"
int(0)
string(4) "0x10"
int(12)
string(6) " 12abc"
int(-1)
string(2) "-1"
int(1)
string(3) "1e3"
int(0)
string(0) ""
int(0)
