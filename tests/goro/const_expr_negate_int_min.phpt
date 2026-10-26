--TEST--
Negating PHP_INT_MIN in constant expressions (class constants, defaults, static vars) gives a float
--FILE--
<?php
class C { const X = -(-9223372036854775807-1); public $p = -(-9223372036854775807-1); }
var_dump(C::X, (new C)->p);
function f($a = -(-9223372036854775807-1), $b = PHP_INT_MAX + 1, $c = 1 << 65) { var_dump($a, $b, $c); }
f();
static $s = -(-9223372036854775807-1);
var_dump($s);
const G = -(-9223372036854775807-1) . "";
var_dump(G);
var_dump(-(-9223372036854775807-1));
--EXPECTF--
float(9.223372036854776E+18)
float(9.223372036854776E+18)
float(9.223372036854776E+18)
float(9.223372036854776E+18)
int(0)
float(9.223372036854776E+18)
string(19) "9.2233720368548E+18"
float(9.223372036854776E+18)
