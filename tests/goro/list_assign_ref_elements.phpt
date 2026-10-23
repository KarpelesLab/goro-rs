--TEST--
Destructuring an array that holds references copies the referenced values
--FILE--
<?php
function f() {
    $a = ['x' => 1];
    [$q] = [&$a['x']];
    var_dump($a, $q);
    $q = 7; var_dump($a);
    $b = [];
    [$r] = [&$b['n']];
    var_dump($b, $r);
    $c = ['k' => [1]];
    [$s] = [&$c['k'][0]];
    var_dump($c);
    $d = ['x' => 1];
    [$t, $u] = [&$d['x'], &$d['y']];
    var_dump($d, $t, $u);
    $e = ['x' => 1];
    ['a' => $v] = ['a' => &$e['x']];
    $v = 3; var_dump($e);
    $arr = [1, 2];
    foreach ([&$arr[0]] as $w) { $w = 9; }
    var_dump($arr);
}
f();
$g = ['x' => 1];
[$q] = [&$g['x']];
$q = 5;
var_dump($g);
[$q2] = $tmp = [&$g['x']];
var_dump($g);
--EXPECT--
array(1) {
  ["x"]=>
  int(1)
}
int(1)
array(1) {
  ["x"]=>
  int(1)
}
array(1) {
  ["n"]=>
  NULL
}
NULL
array(1) {
  ["k"]=>
  array(1) {
    [0]=>
    int(1)
  }
}
array(2) {
  ["x"]=>
  int(1)
  ["y"]=>
  NULL
}
int(1)
NULL
array(1) {
  ["x"]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(1) {
  ["x"]=>
  int(1)
}
array(1) {
  ["x"]=>
  &int(1)
}
