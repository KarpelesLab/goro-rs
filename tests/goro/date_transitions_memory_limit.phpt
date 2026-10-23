--TEST--
getTransitions() over an open-ended range runs into memory_limit
--FILE--
<?php
ini_set('memory_limit', '16M');
echo count((new DateTimeZone('Europe/Paris'))->getTransitions(PHP_INT_MIN, PHP_INT_MAX));
echo "not reached\n";
--EXPECTF--

Fatal error: Allowed memory size of 16777216 bytes exhausted (tried to allocate %d bytes) in %s on line 3
