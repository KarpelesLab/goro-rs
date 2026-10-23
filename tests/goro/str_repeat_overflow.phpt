--TEST--
str_repeat(): an overflowing result size is the allocator's fatal error
--INI--
fatal_error_backtraces=0
--FILE--
<?php
// zend_string_safe_alloc() overflow in str_repeat()
echo strlen(str_repeat('ab', 3)), "\n";
str_repeat('ab', PHP_INT_MAX);
--EXPECTF--
6

Fatal error: Possible integer overflow in memory allocation (2 * 9223372036854775807 + 32) in %s on line 4
