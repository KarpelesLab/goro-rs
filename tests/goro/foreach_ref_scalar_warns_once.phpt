--TEST--
foreach by reference over a scalar warns once
--FILE--
<?php
$d = 5; foreach ($d as $k => &$v) {} echo "x\n";
foreach ($d as &$v) {} echo "y\n";
--EXPECTF--

Warning: foreach() argument must be of type array|object, int given in %s on line 2
x

Warning: foreach() argument must be of type array|object, int given in %s on line 3
y
