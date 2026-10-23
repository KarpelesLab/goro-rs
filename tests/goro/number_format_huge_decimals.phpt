--TEST--
number_format() with huge decimals hits memory_limit with zend_string_alloc's size
--FILE--
<?php
echo "before\n";
echo strlen(number_format(5, 10000000)), "\n";
function f() { return number_format(1.5, PHP_INT_MAX); }
f();
echo "after\n";
--EXPECTF--
before
10000002

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483680 bytes) in %s on line 4
