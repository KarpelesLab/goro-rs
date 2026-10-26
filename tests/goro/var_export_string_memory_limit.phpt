--TEST--
var_export() of a long string over memory_limit: php_addcslashes()'s buffer
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$s = str_repeat("ab<'\"&\n ", 9000000);
var_export($s, true);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 288000032 bytes) in %s on line %d
Stack trace:
#0 %s(%d): var_export('ab<'"&\n ab<'"&\n...', true)
#1 {main}
