--TEST--
mb_str_pad(): a result over memory_limit is the "Allowed memory size" fatal error
--INI--
memory_limit=128M
--FILE--
<?php
echo mb_str_pad("ab", 5, "é"), "\n";
mb_str_pad("aé", 2**40, "éx", STR_PAD_BOTH);
?>
--EXPECTF--
abééé

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 1649267441696 bytes) in %s on line %d
