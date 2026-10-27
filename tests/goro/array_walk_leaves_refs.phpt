--TEST--
array_walk() leaves its elements references: array_pad() copies share them
--FILE--
<?php
$b = [1, 2]; array_walk($b, function (&$x) { $x = 'W'; }); $a = array_pad($b, 3, 'P'); var_dump($a, $b);
$b = [1, 2]; array_walk($b, function (&$x) { $x = 'W'; }); $a = $b; var_dump($a);
$b = [1, 2]; array_walk($b, function ($x) { }); $a = array_pad($b, 3, 'P'); var_dump($a);
$b = [1, 2]; foreach ($b as &$v) {} unset($v); $a = array_pad($b, 3, 'P'); var_dump($a);
$b = [1, 2]; array_walk($b, function (&$x) { $x = 'W'; }); $a = array_pad($b, 3, 'P'); $a[0] = 'changed'; var_dump($b[0]);
$b = ['k' => 1]; array_walk($b, function (&$x) { $x = 'W'; }); $a = array_pad($b, 3, 'P'); $a['k'] = 'changed'; var_dump($b['k']);
$b = [1, 2]; array_walk($b, function (&$x) { }); $a = array_splice($b, 0, 1); $a[0] = 'S'; var_dump($b);
$b = [[1]]; array_walk($b, function (&$x) { }); $a = array_column($b, 0); var_dump($a);
$b = [1]; array_walk($b, function (&$x) { }); $a = array_fill_keys(['x'], $b); var_dump($a);
--EXPECT--
array(3) {
  [0]=>
  &string(1) "W"
  [1]=>
  &string(1) "W"
  [2]=>
  string(1) "P"
}
array(2) {
  [0]=>
  &string(1) "W"
  [1]=>
  &string(1) "W"
}
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
array(3) {
  [0]=>
  &int(1)
  [1]=>
  &int(2)
  [2]=>
  string(1) "P"
}
array(3) {
  [0]=>
  &int(1)
  [1]=>
  &int(2)
  [2]=>
  string(1) "P"
}
string(7) "changed"
string(7) "changed"
array(1) {
  [0]=>
  int(2)
}
array(1) {
  [0]=>
  int(1)
}
array(1) {
  ["x"]=>
  array(1) {
    [0]=>
    int(1)
  }
}
