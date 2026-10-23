--TEST--
Calls of instance methods run without a frame of their own until they need one: scopes, called class, backtraces, exceptions, destructors
--FILE--
<?php
class A {
    public $p = 1;
    private $priv = 10;
    protected $prot = 20;
    public $arr = [1, 2];
    function get() { return $this->p; }
    function getPriv() { return $this->priv; }
    function getProt() { return $this->prot; }
    function set($v) { $this->p = $v; return $this; }
    function add($a, $b = 3) { return $a + $b + $this->p; }
    function who() { return static::class; }
    function cls() { return get_class($this); }
    function bt() { $x = 1; return debug_backtrace(); }
    function thr($n) { $m = $n * 2; throw new RuntimeException("t$m"); }
    function callThr($n) { $k = $n + 1; return $this->thr($k); }
    function inner() { return $this->get() + $this->getPriv(); }
    function arrmod($a) { $a[] = 3; return count($a); }
    function retThis() { return $this; }
    function constv() { return self::C; }
    const C = 42;
    function undef() { $y = 1; return $zz; }
    function propArr() { return $this->arr[1]; }
    function viaPriv() { return $this->privM(); }
    private function privM() { return 'priv'; }
}
class B extends A {
    function get() { return $this->p * 100; }
    function parentGet() { return parent::get(); }
    function getPrivB() { return isset($this->priv) ? 'set' : 'unset'; }
}
$a = new A;
$b = new B;
echo $a->get(), " ", $b->get(), "\n";
echo $a->getPriv(), " ", $b->getPriv(), " ", $b->getProt(), "\n";
echo $a->set(5)->get(), "\n";
echo $a->add(1), " ", $a->add(1, 1), "\n";
echo $a->who(), " ", $b->who(), " ", $b->cls(), "\n";
$t = $b->bt();
echo $t[0]['function'], " ", $t[0]['class'], " ", $t[0]['type'], " ", get_class($t[0]['object']), " ", $t[0]['line'], "\n";
try { $b->callThr(3); } catch (Exception $e) {
    echo $e->getMessage(), " ", $e->getLine(), "\n";
    foreach ($e->getTrace() as $f) { echo "  ", $f['class'] ?? '', $f['type'] ?? '', $f['function'], "(", json_encode($f['args'] ?? null), ") @", $f['line'], "\n"; }
    echo $e->getTraceAsString(), "\n";
}
echo $a->inner(), " ", $b->inner(), "\n";
$arr = [1, 2];
echo $a->arrmod($arr), " ", count($arr), "\n";
var_dump($a->retThis() === $a);
echo $a->constv(), "\n";
var_dump($a->undef());
echo $a->propArr(), " ", $a->viaPriv(), "\n";
echo $b->parentGet(), " ", $b->getPrivB(), "\n";
$s = 0;
for ($i = 0; $i < 1000; $i++) { $s += $a->add($i); }
echo $s, "\n";
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "D{$this->n} gone\n"; } function v() { return $this->n; } }
function mk() { return new D(7); }
echo mk()->v(), "\n";
echo "after\n";
$d = new D(8);
echo $d->v(), "\n";
unset($d);
echo "after2\n";
class Cnt { public $c = 0; function inc() { $this->c++; return $this->c; } }
$c = new Cnt;
for ($i = 0; $i < 5; $i++) { $c->inc(); }
echo $c->inc(), "\n";
class Chain { private $v = []; function push($x) { $this->v[] = $x; return $this; } function all() { return $this->v; } }
$ch = new Chain;
var_dump($ch->push(1)->push('a')->all());
abstract class Base { abstract function f(); function g() { return $this->f() . "!"; } }
class Impl extends Base { function f() { return "impl"; } }
echo (new Impl)->g(), "\n";
interface I { function h(); }
class J implements I { function h() { return __METHOD__ . " " . __FUNCTION__ . " " . __CLASS__; } }
echo (new J)->h(), "\n";
class K { function m($x) { return $x; } }
$k = new K;
echo $k->m(...[5]), " ", $k->m(x: 6), "\n";
class Rec { function f($n) { return $n <= 0 ? 0 : $n + $this->f($n - 1); } }
echo (new Rec)->f(50), "\n";
echo "done\n";
--EXPECTF--
1 100
10 10 20
5
9 7
A B B
bt A -> B 39
t8 15
  A->thr([4]) @16
  A->callThr([3]) @41
#0 %s(16): A->thr(4)
#1 %s(41): A->callThr(3)
#2 {main}
15 110
3 2
bool(true)
42

Warning: Undefined variable $zz in %s on line 22
NULL
2 priv
1 unset
507500
D7 gone
7
after
8
D8 gone
after2
6
array(2) {
  [0]=>
  int(1)
  [1]=>
  string(1) "a"
}
impl!
J::h h J
5 6
1275
done
