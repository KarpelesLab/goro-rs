--TEST--
Fast list() destructuring
--FILE--
<?php
function a($v) { [$x, $y] = $v; return [$x, $y]; }
function b($v) { list('k' => $x, 'z' => $y) = $v; return [$x, $y]; }
function c($v) { [[$x], $y] = $v; return [$x, $y]; }
function d() { $v = [1, 2]; [$v[1], $v[0]] = $v; return $v; }
function e() { $x = 5; $r = [&$x, 2]; [$a, $b] = $r; $a++; return [$x, $a, $b]; }
function f($o) { [$o->p, $o->q] = [1, 2]; return $o; }
function g() { foreach ([[1, 2], [3, 4]] as [$a, $b]) echo $a + $b; echo "\n"; }
function h($s) { [$a, $b] = $s; return [$a, $b]; }
echo json_encode([a([1, 2]), a([1]), b(['k' => 1, 'z' => 2]), c([[1], 2]), d(), e(), f(new stdClass), h("ab"), h(null), a(['1' => 'x', 0 => 'y'])]), "\n";
g();
$GLOBALS['zz'] = 1;
function gl() { [$a] = [$GLOBALS]; return isset($a['zz']); }
var_dump(gl());
--EXPECTF--

Warning: Undefined array key 1 in %s on line 2

Warning: Cannot use string as array in %s on line 9

Warning: Cannot use string as array in %s on line 9
[[1,2],[1,null],[1,2],[1,2],[2,1],[5,6,2],{"p":1,"q":2},[null,null],[null,null],["y","x"]]
37
bool(true)
