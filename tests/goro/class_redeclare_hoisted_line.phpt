--TEST--
A second declaration of a hoisted class is reported at runtime, at its own line
--FILE--
<?php
echo "start\n";
class A {}

class A {}
--EXPECTF--
start

Fatal error: Cannot redeclare class A (previously declared in %s:3) in %s on line 5
