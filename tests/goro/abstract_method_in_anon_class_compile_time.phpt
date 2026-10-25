--TEST--
An abstract method in an anonymous class is a compile error
--FILE--
<?php
echo "never printed\n";
$o = new class { abstract function g(); };
--EXPECTF--

Fatal error: Anonymous class method g() must not be abstract in %s on line 3
