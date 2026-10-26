--TEST--
Fast clone of plain objects and (array) casts
--FILE--
<?php
class P { public $a = 1; public $arr = [1]; public $o; private $priv = 'p'; protected $prot = 'q'; function getPriv() { return $this->priv; } }
class Q extends P { public $more = 'm'; }
class WithClone { public $n = 0; function __clone() { $this->n++; } }
class WithDtor { public $id = 0; function __destruct() { echo "dtor {$this->id}\n"; } }
function cl($o) { $c = clone $o; $c->a = 2; $c->arr[] = 2; return [$o, $c]; }
function cl2($cache, $k) { return clone $cache[$k]; }
function cl3($o) { $c = clone $o; return $c; }
function cl4($o) { $c = clone $o; $c->id = 2; return 0; }
function refprop() { $o = new P; $x = 5; $o->a = &$x; $c = clone $o; $x = 6; return [$o->a, $c->a]; }
function ca($v) { $a = (array) $v; $a[] = 'x'; return [$a, $v]; }
function ca2() { return (array) strtoupper('a'); }
function ca3($v) { return (array) $v; }
for ($i = 0; $i < 40; $i++) {
    $r = [];
    $p = new Q;
    $p->o = new stdClass;
    [$o1, $c1] = cl($p);
    $r[] = [$o1 === $c1, $o1->o === $c1->o, $c1->a, $c1->arr, $o1->arr, $c1->getPriv(), get_class($c1)];
    $std = new stdClass; $std->x = [1]; $std->y = 'z';
    $c2 = cl2(['k' => $std], 'k');
    $c2->x[] = 2;
    $r[] = [$c2, $std];
    $wc = new WithClone;
    $r[] = cl3($wc)->n;
    $r[] = refprop();
    $r[] = ca([1, 2]);
    $r[] = ca(null);
    $r[] = ca("s");
    $r[] = ca(5);
    $r[] = ca(false);
    $r[] = ca(1.5);
    $r[] = ca($std);
    $r[] = ca2();
    $x = [1];
    $y = ca3($x);
    $y[] = 2;
    $r[] = [$x, $y];
}
echo json_encode($r), "\n";
$d = new WithDtor;
$d->id = 1;
cl4($d);
echo "end\n";
--EXPECT--
[[false,true,2,[1,2],[1],"p","Q"],[{"x":[1,2],"y":"z"},{"x":[1],"y":"z"}],1,[6,6],[[1,2,"x"],[1,2]],[["x"],null],[["s","x"],"s"],[[5,"x"],5],[[false,"x"],false],[[1.5,"x"],1.5],[{"x":[1],"y":"z","0":"x"},{"x":[1],"y":"z"}],["A"],[[1],[1,2]]]
dtor 2
end
dtor 1
