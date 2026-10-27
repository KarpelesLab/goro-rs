--TEST--
do_inherit_property() checks of an internal parent's properties (p01)
--FILE--
<?php
class MyEx extends Exception { protected $message = "default msg"; protected $code = 42; protected $line = 999; protected $file = "fake.php"; }
throw new MyEx;
--EXPECTF--

Fatal error: Type of MyEx::$file must be string (as in class Exception) in %s on line 2
