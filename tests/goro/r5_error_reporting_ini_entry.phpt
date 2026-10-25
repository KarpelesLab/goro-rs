--TEST--
error_reporting() changes the error_reporting INI entry
--FILE--
<?php
error_reporting(E_ALL); ini_set("error_reporting", E_ALL);
var_dump(error_reporting(E_ALL & ~E_NOTICE));
var_dump(error_reporting(), ini_get('error_reporting'));
error_reporting(0);
var_dump(ini_get('error_reporting'));
ini_set('error_reporting', '8191');
var_dump(error_reporting(), ini_get('error_reporting'));
error_reporting(E_ALL);
var_dump(@error_reporting(), error_reporting(null), ini_get('error_reporting'));
--EXPECT--
int(30719)
int(30711)
string(5) "30711"
string(1) "0"
int(8191)
string(4) "8191"
int(4437)
int(30719)
string(5) "30719"
