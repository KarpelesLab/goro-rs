--TEST--
clone() with properties: a set hook TypeError has no called-in suffix (internal caller)
--FILE--
<?php
class A { public int $x { set(int $v) { $this->x = $v; } } public function __construct() { $c = clone($this, ['x' => 'str']); } }
new A;
--EXPECTF--

Fatal error: Uncaught TypeError: A::$x::set(): Argument #1 ($v) must be of type int, string given in %s:2
Stack trace:
#0 [internal function]: A->$x::set('str')
#1 %s(2): clone(Object(A), Array)
#2 %s(3): A->__construct()
#3 {main}
  thrown in %s on line 2
