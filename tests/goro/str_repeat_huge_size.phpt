--TEST--
str_repeat(): the size reported when a huge result exceeds memory_limit
--INI--
memory_limit=128M
--FILE--
<?php
str_repeat('ab', 600000000);
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 1200000032 bytes) in %s on line %d
