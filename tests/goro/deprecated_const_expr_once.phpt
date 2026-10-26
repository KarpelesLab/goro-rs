--TEST--
Deprecated constants referenced by a constant expression warn once, at the expression's line
--FILE--
<?php
class I { #[\Deprecated] const IC = 1; }
class Q {
    const X =
        I::IC;
    const Y = self::X + 1;
}
echo "start\n";
var_dump(Q::X);
var_dump(Q::X);
var_dump(Q::Y);
enum E {
    #[\Deprecated] case A;
    const K = self::A;
}
echo "e\n";
var_dump(E::K);
var_dump(E::K);
--EXPECTF--
start

Deprecated: Constant I::IC is deprecated in %s on line 5
int(1)
int(1)
int(2)
e

Deprecated: Enum case E::A is deprecated in %s on line 14
enum(E::A)
enum(E::A)
