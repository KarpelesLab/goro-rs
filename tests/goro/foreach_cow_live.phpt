--TEST--
foreach by value iterates a copy-on-write copy of the array
--FILE--
<?php
function show($label, ...$v) { echo $label, ': ', json_encode($v), "\n"; }
$a = ['x' => 1, 'y' => 2, 'z' => 3];
$out = [];
foreach ($a as $k => $v) {
    $out[] = "$k=$v";
    if ($k === 'x') { unset($a['y']); $a['w'] = 4; $a['z'] = 30; }
}
show('f1', $out, $a);
$out = [];
foreach ($a as $k => $v) {
    foreach ($a as $k2 => $v2) { $out[] = "$k$k2"; if ($k2 == 'z') $a[$k2] = 'zz'; }
}
show('f2', $out, $a);
$a = [1, 2, 3];
$r = &$a[1];
$out = [];
foreach ($a as $v) { $r = 'R'; $out[] = $v; }
show('f3', $out, $a);
unset($r);
$a = [1, 2, 3];
$out = [];
foreach ($a as $i => $v) { $a = null; $out[] = $v; }
show('f4', $out, $a);
$a = [1, 2, 3];
$out = [];
foreach ($a as $i => $v) { $a[] = $v; if (count($a) > 10) break; $out[] = $v; }
show('f5', $out, $a);
class O { public $items = ['a', 'b']; function run() { $o = []; foreach ($this->items as $i) { $this->items[] = $i . '!'; $o[] = $i; } return [$o, $this->items]; } }
show('f6', (new O)->run());
function gen() { $arr = [1, 2]; foreach ($arr as $v) { $arr[] = $v + 10; yield $v; } yield $arr; }
show('f7', iterator_to_array(gen(), false));
$a = [1, 2, 3];
$out = [];
foreach ($a as $v) { foreach ($a as &$w) { $w *= 2; } unset($w); $out[] = $v; }
show('f8', $out, $a);
$a = ['k' => [1, 2]];
$out = [];
foreach ($a['k'] as $v) { $a['k'][] = $v; $out[] = $v; }
show('f9', $out, $a);
$a = [1, 2];
$b = $a;
$out = [];
foreach ($b as $v) { $b[] = 'b'; $a[] = 'a'; $out[] = $v; }
show('f10', $out, $a, $b);
$a = [3, 1, 2];
$out = [];
foreach ($a as $v) { sort($a); $out[] = $v; }
show('f11', $out, $a);
$a = [1, 2, 3];
$out = [];
foreach ($a as $k => $v) { next($a); $out[] = [$k, $v, key($a)]; }
show('f12', $out);
$GLOBALS['gtest'] = 1;
$n = 0;
foreach ($GLOBALS as $k => $v) { if ($k === 'gtest') $n++; }
show('f13', $n);
$out = [];
foreach ([1, 2, 3] as $v) { $out[] = $v; }
foreach (range(1, 3) as $k => $v) { $out[] = $k; }
show('f14', $out);
$a = [1, 2, 3];
$out = [];
foreach ($a as $v) { if ($v == 1) continue; $out[] = $v; array_pop($a); }
show('f15', $out, $a);
$a = ['a' => 1];
$copy = null;
foreach ($a as $k => $v) { $copy = $a; $copy['b'] = 2; }
show('f16', $a, $copy);
function byval(array $x) { $o = []; foreach ($x as $v) { $x[] = $v; $o[] = $v; } return [$o, $x]; }
show('f17', byval([1, 2]));
$a = [1, 2, 3];
foreach ($a as $v) { unset($a); }
show('f18', isset($a));
?>
--EXPECT--
f1: [["x=1","y=2","z=3"],{"x":1,"z":30,"w":4}]
f2: [["xx","xz","xw","zx","zz","zw","wx","wz","ww"],{"x":1,"z":"zz","w":4}]
f3: [[1,"R",3],[1,"R",3]]
f4: [[1,2,3],null]
f5: [[1,2,3],[1,2,3,1,2,3]]
f6: [[["a","b"],["a","b","a!","b!"]]]
f7: [[1,2,[1,2,11,12]]]
f8: [[1,2,3],[8,16,24]]
f9: [[1,2],{"k":[1,2,1,2]}]
f10: [[1,2],[1,2,"a","a"],[1,2,"b","b"]]
f11: [[3,1,2],[1,2,3]]
f12: [[[0,1,1],[1,2,2],[2,3,null]]]
f13: [1]
f14: [[1,2,3,0,1,2]]
f15: [[2,3],[1]]
f16: [{"a":1},{"a":1,"b":2}]
f17: [[[1,2],[1,2,1,2]]]
f18: [false]
