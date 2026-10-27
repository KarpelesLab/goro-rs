--TEST--
Multi-line match arm bodies: opline lines (z01)
--FILE--
<?php
$x = match (true) {
    default =>
        $u1,
};
$y = match (1) {
    1 => $u2
        . "x",
};
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 4

Warning: Undefined variable $u2 in %s on line 8
