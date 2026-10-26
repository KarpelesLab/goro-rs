--TEST--
ReflectionParameter types of an enum's implicit from()/tryFrom() and of internal iterable parameters (Traversable|array)
--FILE--
<?php
enum E: string { case A = "a"; }
foreach ([["BackedEnum","from"],["E","from"],["E","tryFrom"],["RoundingMode","cases"],["Random\\IntervalBoundary","cases"]] as [$c,$m]) { foreach ((new ReflectionMethod($c, $m))->getParameters() as $p) { var_dump((string)$p->getType(), $p->getType()?->allowsNull()); } var_dump((string)(new ReflectionMethod($c, $m))->getReturnType()); }
var_dump(iterator_to_array(new ArrayIterator([1,2])), iterator_count([1,2,3]), iterator_to_array([3=>4]));
try { iterator_to_array(1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { iterator_count("x"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
echo new ReflectionFunction("iterator_count");
?>
--EXPECT--
string(10) "string|int"
bool(false)
string(6) "static"
string(10) "string|int"
bool(false)
string(6) "static"
string(10) "string|int"
bool(false)
string(7) "?static"
string(5) "array"
string(5) "array"
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
int(3)
array(1) {
  [3]=>
  int(4)
}
iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, int given
iterator_count(): Argument #1 ($iterator) must be of type Traversable|array, string given
Function [ <internal:SPL> function iterator_count ] {

  - Parameters [1] {
    Parameter #0 [ <required> Traversable|array $iterator ]
  }
  - Return [ int ]
}
