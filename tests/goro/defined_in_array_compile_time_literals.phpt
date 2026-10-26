--TEST--
defined() / in_array(): literal arguments php-src folds at compile time are not type checked
--FILE--
<?php declare(strict_types=1);
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
const X1 = 1;
t(fn() => var_dump(defined(1)));
t(fn() => var_dump(defined(1.5)));
t(fn() => var_dump(defined(-1)));
t(fn() => var_dump(defined(true)));
t(fn() => var_dump(defined(null)));
t(fn() => var_dump(defined("X1")));
t(fn() => var_dump(\defined(2)));
t(fn() => var_dump(in_array(1, [], null)));
t(fn() => var_dump(in_array(1, ["a", "b"], 0)));
t(fn() => var_dump(in_array(1, ["1", "b"], 0)));
t(fn() => var_dump(in_array("a", ["a", "b"], "x")));
t(fn() => var_dump(in_array("a", ["a", 1], "x")));
t(fn() => var_dump(in_array("a", ["a", 1.5], "x")));
t(fn() => var_dump(in_array(1, [], true)));
t(fn() => var_dump(in_array(1, [X1], 1)));
t(fn() => var_dump(in_array(1, [], X1)));
--EXPECT--
bool(false)
bool(false)
TypeError: defined(): Argument #1 ($constant_name) must be of type string, int given
TypeError: defined(): Argument #1 ($constant_name) must be of type string, true given
TypeError: defined(): Argument #1 ($constant_name) must be of type string, null given
bool(true)
bool(false)
bool(false)
bool(false)
TypeError: in_array(): Argument #3 ($strict) must be of type bool, int given
bool(true)
bool(true)
TypeError: in_array(): Argument #3 ($strict) must be of type bool, string given
bool(false)
TypeError: in_array(): Argument #3 ($strict) must be of type bool, int given
TypeError: in_array(): Argument #3 ($strict) must be of type bool, int given
