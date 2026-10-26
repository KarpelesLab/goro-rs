--TEST--
class_alias() to a reserved name: the fatal error's trace has the call
--INI--
fatal_error_backtraces=1
--FILE--
<?php
class A {}
function g() { class_alias('A', 'array'); }
g();
--EXPECTF--
Fatal error: Cannot use "array" as a class alias as it is reserved in %s on line 3
Stack trace:
#0 %s(3): class_alias('A', 'array')
#1 %s(4): g()
#2 {main}
