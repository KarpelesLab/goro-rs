--TEST--
do_inherit_property() checks of an internal parent's properties (q08)
--FILE--
<?php
class MyEx extends Error { protected $file = "x"; }
--EXPECTF--

Fatal error: Type of MyEx::$file must be string (as in class Error) in %s on line 2
