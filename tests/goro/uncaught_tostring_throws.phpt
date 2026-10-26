--TEST--
An uncaught exception whose __toString() throws: that exception is reported
--FILE--
<?php
class MyEx extends Exception { function __toString(): string { throw new Exception("tostring fails"); } }
throw new MyEx("m");
--EXPECTF--

Fatal error: Uncaught Exception: tostring fails in %s:2
Stack trace:
#0 [internal function]: MyEx->__toString()
#1 {main}
  thrown in %s on line 2
