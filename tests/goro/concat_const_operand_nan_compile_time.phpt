--TEST--
Constant concat operands are stringified at compile time (NAN warning first)
--FILE--
<?php
// Constant operands of a concatenation are converted at compile time: a NAN
// warns before anything runs (after constant arithmetic was folded), a
// float uses the precision in effect while compiling.
echo "start\n";
$z = "z";
echo "1:", NAN . $z, "\n";
echo "2:", $z . NAN, "\n";
echo "3:", (string)NAN, "\n";
echo "4:", "a$z" . NAN, "\n";
echo "5:", NAN . NAN, "\n";
echo "6:", $z . [], "\n";
$z .= NAN; echo "7:", $z, "\n";
echo "8:", NAN, "\n";
echo "9:", "{$z}" . -NAN, "\n";
echo "10:", $z . (NAN + 1), "\n";
const K = NAN;
echo "11:", $z . K, "\n";
echo "12:", NAN . 1 . $z, "\n";
function f() { return "x" . NAN . $GLOBALS['z']; }
echo "13:", f(), "\n";
ini_set('precision', 5);
echo $z . 1.23456789, "\n";
echo $z . (1.23456789 * 2), "\n";
echo 1.23456789, "\n";
try { var_dump(php_int_max); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(-0.0 . $z, 2**-1 . $z, 7/7 . $z, INF . $z, -INF . $z);
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 7

Warning: unexpected NAN value was coerced to string in %s on line 8

Warning: unexpected NAN value was coerced to string in %s on line 10

Warning: unexpected NAN value was coerced to string in %s on line 11

Warning: unexpected NAN value was coerced to string in %s on line 11

Warning: unexpected NAN value was coerced to string in %s on line 15

Warning: unexpected NAN value was coerced to string in %s on line 16

Warning: unexpected NAN value was coerced to string in %s on line 19

Warning: unexpected NAN value was coerced to string in %s on line 20
start
1:NANz
2:zNAN
3:
Warning: unexpected NAN value was coerced to string in %s on line 9
NAN
4:azNAN
5:NANNAN
6:
Warning: Array to string conversion in %s on line 12
zArray

Warning: unexpected NAN value was coerced to string in %s on line 13
7:zNAN
8:
Warning: unexpected NAN value was coerced to string in %s on line 14
NAN
9:zNANNAN
10:zNANNAN
11:
Warning: unexpected NAN value was coerced to string in %s on line 18
zNANNAN
12:NAN1zNAN
13:xNANzNAN
zNAN1.23456789
zNAN2.46913578
1.2346
Undefined constant "php_int_max"
string(6) "-0zNAN"
string(7) "0.5zNAN"
string(5) "1zNAN"
string(7) "INFzNAN"
string(8) "-INFzNAN"
