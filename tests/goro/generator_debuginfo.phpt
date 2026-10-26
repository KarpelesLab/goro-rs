--TEST--
Generator::__debugInfo(); class/object introspection functions given a Generator
--FILE--
<?php
class D { function baz() { yield 1; } static function s() { yield 2; } }
function f() { yield 3; }
$g = (new D)->baz();
var_dump($g->__debugInfo(), (array) $g, get_mangled_object_vars($g));
var_dump(method_exists($g, '__debugInfo'), D::s()->__debugInfo(), f()->__debugInfo());
foreach ($g as $_) {}
var_dump($g->__debugInfo());
var_dump(method_exists($g, 'send'), property_exists($g, 'x'), count(get_class_methods($g)) > 5, get_parent_class($g), is_a($g, 'Iterator'), is_subclass_of($g, 'Traversable'));
var_dump(class_implements($g), get_object_vars($g), spl_object_id($g) > 0, is_callable([$g, 'current']));
var_dump(get_debug_type($g), gettype($g), $g instanceof Generator);

?>
--EXPECT--
array(1) {
  ["function"]=>
  string(6) "D::baz"
}
array(0) {
}
array(0) {
}
bool(true)
array(1) {
  ["function"]=>
  string(4) "D::s"
}
array(1) {
  ["function"]=>
  string(1) "f"
}
array(1) {
  ["function"]=>
  string(6) "D::baz"
}
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(true)
array(2) {
  ["Iterator"]=>
  string(8) "Iterator"
  ["Traversable"]=>
  string(11) "Traversable"
}
array(0) {
}
bool(true)
bool(true)
string(9) "Generator"
string(6) "object"
bool(true)
