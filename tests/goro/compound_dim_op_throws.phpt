--TEST--
Compound assignment to a missing element that throws leaves the element null
--FILE--
<?php
// FETCH_DIM_RW adds a missing element as null before the operation: it
// stays when the operation throws.
$a = [];
try { $a['k'] %= 0; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($a);
$b = [];
try { $b['k'] /= 0; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($b);
$c = [];
try { $c['x']['y'] <<= -1; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($c);
$d = ['a' => 1];
try { $d[true] += []; } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($d);
--EXPECTF--

Warning: Undefined array key "k" in %s on line 5
DivisionByZeroError: Modulo by zero
array(1) {
  ["k"]=>
  NULL
}

Warning: Undefined array key "k" in %s on line 8
DivisionByZeroError: Division by zero
array(1) {
  ["k"]=>
  NULL
}

Warning: Undefined array key "x" in %s on line 11

Warning: Undefined array key "y" in %s on line 11
ArithmeticError: Bit shift by negative number
array(1) {
  ["x"]=>
  array(1) {
    ["y"]=>
    NULL
  }
}

Warning: Undefined array key 1 in %s on line 14
TypeError: Unsupported operand types: null + array
array(2) {
  ["a"]=>
  int(1)
  [1]=>
  NULL
}
