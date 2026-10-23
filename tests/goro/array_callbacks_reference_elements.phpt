--TEST--
Array functions on elements left as references by foreach by reference
--FILE--
<?php
// Elements left as references by foreach-by-reference reach internal
// callbacks and array_flip() as their values.
function mk() { $x = ['b' => 3, 'a' => 1, 'c' => '2']; foreach ($x as &$vv) {} unset($vv); return $x; }
$x = mk();
var_dump(array_filter($x, 'is_numeric'), array_filter($x, 'is_int'), array_flip($x));
$y = [3, 1]; foreach ($y as &$v) {} unset($v);
var_dump(array_chunk($y, 1), array_unique($y), array_diff($y, [9]), array_replace($y, []), array_combine(['p', 'q'], $y));
--EXPECT--
array(3) {
  ["b"]=>
  int(3)
  ["a"]=>
  int(1)
  ["c"]=>
  string(1) "2"
}
array(2) {
  ["b"]=>
  int(3)
  ["a"]=>
  int(1)
}
array(3) {
  [3]=>
  string(1) "b"
  [1]=>
  string(1) "a"
  [2]=>
  string(1) "c"
}
array(2) {
  [0]=>
  array(1) {
    [0]=>
    int(3)
  }
  [1]=>
  array(1) {
    [0]=>
    int(1)
  }
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(1)
}
array(2) {
  ["p"]=>
  int(3)
  ["q"]=>
  int(1)
}
