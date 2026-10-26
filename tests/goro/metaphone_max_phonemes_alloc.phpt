--TEST--
metaphone(): a huge $max_phonemes exhausts the memory limit
--INI--
memory_limit=128M
--FILE--
<?php
// metaphone() allocates $max_phonemes + 1 bytes up front
var_dump(metaphone("Thompson", 3));
var_dump(metaphone("9", PHP_INT_MAX));
--EXPECTF--
string(3) "0MP"

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775840 bytes) in %s on line 4
