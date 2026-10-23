--TEST--
An enum case and a constant of the same name clash
--FILE--
<?php
enum E { case A; const A = 1; }
echo "ok\n";
--EXPECTF--

Fatal error: Cannot redefine class constant E::A in %s on line 2
