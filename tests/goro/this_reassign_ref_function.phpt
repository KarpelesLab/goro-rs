--TEST--
Assigning $this by reference in a function is a compile error
--FILE--
<?php
function f() { $this = &$x; }
--EXPECTF--

Fatal error: Cannot re-assign $this in %s on line 2
