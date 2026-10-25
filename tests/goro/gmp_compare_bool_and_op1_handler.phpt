--TEST--
GMP comparisons: bools through the bool cast, op1's compare handler
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$a = gmp_init(42);
$o = new stdClass;
// a bool compares with the object's bool cast
var_dump($a == true, $a <=> true, gmp_init(0) == false, gmp_init(2) > true, true < gmp_init(2), gmp_init(0) < true);
// op1's compare handler is used: `a > b` is `b < a`
var_dump($a > $o, $a >= $o, $o < $a, $o == $a);
t(fn() => $a < $o);
t(fn() => $a <= $o);
t(fn() => $a == $o);
--EXPECT--
bool(true)
int(0)
bool(true)
bool(false)
bool(false)
bool(true)
bool(false)
bool(false)
bool(false)
bool(false)
TypeError: Number must be of type GMP|string|int, stdClass given
TypeError: Number must be of type GMP|string|int, stdClass given
TypeError: Number must be of type GMP|string|int, stdClass given
