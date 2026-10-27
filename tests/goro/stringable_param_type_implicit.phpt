--TEST--
Stringable parameter / union types accept classes that only declare __toString()
--FILE--
<?php
namespace { class T { function __toString(): string { return "t"; } }
function h(\Stringable|int $x) { return $x; } var_dump(h(new T));
var_dump(new T instanceof Stringable, class_implements(new T)); }
namespace A {
class T2 { function __toString(): string { return "t"; } }
var_dump(new T2 instanceof \Stringable, class_implements(new T2));
function h(\Stringable|int $x) { return $x; } var_dump(h(new T2)); function k(string|\Stringable $x) { return $x; } var_dump(k(new T2)); }

?>
--EXPECT--
object(T)#1 (0) {
}
bool(true)
array(1) {
  ["Stringable"]=>
  string(10) "Stringable"
}
bool(true)
array(1) {
  ["Stringable"]=>
  string(10) "Stringable"
}
object(A\T2)#1 (0) {
}
object(A\T2)#1 (0) {
}
