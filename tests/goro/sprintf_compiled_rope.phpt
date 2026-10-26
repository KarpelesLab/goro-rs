--TEST--
sprintf() with a literal %s/%d format is compiled as a concatenation
--FILE--
<?php
echo "start\n";
var_dump(sprintf('%s', NAN));
var_dump(sprintf(''));
var_dump(sprintf('abc'));
var_dump(sprintf('100%%'));
var_dump(sprintf('%d items', "12abc"));
var_dump(sprintf('%d', 1.9e30));
var_dump(sprintf('%s-%d', 1.5, true));
var_dump(sprintf('%s', null));
var_dump(sprintf('%s', [1]));
var_dump(sprintf('%d', [1]));
$a = 1;
function f() { global $a; $a = 2; return "f"; }
var_dump(sprintf('%s%s', $a, f()));
class T { function __toString(): string { throw new Exception("boom"); } }
try { sprintf('<%s>', new T); } catch (Exception $e) { echo $e->getMessage(), " ", count($e->getTrace()), "\n"; }
try { sprintf('<%d>', new T); } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
try { var_dump(sprintf('%s %s', 1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(sprintf('%5s|%x', 'a', 255));
var_dump(sprintf('%s', 0.1 + 0.2));
var_dump(sprintf("%s", 1e100));
var_dump(sprintf('%d', "abc"));
var_dump(sprintf('%d', NAN));
var_dump(sprintf('%d', INF));
--EXPECTF--

Warning: unexpected NAN value was coerced to string in %s on line 3
start
string(3) "NAN"
string(0) ""
string(3) "abc"
string(4) "100%"
string(8) "12 items"

Warning: The float 1.9E+30 is not representable as an int, cast occurred in %s on line 8
string(19) "2267562412381044736"
string(5) "1.5-1"
string(0) ""

Warning: Array to string conversion in %s on line 11
string(5) "Array"
string(1) "1"
string(2) "2f"
boom 1

Warning: Object of class T could not be converted to int in %s on line 18
ArgumentCountError: 3 arguments are required, 2 given
string(8) "    a|ff"
string(3) "0.3"
string(8) "1.0E+100"
string(1) "0"

Warning: The float NAN is not representable as an int, cast occurred in %s on line 24
string(1) "0"

Warning: The float INF is not representable as an int, cast occurred in %s on line 25
string(1) "0"
