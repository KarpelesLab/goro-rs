--TEST--
strict_types declaration must have 0 or 1 as its value
--FILE--
<?php
declare(strict_types=2);
echo "x";
--EXPECTF--

Fatal error: strict_types declaration must have 0 or 1 as its value in %s on line 2
