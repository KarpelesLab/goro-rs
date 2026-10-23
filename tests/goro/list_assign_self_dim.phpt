--TEST--
Destructuring into elements of the source variable uses the original array
--FILE--
<?php
// Destructuring into elements of the source variable reads the original
// array (zend_compile_assign() evaluates a CV source first)
$a = [1, 2]; [$a[1], $a[0]] = $a; echo json_encode($a), "\n";
$c = [1, 2]; $t = $c; [$c[1], $c[0]] = $t; echo json_encode($c), "\n";
function f() { $a = [1, 2]; [$a[1], $a[0]] = $a; return json_encode($a); } echo f(), "\n";
$d = ['x' => 1, 'y' => 2]; ['x' => $d['y'], 'y' => $d['x']] = $d; echo json_encode($d), "\n";
$e = [1, 2]; list($e[1], $e[0]) = $e; echo json_encode($e), "\n";
$g = [[1, 2]]; [[$g[0][1], $g[0][0]]] = $g; echo json_encode($g), "\n";
$h = [3, 4]; $r = ([$h[1], $h[0]] = $h); echo json_encode([$h, $r]), "\n";
--EXPECT--
[2,1]
[2,1]
[2,1]
{"x":2,"y":1}
[2,1]
[[2,1]]
[[4,3],[3,4]]
