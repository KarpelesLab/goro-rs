--TEST--
Property access caches: classes, scopes, slots, references and values a site meets
--FILE--
<?php
class A {
    public $x = 1;
    protected $p = 'pa';
    private $q = 'qa';
    public $arr = [1, 2];
    public $o;
    function getP() { return $this->p; }
    function getQ() { return $this->q; }
    function setQ($v) { $this->q = $v; }
    function issQ() { return isset($this->q); }
}
class B extends A {
    private $q = 'qb';
    function getQB() { return $this->q; }
    function getP() { return $this->p . '!'; }
}
class C {
    public $x = 'cx';
    public $y = 2;
    function __get($n) { return "magic $n"; }
    function __isset($n) { echo "isset($n) "; return true; }
}
function rx($o) { return $o->x; }
function wx($o, $v) { $o->x = $v; }
function ix($o) { return isset($o->x) ? 'set' : 'unset'; }

// one site, several classes
$objs = [new A, new B, new C, new stdClass, new A];
$objs[3]->x = 'std';
foreach ([1, 2] as $round) {
    foreach ($objs as $o) {
        echo get_class($o), ': ', var_export(rx($o), true), ' ', ix($o), "\n";
    }
}
// writes of all kinds through one site
$a = new A;
$seen = [];
foreach ([5, 'str', 1.5, null, true, [1, 2, 3], new stdClass, 'again', [4]] as $v) {
    wx($a, $v);
    $seen[] = json_encode(rx($a));
}
echo implode(' ', $seen), "\n";
// a copied array stays apart
$arr = [1, 2];
wx($a, $arr);
$arr[] = 3;
echo json_encode($a->x), json_encode($arr), "\n";
// unset, then dynamic re-creation in another slot
$a = new A;
for ($i = 0; $i < 3; $i++) {
    echo var_export(@rx($a), true), ' ', ix($a), "\n";
    if ($i == 0) {
        unset($a->x);
    } elseif ($i == 1) {
        $a->x = 'back';
    }
}
// uninitialized -> magic
$c = new C;
echo rx($c), ' ', ix($c), "\n";
unset($c->x);
echo rx($c), ' ', ix($c), "\n";
// private / protected through scopes
$b = new B;
foreach ([1, 2] as $r) {
    echo $b->getP(), ' ', $b->getQ(), ' ', $b->getQB(), ' ', var_export($b->issQ(), true), "\n";
    $b->setQ('new' . $r);
}
$a = new A;
echo $a->getP(), ' ', $a->getQ(), "\n";
try {
    echo $a->p;
} catch (Error $e) {
    echo get_class($e), ': ', $e->getMessage(), "\n";
}
// references held by properties
$a = new A;
$r = &$a->x;
$r = 'by ref';
echo rx($a), ' ', ix($a), "\n";
wx($a, 'via prop');
echo $r, "\n";
$r = null;
echo ix($a), "\n";
unset($r);
// globals in functions (CVs bound to references)
$g = new A;
function useg() {
    global $g;
    $g->x = $g->x + 1;
    return isset($g->x) ? $g->x : 'none';
}
echo useg(), ' ', useg(), "\n";
// chains through temporaries
$o = new A;
$o->o = new A;
$o->o->o = new C;
for ($i = 0; $i < 3; $i++) {
    echo $o->o->x, ' ', $o->o->o->y, ' ', isset($o->o->o->x) ? 'y' : 'n', ' ', isset($o->o->nope) ? 'y' : 'n', "\n";
    $o->o->x++;
}
function mk() { $o = new A; $o->x = 'temp'; return $o; }
for ($i = 0; $i < 2; $i++) {
    echo mk()->x, "\n";
}
// typed and readonly properties keep their checks
class T {
    public int $n = 0;
    public readonly int $ro;
    function __construct() { $this->ro = 1; }
}
$t = new T;
foreach ([1, '2', 'x'] as $v) {
    try {
        $t->n = $v;
        echo $t->n, "\n";
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
}
for ($i = 0; $i < 2; $i++) {
    try {
        $t->ro = 5;
    } catch (Error $e) {
        echo $e->getMessage(), "\n";
    }
}
// destructors of overwritten values
class D { function __destruct() { echo "destroyed\n"; } }
$a = new A;
for ($i = 0; $i < 3; $i++) {
    wx($a, new D);
    echo "set $i\n";
}
wx($a, 0);
echo "done\n";
?>
--EXPECT--
A: 1 set
B: 1 set
C: 'cx' set
stdClass: 'std' set
A: 1 set
A: 1 set
B: 1 set
C: 'cx' set
stdClass: 'std' set
A: 1 set
5 "str" 1.5 null true [1,2,3] {} "again" [4]
[1,2][1,2,3]
1 set
NULL unset
'back' set
cx set
magic x isset(x) set
pa! qa qb true
pa! new1 qb true
pa qa
Error: Cannot access protected property A::$p
by ref set
via prop
unset
2 3
1 2 y n
2 2 y n
3 2 y n
temp
temp
1
2
Cannot assign string to property T::$n of type int
Cannot modify readonly property T::$ro
Cannot modify readonly property T::$ro
set 0
destroyed
set 1
destroyed
set 2
destroyed
done
