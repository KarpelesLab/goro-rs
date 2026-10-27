--TEST--
Every argument a typed variadic collects gets the implicit float to int deprecation
--FILE--
<?php
function f(int ...$n) { var_dump($n); }
f(2.5);
f(1, 2.5);
function g($a, int ...$n) { var_dump($n); }
g(1, 2.5, "3.5");
$c = function(int ...$n) { var_dump($n); }; $c(4.5);
--EXPECTF--

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 2
array(1) {
  [0]=>
  int(2)
}

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 2
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}

Deprecated: Implicit conversion from float 2.5 to int loses precision in %s on line 5

Deprecated: Implicit conversion from float-string "3.5" to int loses precision in %s on line 5
array(2) {
  [0]=>
  int(2)
  [1]=>
  int(3)
}

Deprecated: Implicit conversion from float 4.5 to int loses precision in %s on line 7
array(1) {
  [0]=>
  int(4)
}
