--TEST--
Two-argument max()/min() with NAN and ints a double cannot hold; float vs non-numeric string order
--FILE--
<?php
// Two-argument max()/min() (ZEND_FRAMELESS_FUNCTION): an int a double
// cannot hold exactly is compared through zend_compare().
$n = NAN;
var_dump(max(PHP_INT_MAX, $n), max(5, $n), max($n, 5), min(PHP_INT_MAX, $n), min(5, $n), min($n, PHP_INT_MAX), max(PHP_INT_MAX - 1, 9.3e18), min(PHP_INT_MIN, -9.3e18));
// a float compared with a non-numeric string compares byte-wise
var_dump(~"abc" <=> -INF, -INF <=> ~"abc", "\xff" <=> 1.5, 1.5 <=> "\xff", "\xff" == INF);
--EXPECT--
int(9223372036854775807)
float(NAN)
int(5)
float(NAN)
float(NAN)
int(9223372036854775807)
float(9.3E+18)
float(-9.3E+18)
int(1)
int(-1)
int(1)
int(-1)
bool(false)
