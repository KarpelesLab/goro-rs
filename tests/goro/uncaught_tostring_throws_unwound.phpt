--TEST--
An uncaught exception's __toString() runs with the stack unwound
--FILE--
<?php
class MyEx extends Exception { function __toString(): string { throw new LogicException("tostring fails", 0, $this); } }
function f($a) { throw new MyEx("m"); }
echo "before\n";
f("arg");
--EXPECTF--
before

Fatal error: Uncaught MyEx: m in %s:3
Stack trace:
#0 %s(5): f('arg')
#1 {main}

Next LogicException: tostring fails in %s:2
Stack trace:
#0 [internal function]: MyEx->__toString()
#1 {main}
  thrown in %s on line 2
