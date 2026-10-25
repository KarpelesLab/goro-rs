--TEST--
bcround(): a huge precision is the allocation fatal error
--INI--
memory_limit=128M
--FILE--
<?php
echo bcround("3.14159", 10), "\n";
echo bcround("3", -5, RoundingMode::AwayFromZero), "\n";
echo bcround("3", -50000000000, RoundingMode::HalfAwayFromZero), "\n";
echo bcround("3", PHP_INT_MAX), "\n";
--EXPECTF--
3.1415900000
100000
0

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775840 bytes) in %s on line 5
