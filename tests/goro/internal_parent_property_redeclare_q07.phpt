--TEST--
do_inherit_property() checks of an internal parent's properties (q07)
--FILE--
<?php
class MyEx extends Exception { protected readonly int $line; }
--EXPECTF--

Fatal error: Cannot redeclare non-readonly property Exception::$line as readonly MyEx::$line in %s on line 2
