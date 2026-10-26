--TEST--
Concatenation converts op1 (its warnings, __toString()) before op2
--FILE--
<?php
class TS { function __toString(): string { echo "[ts]"; return "t"; } }
function c($a, $b) { return $a . $b; }
var_dump(c(NAN, []));
var_dump(c(new TS, []));
var_dump(c([], new TS));
var_dump(c([], NAN));
$x = NAN; $x .= []; var_dump($x);
$y = new TS; $y .= [1]; var_dump($y);
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 3

Warning: Array to string conversion in %s on line 3
string(8) "NANArray"
[ts]
Warning: Array to string conversion in %s on line 3
string(6) "tArray"

Warning: Array to string conversion in %s on line 3
[ts]string(6) "Arrayt"

Warning: Array to string conversion in %s on line 3

Warning: unexpected NAN value was coerced to string in %s on line 3
string(8) "ArrayNAN"

Warning: unexpected NAN value was coerced to string in %s on line 8

Warning: Array to string conversion in %s on line 8
string(8) "NANArray"
[ts]
Warning: Array to string conversion in %s on line 9
string(6) "tArray"
