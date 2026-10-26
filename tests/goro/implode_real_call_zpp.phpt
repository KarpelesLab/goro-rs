--TEST--
implode() called dynamically or with unpacked arguments parses its parameters like PHP_FUNCTION(implode)
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$fn = 'implode';
t(fn() => $fn(...[NAN]));
t(fn() => $fn(NAN));
t(fn() => implode(...[NAN]));
t(fn() => $fn(...[null]));
t(fn() => $fn(...[new stdClass]));
t(fn() => $fn(...[[3, 1], 0]));
t(fn() => $fn([3, 1], 0));
$st = 'settype';
t(function () use ($st) { $a = [[3, 1, 2], 0]; return $st(...$a); });
t(function () { $a = [[3, 1, 2], 0]; return settype(...$a); });
t(function () { $x = [1]; return settype($x, 0); });
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 4
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given

Warning: unexpected NAN value was coerced to string in %s on line 5
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given

Warning: unexpected NAN value was coerced to string in %s on line 6
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given

Deprecated: implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 7
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given
TypeError: implode(): Argument #1 ($separator) must be of type array|string, stdClass given
TypeError: implode(): Argument #2 ($array) must be of type ?array, int given
TypeError: implode(): Argument #2 ($array) must be of type ?array, int given
ValueError: settype(): Argument #2 ($type) must be a valid type
ValueError: settype(): Argument #2 ($type) must be a valid type
ValueError: settype(): Argument #2 ($type) must be a valid type
