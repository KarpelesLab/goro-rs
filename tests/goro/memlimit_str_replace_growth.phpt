--TEST--
str_replace() checks memory_limit for its grown result (zend_string_safe_alloc size)
--INI--
memory_limit=128M
--FILE--
<?php
$x = str_replace("a", str_repeat("x", 100000000), str_repeat("a", 100));
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 10000000028 bytes) in %s on line 2
