--TEST--
Casts in constant expressions warn when evaluated (array to string, float not representable as int)
--FILE--
<?php
class X {
    const B = (string) [1];
    const U = (int) 1e30;
    const V = (int) "12abc";
    const W = (int) NAN;
}
var_dump(X::B);
var_dump(X::U);
var_dump(X::V);
var_dump(X::W);
function f($a = (string) [2], $b = (int) 1e20) { var_dump($a, $b); }
f();
--EXPECTF--

Warning: Array to string conversion in %s on line 3
string(5) "Array"

Warning: The float 1.0E+30 is not representable as an int, cast occurred in %s on line 4
int(5076964154930102272)
int(12)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 6
int(0)

Warning: Array to string conversion in %s on line 12

Warning: The float 1.0E+20 is not representable as an int, cast occurred in %s on line 12
string(5) "Array"
int(7766279631452241920)
