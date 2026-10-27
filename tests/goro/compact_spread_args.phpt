--TEST--
compact(...$names)
--FILE--
<?php
function h() { $arr = ["x", ["y"]]; $x = 1; $y = 2; var_dump(compact(...$arr)); }
h();
function k() { $a = 1; var_dump(compact(...["a"], ...[["a"]])); try { compact(...[]); } catch (ArgumentCountError $e) { echo $e->getMessage(), "\n"; } try { compact(...["n" => "a"]); } catch (Error $e) { echo $e->getMessage(), "\n"; } try { compact("a", ...["n" => "a"]); } catch (Error $e) { echo $e->getMessage(), "\n"; } }
k();
--EXPECT--
array(2) {
  ["x"]=>
  int(1)
  ["y"]=>
  int(2)
}
array(1) {
  ["a"]=>
  int(1)
}
compact() expects at least 1 argument, 0 given
compact() expects at least 1 argument, 0 given
compact() does not accept unknown named parameters
