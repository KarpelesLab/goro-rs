--TEST--
Private and protected members from the declaring scope, stdClass properties, isset() and variadic calls
--FILE--
<?php
class P {
    private $priv = 1;
    protected $prot = [1, 2];
    public $pub = 'p';
    private function pm($x) { return "P::pm($x)"; }
    protected function prm(...$rest) { return 'P::prm(' . implode(',', $rest) . ')'; }
    function run() {
        $r = [];
        for ($i = 0; $i < 3; $i++) {
            $r[] = $this->pm($i);
            $r[] = $this->prm($i, $i + 1);
            $r[] = $this->priv;
            $this->priv = $this->priv + 1;
            $r[] = isset($this->priv) ? 'set' : 'unset';
            $r[] = isset($this->prot[1]) ? 'set' : 'unset';
            $r[] = isset($this->prot[5]) ? 'set' : 'unset';
        }
        return $r;
    }
}
class C extends P {
    private function pm($x) { return "C::pm($x)"; }
    function prm(...$rest) { return 'C::prm(' . count($rest) . ')'; }
    function own() { return $this->pm(1) . ' ' . $this->prm(1, 2, 3); }
}
$c = new C;
print_r($c->run());
echo $c->own(), "\n";
$c->priv = 'dyn';
print_r($c->run());
var_dump($c);
// stdClass dynamic properties
$o = new stdClass;
$o->a = 1;
$o->list = [1, 2];
for ($i = 0; $i < 3; $i++) {
    $o->a = $o->a + 1;
    $o->list = [$i];
    $o->obj = new stdClass;
    $name = 'a';
    $o->$name = $o->$name * 2;
    var_dump(isset($o->a), isset($o->nope), isset($o->list[0]), isset($o->list[3]));
}
var_dump($o);
// variadics
function v($a, ...$rest) { return $a . ':' . implode('|', $rest) . ':' . count(func_get_args()); }
for ($i = 0; $i < 2; $i++) { echo v(1), ' ', v(1, 2), ' ', v(1, 2, 3, 4), "\n"; }
function vt(int ...$n) { return array_sum($n); }
echo vt(1, 2, 3), "\n";
try { echo vt(1, 'x'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
// destructor on overwritten property
class D { function __destruct() { echo "D destroyed\n"; } }
class H { public $d; public $arr = ['a', 'b']; }
$h = new H;
$h->d = new D;
echo "before\n";
$h->d = 5;
echo "after\n";
$h->arr = 'x';
var_dump($h);
// anonymous class extending a class with privates
$a = new class extends P { function go() { return $this->run(); } };
echo count($a->go()), "\n";
// an object overwritten in a property is destroyed at once
class Box2 { public $value; }
class T2 {
    function __destruct() { global $box2; echo "dtor ", spl_object_id($this), "\n"; $box2->value = null; }
}
function t2($box) { var_dump($box->value = new T2); echo "after dump\n"; }
$box2 = new Box2();
$box2->value = new T2;
t2($box2);
t2($box2);
echo "end\n";
--EXPECTF--
Array
(
    [0] => P::pm(0)
    [1] => C::prm(2)
    [2] => 1
    [3] => set
    [4] => set
    [5] => unset
    [6] => P::pm(1)
    [7] => C::prm(2)
    [8] => 2
    [9] => set
    [10] => set
    [11] => unset
    [12] => P::pm(2)
    [13] => C::prm(2)
    [14] => 3
    [15] => set
    [16] => set
    [17] => unset
)
C::pm(1) C::prm(3)

Deprecated: Creation of dynamic property C::$priv is deprecated in %s on line 30
Array
(
    [0] => P::pm(0)
    [1] => C::prm(2)
    [2] => 4
    [3] => set
    [4] => set
    [5] => unset
    [6] => P::pm(1)
    [7] => C::prm(2)
    [8] => 5
    [9] => set
    [10] => set
    [11] => unset
    [12] => P::pm(2)
    [13] => C::prm(2)
    [14] => 6
    [15] => set
    [16] => set
    [17] => unset
)
object(C)#1 (4) {
  ["priv":"P":private]=>
  int(7)
  ["prot":protected]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
  ["pub"]=>
  string(1) "p"
  ["priv"]=>
  string(3) "dyn"
}
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(false)
bool(true)
bool(false)
object(stdClass)#2 (3) {
  ["a"]=>
  int(22)
  ["list"]=>
  array(1) {
    [0]=>
    int(2)
  }
  ["obj"]=>
  object(stdClass)#3 (0) {
  }
}
1::1 1:2:2 1:2|3|4:4
1::1 1:2:2 1:2|3|4:4
6
vt(): Argument #2 must be of type int, string given, called in %s on line 51
before
D destroyed
after
object(H)#5 (2) {
  ["d"]=>
  int(5)
  ["arr"]=>
  string(1) "x"
}
18
dtor 8
object(T2)#9 (0) {
}
dtor 9
after dump
object(T2)#9 (0) {
}
after dump
end
dtor 9
