--TEST--
strip_tags(): the $allowed_tags array elements are converted with zval_get_string()
--FILE--
<?php
// strip_tags(): each element of an $allowed_tags array goes through zval_get_string()
var_dump(strip_tags("<b>x</b><i>y</i>", [[1], "b"]));
try { strip_tags("<b>x</b>", [new stdClass]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class T { function __toString(): string { return "i"; } }
var_dump(strip_tags("<b>x</b><i>y</i>", [new T]));
--EXPECTF--
Warning: Array to string conversion in %s on line 3
string(9) "<b>x</b>y"
Object of class stdClass could not be converted to string
string(9) "x<i>y</i>"
