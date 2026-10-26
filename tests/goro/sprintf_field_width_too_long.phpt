--TEST--
sprintf(): a field width past INT_MAX - 1 is a fatal "Field width ... is too long"
--FILE--
<?php
echo "start\n";
echo sprintf("%*s", 2147483647, "x");
echo "not reached\n";
?>
--EXPECTF--
start

Fatal error: Field width 2147483647 is too long in %s on line %d
