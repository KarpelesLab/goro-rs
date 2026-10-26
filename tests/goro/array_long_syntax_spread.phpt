--TEST--
array(...$x) spreads like [...$x]; getIterator() returning a non-Traversable
--FILE--
<?php
class IA implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator(['a' => 1, 'b' => 2]); } }
var_dump(array(...[1, 2], ...new IA, 3));
var_dump(array(0, ...[], ...(function() { yield 5; })()));
class IA4 implements IteratorAggregate { #[\ReturnTypeWillChange] function getIterator() { return [1,2]; } }
try { iterator_count(new IA4); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { iterator_to_array(new IA4); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump([...new IA4]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
array(5) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(0)
  [1]=>
  int(5)
}
Exception: Objects returned by IA4::getIterator() must be traversable or implement interface Iterator
Exception: Objects returned by IA4::getIterator() must be traversable or implement interface Iterator
Exception: Objects returned by IA4::getIterator() must be traversable or implement interface Iterator
