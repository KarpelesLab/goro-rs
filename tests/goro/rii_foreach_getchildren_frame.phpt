--TEST--
foreach over RecursiveIteratorIterator: the inner getChildren() call has a frame at the foreach line
--FILE--
<?php
class RI extends RecursiveArrayIterator { function hasChildren(): bool { return true; } }
try {
  foreach (new RecursiveIteratorIterator(new RI([1])) as $v) { echo gettype($v), "\n"; }
} catch (Throwable $e) { echo $e, "\n"; }
class RI2 extends RecursiveArrayIterator { function getChildren(): ?RecursiveArrayIterator { throw new Exception("x"); } }
try {
  foreach (new RecursiveIteratorIterator(new RI2([[1]])) as $v) { echo gettype($v), "\n"; }
} catch (Throwable $e) { echo $e, "\n"; }
try {
  $it = new RecursiveIteratorIterator(new RI([1]));
  $it->rewind();
} catch (Throwable $e) { echo $e, "\n"; }
try {
  $r = new RI([1]);
  $r->getChildren();
} catch (Throwable $e) { echo $e, "\n"; }
try {
  iterator_to_array(new RecursiveIteratorIterator(new RI([1])));
} catch (Throwable $e) { echo $e, "\n"; }
--EXPECTF--
TypeError: ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given in %s:4
Stack trace:
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 %s(4): RecursiveArrayIterator->getChildren()
#2 {main}
Exception: x in %s:6
Stack trace:
#0 %s(8): RI2->getChildren()
#1 {main}
TypeError: ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given in %s:12
Stack trace:
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(12): RecursiveIteratorIterator->rewind()
#3 {main}
TypeError: ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given in %s:16
Stack trace:
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 %s(16): RecursiveArrayIterator->getChildren()
#2 {main}
TypeError: ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given in %s:19
Stack trace:
#0 [internal function]: ArrayIterator->__construct(1, 0)
#1 [internal function]: RecursiveArrayIterator->getChildren()
#2 %s(19): iterator_to_array(Object(RecursiveIteratorIterator))
#3 {main}
