--TEST--
random_bytes() / Randomizer byte strings over memory_limit: the fatal error, not an abort
--INI--
memory_limit=128M
fatal_error_backtraces=1
--FILE--
<?php
$r = new Random\Randomizer(new Random\Engine\Mt19937(1));
try { $r->getBytesFromString("", 1); } catch (ValueError $e) { echo $e->getMessage(), "\n"; }
var_dump(strlen(random_bytes(3)));
random_bytes(PHP_INT_MAX);
--EXPECTF--
Random\Randomizer::getBytesFromString(): Argument #1 ($string) must not be empty
int(3)

Fatal error: Allowed memory size of 134217728 bytes exhausted (tried to allocate 9223372036854775832 bytes) in %s on line 5
Stack trace:
#0 %s(5): random_bytes(9223372036854775807)
#1 {main}
