--TEST--
BcMath\Number::pow(): the work buffer size overflow fatal error
--FILE--
<?php
echo (new BcMath\Number("2"))->pow(10), "\n";
echo (new BcMath\Number("3"))->pow(PHP_INT_MAX), "\n";
echo "not reached\n";
--EXPECTF--
1024

Fatal error: Possible integer overflow in memory allocation (9223372036854775807 * 24 + 0) in %s on line 3
