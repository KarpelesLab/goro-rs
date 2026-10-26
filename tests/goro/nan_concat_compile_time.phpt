--TEST--
NAN concatenated in class constants / property defaults warns at compile time, in parameter defaults at run time
--FILE--
<?php
echo "start\n";
const G = "g" . NAN;
class C { const K = "k" . NAN; public $p = "p" . NAN; }
function f($a = "d" . NAN) { return $a; }
echo C::K, "\n";
echo G, "\n";
echo "x" . NAN, "\n";
$n = NAN; echo "y" . $n, "\n";
echo f(), "\n";
echo (new C)->p, "\n";
$c = function($x = "c" . NAN) { return $x; }; echo $c(), "\n";
class D { function m($a = "m" . NAN) { return $a; } static $s = "s" . NAN; }
echo (new D)->m(), D::$s, "\n";
echo (fn($z = "z" . NAN) => $z)(), "\n";
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 3

Warning: unexpected NAN value was coerced to string in %s on line 4

Warning: unexpected NAN value was coerced to string in %s on line 4

Warning: unexpected NAN value was coerced to string in %s on line 8

Warning: unexpected NAN value was coerced to string in %s on line 13
start
kNAN
gNAN
xNAN

Warning: unexpected NAN value was coerced to string in %s on line 9
yNAN

Warning: unexpected NAN value was coerced to string in %s on line 5
dNAN
pNAN

Warning: unexpected NAN value was coerced to string in %s on line 12
cNAN

Warning: unexpected NAN value was coerced to string in %s on line 13
mNANsNAN

Warning: unexpected NAN value was coerced to string in %s on line 15
zNAN
