--TEST--
strict_types declaration in a function body: must be the very first statement
--FILE--
<?php
function f() { declare(strict_types=1); }
echo "x";
--EXPECTF--

Fatal error: strict_types declaration must be the very first statement in the script in %s on line 2
