--TEST--
Static variables in functions and methods called without a frame of their own: counters, recursion, arrays, references, initializers, inheritance, traits, closures, reflection, destructors
--FILE--
<?php
function counter() { static $n = 0; $n++; return $n; }
function rec($d) { static $calls = 0; $calls++; if ($d > 0) { rec($d - 1); } return $calls; }
function arr($v) { static $a = [1, 2]; $a[] = $v; return count($a); }
function arrcopy() { static $a = ['x' => 1]; $b = $a; $b['y'] = 2; $a['z'] = 3; return count($b) . count($a); }
function init() { echo "init ran\n"; return 40; }
function lazy($k) { static $x = init(); $x += $k; return $x; }
function lazy2() { static $y = [init()]; $y[] = 1; return count($y); }
function retref() { static $r = 10; $q = &$r; $q++; return $r; }
function rebind() { static $s = 5; $o = 100; $s = &$o; $s++; return $s; }
function twice() { static $t = 1; $t++; static $u = 10; $u += $t; return "$t/$u"; }
function prior($x) { $p = 7; static $p; $p = ($p ?? 0) + $x; return $p; }
function strprior($x) { $p = "str$x"; static $p; $p = ($p ?? 0) + $x; return $p; }
function unsetit() { static $w = 0; $w++; $r = $w; unset($w); return "$r:" . ($w ?? "null"); }
function bt() { static $b = 0; $b++; return debug_backtrace()[0]['function'] . $b; }
function vars($a) { static $z = 3; $z++; return implode(',', array_keys(get_defined_vars())) . "=$z"; }
function many($a, $b) { static $sum = 0; $sum += $a * $b; return $sum; }
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "D{$this->n} destroyed\n"; } }
function holder($n) { static $d = null; $old = $d; $d = new D($n); return $old ? $old->n : 0; }

class A {
    function m() { static $c = 0; $c++; return static::class . $c; }
    static function s() { static $c = 100; $c++; return $c; }
}
class B extends A {}
class C extends A { function m() { static $c = 50; $c++; return "C$c"; } }
trait T { function tm() { static $tc = 0; $tc++; return get_class($this) . $tc; } }
class U1 { use T; }
class U2 { use T; }

for ($i = 0; $i < 3; $i++) { echo counter(), " "; }
echo "\n";
echo rec(3), " ", rec(2), "\n";
for ($i = 0; $i < 3; $i++) { echo arr($i), " "; }
echo "\n";
echo arrcopy(), " ", arrcopy(), "\n";
echo lazy(1), " ", lazy(2), " ", lazy(3), "\n";
echo lazy2(), " ", lazy2(), "\n";
echo retref(), " ", retref(), "\n";
echo rebind(), " ", rebind(), "\n";
echo twice(), " ", twice(), " ", twice(), "\n";
echo prior(1), " ", prior(2), "\n";
echo strprior(1), " ", strprior(2), "\n";
echo unsetit(), " ", unsetit(), "\n";
echo bt(), " ", bt(), "\n";
echo vars(1), " ", vars(2), "\n";
$s = 0;
for ($i = 0; $i < 100; $i++) { $s = many($i, 2); }
echo $s, "\n";
$a = new A; $b = new B; $c = new C;
for ($i = 0; $i < 2; $i++) { echo $a->m(), " ", $b->m(), " ", $c->m(), " ", A::s(), " ", B::s(), "\n"; }
$u1 = new U1; $u2 = new U2;
for ($i = 0; $i < 2; $i++) { echo $u1->tm(), " ", $u2->tm(), "\n"; }
echo holder(1), " ", holder(2), " ", holder(3), "\n";
echo "after holders\n";

$rf = new ReflectionFunction('counter');
var_dump($rf->getStaticVariables());
$rf = new ReflectionFunction('lazy');
var_dump($rf->getStaticVariables());
$rm = new ReflectionMethod('A', 'm');
var_dump($rm->getStaticVariables());

$mk = function () { return function () { static $cc = 0; $cc++; return $cc; }; };
$c1 = $mk(); $c2 = $mk();
echo $c1(), $c1(), $c2(), $c1(), $c2(), "\n";
$c3 = $c1->bindTo(null);
echo $c3(), $c1(), $c3(), "\n";
$fn = function ($x) { static $acc = []; $acc[] = $x; return count($acc); };
for ($i = 0; $i < 3; $i++) { echo $fn($i), " "; }
echo "\n";
function gen() { static $g = 0; $g++; yield $g; }
foreach (gen() as $v) { echo $v, " "; }
foreach (gen() as $v) { echo $v, "\n"; }
echo counter(), "\n";
echo "end\n";
--EXPECT--
1 2 3 
4 7
3 4 5 
22 32
init ran
41 43 46
init ran
2 3
11 12
101 101
2/12 3/15 4/19
1 3
1 3
1:null 2:null
bt1 bt2
a,z=4 a,z=5
9900
A1 B2 C51 101 102
A3 B4 C52 103 104
U11 U21
U12 U22
0 D1 destroyed
1 D2 destroyed
2
after holders
array(1) {
  ["n"]=>
  int(3)
}
array(1) {
  ["x"]=>
  int(46)
}
array(1) {
  ["c"]=>
  int(4)
}
12132
445
1 2 3 
1 2
4
end
D3 destroyed
