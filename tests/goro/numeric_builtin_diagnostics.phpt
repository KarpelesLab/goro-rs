--TEST--
boolval() NAN warning, intval() out-of-range warning, **= zero base deprecation
--FILE--
<?php
// boolval() warns for NAN, intval() for floats out of range, and **= for
// a zero base with a negative exponent.
$n = NAN;
var_dump(boolval($n), intval(INF), intval($n), intval(1e30), intval(-1e30, 10), intval(1.5));
$z = "0"; $z **= -1; var_dump($z);
$y = null; $y **= -2; var_dump($y);
$x = 0; $x **= -1.5; var_dump($x);
$w = 2; $w **= -1; var_dump($w);
--EXPECTF--

Warning: unexpected NAN value was coerced to bool in %s on line 5

Warning: The float INF is not representable as an int, cast occurred in %s on line 5

Warning: The float NAN is not representable as an int, cast occurred in %s on line 5

Warning: The float 1.0E+30 is not representable as an int, cast occurred in %s on line 5

Warning: The float -1.0E+30 is not representable as an int, cast occurred in %s on line 5
bool(true)
int(0)
int(0)
int(5076964154930102272)
int(-5076964154930102272)
int(1)

Deprecated: Power of base 0 and negative exponent is deprecated in %s on line 6
float(INF)

Deprecated: Power of base 0 and negative exponent is deprecated in %s on line 7
float(INF)

Deprecated: Power of base 0 and negative exponent is deprecated in %s on line 8
float(INF)
float(0.5)
