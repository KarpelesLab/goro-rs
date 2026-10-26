--TEST--
Array spread of a generator: numeric-string keys only when canonical; the generator runs from the spread line
--FILE--
<?php
var_dump([...(function() { yield "01" => 1; yield "-0" => 2; yield "+5" => 3; yield " 7" => 4; yield "8" => 5; yield "-9" => 6; yield "9223372036854775808" => 7; })()]);
function gen() { yield 1; throw new Exception("x"); }
try { $a = [...gen()]; } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
function f(...$a) {}
try { f(...gen()); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
array(7) {
  ["01"]=>
  int(1)
  ["-0"]=>
  int(2)
  ["+5"]=>
  int(3)
  [" 7"]=>
  int(4)
  [0]=>
  int(5)
  [1]=>
  int(6)
  ["9223372036854775808"]=>
  int(7)
}
#0 %s(4): gen()
#1 {main}
#0 %s(6): gen()
#1 {main}
