--TEST--
A hook run by an internal function is called from [internal function]
--FILE--
<?php
class A { public $c { get { throw new Exception("x"); } } } var_export(new A);
echo "ok\n";
--EXPECTF--

Fatal error: Uncaught Exception: x in %s:2
Stack trace:
#0 [internal function]: A->$c::get()
#1 %s(2): var_export(Object(A))
#2 {main}
  thrown in %s on line 2
