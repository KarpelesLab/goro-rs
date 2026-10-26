--TEST--
array_chunk() over memory_limit: the result array's allocation is refused up front
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
array_chunk(range(1, 3000000), 1);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 67108872 bytes) in %s on line %d
Stack trace:
#0 %s(%d): array_chunk(Array, 1)
#1 {main}
