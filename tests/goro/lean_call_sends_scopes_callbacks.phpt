--TEST--
Lean calls: sends made at once (copies, temporaries, $GLOBALS, references), method scopes kept, callbacks by name and closure
--FILE--
<?php
function two($a, $b) { $a[] = 9; $b[] = 8; return count($a) * 10 + count($b); }
function one($a) { return $a; }
function mod($a) { $a['x'] = 1; return $a; }
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
function keep($o) { return $o->n; }
function drop1($o) { return 1; }
$arr = [1, 2];
for ($i = 0; $i < 3; $i++) {
    echo two($arr, $arr), " ", count($arr), "\n";
    echo two($arr, [1]), "\n";
    echo two([$i], $arr), "\n";
    $r = &$arr;
    echo two($r, $arr), "\n";
    unset($r);
    echo one($i + 1), one("s" . $i), one(null) === null ? "N" : "?", "\n";
    $g = mod($GLOBALS['arr']);
    echo count($g), count($arr), "\n";
    echo keep(new D($i)), "\n";
    echo drop1(new D("t$i")), "\n";
    $o = new D("v$i");
    echo keep($o), "\n";
    unset($o);
    echo one(@$undef) === null ? "undef-null" : "?", "\n";
}
function fga($a, $b = 2) { return func_get_args(); }
for ($i = 0; $i < 3; $i++) { echo json_encode(fga($i)), json_encode(fga($i, $i)), "\n"; }
function bt($a) { return count(debug_backtrace()) . debug_backtrace()[0]['function'] . json_encode(debug_backtrace()[0]['args']); }
for ($i = 0; $i < 2; $i++) { echo bt([$i]), "\n"; }
function st($a) { static $n = 0; $n += $a; return $n; }
for ($i = 0; $i < 3; $i++) { echo st($i), "\n"; }
function rec($n) { return $n <= 0 ? 0 : $n + rec($n - 1); }
for ($i = 0; $i < 3; $i++) { echo rec($i + 3), "\n"; }
function thr($a) { if ($a > 1) throw new Exception("e$a"); return $a; }
for ($i = 0; $i < 3; $i++) { try { echo thr($i), "\n"; } catch (Exception $e) { echo $e->getMessage(), "\n"; } }
function byref(&$a) { $a++; return $a; }
$z = 1; for ($i = 0; $i < 3; $i++) { echo byref($z), "\n"; }
class M { private $v = 0; function add($x) { $this->v += $x; return $this->v; } function __destruct() { echo "M gone\n"; } }
class N extends M { }
$m = new M; $n = new N;
for ($i = 0; $i < 3; $i++) { echo $m->add($i), $n->add($i), "\n"; }
echo implode(",", array_map(fn($x) => $x * 2, [1, 2, 3])), "\n";
$f = function ($x) { return $x + 1; };
echo implode(",", array_map($f, [1, 2, 3])), implode(",", array_map('one', [4, 5])), "\n";
echo implode(",", array_map([$m, 'add'], [1, 2])), "\n";
$c = function () { return Closure::getCurrent() === $GLOBALS['c'] ? "cur" : "no"; };
echo implode(",", array_map($c, [1, 2])), "\n";
usort($arr, fn($a, $b) => $b <=> $a); echo implode(",", $arr), "\n";
echo "end\n";
--EXPECT--
33 2
32
23
33
1s0N
32
dtor 0
0
dtor t0
1
v0
dtor v0
undef-null
33 2
32
23
33
2s1N
32
dtor 1
1
dtor t1
1
v1
dtor v1
undef-null
33 2
32
23
33
3s2N
32
dtor 2
2
dtor t2
1
v2
dtor v2
undef-null
[0][0,0]
[1][1,1]
[2][2,2]
1bt[[0]]
1bt[[1]]
0
1
3
6
10
15
0
1
e2
2
3
4
00
11
33
2,4,6
2,3,44,5
4,6
cur,cur
2,1
end
M gone
M gone
