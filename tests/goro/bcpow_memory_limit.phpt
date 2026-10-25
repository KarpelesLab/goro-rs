--TEST--
bcpow(): a power too large for memory_limit is the allocation fatal error
--INI--
memory_limit=128M
--FILE--
<?php
echo bcpow("2", "100"), "\n";
echo strlen(bcpow("12345678901234567", "1000")), "\n";
echo strlen(bcpow("0.5", "200", 300)), "\n";
try { echo bcpow("2", "100000000000"), "\n"; } catch (Throwable $e) { echo get_class($e), "\n"; }
echo "not reached\n";
--EXPECTF--
1267650600228229401496703205376
16092
302

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 2400000000000 bytes) in %s on line 5
