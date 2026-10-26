--TEST--
Destructuring into $this with keyed list() is a compile error
--FILE--
<?php
class A { function f() { list("a" => $this) = []; } }
--EXPECTF--

Fatal error: Cannot re-assign $this in %s on line 2
