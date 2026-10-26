--TEST--
extract(): zpp of $array, $flags and $prefix (types, null deprecations, count, named arguments)
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
function f() {
    $a = ['x' => 1, 'y' => 2];
    t(function() use ($a) { var_dump(extract($a, EXTR_PREFIX_ALL, null), get_defined_vars()); });
    t(function() use ($a) { var_dump(extract($a, EXTR_PREFIX_ALL, "")); });
    t(function() use ($a) { var_dump(extract($a, EXTR_PREFIX_ALL)); });
    t(function() use ($a) { var_dump(extract($a, "1"), $x); });
    t(function() use ($a) { var_dump(extract($a, 2.0, "p"), $p_x); });
    t(function() use ($a) { var_dump(extract($a, null)); });
    t(function() use ($a) { var_dump(extract($a, 99)); });
    t(function() use ($a) { var_dump(extract($a, [])); });
    t(function() use ($a) { var_dump(extract($a, 1, new stdClass)); });
    t(function() { $s = "abc"; var_dump(extract($s)); });
    t(function() { $s = null; var_dump(extract($s)); });
    t(function() use ($a) { var_dump(extract($a, 0, 1, 2)); });
    t(function() { var_dump(extract([])); });
    t(function() { var_dump(extract(['a' => 1], prefix: 'z', flags: EXTR_PREFIX_ALL), $z_a); });
    t(function() { var_dump(extract(['a' => 1], prefix: 'z'), $a); });
    t(function() { var_dump(extract(['a' => 1], foo: 'z')); });
}
f();
--EXPECTF--

Deprecated: extract(): Passing null to parameter #3 ($prefix) of type string is deprecated in %s on line 5
int(2)
array(3) {
  ["a"]=>
  array(2) {
    ["x"]=>
    int(1)
    ["y"]=>
    int(2)
  }
  ["_x"]=>
  int(1)
  ["_y"]=>
  int(2)
}
int(2)
ValueError: extract(): Argument #3 ($prefix) is required when using this extract type
int(2)
int(1)

Warning: Undefined variable $p_x in %s on line 9
int(2)
NULL

Deprecated: extract(): Passing null to parameter #2 ($flags) of type int is deprecated in %s on line 10
int(2)
ValueError: extract(): Argument #2 ($flags) must be a valid extract type
TypeError: extract(): Argument #2 ($flags) must be of type int, array given
TypeError: extract(): Argument #3 ($prefix) must be of type string, stdClass given
TypeError: extract(): Argument #1 ($array) must be of type array, string given
TypeError: extract(): Argument #1 ($array) must be of type array, null given
ArgumentCountError: extract() expects at most 3 arguments, 4 given
int(0)
int(1)
int(1)
int(1)
int(1)
Error: Unknown named parameter $foo
