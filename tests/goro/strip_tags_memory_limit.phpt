--TEST--
strip_tags() over memory_limit: the copy it works in
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$s = str_repeat("ab<'\"&\n ", 9000000);
strip_tags($s);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 72000032 bytes) in %s on line %d
Stack trace:
#0 %s(%d): strip_tags('ab<'"&\n ab<'"&\n...')
#1 {main}
