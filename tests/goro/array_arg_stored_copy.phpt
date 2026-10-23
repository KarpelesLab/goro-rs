--TEST--
Arrays stored by builtins and SPL containers, and generator parameters, are copies of the argument
--FILE--
<?php
$x=[1]; $a=[]; array_push($a,$x); $x[0]=9; echo json_encode($a), "\n";
$x=[1]; $a=[]; array_push($a,$x); $x[]=9; echo json_encode($a), "\n";
$x=[1]; $a=[]; array_unshift($a,$x); $x[0]=9; echo json_encode($a), "\n";
$x=[1]; $a=array_fill_keys(['k'], $x); $x[0]=9; echo json_encode($a), "\n";
$x=[1]; $a=array_fill(0, 1, $x); $x[0]=9; echo json_encode($a), "\n";
$x=[1]; $a=array_pad([], 1, $x); $x[0]=9; echo json_encode($a), "\n";
$x=[1]; $a=[0]; array_splice($a, 0, 0, [$x]); $x[0]=9; echo json_encode($a), "\n";
$x=[1]; $l=new SplDoublyLinkedList; $l->push($x); $x[]=2; echo count($l[0]), "\n";
$x=[1]; $l=new ArrayObject; $l->append($x); $x[]=2; echo count($l[0]), "\n";
$x=[1]; $l=new ArrayObject; $l->offsetSet(0, $x); $x[]=2; echo count($l[0]), "\n";
$x=[1]; $l=new SplObjectStorage; $o=new stdClass; $l[$o] = $x; $x[]=2; echo count($l[$o]), "\n";
$x=[1]; $l=new SplFixedArray(1); $l->offsetSet(0, $x); $x[]=2; echo count($l[0]), "\n";
$x=[1]; $l=new SplPriorityQueue; $l->insert($x, 1); $x[]=2; echo count($l->top()), "\n";
$x=[1]; $l=new SplMinHeap; $l->insert($x); $x[]=2; echo count($l->top()), "\n";
$x=[1]; $l=new SplQueue; $l->enqueue($x); $x[]=2; echo count($l[0]), "\n";
$x=[1]; $o=new stdClass; $o->p = $x; $x[]=2; echo count($o->p), "\n";
function keep($v) { static $s; $s = $v; return $s; }
$x=[1]; $k = keep($x); $x[]=2; echo count($k), "\n";
$x=[1]; $c = call_user_func(fn($v) => $v, $x); $x[]=2; echo count($c), "\n";
$x=[1]; $c = array_map(null, [$x]); $x[]=2; echo count($c[0]), "\n";
$x=[1]; $c = [$x]; $x[]=2; echo count($c[0]), "\n";
$x=[1]; $c = array_merge([], [$x]); $x[]=2; echo count($c[0]), "\n";
$x=[1]; $c = array_combine(['a'], [$x]); $x[]=2; echo count($c['a']), "\n";
$x=[1]; $c = compact('x'); $x[]=2; echo count($c['x']), "\n";
$x=[1]; $c = func(...[$x]); $x[]=2; echo count($c), "\n";
function func($v) { return $v; }
$x=[1]; $g = (function($v) { yield $v; })($x); $x[]=2; echo count($g->current()), "\n";
$x=[1]; $it = new ArrayIterator([$x]); $x[]=2; echo count($it[0]), "\n";
$x=[1]; $m = new SplObjectStorage; $o = new stdClass; $m->attach($o, $x); $x[]=2; echo count($m[$o]), "\n";
$x=[1]; $w = new WeakMap; $o = new stdClass; $w[$o] = $x; $x[]=2; echo count($w[$o]), "\n";
$x=[1]; $c = iterator_to_array(new ArrayIterator(['a' => $x])); $x[]=2; echo count($c['a']), "\n";
$x=[1]; $c = array_values([$x]); $x[]=2; echo count($c[0]), "\n";
$x=[1]; $c = array_reverse([$x]); $x[]=2; echo count($c[0]), "\n";
$x=[1]; define('CX', $x); $x[]=2; echo count(CX), "\n";
--EXPECTF--
[[1]]
[[1]]
[[1]]
{"k":[1]}
[[1]]
[[1]]
[[1],0]
1
1
1
1
1
1
1
1
1
1
1
1
1
1
1
1
1
1
1

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 30
1
1
1
1
1
1
