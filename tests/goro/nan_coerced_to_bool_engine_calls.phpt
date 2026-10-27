--TEST--
zend_is_true() of NAN results of valid()/accept()/hasChildren()/__isset()/offsetExists()/assertions warns; ini_set() value to string too
--FILE--
<?php
class It2 implements Iterator { private $i = 0; function current(): mixed { return 1; } function key(): mixed { return 0; } function next(): void { $this->i++; } function rewind(): void { $this->i = 0; } #[\ReturnTypeWillChange] function valid() { return $this->i < 1 ? NAN : 0; } }
echo "foreach\n"; foreach (new It2 as $v) echo $v, "\n";
echo "iterator_to_array\n"; var_dump(iterator_to_array(new It2));
echo "yield from\n"; function g() { yield from new It2; } foreach (g() as $v) echo $v, "\n";
class M { function __isset($n) { return NAN; } function __get($n) { return 5; } }
echo "isset\n"; $m = new M; var_dump(isset($m->x), empty($m->x), $m->x ?? 'd');
class AB implements ArrayAccess { #[\ReturnTypeWillChange] function offsetExists($o) { return NAN; } function offsetGet($o): mixed { return 1; } function offsetSet($o, $v): void {} function offsetUnset($o): void {} }
echo "ab\n"; $b = new AB; var_dump(isset($b[0]), empty($b[0]), $b[0] ?? 'd');
echo "filter\n"; class FI extends FilterIterator { #[\ReturnTypeWillChange] function accept() { return NAN; } }
foreach (new FI(new ArrayIterator([1])) as $v) echo $v, "\n";
echo "cfi\n"; foreach (new CallbackFilterIterator(new ArrayIterator([1]), fn() => NAN) as $v) echo $v, "\n";
echo "rii\n"; class RI extends RecursiveArrayIterator { #[\ReturnTypeWillChange] function hasChildren() { return NAN; } }
try { foreach (new RecursiveIteratorIterator(new RI([1])) as $v) { echo gettype($v), "\n"; } } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
echo "ini_set\n"; var_dump(ini_set('nonexistent.entry', NAN));
echo "assert\n"; $f = 'assert'; var_dump($f(NAN, null));
--EXPECTF--
foreach

Warning: unexpected NAN value was coerced to bool in %s on line 3
1
iterator_to_array

Warning: unexpected NAN value was coerced to bool in %s on line 4
array(1) {
  [0]=>
  int(1)
}
yield from

Warning: unexpected NAN value was coerced to bool in %s on line 5
1
isset

Warning: unexpected NAN value was coerced to bool in %s on line 7

Warning: unexpected NAN value was coerced to bool in %s on line 7

Warning: unexpected NAN value was coerced to bool in %s on line 7
bool(true)
bool(false)
int(5)
ab

Warning: unexpected NAN value was coerced to bool in %s on line 9

Warning: unexpected NAN value was coerced to bool in %s on line 9

Warning: unexpected NAN value was coerced to bool in %s on line 9
bool(true)
bool(false)
int(1)
filter

Warning: unexpected NAN value was coerced to bool in %s on line 11
1
cfi

Warning: unexpected NAN value was coerced to bool in %s on line 12
1
rii

Warning: unexpected NAN value was coerced to bool in %s on line 14
ArrayIterator::__construct(): Argument #1 ($array) must be of type array, int given
ini_set

Warning: unexpected NAN value was coerced to string in %s on line 15
bool(false)
assert

Warning: unexpected NAN value was coerced to bool in %s on line 16
bool(true)
