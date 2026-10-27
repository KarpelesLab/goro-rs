--TEST--
do_inherit_property() checks of an internal parent's properties (q05)
--FILE--
<?php
class MyEx extends Exception { protected string $message = "m"; }
--EXPECTF--

Fatal error: Type of MyEx::$message must be omitted to match the parent definition in class Exception in %s on line 2
