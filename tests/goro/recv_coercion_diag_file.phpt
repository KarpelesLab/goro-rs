--TEST--
Parameter coercion diagnostics are reported in the callee's file (its RECV), e.g. a function declared in eval()
--FILE--
<?php
eval('function h(int $x) { return $x; }
function g(string $s) { return $s; }');
var_dump(h(1.5));
var_dump(g(NAN));
--EXPECTF--

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s(2) : eval()'d code on line %d
int(1)

Warning: unexpected NAN value was coerced to string in %s(2) : eval()'d code on line %d
string(3) "NAN"
