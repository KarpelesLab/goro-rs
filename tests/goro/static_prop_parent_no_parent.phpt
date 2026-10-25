--TEST--
parent::$x in a method of a class without a parent
--FILE--
<?php
class A { function f() { return parent::$x; } }
echo "ok\n";
--EXPECTF--

Fatal error: Cannot use "parent" when current class scope has no parent in %s on line 2
