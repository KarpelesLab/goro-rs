--TEST--
Assigning a fractional float through a typed reference raises the implicit conversion deprecation
--FILE--
<?php
class U { public ?int $x = null; public int $y = 0; public static int $s = 0; }
$u = new U;
$refx = &$u->x;
$refx = 3.5;
var_dump($u->x);
$refx = "4.5";
var_dump($u->x);
$u->y = 1;
$ry = &$u->y;
$ry = 7.25;
var_dump($u->y);
$u->y = 8.5;
var_dump($u->y);
$rs = &U::$s;
$rs = 9.5;
var_dump(U::$s);
$arr = [&$u->x];
$arr[0] = 1.5;
var_dump($u->x);
--EXPECTF--

Deprecated: Implicit conversion from float 3.5 to int loses precision in %s on line 5
int(3)

Deprecated: Implicit conversion from float-string "4.5" to int loses precision in %s on line 7
int(4)

Deprecated: Implicit conversion from float 7.25 to int loses precision in %s on line 11
int(7)

Deprecated: Implicit conversion from float 8.5 to int loses precision in %s on line 13
int(8)

Deprecated: Implicit conversion from float 9.5 to int loses precision in %s on line 16
int(9)

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 19
int(1)
