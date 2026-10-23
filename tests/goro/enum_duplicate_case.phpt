--TEST--
An enum case redeclaring a case is a compile error at its line
--FILE--
<?php
enum E {
 case A;

 case A;
}
echo "ok\n";
--EXPECTF--

Fatal error: Cannot redefine class constant E::A in %s on line 5
