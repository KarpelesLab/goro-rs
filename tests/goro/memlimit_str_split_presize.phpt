--TEST--
str_split() presizes its result array (array_init_size) under memory_limit
--INI--
memory_limit=128M
--FILE--
<?php
$x = str_split(str_repeat("a", 1 << 26), 1);
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 1073741832 bytes) in %s on line 2
