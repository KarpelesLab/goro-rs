--TEST--
Unpacking a Traversable goes through its get_iterator (overrides, handles, errors)
--FILE--
<?php
class A implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator([1,2]); } }
$x = [...new A];
$a = new stdClass; echo "a ", spl_object_id($a), "\n";
$b = new stdClass; echo "b ", spl_object_id($b), "\n";
$c = new stdClass; echo "c ", spl_object_id($c), "\n";
unset($a, $b, $c);
class I extends ArrayIterator { function current(): mixed { return parent::current() * 10; } }
var_dump([...new I([1, 2])]);
function f(...$a) { return $a; }
var_dump(f(...new I([3, 4])));
function g() { yield 'a' => 1; yield '5' => 2; yield 3; }
var_dump([...g()]);
$d = new stdClass; echo "d ", spl_object_id($d), "\n";
class G implements IteratorAggregate { function getIterator(): Generator { yield 'k' => 'v'; yield 7; } }
var_dump([...new G]);
try { var_dump([...new stdClass]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { $five = 5; var_dump([...$five]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
function kk() { yield 1.5 => 1; }
try { var_dump([...kk()]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
class B implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator(['x' => 1, '01' => 2, '1' => 3]); } }
var_dump([...new B]);
var_dump([...new ArrayObject([1, 'q' => 2])]);
$s = new SplDoublyLinkedList; $s->push(1); $s->push(2);
var_dump([...$s]);
$e = new stdClass; echo "e ", spl_object_id($e), "\n";
var_dump([...new SplFixedArray(2)]);
$h = new stdClass; echo "h ", spl_object_id($h), "\n";
class BadAgg implements IteratorAggregate { function getIterator(): Traversable { throw new Exception("no"); } }
try { var_dump([...new BadAgg]); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
--EXPECT--
a 1
b 3
c 2
array(2) {
  [0]=>
  int(10)
  [1]=>
  int(20)
}
array(2) {
  [0]=>
  int(30)
  [1]=>
  int(40)
}
array(3) {
  ["a"]=>
  int(1)
  [0]=>
  int(2)
  [1]=>
  int(3)
}
d 2
array(2) {
  ["k"]=>
  string(1) "v"
  [0]=>
  int(7)
}
TypeError: Only arrays and Traversables can be unpacked, stdClass given
Error: Only arrays and Traversables can be unpacked, int given
Error: Keys must be of type int|string during array unpacking
array(3) {
  ["x"]=>
  int(1)
  ["01"]=>
  int(2)
  [0]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  ["q"]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
e 4
array(2) {
  [0]=>
  NULL
  [1]=>
  NULL
}
h 5
Exception: no
