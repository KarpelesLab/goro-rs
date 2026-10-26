--TEST--
Assigning $this inside a ternary is a compile error
--FILE--
<?php
class A { function f() { $z = 1 ? $this = 1 : 2; } }
--EXPECTF--

Fatal error: Cannot re-assign $this in %s on line 2
