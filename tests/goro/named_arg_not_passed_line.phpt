--TEST--
'Argument #N not passed' is raised by the callee's RECV: its line and file
--FILE--
<?php
function f($a, $b) {}

try { f(b: 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), " ", $e->getFile() === __FILE__ ? "same" : "other", "\n"; }
class C { function m($a, $b) {} }
try { (new C)->m(b: 1); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
try { call_user_func_array('f', ['b' => 1]); } catch (Error $e) { echo $e->getMessage(), " @", $e->getLine(), "\n"; }
f(b: 2);
--EXPECTF--
f(): Argument #1 ($a) not passed @2 same
C::m(): Argument #1 ($a) not passed @5
f(): Argument #1 ($a) not passed @2

Fatal error: Uncaught ArgumentCountError: f(): Argument #1 ($a) not passed in %s:2
Stack trace:
#0 %s(8): f(NULL, 2)
#1 {main}
  thrown in %s on line 2
