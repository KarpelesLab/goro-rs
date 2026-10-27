--TEST--
ArrayIterator of a shared (literal) array keeps a duplicate the collector counts
--FILE--
<?php
class D { public $r; public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
$d = new D('a'); $c = new RecursiveCachingIterator(new RecursiveArrayIterator([[1], [$d]]), 0); foreach ($c as $_) { if ($c->hasChildren()) $c->getChildren(); } $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
$d = new D('b'); $c = new RecursiveCachingIterator(new RecursiveArrayIterator([[$d], [$d]]), 0); foreach ($c as $_) { if ($c->hasChildren()) $c->getChildren(); } $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
$x = 1;
$d = new D('c'); $c = new RecursiveCachingIterator(new RecursiveArrayIterator([[$x], [$d]]), 0); foreach ($c as $_) { if ($c->hasChildren()) $c->getChildren(); } $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
$d = new D('d'); $c = new RecursiveCachingIterator(new RecursiveArrayIterator([[1], [$d]]), 0); foreach ($c as $_) { } $d->r = $c; unset($d, $c, $_);
var_dump(gc_collect_cycles());
--EXPECT--
dtor a
int(8)
dtor b
int(8)
dtor c
int(8)
dtor d
int(8)
