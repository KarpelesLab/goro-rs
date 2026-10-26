--TEST--
Generator::__debugInfo() and get_mangled_object_vars() of a Generator
--FILE--
<?php
class D { function baz() { yield 1; } static function s() { yield 2; } }
function f() { yield 3; }
$g = (new D)->baz();
var_dump($g->__debugInfo(), (array) $g, get_mangled_object_vars($g));
var_dump(method_exists($g, '__debugInfo'), D::s()->__debugInfo(), f()->__debugInfo());
foreach ($g as $_) {}
var_dump($g->__debugInfo());

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
