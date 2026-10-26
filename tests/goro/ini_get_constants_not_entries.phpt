--TEST--
ini_get()/ini_set()/ini_restore(): constants (NAN, PHP_VERSION, PHP_EOL) are not INI entries
--FILE--
<?php
var_dump(ini_get("NAN"), ini_get("PHP_VERSION"), ini_get("E_ALL"), ini_get("INF"), ini_get("PHP_EOL"), ini_get("M_PI"));
var_dump(ini_set("NAN", "1"), ini_set("PHP_EOL", "x"), PHP_EOL === "\n", ini_restore("NAN"), NAN);
var_dump(ini_get("precision"), ini_get("SMTP"), ini_get("date.timezone") !== false);
--EXPECT--
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(false)
bool(true)
NULL
float(NAN)
string(2) "14"
string(9) "localhost"
bool(true)
