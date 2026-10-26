--TEST--
array_pad(): array_init_size() allocation over the memory limit
--FILE--
<?php
try { $x = array_pad([1], 100000000, 0); echo count($x), "\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
echo "done\n";
--EXPECTF--

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483656 bytes) in %s on line 2
