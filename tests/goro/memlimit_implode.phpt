--TEST--
implode() checks memory_limit for its result
--INI--
memory_limit=128M
--FILE--
<?php
$s = implode(str_repeat("x", 100000000), range(1, 10));
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 900000040 bytes) in %s on line 2
