--TEST--
do_inherit_property() checks of an internal parent's properties (q03)
--FILE--
<?php
class MyEx extends Exception { protected $line = 5; }
--EXPECTF--

Fatal error: Type of MyEx::$line must be int (as in class Exception) in %s on line 2
