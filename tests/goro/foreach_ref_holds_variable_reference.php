<?php
$a = [1,2,3];
foreach ($a as &$x) { echo $x, " "; if ($x == 1) unset($a); }
echo "\n"; var_dump($a ?? 'unset');
$a = [1,2,3]; $b = [7,8,9];
foreach ($a as &$x) { echo $x, " "; if ($x == 1) $a = &$b; }
echo "\n"; unset($x);
$a = [1,2,3];
foreach ($a as &$x) { echo $x, " "; if ($x == 1) $a = [4,5,6]; }
echo "\n"; unset($x);
$a = [1,2,3];
echo "\n"; unset($x);
$o = new stdClass; $o->p = [1,2,3];
foreach ($o->p as &$x) { echo $x, " "; if ($x == 1) $o->p = [9]; }
echo "\n"; unset($x);
$o = new stdClass; $o->p = [1,2,3];
foreach ($o->p as &$x) { echo $x, " "; if ($x == 1) $o = null; }
echo "\n"; unset($x);
$arr = ['k' => [1,2,3]];
foreach ($arr['k'] as &$x) { echo $x, " "; if ($x == 1) unset($arr['k']); }
echo "\n"; var_dump($arr);
$a = [1,2,3];
foreach ($a as $k => &$x) { echo "$k=$x "; if ($k == 0) { $a[] = 4; } }
echo "\n";
$a = [1,2,3];
function f() { global $a; foreach ($a as &$x) { $x *= 2; } unset($x); $a[] = 9; }
f();
var_dump($a);
$b = [1,2];
$c = &$b;
foreach ($b as &$x) { $x++; }
unset($x);
$c[] = 5;
var_dump($b);
$d = [1,2,3];
foreach ($d as $k => &$x) { echo "$k=$x "; if ($k == 0) { unset($d); $d = [8, 9]; } }
echo "\n"; var_dump($d);
$e = [1,2,3];
foreach ($e as $k => &$x) { echo "$k=$x "; if ($k == 0) { $e[] = 4; array_pop($e); $e[] = 5; } }
echo "\n";
unset($x);
$f = [1,2,3];
foreach ($f as $k => &$x) { echo "$k=$x "; if ($k == 0) { $g = $f; $g[] = 7; sort($f); } }
echo "\n"; var_dump($f);

?>
