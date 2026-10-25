--TEST--
Modifier errors raised by the parser have no stack trace (hook)
--INI--
fatal_error_backtraces=1
--FILE--
<?php
class A { public $x { abstract get; } }
echo "ok\n";
--EXPECTF--

Fatal error: Cannot use the abstract modifier on a property hook in %s on line 2
