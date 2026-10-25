--TEST--
A class must implement the methods of the internal interfaces it implements
--FILE--
<?php
echo "start\n";
class Y implements Countable {}
echo "survived\n";
--EXPECTF--
start

Fatal error: Class Y contains 1 abstract method and must therefore be declared abstract or implement the remaining method (Countable::count) in %s on line 3
