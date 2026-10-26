--TEST--
A parameter type naming an alias of an interface the class implements
--FILE--
<?php
interface RealI {}
class_alias("RealI", "AliasI");
class X implements AliasI {}
var_dump(class_implements("X"), (new X) instanceof RealI, (new ReflectionClass("X"))->getInterfaceNames());
function g(AliasI $i) { return get_class($i); }
var_dump(g(new X));

?>
--EXPECT--
array(1) {
  ["RealI"]=>
  string(5) "RealI"
}
bool(true)
array(1) {
  [0]=>
  string(5) "RealI"
}
string(1) "X"
