--TEST--
Arithmetic of int strings, / and % of numbers
--FILE--
<?php
function f($a, $b) {
    $r = [];
    $r[] = $a + 1; $r[] = $a - 2; $r[] = $a * 3; $r[] = $a / 4; $r[] = $a % 5; $r[] = 7 % $a;
    $r[] = $b + $a; $r[] = "-0" + 0; $r[] = "+5" * 2; $r[] = "007" - 1; $r[] = "999999999999999999" + 1; $r[] = "999999999999999999" * 100;
    $r[] = "9223372036854775807" + 1; $r[] = "1.5" + 1; $r[] = " 5" + 1; $r[] = "5 " + 1; $r[] = "1e3" + 0;
    $c = 10; $c += $a; $r[] = $c; $c -= "3"; $r[] = $c; $c *= "-2"; $r[] = $c;
    $d = 1.5; $d += "2"; $r[] = $d;
    $r[] = $a / "0.5";
    try { $r[] = $a % "0"; } catch (\DivisionByZeroError $e) { $r[] = $e->getMessage(); }
    try { $r[] = $a / "0"; } catch (\DivisionByZeroError $e) { $r[] = $e->getMessage(); }
    $r[] = 10 / 4; $r[] = 10 / 5; $r[] = -7 % 3; $r[] = PHP_INT_MIN % -1; $r[] = PHP_INT_MIN / -1; $r[] = 7.5 / 2;
    var_dump($r);
}
f("12", "-3");
f("3", "100");
set_error_handler(function ($no, $str) { echo "ERR: $str\n"; return true; });
var_dump("5x" + 1, "12" + "0x1A");
--EXPECT--
array(30) {
  [0]=>
  int(13)
  [1]=>
  int(10)
  [2]=>
  int(36)
  [3]=>
  int(3)
  [4]=>
  int(2)
  [5]=>
  int(7)
  [6]=>
  int(9)
  [7]=>
  int(0)
  [8]=>
  int(10)
  [9]=>
  int(6)
  [10]=>
  int(1000000000000000000)
  [11]=>
  float(1.0E+20)
  [12]=>
  float(9.223372036854776E+18)
  [13]=>
  float(2.5)
  [14]=>
  int(6)
  [15]=>
  int(6)
  [16]=>
  float(1000)
  [17]=>
  int(22)
  [18]=>
  int(19)
  [19]=>
  int(-38)
  [20]=>
  float(3.5)
  [21]=>
  float(24)
  [22]=>
  string(14) "Modulo by zero"
  [23]=>
  string(16) "Division by zero"
  [24]=>
  float(2.5)
  [25]=>
  int(2)
  [26]=>
  int(-1)
  [27]=>
  int(0)
  [28]=>
  float(9.223372036854776E+18)
  [29]=>
  float(3.75)
}
array(30) {
  [0]=>
  int(4)
  [1]=>
  int(1)
  [2]=>
  int(9)
  [3]=>
  float(0.75)
  [4]=>
  int(3)
  [5]=>
  int(1)
  [6]=>
  int(103)
  [7]=>
  int(0)
  [8]=>
  int(10)
  [9]=>
  int(6)
  [10]=>
  int(1000000000000000000)
  [11]=>
  float(1.0E+20)
  [12]=>
  float(9.223372036854776E+18)
  [13]=>
  float(2.5)
  [14]=>
  int(6)
  [15]=>
  int(6)
  [16]=>
  float(1000)
  [17]=>
  int(13)
  [18]=>
  int(10)
  [19]=>
  int(-20)
  [20]=>
  float(3.5)
  [21]=>
  float(6)
  [22]=>
  string(14) "Modulo by zero"
  [23]=>
  string(16) "Division by zero"
  [24]=>
  float(2.5)
  [25]=>
  int(2)
  [26]=>
  int(-1)
  [27]=>
  int(0)
  [28]=>
  float(9.223372036854776E+18)
  [29]=>
  float(3.75)
}
ERR: A non-numeric value encountered
ERR: A non-numeric value encountered
int(6)
int(12)
