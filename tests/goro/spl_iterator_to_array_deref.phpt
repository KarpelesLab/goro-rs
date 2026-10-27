--TEST--
iterator_to_array() and SPL storage readers store element references by value
--FILE--
<?php
function g(array $x) { foreach ($x as &$v) { $v = 'g'; } return $x; }
$c = g([1, 2]);
$d = iterator_to_array(new ArrayIterator($c));
$d2 = iterator_to_array(new ArrayIterator($c), false);
$d3 = iterator_to_array(new IteratorIterator(new ArrayObject($c)));
$ai = new ArrayIterator($c); $cur = $ai->current(); $ai->next(); $cur2 = $ai->current(); $off = $ai[1];
$fa = SplFixedArray::fromArray($c);
$ac = (new ArrayObject($c))->getArrayCopy();
$s = &$c[1];
$s = 'changed';
var_dump($d, $d2, $d3, $cur2, $off, $fa->toArray(), $ac);
?>
--EXPECT--
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
string(1) "g"
string(1) "g"
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
