--TEST--
get_declared_classes()/interfaces() list internal classes in registration order
--FILE--
<?php
var_dump(array_slice(get_declared_classes(), 0, 6));
var_dump(array_slice(get_declared_interfaces(), 0, 4));
class_alias('stdClass', '');
var_dump(in_array('', get_declared_classes(), true));
--EXPECT--
array(6) {
  [0]=>
  string(16) "InternalIterator"
  [1]=>
  string(9) "Exception"
  [2]=>
  string(14) "ErrorException"
  [3]=>
  string(5) "Error"
  [4]=>
  string(12) "CompileError"
  [5]=>
  string(10) "ParseError"
}
array(4) {
  [0]=>
  string(11) "Traversable"
  [1]=>
  string(17) "IteratorAggregate"
  [2]=>
  string(8) "Iterator"
  [3]=>
  string(12) "Serializable"
}
bool(false)
