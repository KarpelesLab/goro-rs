--TEST--
addcslashes() over memory_limit: its buffer of 4 times the length
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$s = str_repeat("ab<'\"&\n ", 9000000);
addcslashes($s, "a..z");
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 288000032 bytes) in %s on line %d
Stack trace:
#0 %s(%d): addcslashes('ab<'"&\n ab<'"&\n...', 'a..z')
#1 {main}
