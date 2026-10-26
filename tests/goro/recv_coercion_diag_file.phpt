--TEST--
Parameter coercion diagnostics are reported in the callee's file (its RECV), e.g. a function declared in eval()
--FILE--
<?php
eval('function h(int $x) { return $x; }
function g(string $s) { return $s; }');
var_dump(h(1.5));
var_dump(g(NAN));
eval('$cl = function(int $x) { return $x; };');
var_dump(array_map($cl, [4.5]), call_user_func("h", "2.5"));
--EXPECTF--

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s(2) : eval()'d code on line %d
int(1)

Warning: unexpected NAN value was coerced to string in %s(2) : eval()'d code on line %d
string(3) "NAN"

Deprecated: Implicit conversion from float 4.5 to int loses precision in %s(6) : eval()'d code on line %d

Deprecated: Implicit conversion from float-string "2.5" to int loses precision in %s(2) : eval()'d code on line %d
array(1) {
  [0]=>
  int(4)
}
int(2)
