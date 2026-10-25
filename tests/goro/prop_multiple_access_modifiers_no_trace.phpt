--TEST--
Modifier errors raised by the parser have no stack trace (property)
--INI--
fatal_error_backtraces=1
--FILE--
<?php
class A { public protected(set) private(set) $x; }
echo "ok\n";
--EXPECTF--

Fatal error: Multiple access type modifiers are not allowed in %s on line 2
