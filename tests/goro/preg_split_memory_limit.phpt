--TEST--
preg_split() over memory_limit: one-char pieces are interned, the table's growth fails inside the call
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
preg_split('//', str_repeat("a", 20000000));
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 134217736 bytes) in %s on line %d
Stack trace:
#0 %s(%d): preg_split('//', 'aaaaaaaaaaaaaaa...')
#1 {main}
