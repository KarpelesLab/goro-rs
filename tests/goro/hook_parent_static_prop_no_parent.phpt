--TEST--
parent::$x in a hook of a class without a parent
--FILE--
<?php
class A { public $x { get { return parent::$x::foo(); } } }
echo "ok\n";
--EXPECTF--

Fatal error: Cannot use "parent" when current class scope has no parent in %s on line 2
