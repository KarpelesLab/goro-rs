--TEST--
ArrayObject ARRAY_AS_PROPS: nested writes, compound ops and references reach the storage
--FILE--
<?php
$ao = new ArrayObject(['a' => [1]], ArrayObject::ARRAY_AS_PROPS);
$ao->a[] = 2;
$ao->b['k'] = 'v';
$ao->c .= 'x';
$ao->d++;
$ao->e += 3;
$ao->a[0] += 10;
var_dump($ao->getArrayCopy());
$r = &$ao->a;
$r[] = 99;
var_dump($ao->a);
function addone(&$x) { $x[] = 1; }
addone($ao->f);
var_dump($ao->f);
foreach ($ao->a as &$v) { $v *= 2; }
unset($v);
var_dump($ao->a);
class P extends ArrayObject { public $real = []; }
$p = new P([], ArrayObject::ARRAY_AS_PROPS);
$p->real[] = 1;
$p->virt[] = 2;
var_dump($p);
$it = new ArrayIterator([], ArrayObject::ARRAY_AS_PROPS);
$it->q[] = 5;
var_dump($it->getArrayCopy());
$o = new ArrayObject(new stdClass, ArrayObject::ARRAY_AS_PROPS);
$o->z[] = 1;
var_dump($o);
--EXPECTF--

Warning: Undefined array key "c" in %s on line 5

Warning: Undefined array key "d" in %s on line 6

Warning: Undefined array key "e" in %s on line 7
array(5) {
  ["a"]=>
  array(2) {
    [0]=>
    int(11)
    [1]=>
    int(2)
  }
  ["b"]=>
  array(1) {
    ["k"]=>
    string(1) "v"
  }
  ["c"]=>
  string(1) "x"
  ["d"]=>
  int(1)
  ["e"]=>
  int(3)
}
array(3) {
  [0]=>
  int(11)
  [1]=>
  int(2)
  [2]=>
  int(99)
}
array(1) {
  [0]=>
  int(1)
}
array(3) {
  [0]=>
  int(22)
  [1]=>
  int(4)
  [2]=>
  int(198)
}
object(P)#2 (2) {
  ["real"]=>
  array(1) {
    [0]=>
    int(1)
  }
  ["storage":"ArrayObject":private]=>
  array(1) {
    ["virt"]=>
    array(1) {
      [0]=>
      int(2)
    }
  }
}
array(1) {
  ["q"]=>
  array(1) {
    [0]=>
    int(5)
  }
}

Deprecated: ArrayObject::__construct(): Using an object as a backing array for ArrayObject is deprecated, as it allows violating class constraints and invariants in %s on line 27
object(ArrayObject)#4 (1) {
  ["storage":"ArrayObject":private]=>
  object(stdClass)#5 (1) {
    ["z"]=>
    array(1) {
      [0]=>
      int(1)
    }
  }
}
