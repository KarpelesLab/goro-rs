--TEST--
NAN to int diagnostics, shortest floats in implicit conversion deprecations, xor warnings
--FILE--
<?php
// NAN converted to int warns and is deprecated; implicit float to int
// deprecations print the shortest round-trip float; xor warns on
// undefined and NAN operands.
function k() {
    $a = null; $b = NAN;
    var_dump($a >> $b);
    var_dump($b >> 1);
    var_dump(1 << $b);
    var_dump($a | $b);
    var_dump(~$b);
}
k();
class C { public int $t = 5; }
$o = new C;
$o->t **= "4.5";
var_dump($o->t);
$o->t = 5;
$o->t /= 7;
var_dump($o->t);
$arr = [1, 2, 3];
var_dump($arr[0.7142857142857143]);
var_dump(str_repeat("x", 2.0000000000000004));
function j() {
    $a = "0"; $b = NAN;
    var_dump($a xor $b);
    var_dump($b xor $a);
    var_dump($u xor 1);
}
j();
--EXPECTF--

Warning: The float NAN is not representable as an int, cast occurred in %s on line 7

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 7
int(0)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 8

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 8
int(0)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 9

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 9
int(1)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 10

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 10
int(0)

Warning: The float NAN is not representable as an int, cast occurred in %s on line 11

Deprecated: Implicit conversion from float NAN to int loses precision in %s on line 11
int(-1)

Deprecated: Implicit conversion from float 1397.5424859373686 to int loses precision in %s on line 16
int(1397)

Deprecated: Implicit conversion from float 0.7142857142857143 to int loses precision in %s on line 19
int(0)

Deprecated: Implicit conversion from float 0.7142857142857143 to int loses precision in %s on line 22
int(1)

Deprecated: Implicit conversion from float 2.0000000000000004 to int loses precision in %s on line 23
string(2) "xx"

Warning: unexpected NAN value was coerced to bool in %s on line 26
bool(true)

Warning: unexpected NAN value was coerced to bool in %s on line 27
bool(true)

Warning: Undefined variable $u in %s on line 28
bool(true)
