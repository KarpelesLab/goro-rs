--TEST--
iterator_to_array()/iterator_count()/yield from of a WeakMap use its getIterator()
--FILE--
<?php
$m = new WeakMap;
$a = new stdClass; $b = new stdClass;
$m[$a] = 'x'; $m[$b] = 'y';
var_dump(iterator_to_array($m, false));
var_dump(iterator_count($m));
function g($m) { yield from $m; }
foreach (g($m) as $k => $v) var_dump($k === $a || $k === $b, $v);
var_dump(iterator_to_array(new IteratorIterator($m), false));

?>
--EXPECT--
array(2) {
  [0]=>
  string(1) "x"
  [1]=>
  string(1) "y"
}
int(2)
bool(true)
string(1) "x"
bool(true)
string(1) "y"
array(2) {
  [0]=>
  string(1) "x"
  [1]=>
  string(1) "y"
}
