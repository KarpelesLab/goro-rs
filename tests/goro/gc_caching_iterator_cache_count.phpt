--TEST--
gc_collect_cycles() counts CachingIterator's cache array (spl_dual_it_get_gc)
--FILE--
<?php
class D { public $r; public $n; function __construct($n) { $this->n = $n; } function __toString(): string { return "s"; } function __destruct() { echo "dtor {$this->n}\n"; } }
// CachingIterator's zcache is always a (counted) array, even when empty
$d = new D('full'); $c = new CachingIterator(new ArrayIterator([$d]), CachingIterator::FULL_CACHE); foreach ($c as $_); $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
$d = new D('empty'); $c = new CachingIterator(new ArrayIterator([$d]), CachingIterator::FULL_CACHE); $d->r = $c; unset($d, $c);
var_dump(gc_collect_cycles());
$d = new D('tostring'); $c = new CachingIterator(new ArrayIterator([$d, 2])); foreach ($c as $_) break; $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
$d = new D('recursive'); $c = new RecursiveCachingIterator(new RecursiveArrayIterator([[1], [$d]]), 0); foreach ($c as $_) { if ($c->hasChildren()) $c->getChildren(); } $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
$d = new D('ii'); $c = new IteratorIterator(new ArrayIterator([$d])); foreach ($c as $_); $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
?>
--EXPECT--
dtor full
int(6)
dtor empty
int(6)
dtor tostring
int(6)
dtor recursive
int(8)
dtor ii
int(5)
