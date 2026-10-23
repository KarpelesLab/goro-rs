--TEST--
?? only fetches variables, elements and properties quietly
--FILE--
<?php
// Only a variable, element or property on the left of ?? is fetched
// quietly; any other expression warns as usual.
$arr = [1];
var_dump(($u1 + 1) ?? 5);
var_dump($arr[$u2] ?? 5);
var_dump(strlen($u3) ?? 5);
var_dump("x$u4" ?? 5);
var_dump($u5 ?? 5);
var_dump($arr[9][8] ?? 5);
var_dump($arr[$arr[7]] ?? 5);
var_dump((string)$u6 ?? 5);
var_dump(-$u7 ?? 5);
var_dump($u8->p ?? 5);
var_dump("0x1A" + 0 ?? 0);
echo "neg: $arr[-1]\n" ?? '';
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 5
int(1)

Warning: Undefined variable $u2 in %s on line 6

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 6
int(5)

Warning: Undefined variable $u3 in %s on line 7

Deprecated: strlen(): Passing null to parameter #1 ($string) of type string is deprecated in %s on line 7
int(0)

Warning: Undefined variable $u4 in %s on line 8
string(1) "x"
int(5)
int(5)

Warning: Undefined array key 7 in %s on line 11

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 11
int(5)

Warning: Undefined variable $u6 in %s on line 12
string(0) ""

Warning: Undefined variable $u7 in %s on line 13
int(0)
int(5)

Warning: A non-numeric value encountered in %s on line 15
int(0)

Warning: Undefined array key -1 in %s on line 16
neg: 
