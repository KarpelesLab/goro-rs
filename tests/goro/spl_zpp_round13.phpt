--TEST--
SPL zpp: Z_PARAM_CLASS iterator classes are converted to strings first; SplFileObject::__toString() arity; SplDoublyLinkedList index float-strings; LIFO key of an empty list from Reflection
--FILE--
<?php
$ao = new ArrayObject([1]);
foreach ([new stdClass, [1], null, 5, 1.5, true, "ArrayIterator", "RecursiveArrayIterator", "nope", new class { function __toString(): string { return "ArrayIterator"; } }] as $c) {
  try { var_dump($ao->setIteratorClass($c), $ao->getIteratorClass()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
foreach ([new stdClass, [1], 5] as $c) {
  try { var_dump(new ArrayObject([], 0, $c)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
$t = new SplTempFileObject();
$t->fwrite("line1\nline2\n"); $t->rewind();
try { var_dump($t->__toString(1)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump($t->__toString(), (string)$t);
$d = new SplDoublyLinkedList; $d->push('a'); $d->push('b');
foreach (["1.5", "1e400", "inf", "1 ", " 1", "1abc", "0x1", "1_0", 1e300, -0.0] as $i) {
  try { var_dump($d->offsetGet($i)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
$o = (new ReflectionClass('SplStack'))->newInstance(); $o->rewind(); var_dump($o->key());
try { var_dump(new ArrayObject([], 0, 5)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_class_vars(5)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_class_vars([])); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_class_vars(new stdClass)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_dump(get_class_vars(null)); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECTF--
Error: Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line 4
TypeError: ArrayObject::setIteratorClass(): Argument #1 ($iteratorClass) must be a class name derived from ArrayIterator, Array given
TypeError: ArrayObject::setIteratorClass(): Argument #1 ($iteratorClass) must be a class name derived from ArrayIterator,  given
TypeError: ArrayObject::setIteratorClass(): Argument #1 ($iteratorClass) must be a class name derived from ArrayIterator, 5 given
TypeError: ArrayObject::setIteratorClass(): Argument #1 ($iteratorClass) must be a class name derived from ArrayIterator, 1.5 given
TypeError: ArrayObject::setIteratorClass(): Argument #1 ($iteratorClass) must be a class name derived from ArrayIterator, 1 given
NULL
string(13) "ArrayIterator"
NULL
string(22) "RecursiveArrayIterator"
TypeError: ArrayObject::setIteratorClass(): Argument #1 ($iteratorClass) must be a class name derived from ArrayIterator, nope given
NULL
string(13) "ArrayIterator"
Error: Object of class stdClass could not be converted to string

Warning: Array to string conversion in %s on line 7
TypeError: ArrayObject::__construct(): Argument #3 ($iteratorClass) must be a class name derived from ArrayIterator, Array given
TypeError: ArrayObject::__construct(): Argument #3 ($iteratorClass) must be a class name derived from ArrayIterator, 5 given
ArgumentCountError: SplFileObject::__toString() expects exactly 0 arguments, 1 given
string(6) "line1
"
string(6) "line1
"

Deprecated: Implicit conversion from float-string "1.5" to int loses precision in %s on line 15
string(1) "b"
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
string(1) "b"
string(1) "b"
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, string given
TypeError: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) must be of type int, float given
string(1) "a"
int(-1)
TypeError: ArrayObject::__construct(): Argument #3 ($iteratorClass) must be a class name derived from ArrayIterator, 5 given
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, 5 given

Warning: Array to string conversion in %s on line 20
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, Array given
Error: Object of class stdClass could not be converted to string
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name,  given
