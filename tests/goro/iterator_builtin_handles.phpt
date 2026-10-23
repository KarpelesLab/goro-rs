--TEST--
iterator_to_array() and iterator_count() take an iterator object handle
--FILE--
<?php
// iterator_to_array() / iterator_count() take an iterator handle (released after the iterator).
class AA implements IteratorAggregate { public $c = [1]; public function getIterator(): Iterator { return new ArrayIterator($this->c); } }
$a = new AA;
var_dump(iterator_to_array($a));
var_dump(new stdClass);
$b = new ArrayObject([1]);
var_dump(iterator_to_array($b));
var_dump(new stdClass);
var_dump(iterator_count($a), new stdClass);
foreach ($a as $v) {} var_dump(new stdClass);
--EXPECT--
array(1) {
  [0]=>
  int(1)
}
object(stdClass)#3 (0) {
}
array(1) {
  [0]=>
  int(1)
}
object(stdClass)#4 (0) {
}
int(1)
object(stdClass)#2 (0) {
}
object(stdClass)#4 (0) {
}
