--TEST--
htmlspecialchars() over memory_limit: php_escape_html_entities_ex()'s buffer of twice the length
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$s = str_repeat("ab<'\"&\n ", 9000000);
htmlspecialchars($s);
echo "not reached\n";
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 144000032 bytes) in %s on line %d
Stack trace:
#0 %s(%d): htmlspecialchars('ab<'"&\n ab<'"&\n...')
#1 {main}
