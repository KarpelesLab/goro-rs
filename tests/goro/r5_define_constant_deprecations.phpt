--TEST--
define() case_insensitive warning, constant() deprecations, no money_format()
--FILE--
<?php
var_dump(define('X', 1, true));
var_dump(define('Y', 1, false));
try { define('A::B', 1, true); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
var_dump(constant('E_STRICT'));
var_dump(constant('FILE_TEXT'));
var_dump(constant('MT_RAND_PHP'));
var_dump(function_exists('money_format'));
--EXPECTF--

Warning: define(): Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported in %s on line 2
bool(true)
bool(true)
define(): Argument #1 ($constant_name) cannot be a class constant

Deprecated: Constant E_STRICT is deprecated since 8.4, the error level was removed in %s on line 5
int(2048)

Deprecated: Constant FILE_TEXT is deprecated since 8.1, as the constant has no effect in %s on line 6
int(0)

Deprecated: Constant MT_RAND_PHP is deprecated since 8.3, as it uses a biased non-standard variant of Mt19937 in %s on line 7
int(1)
bool(false)
