--TEST--
strict_types declaration in a method body: must be the very first statement
--FILE--
<?php
class A { function m() { declare(strict_types=1); } }
--EXPECTF--

Fatal error: strict_types declaration must be the very first statement in the script in %s on line 2
