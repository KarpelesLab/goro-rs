--TEST--
Reference assignment to a dimension binds an undefined variable
--FILE--
<?php
// `$a[k] = &$v` binds an undefined $v as null (no warning when it is read
// afterwards), unless the container fetch fails first.
function f() { $b['z'] = &$e; $e = $e; var_dump($b, $e); }
f();
function g() { $b[] = &$e; $e = $e; var_dump($b, $e); }
g();
function h() { $o = new stdClass; $o->p = &$e; $e = $e; var_dump($o, $e); }
h();
function l() { $b = 1; try { $b['z'] = &$e; } catch (Error $ex) { echo $ex->getMessage(), "\n"; } $e = $e; var_dump($e); }
l();
$gb["z"] = &$ge; $ge = $ge; var_dump($gb, $ge);
--EXPECTF--
array(1) {
  ["z"]=>
  &NULL
}
NULL
array(1) {
  [0]=>
  &NULL
}
NULL
object(stdClass)#1 (1) {
  ["p"]=>
  &NULL
}
NULL
Cannot use a scalar value as an array

Warning: Undefined variable $e in %s on line 10
NULL
array(1) {
  ["z"]=>
  &NULL
}
NULL
