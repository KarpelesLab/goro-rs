--TEST--
array_filter() copies a refcount-1 reference as its value
--FILE--
<?php
// array_filter() copies a reference only the array holds as its value
// (zval_add_ref), with or without a callback.
$x = ['A' => 1, 'a' => 2, 'B' => [3]]; $y = &$x;
foreach ($x as &$vv) { if (is_int($vv)) $vv *= 2; }
unset($vv);
var_dump(array_filter($x), array_filter($x, fn($v) => true), array_filter($x, 'is_int'));
$r = 7; $z = [&$r, 8];
var_dump(array_filter($z));
--EXPECT--
array(3) {
  ["A"]=>
  int(2)
  ["a"]=>
  int(4)
  ["B"]=>
  array(1) {
    [0]=>
    int(3)
  }
}
array(3) {
  ["A"]=>
  int(2)
  ["a"]=>
  int(4)
  ["B"]=>
  array(1) {
    [0]=>
    int(3)
  }
}
array(2) {
  ["A"]=>
  int(2)
  ["a"]=>
  int(4)
}
array(2) {
  [0]=>
  &int(7)
  [1]=>
  int(8)
}
