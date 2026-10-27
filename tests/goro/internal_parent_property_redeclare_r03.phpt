--TEST--
do_inherit_property() checks of an internal parent's properties (r03)
--FILE--
<?php
class E extends Exception { protected $message = "m"; protected $code = 5; protected string $file = "f"; protected int $line = 3; }
echo "ok\n";
--EXPECT--
ok
