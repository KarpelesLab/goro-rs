--TEST--
Returning a referenced array by value gives the caller a copy
--FILE--
<?php
// Returning a static / global / by-ref / captured array by value hands
// the caller a copy: later writes to the variable do not show through.
function f() { static $y = []; $y[] = count($y); return $y; }
var_dump(f(), f());
function g() { global $gy; $gy[] = 1; return $gy; }
echo json_encode([g(), g()]), "\n";
function h(&$r) { $r[] = 1; return $r; }
$q = [];
echo json_encode([h($q), h($q)]), "\n";
$fn = function () use (&$q) { $q[] = 2; return $q; };
echo json_encode([$fn(), $fn()]), "\n";
echo json_encode([$fn(), $q[] = 5, $q]), "\n";
class C {
    public static $s = [];
    public function m() { static $y = []; $y[] = static::class; return $y; }
}
class D extends C {}
var_dump((new C)->m(), (new D)->m());
--EXPECT--
array(1) {
  [0]=>
  int(0)
}
array(2) {
  [0]=>
  int(0)
  [1]=>
  int(1)
}
[[1],[1,1]]
[[1],[1,1]]
[[1,1,2],[1,1,2,2]]
[[1,1,2,2,2],5,[1,1,2,2,2,5]]
array(1) {
  [0]=>
  string(1) "C"
}
array(2) {
  [0]=>
  string(1) "C"
  [1]=>
  string(1) "D"
}
