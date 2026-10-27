--TEST--
GC closes garbage generators (frame freed, count like Zend); iterator handles of WeakMap, InternalIterator and yield from
--FILE--
<?php
class D { public $o; function __destruct() { echo "dtor\n"; } }
$a = new D; $b = new stdClass; $a->o = $b; $b->a = $a; unset($a, $b);
var_dump(gc_collect_cycles());
$a = new D; $a->o = $a; unset($a);
var_dump(gc_collect_cycles());
class R { public $o; function __destruct() { echo "dtor R\n"; $this->o = null; } }
$a = new R; $b = new stdClass; $c = new stdClass; $a->o = $b; $b->c = $c; $c->a = $a; unset($a, $b, $c);
var_dump(gc_collect_cycles());
function gen() { $x = yield 1; yield 2; }
$g = gen(); $g->current();
$o = new stdClass; $o->g = $g;
$g->send($o);
unset($g, $o);
var_dump(gc_collect_cycles());
function gen2($a) { yield $a; }
$o = new stdClass; $o->g = gen2($o); $o->g->current(); unset($o);
var_dump(gc_collect_cycles());
$o = new stdClass; $o->g = gen2($o); unset($o);
var_dump(gc_collect_cycles());
function gen3() { $self = yield; yield 5; }
$g = gen3(); $g->current(); $g->send($g); unset($g);
var_dump(gc_collect_cycles());
function gen4() { try { $self = yield; yield 5; } finally { echo "fin\n"; } }
$g = gen4(); $g->current(); $g->send($g); unset($g);
var_dump(gc_collect_cycles());
function g1($it) { yield from $it; echo "after yf\n"; $o = new stdClass; echo "in gen: ", spl_object_id($o), "\n"; yield 99; }
foreach (g1(new ArrayIterator([1, 2])) as $v) { $o = new stdClass; echo "$v: ", spl_object_id($o), "\n"; unset($o); }
$f = new SplFixedArray(2);
$it = $f->getIterator(); echo get_class($it), " ", spl_object_id($it), "\n"; unset($it);
$o = new stdClass; echo spl_object_id($o), "\n";
$o2 = new stdClass; echo spl_object_id($o2), "\n";
$wm = new WeakMap; $it = $wm->getIterator(); echo spl_object_id($it), "\n"; unset($it);
$o3 = new stdClass; echo spl_object_id($o3), "\n";
$o4 = new stdClass; echo spl_object_id($o4), "\n";
function g2() { yield from new ArrayObject([5]); }
$g = g2(); foreach ($g as $v) { echo $v, " ", spl_object_id(new stdClass), "\n"; }
unset($g); echo spl_object_id(new stdClass), "\n";
$wm = new WeakMap; $k = new stdClass; $wm[$k] = 1;
var_dump(spl_object_id($wm), spl_object_id($k));
$it = $wm->getIterator();
var_dump(get_class($it), spl_object_id($it));
foreach ($wm as $kk => $v) { var_dump(spl_object_id($kk), $v); }
$n = new stdClass; var_dump(spl_object_id($n));
$it2 = $wm->getIterator(); var_dump(spl_object_id($it2));
function g2b() { yield from new ArrayObject([5]); }
$g = g2b(); foreach ($g as $v) { echo $v, " ", spl_object_id(new stdClass), "\n"; }
unset($g); echo spl_object_id(new stdClass), "\n";
$a = new stdClass; $b = new stdClass; $c = new stdClass; $d = new stdClass;
var_dump(spl_object_id($a), spl_object_id($b), spl_object_id($c), spl_object_id($d));
--EXPECT--
dtor
int(2)
dtor
int(1)
dtor R
int(1)
int(1)
int(2)
int(1)
int(1)
fin
int(1)
1: 5
2: 5
after yf
in gen: 4
99: 5
InternalIterator 4
4
1
6
6
5
5 12
7
int(7)
int(2)
string(16) "InternalIterator"
int(11)
int(2)
int(1)
int(10)
int(12)
5 18
13
int(13)
int(14)
int(17)
int(16)
