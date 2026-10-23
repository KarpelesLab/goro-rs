--TEST--
Object handles freed by unpacking an IteratorAggregate
--FILE--
<?php
class A implements IteratorAggregate { function getIterator(): Iterator { $i = new ArrayIterator([1,2]); echo "it ", spl_object_id($i), "\n"; return $i; } }
$x = [...new A];
echo "s1 ", spl_object_id(new stdClass), "\n";
$o = new stdClass; echo "o ", spl_object_id($o), "\n";
$f = function() { yield 1; yield 2; };
echo "f ", spl_object_id($f), "\n";
$g = $f(); echo "g ", spl_object_id($g), "\n"; unset($g);
var_dump(iterator_to_array($f()));
echo "s2 ", spl_object_id(new stdClass), "\n";
$h = new stdClass; echo "h ", spl_object_id($h), "\n";
function gen() { yield 1; }
var_dump(iterator_to_array(gen()));
echo "s3 ", spl_object_id(new stdClass), "\n";
--EXPECT--
it 2
s1 1
o 1
f 3
g 2
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
s2 2
h 2
array(1) {
  [0]=>
  int(1)
}
s3 4
