--TEST--
fread() / SplFileObject::fread() of a length over memory_limit is the memory-limit fatal error
--INI--
memory_limit=128M
--FILE--
<?php
$f = new SplTempFileObject();
$f->fread(PHP_INT_MAX);
?>
--EXPECTF--
Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775832 bytes) in %s on line 3
