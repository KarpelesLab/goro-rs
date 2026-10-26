--TEST--
print_r() of an object whose __debugInfo() returns a non-array is fatal
--FILE--
<?php
class C { function __debugInfo() { return 5; } }
print_r([new C]);
echo "not reached\n";
?>
--EXPECTF--
Fatal error: __debuginfo() must return an array in %s on line 3
