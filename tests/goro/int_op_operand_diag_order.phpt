--TEST--
Integer operators: each operand's warning/deprecation in operand order
--FILE--
<?php
// Integer operators convert op1 then op2, each with its own diagnostics
var_dump(2.5 % "3 apples");
var_dump("1.5" & "0x1A" . "");
var_dump(2.5 | "7 days");
var_dump("2 x" << 1.5);
$x = 7.5; $x %= "2 y"; var_dump($x);
$y = "3 z"; $y >>= 1.5; var_dump($y);
$z = 9.9; $z ^= "1 q"; var_dump($z);
--EXPECTF--

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 3

Warning: A non-numeric value encountered in %s on line 3
int(2)
string(3) "0(1"

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 5

Warning: A non-numeric value encountered in %s on line 5
int(7)

Warning: A non-numeric value encountered in %s on line 6

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 6
int(4)

Deprecated: Implicit conversion from float 7.5 to int loses precision in %s on line 7

Warning: A non-numeric value encountered in %s on line 7
int(1)

Warning: A non-numeric value encountered in %s on line 8

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 8
int(1)

Deprecated: Implicit conversion from float 9.9 to int loses precision in %s on line 9

Warning: A non-numeric value encountered in %s on line 9
int(8)
