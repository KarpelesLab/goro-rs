--TEST--
ini_alter() / user_error(): errors name the alias called
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => ini_alter("precision"));
t(fn() => ini_alter("precision", []));
t(fn() => var_dump(ini_alter("assert.active", "1")));
t(fn() => user_error("x", 5));
t(fn() => trigger_error("x", 5));
--EXPECT--
ArgumentCountError: ini_alter() expects exactly 2 arguments, 1 given
TypeError: ini_alter(): Argument #2 ($value) must be of type string|int|float|bool|null
string(1) "1"
ValueError: user_error(): Argument #2 ($error_level) must be one of E_USER_ERROR, E_USER_WARNING, E_USER_NOTICE, or E_USER_DEPRECATED
ValueError: trigger_error(): Argument #2 ($error_level) must be one of E_USER_ERROR, E_USER_WARNING, E_USER_NOTICE, or E_USER_DEPRECATED
