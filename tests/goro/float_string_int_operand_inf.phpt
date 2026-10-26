--TEST--
Integer operators on an infinite float-string give 0; the deprecation stops at a NUL byte
--FILE--
<?php
var_dump("1e400" | 0);
var_dump("-1e400" & -1);
var_dump("1e400" << 0);
var_dump("1.5\0x" | 0);
?>
--EXPECTF--
Deprecated: Implicit conversion from float-string "1e400" to int loses precision in %s on line %d
int(0)

Deprecated: Implicit conversion from float-string "-1e400" to int loses precision in %s on line %d
int(0)

Deprecated: Implicit conversion from float-string "1e400" to int loses precision in %s on line %d
int(0)

Warning: A non-numeric value encountered in %s on line %d

Deprecated: Implicit conversion from float-string "1.5" to int loses precision in %s on line %d
int(1)
