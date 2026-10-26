--TEST--
array_fill_keys() over memory_limit: the result table's allocation is refused
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
array_fill_keys(range(1, 3000000), 0);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 67108872 bytes) in %s on line %d
Stack trace:
#0 %s(%d): array_fill_keys(Array, 0)
#1 {main}
