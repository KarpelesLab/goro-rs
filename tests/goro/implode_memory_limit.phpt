--TEST--
implode() over memory_limit: the pieces table and zend_string_safe_alloc() sizes
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
implode(",", array_fill(0, 3000000, "abcdefghij"));
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 33000031 bytes) in %s on line %d
Stack trace:
#0 %s(%d): implode(',', Array)
#1 {main}
