--TEST--
array_fill(): the packed allocation size (zvals + hash) in the memory limit fatal
--FILE--
<?php
$x = array_fill(0, 100000000, 0);
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483656 bytes) in %s on line 2
