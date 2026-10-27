--TEST--
f()->p = v() and friends: evaluation order
--FILE--
<?php
function o($n) { echo "o$n "; return new stdClass; }
function v($n) { echo "v$n "; return $n; }
class S { static $o; static function m() { echo "m "; return new stdClass; } }
S::$o = new stdClass; S::$o->p = v(1); echo "\n";
S::m()->p = v(2); echo "\n";
$r = o(1)->p = v(2); var_dump($r);
o(1)->p++; echo "\n";
o(1)->a->b = v(2); echo "\n";
o(1)->p[] = v(2); echo "\n";
isset(o(1)->p); echo "\n";
--EXPECTF--
v1 
m v2 
o1 v2 int(2)
o1 
Warning: Undefined property: stdClass::$p in %s on line 8

o1 v2 
Fatal error: Uncaught Error: Attempt to assign property "b" on null in %s:9
Stack trace:
#0 {main}
  thrown in %s on line 9
