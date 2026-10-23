--TEST--
strtr(): replacement values are converted like strings
--FILE--
<?php
var_dump(strtr("abc", ["a" => []]));
var_dump(strtr("xyz", ["a" => []]));
var_dump(strtr("abca", ["a" => [], "b" => "B"]));
var_dump(strtr("xyz", ["a" => [], "b" => "B"]));
var_dump(strtr("abc", ["" => "x", "a" => "A"]));
var_dump(strtr("abc", ["" => "x"]));
var_dump(strtr("abc", [1 => "x", "ab" => "Y", "abc" => "Z", "abcd" => "W"]));
var_dump(strtr("a1b1", [1 => 2]));
try { strtr("abc", ["a" => new stdClass, "b" => 1]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { strtr("abc", ["a" => new stdClass]); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(strtr("xbc", ["a" => new stdClass, "b" => 1]));
--EXPECTF--

Warning: Array to string conversion in %s on line 2
string(7) "Arraybc"

Warning: Array to string conversion in %s on line 3
string(3) "xyz"

Warning: Array to string conversion in %s on line 4

Warning: Array to string conversion in %s on line 4
string(12) "ArrayBcArray"
string(3) "xyz"

Warning: strtr(): Ignoring replacement of empty string in %s on line 6
string(3) "Abc"

Warning: strtr(): Ignoring replacement of empty string in %s on line 7
string(3) "abc"
string(1) "Z"
string(4) "a2b2"
Object of class stdClass could not be converted to string
Object of class stdClass could not be converted to string
string(3) "x1c"
