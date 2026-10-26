--TEST--
A generator function's variadic parameter collects the extra named arguments
--FILE--
<?php
function gf($a = 1, ...$rest) { var_dump($rest); yield; }
gf(rest: 1)->current();
gf(x: 1, y: 2)->current();
gf(1, 2, x: 3)->current();
function gf2(...$rest) { var_dump($rest, func_get_args()); yield; }
gf2(x: 1)->current();
class C {
    function m(...$r) { var_dump($r); yield; }
    static function s($a, ...$r) { var_dump($r); yield; }
}
(new C)->m(1, k: 2)->current();
C::s(1, k: 2)->current();
$f = function(...$r) { var_dump($r); yield; };
$f(z: 9)->current();
call_user_func('gf2', q: 1)->current();
call_user_func_array([new C, 'm'], ['w' => 5])->current();
$g = gf2(...['a' => 1]);
foreach ($g as $_) {}
--EXPECT--
array(1) {
  ["rest"]=>
  int(1)
}
array(2) {
  ["x"]=>
  int(1)
  ["y"]=>
  int(2)
}
array(2) {
  [0]=>
  int(2)
  ["x"]=>
  int(3)
}
array(1) {
  ["x"]=>
  int(1)
}
array(0) {
}
array(2) {
  [0]=>
  int(1)
  ["k"]=>
  int(2)
}
array(1) {
  ["k"]=>
  int(2)
}
array(1) {
  ["z"]=>
  int(9)
}
array(1) {
  ["q"]=>
  int(1)
}
array(0) {
}
array(1) {
  ["w"]=>
  int(5)
}
array(1) {
  ["a"]=>
  int(1)
}
array(0) {
}
