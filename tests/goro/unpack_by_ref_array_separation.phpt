--TEST--
An array unpacked into an internal function's by-reference parameter is separated from its other holders
--FILE--
<?php
$vals = [[1], ["a" => 1, "b" => 2], [3, 1, 2]];
$args = []; $args[] = $vals[1]; $args[] = 1; $args[] = null; $args[] = [5];
var_dump(array_splice(...$args)); var_dump($vals[1], $args[0]);
$a = []; $a[] = $vals[2]; sort(...$a); var_dump($vals[2], $a[0]);
$m = [$vals[2], SORT_DESC, $vals[2]]; var_dump(array_multisort(...$m)); var_dump($vals[2], $m);
$p = [$vals[0], 9]; array_push(...$p); var_dump($vals[0], $p[0]);
$w = [$vals[2], function (&$v) { $v *= 10; }]; array_walk(...$w); var_dump($vals[2], $w[0]);
--EXPECTF--
array(1) {
  ["b"]=>
  int(2)
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
array(2) {
  ["a"]=>
  int(1)
  [0]=>
  int(5)
}
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
bool(true)
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(3) {
  [0]=>
  array(3) {
    [0]=>
    int(3)
    [1]=>
    int(2)
    [2]=>
    int(1)
  }
  [1]=>
  int(3)
  [2]=>
  array(3) {
    [0]=>
    int(3)
    [1]=>
    int(2)
    [2]=>
    int(1)
  }
}
array(1) {
  [0]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(9)
}
array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(3) {
  [0]=>
  int(30)
  [1]=>
  int(10)
  [2]=>
  int(20)
}
