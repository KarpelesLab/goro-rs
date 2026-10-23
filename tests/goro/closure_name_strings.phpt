--TEST--
Strings that look like internal closure names are strings
--FILE--
<?php
// Strings and arrays that look like goro's internal closure names are plain values.
$s = "__closure_x";
var_dump($s instanceof Closure, "__closure_1" instanceof Closure, ["__closure_1"] instanceof Closure);
var_dump(gettype($s), is_string($s), is_object($s), is_scalar($s), is_callable($s), is_object(["__arrow_1", 2]));
var_dump($s, ["__closure_x", 1]);
var_export($s); echo "\n";
function f(Closure $c) { return $c(); }
try { f($s); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump(f(fn() => 1), (fn() => 1) instanceof Closure);
--EXPECTF--
bool(false)
bool(false)
bool(false)
string(6) "string"
bool(true)
bool(false)
bool(true)
bool(false)
bool(false)
string(11) "__closure_x"
array(2) {
  [0]=>
  string(11) "__closure_x"
  [1]=>
  int(1)
}
'__closure_x'
f(): Argument #1 ($c) must be of type Closure, string given, called in %s on line 9
int(1)
bool(true)
