--TEST--
A dynamic call of call_user_func()/call_user_func_array() is a real call with its own frame
--FILE--
<?php
function bt() { debug_print_backtrace(); return array_map(fn($f) => $f['function'], debug_backtrace()); }
$g = 'call_user_func';
var_dump($g('bt'));
var_dump(call_user_func('bt'));
$h = 'call_user_func_array';
var_dump($h('bt', []));
function thrower() { throw new Exception("x"); }
try { $g('thrower'); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
try { call_user_func('thrower'); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
#0 [internal function]: bt()
#1 %s(4): call_user_func('bt')
array(2) {
  [0]=>
  string(2) "bt"
  [1]=>
  string(14) "call_user_func"
}
#0 %s(5): bt()
array(1) {
  [0]=>
  string(2) "bt"
}
#0 [internal function]: bt()
#1 %s(7): call_user_func_array('bt', Array)
array(2) {
  [0]=>
  string(2) "bt"
  [1]=>
  string(20) "call_user_func_array"
}
#0 [internal function]: thrower()
#1 %s(9): call_user_func('thrower')
#2 {main}
#0 %s(10): thrower()
#1 {main}
