--TEST--
wordwrap() over memory_limit: zend_string_safe_alloc() size
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
wordwrap(str_repeat("a ", 30000000), 1, "\n\n\n\n", true);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 300000036 bytes) in %s on line %d
Stack trace:
#0 %s(%d): wordwrap('a a a a a a a a...', 1, '\n\n\n\n', true)
#1 {main}
