--TEST--
${'a' . 'b'} names a compiled variable
--FILE--
<?php
function f() {
    $name = 'dyn'; ${'na' . 'me2'} = 'x'; $q = 1;
    var_dump(array_keys(get_defined_vars()));
    var_dump($name2);
}
f();
function g() { $a = 1; ${'b'} = 2; $c = 'cc'; $$c = 3; var_dump(array_keys(get_defined_vars())); }
g();
--EXPECT--
array(3) {
  [0]=>
  string(4) "name"
  [1]=>
  string(5) "name2"
  [2]=>
  string(1) "q"
}
string(1) "x"
array(4) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
  [2]=>
  string(1) "c"
  [3]=>
  string(2) "cc"
}
