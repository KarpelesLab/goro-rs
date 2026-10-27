--TEST--
do_inherit_property() checks of an internal parent's properties (q01)
--FILE--
<?php
class MyEx extends Exception { private $message = "x"; }
--EXPECTF--

Fatal error: Access level to MyEx::$message must be protected (as in class Exception) or weaker in %s on line 2
