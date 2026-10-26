--TEST--
isset()/empty()/?? evaluate every key (and property name) before the quiet fetches (symfony PhpDumper isset($a[$x][$class = f()]))
--FILE--
<?php
function f() { echo "f\n"; return 'C'; }
function g() { echo "g\n"; return 'T'; }
$done = [];
var_dump(isset($done[true][$class = f()][$tags = g()]));
var_dump($class, $tags);
var_dump(empty($done['x'][$k = 'kk']), $k);
var_dump($done['q'][$z = 'zz'] ?? 'dflt', $z);
var_dump(isset($undef[$u1 = 1][$u2 = 2]), $u1, $u2);
$arr = [10, 20, 30]; $i = 0;
var_dump($arr[$i++] ?? null, $arr[$i++] ?? null, $i);
$m = ['a' => ['b' => 5]];
var_dump(isset($m['a'][$q = 'b']), $q, $m['a'][strtolower('B')] ?? 0);
class AA implements ArrayAccess {
    function offsetExists($o): bool { echo "exists($o)\n"; return $o === 'k'; }
    function offsetGet($o): mixed { echo "get($o)\n"; return ['in' => 1]; }
    function offsetSet($o, $v): void {}
    function offsetUnset($o): void {}
}
$aa = new AA;
var_dump(isset($aa[$kk = 'k'][$jj = 'in']), $kk, $jj);
var_dump(isset($aa['nope'][$j2 = f()]), $j2);
$o = new stdClass; $o->x = ['y' => 1];
var_dump(isset($o->{$pn = 'x'}[$yn = 'y']), $pn, $yn);
var_dump(isset($o->{$pn2 = 'nope'}->z[$yn2 = g()]), $pn2, $yn2);
var_dump(empty($o->x[$e1 = 'y']), $e1);

?>
--EXPECT--
f
g
bool(false)
string(1) "C"
string(1) "T"
bool(true)
string(2) "kk"
string(4) "dflt"
string(2) "zz"
bool(false)
int(1)
int(2)
int(10)
int(20)
int(2)
bool(true)
string(1) "b"
int(5)
exists(k)
get(k)
bool(true)
string(1) "k"
string(2) "in"
f
exists(nope)
bool(false)
string(1) "C"
bool(true)
string(1) "x"
string(1) "y"
g
bool(false)
string(4) "nope"
string(1) "T"
bool(false)
string(1) "y"
