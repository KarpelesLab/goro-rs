--TEST--
$s[PHP_INT_MAX] = 'X' is a memory_limit fatal error, not an abort
--FILE--
<?php
$s = "hello"; $s[PHP_INT_MAX] = 'X';
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775840 bytes) in %s on line 2
