--TEST--
By-ref foreach separates an array shared with a property; elements stay references
--FILE--
<?php
// The array a by-reference foreach writes through is separated from the
// property it was copied from (FE_RESET_RW's SEPARATE_ARRAY).
$o = new stdClass; $c = []; $b = &$c; $b['z'] = &$e;
$o->p = [[1], 'abc']; $c['z'] = $o->p;
foreach ($e as &$v) { $v = 1; }
unset($v);
var_dump($o->p, $e);

function g() {
    $o = new stdClass; $o->p = [1, 2];
    $x = $o->p;
    foreach ($x as &$v) { $v *= 10; }
    unset($v);
    var_dump($o->p, $x);
}
g();

// Nested loops over the same array still share it.
$a = [1, 2];
foreach ($a as &$x) { foreach ($a as &$y) { $y++; } }
unset($x, $y);
var_dump($a);

// Elements stay references after the loop (refcount 1).
function f() { $x = [3, 1]; foreach ($x as &$v) {} unset($v); var_dump(array_pad($x, 3, 0)); }
f();
$x = [3, 1]; foreach ($x as &$v) {} unset($v); var_dump(array_pad($x, 3, 0));
--EXPECT--
array(2) {
  [0]=>
  array(1) {
    [0]=>
    int(1)
  }
  [1]=>
  string(3) "abc"
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(10)
  [1]=>
  int(20)
}
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(4)
}
array(3) {
  [0]=>
  &int(3)
  [1]=>
  &int(1)
  [2]=>
  int(0)
}
array(3) {
  [0]=>
  &int(3)
  [1]=>
  &int(1)
  [2]=>
  int(0)
}
