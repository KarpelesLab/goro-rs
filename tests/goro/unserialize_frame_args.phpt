--TEST--
unserialize()'s frame in a backtrace has both of its arguments
--FILE--
<?php
class W { function __wakeup() { var_dump((new Exception)->getTrace()[1]['args']); } }
unserialize('O:1:"W":0:{}', []);
unserialize('O:1:"W":0:{}');
unserialize('O:1:"W":0:{}', ['allowed_classes' => true]);
class U { function __unserialize($d) { echo (new Exception)->getTraceAsString(), "\n"; } }
unserialize('O:1:"U":0:{}', ['max_depth' => 5]);
--EXPECTF--
array(2) {
  [0]=>
  string(12) "O:1:"W":0:{}"
  [1]=>
  array(0) {
  }
}
array(1) {
  [0]=>
  string(12) "O:1:"W":0:{}"
}
array(2) {
  [0]=>
  string(12) "O:1:"W":0:{}"
  [1]=>
  array(1) {
    ["allowed_classes"]=>
    bool(true)
  }
}
#0 [internal function]: U->__unserialize(Array)
#1 %s(7): unserialize('O:1:"U":0:{}', Array)
#2 {main}
