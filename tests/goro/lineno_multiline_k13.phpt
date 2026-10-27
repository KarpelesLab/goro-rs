--TEST--
Multi-line static call / property assignment / unary operators / sprintf(): opline lines (k13)
--FILE--
<?php
$x = null;
$x
    ->y
    = 5;
--EXPECTF--

Fatal error: Uncaught Error: Attempt to assign property "y" on null in %s:4
Stack trace:
#0 {main}
  thrown in %s on line 4
