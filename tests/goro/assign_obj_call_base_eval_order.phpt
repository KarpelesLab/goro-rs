--TEST--
f()->p = v(): the object expression is evaluated before the assigned value
--FILE--
<?php
function o($n) { echo "o$n "; return new stdClass; }
function v($n) { echo "v$n "; return $n; }
o(1)->p = v(2); echo "\n";
o(1)->p->q ?? null; echo "\n";
$a = [o(1)]; $a[0]->p = v(2); echo "\n";
o(1)->{v(2)} = v(3); echo "\n";
$x = o(1); $x->{v(2)} = v(3); echo "\n";
o(1)->p[v(2)] = v(3); echo "\n";
o(1)->p .= v(2); echo "\n";
o(1)->p ??= v(2); echo "\n";
$arr = []; $arr[v(1)] = v(2); echo "\n";
class S { static $s; } S::$s = v(1); echo "\n";
[o(1)->p, o(2)->q] = [v(3), v(4)]; echo "\n";
--EXPECTF--
o1 v2 
o1 
o1 v2 
o1 v2 v3 
o1 v2 v3 
o1 v2 v3 
o1 v2 
Warning: Undefined property: stdClass::$p in %s on line 10

o1 v2 
v1 v2 
v1 
v3 v4 o1 o2 
