--TEST--
zpp checks the parameters after a callable one (array_filter() $mode)
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => array_filter([1, 2], null, 1.5));
t(fn() => array_filter([1, 2], null, "x"));
t(fn() => array_filter([1, 2], "trim", []));
t(fn() => array_filter([1, 2], "trim", null));
t(fn() => array_filter([1, 2], "intval", NAN));
t(fn() => array_filter([1, 2], mode: "x"));
$f = 'array_filter';
--EXPECTF--
Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 3
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
TypeError: array_filter(): Argument #3 ($mode) must be of type int, string given
TypeError: array_filter(): Argument #3 ($mode) must be of type int, array given

Deprecated: array_filter(): Passing null to parameter #3 ($mode) of type int is deprecated in %s on line 6
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
TypeError: array_filter(): Argument #3 ($mode) must be of type int, float given
TypeError: array_filter(): Argument #3 ($mode) must be of type int, string given
