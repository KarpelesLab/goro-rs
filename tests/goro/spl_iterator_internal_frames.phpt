--TEST--
Iterator method frames of foreach and the internal iterator functions
--FILE--
<?php
try{foreach(new LimitIterator(new ArrayIterator([1]),5) as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{iterator_to_array(new LimitIterator(new ArrayIterator([1]),5));}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{iterator_count(new LimitIterator(new ArrayIterator([1]),5));}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{$l = new LimitIterator(new ArrayIterator([1]),5); $l->rewind();}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
class It implements Iterator {
    function rewind(): void { throw new Exception("x"); }
    function valid(): bool { return false; }
    function current(): mixed { return 1; }
    function key(): mixed { return 1; }
    function next(): void {}
}
try{foreach(new IteratorIterator(new It) as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{foreach(new It as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{iterator_to_array(new IteratorIterator(new It));}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{foreach(new CachingIterator(new IteratorIterator(new It)) as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
class AI extends ArrayIterator { function current(): mixed { throw new Exception("c"); } }
try{foreach(new AI([1]) as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{iterator_to_array(new AI([1]));}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
try{foreach(new IteratorIterator(new AI([1])) as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
class Agg implements IteratorAggregate { function getIterator(): Iterator { return new LimitIterator(new ArrayIterator([1]), 5); } }
try{foreach(new Agg as $v){}}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
function f($x) { foreach ($x as $v) {} }
try{f(new LimitIterator(new ArrayIterator([1]),5));}catch(Exception $e){echo $e->getTraceAsString(), "\n";}
--EXPECTF--
#0 [internal function]: ArrayIterator->seek(5)
#1 %s(2): LimitIterator->rewind()
#2 {main}
#0 [internal function]: ArrayIterator->seek(5)
#1 [internal function]: LimitIterator->rewind()
#2 %s(3): iterator_to_array(Object(LimitIterator))
#3 {main}
#0 [internal function]: ArrayIterator->seek(5)
#1 [internal function]: LimitIterator->rewind()
#2 %s(4): iterator_count(Object(LimitIterator))
#3 {main}
#0 [internal function]: ArrayIterator->seek(5)
#1 %s(5): LimitIterator->rewind()
#2 {main}
#0 [internal function]: It->rewind()
#1 %s(13): IteratorIterator->rewind()
#2 {main}
#0 %s(14): It->rewind()
#1 {main}
#0 [internal function]: It->rewind()
#1 [internal function]: IteratorIterator->rewind()
#2 %s(15): iterator_to_array(Object(IteratorIterator))
#3 {main}
#0 [internal function]: It->rewind()
#1 [internal function]: IteratorIterator->rewind()
#2 %s(16): CachingIterator->rewind()
#3 {main}
#0 %s(18): AI->current()
#1 {main}
#0 [internal function]: AI->current()
#1 %s(19): iterator_to_array(Object(AI))
#2 {main}
#0 [internal function]: AI->current()
#1 %s(20): IteratorIterator->rewind()
#2 {main}
#0 [internal function]: ArrayIterator->seek(5)
#1 %s(22): LimitIterator->rewind()
#2 {main}
#0 [internal function]: ArrayIterator->seek(5)
#1 %s(23): LimitIterator->rewind()
#2 %s(24): f(Object(LimitIterator))
#3 {main}
