--TEST--
Releasing deeply nested arrays (shared storage) does not overflow the stack
--FILE--
<?php
$a=[];for($i=0;$i<1e5;$i++)$a=[$a]; unset($a); echo "ok1\n";
$a=[];for($i=0;$i<1e5;$i++)$a=[$a]; function f($x){throw new Exception;} try{f($a);}catch(Exception $e){} unset($a); unset($e); echo "ok2\n";
$a=[];for($i=0;$i<1e5;$i++)$a=["k"=>$a, 1];$b=$a; unset($a); unset($b); echo "ok1\n";
$a=[];for($i=0;$i<1e5;$i++)$a=[$a]; $c = $a; $c[] = 1; unset($a, $c); echo "ok2\n";
function g() { $a=[];for($i=0;$i<1e5;$i++)$a=[$a]; return 1; } g(); echo "ok3\n";
$a=[];for($i=0;$i<1e5;$i++)$a=[$a]; echo strlen(serialize($a)) > 0 ? "s\n" : "";
--EXPECTF--
ok1
ok2
ok1
ok2
ok3

Fatal error: Uncaught Error: Maximum call stack size of 8339456 bytes (zend.max_allowed_stack_size - zend.reserved_stack_size) reached. Infinite recursion? in %s:7
Stack trace:
#0 %s(7): serialize(Array)
#1 {main}
  thrown in %s on line 7
