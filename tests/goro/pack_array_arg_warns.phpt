--TEST--
pack(): an array argument to a string format code warns (Array to string conversion)
--FILE--
<?php
var_dump(pack("a", []));
var_dump(pack("A*", []));
var_dump(bin2hex(pack("H*", [1])));
var_dump(pack("Z3", "x", ) === "x\0\0");
--EXPECTF--

Warning: Array to string conversion in %s on line 2
string(1) "A"

Warning: Array to string conversion in %s on line 3
string(5) "Array"

Warning: Array to string conversion in %s on line 4

Warning: pack(): Type H: illegal hex digit r in %s on line 4

Warning: pack(): Type H: illegal hex digit r in %s on line 4

Warning: pack(): Type H: illegal hex digit y in %s on line 4
string(6) "a00a00"
bool(true)
