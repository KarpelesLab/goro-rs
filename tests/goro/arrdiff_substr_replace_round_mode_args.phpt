--TEST--
array_udiff() keeps a callback's exception, substr_replace() replacement array conversion, round() $mode string range
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
// The callback's exception, thrown while the first array is sorted, is not
// replaced by the TypeError of the next argument
t(fn() => array_uintersect(["b", "a"], 1, "intval"));
t(fn() => array_udiff(["b", "a"], "x", "intval"));
// substr_replace(): the first element of a replacement array is converted with zval_get_string()
t(fn() => substr_replace("abcABC", [[1]], -2, 100));
// round(): an integer string past the int range is no valid int $mode
t(fn() => round(1.5, 0, "9223372036854775808"));
t(fn() => round(1.5, 0, "1"));
t(fn() => round(1.5, 0, " 2"));
t(fn() => round(1.5, 0, "1e30"));
--EXPECTF--
TypeError: intval(): Argument #2 ($base) must be of type int, string given
TypeError: intval(): Argument #2 ($base) must be of type int, string given

Warning: Array to string conversion in %s on line 8
string(9) "abcAArray"
TypeError: round(): Argument #3 ($mode) must be of type RoundingMode|int, string given
float(2)
float(1)
TypeError: round(): Argument #3 ($mode) must be of type RoundingMode|int, string given
