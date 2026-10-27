--TEST--
Multi-line static call / property assignment / unary operators / sprintf(): opline lines (a22)
--FILE--
<?php
$x = 5;
$s = sprintf(
    "%d %s",
    1,
    $u
);
$s = sprintf(
    "%d %s",
    $x,
    $u1
);
$s = str_repeat(
    $x,
    $u2
);
function f() {
    $x = 1;
    $s = str_repeat(
        $x,
        $u4
    );
}
f();
--EXPECTF--

Warning: Undefined variable $u in %s on line 6

Warning: Undefined variable $u1 in %s on line 11

Warning: Undefined variable $u2 in %s on line 15

Deprecated: str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated in %s on line 13

Warning: Undefined variable $u4 in %s on line 21

Deprecated: str_repeat(): Passing null to parameter #2 ($times) of type int is deprecated in %s on line 19
