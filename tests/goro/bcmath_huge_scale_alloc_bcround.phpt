--TEST--
bcmath memory_limit error of bcround with a huge scale reports the aligned allocation size
--EXTENSIONS--
bcmath
--INI--
memory_limit=128M
--FILE--
<?php
bcround(str_repeat('9', 10), 2147483647);
?>
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483696 bytes) in %s on line %d
