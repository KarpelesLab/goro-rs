--TEST--
foreach with by-reference list(): bindings do not keep element references
--FILE--
<?php
$data = [[1, 2], [3, 4]];
foreach ($data as [&$m, $n]) { $m *= 10; }
unset($m);
var_dump($data);
$d2 = [[1, 2], [3, 4]];
foreach ($d2 as &$row) { [&$x, $y] = $row; $x *= 10; }
unset($row, $x);
var_dump($d2);
$d3 = [[1, 2]];
[[&$z]] = $d3; unset($z); var_dump($d3);
--EXPECT--
array(2) {
  [0]=>
  array(2) {
    [0]=>
    int(10)
    [1]=>
    int(2)
  }
  [1]=>
  array(2) {
    [0]=>
    int(30)
    [1]=>
    int(4)
  }
}
array(2) {
  [0]=>
  array(2) {
    [0]=>
    int(10)
    [1]=>
    int(2)
  }
  [1]=>
  array(2) {
    [0]=>
    int(30)
    [1]=>
    int(4)
  }
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
}
