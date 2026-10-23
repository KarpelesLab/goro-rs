--TEST--
Nested list() destructuring of an element that is a reference
--FILE--
<?php
// A nested list() reads an element that is a reference through it.
function f() {
    $data = ['x' => 1, 'y' => [1, 2, 3]];
    foreach ($data as $k => &$v) {}
    list('x' => $lx, 'y' => list(, $ly)) = $data;
    var_dump($lx, $ly);
}
f();
$d = ['y' => [1, 2]];
$r = &$d['y'];
['y' => [, $b]] = $d;
var_dump($b);
$e = [[1, [2, 3]]];
$s = &$e[0][1];
[[$p, [$q, $t]]] = $e;
var_dump($p, $q, $t);
--EXPECT--
int(1)
int(2)
int(2)
int(1)
int(2)
int(3)
