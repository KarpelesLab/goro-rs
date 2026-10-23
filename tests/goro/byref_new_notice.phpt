--TEST--
A new expression passed by reference: "Only variables should be passed by reference" notice
--FILE--
<?php
function f() { return [3, 1]; }
var_dump(sort(f()));
try { var_dump(sort(new ArrayObject([2, 1]))); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
function g(&$x) { $x = 5; return 'g'; }
var_dump(g(new stdClass));
var_dump(end(f()));
var_dump(array_pop(f()));
try { var_dump(array_shift([1, 2])); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$r = preg_match('/a/', 'a', new ArrayObject);
var_dump($r);
--EXPECTF--

Notice: Only variables should be passed by reference in %s on line 3
bool(true)

Notice: Only variables should be passed by reference in %s on line 4
sort(): Argument #1 ($array) must be of type array, ArrayObject given

Notice: Only variables should be passed by reference in %s on line 6
string(1) "g"

Notice: Only variables should be passed by reference in %s on line 7
int(1)

Notice: Only variables should be passed by reference in %s on line 8
int(1)
array_shift(): Argument #1 ($array) could not be passed by reference

Notice: Only variables should be passed by reference in %s on line 10
int(1)
