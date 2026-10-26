--TEST--
str_split() over memory_limit: the result array's allocation is refused up front
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
str_split(str_repeat("a", 30000000));
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 536870920 bytes) in %s on line %d
Stack trace:
#0 %s(%d): str_split('aaaaaaaaaaaaaaa...')
#1 {main}
