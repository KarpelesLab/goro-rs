--TEST--
Memory limit error: a large block reports its whole pages
--FILE--
<?php
function f() { $a = []; while (true) { $a[] = str_repeat("x", 1 << 20); } }
ini_set('memory_limit', '8M');
f();
--EXPECTF--

Fatal error: Allowed memory size of 8388608 bytes exhausted (tried to allocate 1052672 bytes) in %s on line 2
