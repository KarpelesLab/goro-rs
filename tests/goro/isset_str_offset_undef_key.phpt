--TEST--
isset()/empty()/?? on a string offset with an undefined variable key (null offset is 0)
--FILE--
<?php
$str = "abc";
var_dump(isset($str[$undef]));
var_dump(empty($str[$undef]));
var_dump(isset($str[null]));
var_dump(isset($str[0][$undef]));
var_dump($str[$undef] ?? "dflt");
$a = [1];
var_dump(isset($a[$undef]));
var_dump(isset($str[$undef2], $str[1]));
$o = new ArrayObject([1]); var_dump(isset($o[$undef3]));
--EXPECTF--

Warning: Undefined variable $undef in %s on line 3
bool(true)

Warning: Undefined variable $undef in %s on line 4
bool(false)
bool(true)

Warning: Undefined variable $undef in %s on line 6
bool(true)

Warning: Undefined variable $undef in %s on line 7
string(1) "a"

Warning: Undefined variable $undef in %s on line 9

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 9
bool(false)

Warning: Undefined variable $undef2 in %s on line 10
bool(true)

Warning: Undefined variable $undef3 in %s on line 11

Deprecated: Using null as an array offset is deprecated, use an empty string instead in %s on line 11
bool(false)
