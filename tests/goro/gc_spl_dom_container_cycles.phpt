--TEST--
The cycle collector sees through SPL containers, DOM node dynamic properties and DOMXPath PHP callbacks
--FILE--
<?php
class D { public $r; public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
$a = new ArrayObject(); $d = new D('ao'); $d->r = $a; $a['x'] = $d; unset($a, $d);
var_dump(gc_collect_cycles());
$s = new SplObjectStorage; $d = new D('sos'); $d->r = $s; $s[$d] = $s; unset($s, $d);
var_dump(gc_collect_cycles());
$l = new SplDoublyLinkedList; $d = new D('dll'); $d->r = $l; $l->push($d); unset($l, $d);
var_dump(gc_collect_cycles());
$f = new SplFixedArray(1); $d = new D('fixed'); $d->r = $f; $f[0] = $d; unset($f, $d);
var_dump(gc_collect_cycles());
$h = new SplMinHeap; $d = new D('heap'); $d->r = $h; $h->insert($d); unset($h, $d);
var_dump(gc_collect_cycles());
$q = new SplPriorityQueue; $d = new D('pq'); $d->r = $q; $q->insert($d, 1); unset($q, $d);
var_dump(gc_collect_cycles());
$ai = new ArrayIterator([]); $d = new D('ai'); $d->r = $ai; $ai[] = $d; unset($ai, $d);
var_dump(gc_collect_cycles());
$w = new WeakMap; $d = new D('wm'); $w[$d] = $d; unset($d);
var_dump(gc_collect_cycles(), count($w));
$it = new ArrayIterator([1]); $d = new D('lim'); $lim = new LimitIterator($it); $d->r = $lim; $it[0] = $d; unset($it, $d, $lim);
var_dump(gc_collect_cycles());
$doc = new DOMDocument; $doc->loadXML('<r/>'); $d = new D('dom'); $d->r = $doc; $doc->dynprop = $d; unset($doc, $d);
var_dump(gc_collect_cycles());
$x = new DOMXPath(new DOMDocument); $d = new D('xp'); $d->r = $x; $x->registerPhpFunctions(['f' => function() use ($d) {}]); unset($x, $d);
var_dump(gc_collect_cycles());
echo "end\n";
?>
--EXPECTF--
dtor ao
int(3)
dtor sos
int(2)
dtor dll
int(2)
dtor fixed
int(2)
dtor heap
int(2)
dtor pq
int(2)
dtor ai
int(3)
dtor wm
int(1)
int(0)
dtor lim
int(5)

Deprecated: Creation of dynamic property DOMDocument::$dynprop is deprecated in %s on line 21
dtor dom
int(2)
dtor xp
int(3)
end
