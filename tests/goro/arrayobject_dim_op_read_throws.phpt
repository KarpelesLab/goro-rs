--TEST--
ArrayObject: $ao[k] op= v goes on after the read throws; Generator storage; ARRAY_AS_PROPS write fetches
--FILE--
<?php
class AO extends ArrayObject {
    function offsetGet($k): mixed { throw new Exception("get $k"); }
}
$ao = new AO;
try { $ao['k'] .= 'x'; } catch (Exception $e) { echo $e->getMessage(), "\n"; }
var_dump($ao->getArrayCopy());
$ao2 = new ArrayObject;
try { $ao2[[]] .= 'x'; } catch (Throwable $e) { do { echo get_class($e), ": ", $e->getMessage(), "\n"; } while ($e = $e->getPrevious()); }
var_dump($ao2->getArrayCopy());
function g() { yield 1; yield 2; }
$ao3 = new ArrayObject(g());
var_dump($ao3);
$ao4 = new ArrayObject([], ArrayObject::ARRAY_AS_PROPS);
$ao4->x[] = 1;
$ao4->x[] = 2;
$r = &$ao4->y;
$r = 5;
var_dump($ao4);
var_dump($ao4->getArrayCopy());
--EXPECTF--
get k
array(1) {
  ["k"]=>
  string(1) "x"
}
TypeError: Cannot access offset of type array on ArrayObject
TypeError: Cannot access offset of type array on ArrayObject
array(0) {
}

Deprecated: ArrayObject::__construct(): Using an object as a backing array for ArrayObject is deprecated, as it allows violating class constraints and invariants in %s on line 12
object(ArrayObject)#4 (1) {
  ["storage":"ArrayObject":private]=>
  object(Generator)#5 (1) {
    ["function"]=>
    string(1) "g"
  }
}
object(ArrayObject)#2 (1) {
  ["storage":"ArrayObject":private]=>
  array(2) {
    ["x"]=>
    array(2) {
      [0]=>
      int(1)
      [1]=>
      int(2)
    }
    ["y"]=>
    &int(5)
  }
}
array(2) {
  ["x"]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
  ["y"]=>
  &int(5)
}
