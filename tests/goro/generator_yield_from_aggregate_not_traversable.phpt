--TEST--
yield from an IteratorAggregate whose getIterator() returns a non-Traversable throws
--FILE--
<?php
class BadAgg implements IteratorAggregate { #[\ReturnTypeWillChange] function getIterator() { return [1, 2]; } }
class Outer implements IteratorAggregate { function getIterator(): Traversable { return new BadAgg; } }
function yf($x) { try { yield from $x; } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } yield 'after'; }
foreach (yf(new BadAgg) as $v) echo $v, "\n";
foreach (yf(new Outer) as $v) echo $v, "\n";
foreach (yf(new ArrayObject([1])) as $v) echo $v, "\n";
foreach (yf(new Outer) as $v) echo $v, "\n";
--EXPECT--
Exception: Objects returned by BadAgg::getIterator() must be traversable or implement interface Iterator
after
Exception: Objects returned by BadAgg::getIterator() must be traversable or implement interface Iterator
after
1
after
Exception: Objects returned by BadAgg::getIterator() must be traversable or implement interface Iterator
after
