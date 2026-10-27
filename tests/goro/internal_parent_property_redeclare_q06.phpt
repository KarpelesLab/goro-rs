--TEST--
do_inherit_property() checks of an internal parent's properties (q06)
--FILE--
<?php
class MyEx extends Exception { public static $code = 1; }
--EXPECTF--

Fatal error: Cannot redeclare non static Exception::$code as static MyEx::$code in %s on line 2
