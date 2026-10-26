--TEST--
Compound assignments to properties and unset() of a property's element take the same paths as before
--FILE--
<?php
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "destroy {$this->n}\n"; } }
class A {
    public $n = 0;
    public $s = 'a';
    private $p = 1;
    protected $arr = ['x' => 1, 'y' => 2, 'z' => [1, 2]];
    public readonly int $ro;
    public int $typed = 1;
    public $o;
    function __construct() { $this->ro = 5; $this->o = new stdClass; }
    function bump($k) {
        for ($i = 0; $i < 3; $i++) {
            $this->n += $i;
            $this->s .= $k;
            $this->p *= 2;
            $this->typed += 1;
        }
        return [$this->n, $this->s, $this->p, $this->typed];
    }
    function drop($k) {
        unset($this->arr[$k]);
        return $this->arr;
    }
    function shared() {
        $copy = $this->arr;
        unset($this->arr['x']);
        return [$copy, $this->arr];
    }
    function ro() {
        try {
            $this->ro += print("rhs ");
        } catch (Error $e) {
            echo get_class($e), ': ', $e->getMessage(), "\n";
        }
    }
    function typed() {
        try {
            $this->typed .= 'x';
        } catch (TypeError $e) {
            echo $e->getMessage(), "\n";
        }
        return $this->typed;
    }
    function obj() {
        try {
            $this->o += 1;
        } catch (TypeError $e) {
            echo $e->getMessage(), "\n";
        }
    }
    function refs() {
        $r = &$this->arr;
        unset($this->arr['y']);
        $r['w'] = 9;
        return $this->arr;
    }
    function dtors() {
        $this->arr = ['a' => new D('a'), 'b' => new D('b')];
        unset($this->arr['a']);
        echo "after unset\n";
        $this->arr = [];
        echo "after reset\n";
    }
    function nonarr() {
        $this->s = 'str';
        try {
            unset($this->s[0]);
        } catch (Error $e) {
            echo $e->getMessage(), "\n";
        }
        $this->n = null;
        unset($this->n['k']);
        var_dump($this->n);
    }
}
$a = new A;
echo json_encode($a->bump('b')), "\n";
echo json_encode($a->bump('c')), "\n";
echo json_encode($a->drop('x')), "\n";
echo json_encode($a->drop('nope')), "\n";
echo json_encode($a->drop('z')), "\n";
$b = new A;
echo json_encode($b->shared()), "\n";
$b->ro();
echo $b->typed(), "\n";
$b->obj();
$c = new A;
echo json_encode($c->refs()), "\n";
$c->dtors();
$c->nonarr();
$o = new stdClass;
$o->cnt = 1;
for ($i = 0; $i < 3; $i++) {
    $o->cnt += 10;
    $o->list = [1, 2, 3];
    unset($o->list[1]);
}
echo json_encode($o), "\n";
?>
--EXPECT--
[3,"abbb",8,4]
[6,"abbbccc",64,7]
{"y":2,"z":[1,2]}
{"y":2,"z":[1,2]}
{"y":2}
[{"x":1,"y":2,"z":[1,2]},{"y":2,"z":[1,2]}]
rhs Error: Cannot modify readonly property A::$ro
Cannot assign string to property A::$typed of type int
1
Unsupported operand types: stdClass + int
{"x":1,"z":[1,2],"w":9}
destroy a
after unset
destroy b
after reset
Cannot unset string offsets
NULL
{"cnt":31,"list":{"0":1,"2":3}}
