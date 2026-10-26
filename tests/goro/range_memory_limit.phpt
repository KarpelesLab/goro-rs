--TEST--
range(): array_init_size() over the memory limit is a fatal error, not an abort
--FILE--
<?php
var_dump(count(range(1, 100000)), count(range(0.5, 1000.5)));
try { $r = range(0.0, 1e8); } catch (Throwable $e) { echo get_class($e), "\n"; }
--EXPECTF--
int(100000)
int(1001)

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2147483656 bytes) in %s on line 3
