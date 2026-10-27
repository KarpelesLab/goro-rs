--TEST--
A positional argument unpacked after an earlier unpack's named ones takes the slot after them
--INI--
zend.exception_ignore_args=1
--FILE--
<?php
function f(...$a) { var_dump($a); }
try { f(...['a'=>1], ...[2]); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function g($x, $y) { var_dump($x, $y); }
try { g(...['y'=>1], ...[2]); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
array(2) {
  [0]=>
  int(2)
  ["a"]=>
  int(1)
}
ArgumentCountError: g(): Argument #1 ($x) not passed
