--TEST--
urlencode() over memory_limit: its buffer of 3 times the length
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$s = str_repeat("ab<'\"&\n ", 9000000);
rawurlencode($s);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 216000032 bytes) in %s on line %d
Stack trace:
#0 %s(%d): rawurlencode('ab<'"&\n ab<'"&\n...')
#1 {main}
