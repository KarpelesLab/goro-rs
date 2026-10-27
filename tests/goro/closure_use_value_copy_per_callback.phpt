--TEST--
A closure's by-value captured arrays start fresh on every call made by a builtin callback
--FILE--
<?php
$r = [];
$f = function ($m) use ($r) { $k = $m[0] ?? $m; $n = isset($r[$k]) ? "DUP" : "new"; $r[$k] = true; return $n; };
echo preg_replace_callback('/a/', $f, "aa"), "\n";
echo implode(",", array_map($f, ['a', 'a'])), "\n";
$s = 'x';
$g = function ($m) use ($s) { $s .= 'y'; return $s; };
echo preg_replace_callback('/a/', $g, "aa"), "\n";
echo implode(",", array_map($g, [1, 2])), "\n";
$h = function ($m) use ($r) { $r[] = 1; return count($r); };
echo implode(",", array_map($h, [1, 2])), "\n";
echo preg_replace_callback('/a/', $h, "aa"), "\n";
$r = [];
$h = function ($m) use ($r) { $r[] = 1; return count($r); };
echo call_user_func($h, 1), call_user_func($h, 1), "\n";
$h2 = function ($m) use ($r) { $r[] = 1; func_get_args(); return count($r); };
echo implode(",", array_map($h2, [1, 2])), "\n";
$h3 = function ($m) use ($r) { $r['a'] = 1; $r['b'] = 1; return count($r); };
echo implode(",", array_map($h3, [1, 2])), "\n";
$h4 = function ($m) use ($r) { $r[] = 1; return count($r); };
var_dump($r);
echo implode(",", array_map($h4, [1, 2])), "\n";
$x = [1];
$h5 = function ($m) use ($x) { $x[] = 1; return count($x); };
echo implode(",", array_map($h5, [1, 2])), "\n";
$x = range(1, 1);
$h5 = function ($m) use ($x) { $x[] = 1; return count($x); };
echo implode(",", array_map($h5, [1, 2])), "\n";
echo $h5(1), $h5(1), "\n";
function mk() { $y = range(1,1); return function ($m) use ($y) { $y[] = 1; return count($y); }; }
$h6 = mk();
echo implode(",", array_map($h6, [1, 2])), "\n";
$h7 = function ($m) use ($x) { $x[0] = 5; return $x[0] + count($x); };
echo implode(",", array_map($h7, [1, 2])), "\n";

?>
--EXPECT--
newnew
new,new
xyxy
xy,xy
1,1
11
11
1,1
2,2
array(0) {
}
1,1
2,2
2,2
22
2,2
6,6
