--TEST--
auto_globals_jit: $_ENV / $_REQUEST are in the symbol table only once code referring to them is compiled
--FILE--
<?php
var_dump(count(get_defined_vars()));
function f() { return $_ENV; }
var_dump(array_keys(get_defined_vars()));
var_dump(array_key_exists('_REQUEST', $GLOBALS));
eval('$x = $_REQUEST;');
var_dump(array_keys($GLOBALS));
--EXPECT--
int(9)
array(9) {
  [0]=>
  string(4) "argv"
  [1]=>
  string(4) "argc"
  [2]=>
  string(4) "_GET"
  [3]=>
  string(5) "_POST"
  [4]=>
  string(7) "_COOKIE"
  [5]=>
  string(7) "_SERVER"
  [6]=>
  string(4) "_ENV"
  [7]=>
  string(8) "_REQUEST"
  [8]=>
  string(6) "_FILES"
}
bool(true)
array(10) {
  [0]=>
  string(4) "argv"
  [1]=>
  string(4) "argc"
  [2]=>
  string(4) "_GET"
  [3]=>
  string(5) "_POST"
  [4]=>
  string(7) "_COOKIE"
  [5]=>
  string(7) "_SERVER"
  [6]=>
  string(4) "_ENV"
  [7]=>
  string(8) "_REQUEST"
  [8]=>
  string(6) "_FILES"
  [9]=>
  string(1) "x"
}
