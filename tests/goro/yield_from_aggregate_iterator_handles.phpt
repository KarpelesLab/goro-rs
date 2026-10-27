--TEST--
yield from / foreach over IteratorAggregate: iterator handle and release order
--FILE--
<?php
function id($l) { echo $l, ": ", spl_object_id(new stdClass), "\n"; }
class AG implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator([1, 2]); } }
class GA implements IteratorAggregate { function getIterator(): Generator { $x = yield 1; try { yield 2; } catch (Exception $e) { echo "caught in GA\n"; } return 7; } }
class UI implements Iterator { private $i = 0; function current(): mixed { return $this->i; } function key(): mixed { return $this->i; } function next(): void { $this->i++; } function rewind(): void { $this->i = 0; } function valid(): bool { return $this->i < 2; } }
class AU implements IteratorAggregate { function getIterator(): Iterator { return new UI; } }
class AA implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator([1]); } }
class AAA implements IteratorAggregate { function getIterator(): Traversable { return new AA; } }
// The aggregate is released only once the iterator (a handle) exists.
function g1() { yield from new AG; id("g1 after"); }
foreach (g1() as $v) id("g1");
function g3() { yield from new AU; id("g3 after"); }
foreach (g3() as $v) id("g3");
function g5() { yield from new AAA; id("g5 after"); }
foreach (g5() as $v) id("g5");
// A Generator from getIterator() is walked through its iterator, not delegated to.
function g2() { $r = yield from new GA; var_dump($r); id("g2 after"); }
foreach (g2() as $k => $v) { var_dump($k); id("g2"); }
function g6() { try { yield from new GA; } catch (Exception $e) { echo "caught in g6\n"; } yield 3; }
$g = g6(); $g->current(); $g->next(); var_dump($g->throw(new Exception("x"))); unset($g, $e); id("g6");
// An internal aggregate's get_iterator handler makes the one iterator.
function g8() { yield from new DatePeriod(new DateTime('2020-01-01'), new DateInterval('P1D'), 1); id("g8 after"); }
foreach (g8() as $v) id("g8");
// foreach: nested aggregates go after the iterator exists, innermost first.
foreach (new AAA as $v) id("fe-aaa");
id("end");
?>
--EXPECT--
g1: 3
g1: 3
g1 after: 5
g3: 5
g3: 5
g3 after: 3
g5: 3
g5 after: 6
int(0)
g2: 4
int(1)
g2: 4
NULL
g2 after: 3
caught in g6
int(3)
g6: 1
g8: 4
g8: 3
g8 after: 2
fe-aaa: 4
end: 5
