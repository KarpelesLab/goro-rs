--TEST--
iterator_to_array() of an ArrayIterator keeps references of its storage (unless current() is overridden)
--FILE--
<?php
$a = [1, 2]; $r = &$a[1];
var_dump(iterator_to_array(new ArrayIterator($a)));
var_dump(iterator_to_array(new ArrayIterator($a), false));
var_dump(iterator_to_array(new ArrayObject($a)));
class AI extends ArrayIterator { function current(): mixed { return parent::current(); } }
var_dump(iterator_to_array(new AI($a)));
class AI2 extends ArrayIterator {}
var_dump(iterator_to_array(new AI2($a)));
function &g() { $x = 1; yield $x; $y = [5]; yield $y[0]; }
var_dump(iterator_to_array(g()));
$o = new ArrayIterator($a); foreach ($o as $v) { var_dump($v); }
var_dump(iterator_to_array(new IteratorIterator(new ArrayIterator($a))));
var_dump(iterator_to_array(new ArrayIterator(new ArrayObject($a))));
var_dump(iterator_to_array(new LimitIterator(new ArrayIterator($a), 0, 5)));
$obj = new stdClass; $obj->p = 1; $rr = &$obj->p;
var_dump(iterator_to_array(new ArrayIterator($obj)));
var_dump(array_values(iterator_to_array((function () use (&$a) { yield from $a; })())));
$s = new SplFixedArray(1); $s[0] = 1; var_dump(iterator_to_array($s));
--EXPECTF--
array(2) {
  [0]=>
  int(1)
  [1]=>
  &int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  &int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  &int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  &int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(5)
}
int(1)
int(2)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}

Deprecated: ArrayIterator::__construct(): Using an object as a backing array for ArrayIterator is deprecated, as it allows violating class constraints and invariants in %s on line 14
array(2) {
  [0]=>
  int(1)
  [1]=>
  &int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}

Deprecated: ArrayIterator::__construct(): Using an object as a backing array for ArrayIterator is deprecated, as it allows violating class constraints and invariants in %s on line 17
array(1) {
  ["p"]=>
  &int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  &int(2)
}
array(1) {
  [0]=>
  int(1)
}
