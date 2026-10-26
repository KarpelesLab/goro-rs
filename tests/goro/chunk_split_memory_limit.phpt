--TEST--
chunk_split() over memory_limit: the fatal error from inside the call
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
chunk_split(str_repeat("a", 30000000), 1, "xxxxx");
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 180000032 bytes) in %s on line %d
Stack trace:
#0 %s(%d): chunk_split('aaaaaaaaaaaaaaa...', 1, 'xxxxx')
#1 {main}
