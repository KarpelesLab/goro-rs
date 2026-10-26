--TEST--
Coalescing an append ($a[] ?? x) is a compile-time error
--FILE--
<?php
echo "start\n";
$s = [];
$s[] ?? 1;
--EXPECTF--

Fatal error: Cannot use [] for reading in %s on line 4
