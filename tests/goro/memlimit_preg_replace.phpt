--TEST--
preg_replace() checks memory_limit as its result grows
--INI--
memory_limit=128M
--FILE--
<?php
$x = preg_replace("/a/", str_repeat("x", 100000000), str_repeat("a", 100));
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 200000032 bytes) in %s on line 2
