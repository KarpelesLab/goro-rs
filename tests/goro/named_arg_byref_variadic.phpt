--TEST--
Named arguments collected by a by-reference variadic parameter
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => sscanf(nonexistent_param: 1));
t(function() { sscanf(nonexistent_param: $x); var_dump($x); });
t(fn() => sscanf("a", "%s", foo: 1));
t(function() { var_dump(sscanf("a b", "%s %s", $a, foo: $b)); var_dump($a, $b); });
t(fn() => array_push(nonexistent: 1));
t(function() { $a = []; array_push($a, x: 1); var_dump($a); });
t(fn() => array_multisort(foo: 1));
function uref(&...$r) { var_dump(array_keys($r)); }
t(fn() => uref(a: 1));
t(function() { uref(a: $q); });
try { preg_match("/a/", "a", matches: 1); } catch (Error $e) { echo $e->getMessage(), "\n"; }
preg_match("/a/", "a", matches: $m); var_dump($m);
try { sort(array: [3,1]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
Error: sscanf(): Argument #3 could not be passed by reference
ArgumentCountError: sscanf() expects at least 2 arguments, 0 given
Error: sscanf(): Argument #3 could not be passed by reference
ArgumentCountError: sscanf() does not accept unknown named parameters
ArgumentCountError: array_push() expects at least 1 argument, 0 given
ArgumentCountError: array_push() does not accept unknown named parameters
ArgumentCountError: array_multisort() expects at least 1 argument, 0 given
Error: uref(): Argument #1 could not be passed by reference
array(1) {
  [0]=>
  string(1) "a"
}
preg_match(): Argument #3 ($matches) could not be passed by reference
array(1) {
  [0]=>
  string(1) "a"
}
sort(): Argument #1 ($array) could not be passed by reference
