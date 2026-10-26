--TEST--
parent::$x::get() private error is thrown in the hook frame
--FILE--
<?php
class P { private $x { get => 1; } } class C extends P { public $x { get => parent::$x::get(); } } var_dump((new C)->x);
--EXPECTF--

Fatal error: Uncaught Error: Cannot access private property P::$x in %s:2
Stack trace:
#0 %s(2): C->$x::get()
#1 {main}
  thrown in %s on line 2
