--TEST--
SensitiveParameterValue objects of a frame with extra named arguments are created once
--FILE--
<?php
function vari($a, #[SensitiveParameter] ...$r) { throw new Exception("v"); }
try { vari(1, 2, k: 4); } catch (Exception $e) { var_dump($e->getTrace()[0]['args']); }
unset($e);
function v0($a, ...$r) { return debug_backtrace(); }
var_dump(v0(1, 2, k: new stdClass));
?>
--EXPECTF--
array(3) {
  [0]=>
  int(1)
  [1]=>
  object(SensitiveParameterValue)#2 (0) {
  }
  ["k"]=>
  object(SensitiveParameterValue)#3 (0) {
  }
}
array(1) {
  [0]=>
  array(4) {
    ["file"]=>
    string(%d) "%s"
    ["line"]=>
    int(6)
    ["function"]=>
    string(2) "v0"
    ["args"]=>
    array(3) {
      [0]=>
      int(1)
      [1]=>
      int(2)
      ["k"]=>
      object(stdClass)#1 (0) {
      }
    }
  }
}
