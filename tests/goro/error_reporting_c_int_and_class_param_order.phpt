--TEST--
error_reporting is a C int (atoi of the INI value); a scalar given for a class-typed builtin parameter stops the argument conversions
--FILE--
<?php
error_reporting(-1);
var_dump(error_reporting(PHP_INT_MIN), error_reporting(), error_reporting(4294967296 + 5), error_reporting(), error_reporting(PHP_INT_MAX), error_reporting(), error_reporting(-1));
var_dump(error_reporting("1e3"), error_reporting());
error_reporting(-1);
var_dump(ini_set('error_reporting', '4294967297'), error_reporting(), ini_get('error_reporting'));
error_reporting(-1);
var_dump(ini_set('error_reporting', '99999999999999999999'), error_reporting(), ini_get('error_reporting'));
error_reporting(-1);
var_dump(error_reporting(E_ALL & ~E_DEPRECATED), ini_get('error_reporting'));
error_reporting(-1);
var_dump(error_reporting());
function t($c) { try { var_dump($c()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; } }
t(fn() => hash_update_file([], NAN));
t(fn() => hash_update_file("x", NAN));
t(fn() => hash_update_stream("x", null, 1e20));
t(fn() => hash_update_stream(1.5, [], null));
t(fn() => hash_update_stream(false, "x", 1e-10));
t(fn() => hash_update("x", NAN));
t(fn() => hash_update(hash_init('md5'), NAN));
--EXPECTF--
int(-1)
int(0)
int(0)
int(5)
int(5)
int(-1)
int(-1)
int(-1)
int(1000)
string(2) "-1"
int(1)
string(10) "4294967297"
string(2) "-1"
int(-1)
string(20) "99999999999999999999"
int(-1)
string(5) "22527"
int(-1)
TypeError: hash_update_file(): Argument #1 ($context) must be of type HashContext, array given
TypeError: hash_update_file(): Argument #1 ($context) must be of type HashContext, string given
TypeError: hash_update_stream(): Argument #1 ($context) must be of type HashContext, string given
TypeError: hash_update_stream(): Argument #1 ($context) must be of type HashContext, float given
TypeError: hash_update_stream(): Argument #1 ($context) must be of type HashContext, false given
TypeError: hash_update(): Argument #1 ($context) must be of type HashContext, string given

Warning: unexpected NAN value was coerced to string in %s on line 20
bool(true)
