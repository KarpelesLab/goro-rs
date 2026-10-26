--TEST--
Assigning $this in an arrow function is a compile error
--FILE--
<?php
class A { function f() { $f = fn() => $this = 1; } }
--EXPECTF--

Fatal error: Cannot re-assign $this in %s on line 2
