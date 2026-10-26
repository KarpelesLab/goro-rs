--TEST--
compact(var_name: ...) is not a dynamic call; a named argument to a prefer-ref parameter is sent by reference
--FILE--
<?php
$x = 1;
var_dump(compact(var_name: "x"));
function f() { $y = 2; return compact(var_name: "y"); }
var_dump(f());
var_dump((fn() => compact(var_name: 'x'))());
$arr = [3, 1, 2];
var_dump(array_multisort(array: $arr), $arr);
try { var_dump(array_multisort(array: $undef)); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$m = [[2, 1]];
array_multisort(array: $m[0]);
var_dump($m);
--EXPECTF--
array(1) {
  ["x"]=>
  int(1)
}
array(1) {
  ["y"]=>
  int(2)
}

Warning: compact(): Undefined variable $x in %s on line 6
array(0) {
}
bool(true)
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
TypeError: array_multisort(): Argument #1 ($array) must be an array or a sort flag
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
}
