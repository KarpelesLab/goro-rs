--TEST--
array_pad() without padding returns the input array (a write separates it)
--FILE--
<?php
function g(array $x) { foreach ($x as &$v) { $v = 'g'; } return $x; }
$d = g([1, 2, 3]);
$b = array_pad($d, 3, 'P');
foreach ($b as $k => $v) { $b[$k] = 'F'; }
var_dump($d, $b);
$d = g([1, 2]);
$b = array_pad($d, 3, 'P');
$b[0] = 'F';
var_dump($d, $b);
$e = [1, 2]; $r = &$e[0];
$b = array_pad($e, 2, 'P'); $b[0] = 'X';
var_dump($e, $b);
$b = array_pad($e, -3, 'P'); $b[1] = 'Y';
var_dump($e, $b);
--EXPECT--
array(3) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
  [2]=>
  string(1) "g"
}
array(3) {
  [0]=>
  string(1) "F"
  [1]=>
  string(1) "F"
  [2]=>
  string(1) "F"
}
array(2) {
  [0]=>
  &string(1) "F"
  [1]=>
  &string(1) "g"
}
array(3) {
  [0]=>
  &string(1) "F"
  [1]=>
  &string(1) "g"
  [2]=>
  string(1) "P"
}
array(2) {
  [0]=>
  &string(1) "X"
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  &string(1) "X"
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  &string(1) "Y"
  [1]=>
  int(2)
}
array(3) {
  [0]=>
  string(1) "P"
  [1]=>
  &string(1) "Y"
  [2]=>
  int(2)
}
