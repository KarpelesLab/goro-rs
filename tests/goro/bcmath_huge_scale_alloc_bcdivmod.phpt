--TEST--
bcmath memory_limit error of bcdivmod with a huge scale reports the aligned allocation size
--EXTENSIONS--
bcmath
--INI--
memory_limit=128M
--FILE--
<?php
bcdivmod('7', '3', 2147483647);
?>
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483680 bytes) in %s on line %d
