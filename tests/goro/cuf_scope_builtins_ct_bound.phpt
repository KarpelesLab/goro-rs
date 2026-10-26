--TEST--
call_user_func('compact', ...) and friends are bound at compile time; \compact() / \extract() work
--FILE--
<?php
function f() { $a = 1; return call_user_func('compact', 'a'); }
var_dump(f());
function g() { $r = call_user_func('extract', ['z' => 1]); return [$r, $z]; }
var_dump(g());
function h() { return [call_user_func('func_get_args'), call_user_func('func_num_args')]; }
var_dump(h(1, 2));
function i() { $a = 1; return [call_user_func_array('get_defined_vars', []), \call_user_func('get_defined_vars')]; }
var_dump(i());
function j() { $n = 'get_defined_vars'; try { call_user_func($n); } catch (Error $e) { echo $e->getMessage(), "\n"; } }
j();
function k() { $a = 1; $r = \extract(['b' => 2]); return [\compact('a', 'b'), $r]; }
var_dump(k());
?>
--EXPECT--
array(1) {
  ["a"]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  array(1) {
    ["a"]=>
    int(1)
  }
  [1]=>
  array(1) {
    ["a"]=>
    int(1)
  }
}
Cannot call get_defined_vars() dynamically
array(2) {
  [0]=>
  array(2) {
    ["a"]=>
    int(1)
    ["b"]=>
    int(2)
  }
  [1]=>
  int(1)
}
