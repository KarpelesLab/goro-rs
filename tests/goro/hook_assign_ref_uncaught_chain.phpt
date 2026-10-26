--TEST--
An uncaught by-reference assignment to a hooked property shows the chained error
--FILE--
<?php
class A { public $x { get => 1; set {} } } $a = new A; $v = 1; $a->x = &$v;
--EXPECTF--

Fatal error: Uncaught Error: Indirect modification of A::$x is not allowed in %s:2
Stack trace:
#0 {main}

Next Error: Cannot assign by reference to overloaded object in %s:2
Stack trace:
#0 {main}
  thrown in %s on line 2
