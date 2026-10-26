--TEST--
A by-value variadic parameter of a callback gets the values of the references a builtin passes
--FILE--
<?php
$a = [1];
array_walk($a, function(...$x) { var_dump($x); });
array_walk($a, function(int ...$x) { var_dump($x); });
array_walk($a, function($v, ...$x) { var_dump($x); }, 5);
function f(...$x) { var_dump($x); }
array_walk($a, 'f');
$o = [3, 1]; usort($o, function(...$x) { var_dump($x); return 0; });
preg_replace_callback('/a/', function(...$m) { var_dump($m); return ''; }, 'a');
--EXPECT--
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(0)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(0)
}
array(2) {
  [0]=>
  int(0)
  [1]=>
  int(5)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(0)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(1)
}
array(1) {
  [0]=>
  array(1) {
    [0]=>
    string(1) "a"
  }
}
