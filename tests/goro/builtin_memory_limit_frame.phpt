--TEST--
A builtin whose allocation went over memory_limit: the fatal error's trace shows the builtin's frame
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
array_reverse(range(1, 4000000));
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 67108872 bytes) in %s on line %d
Stack trace:
#0 %s(%d): array_reverse(Array)
#1 {main}
