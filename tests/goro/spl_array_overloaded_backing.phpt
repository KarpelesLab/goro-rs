--TEST--
SPL: WeakMap and other objects with a get_properties_for handler cannot back an ArrayObject
--FILE--
<?php
enum E { case A; }
$cands = ['closure' => fn() => 1, 'enum' => E::A, 'ao' => new ArrayObject([1]), 'sos' => new SplObjectStorage, 'dt' => new DateTime('2020-01-01'), 'weakmap' => new WeakMap, 'fiber' => new Fiber(fn() => 1)];
foreach ($cands as $k => $c) {
    echo "$k: ";
    try { $a = @new ArrayObject($c); echo count($a), "\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    echo "  it: ";
    try { $a = @new ArrayIterator($c); echo count($a), "\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
?>
--EXPECT--
closure: 0
  it: 0
enum: InvalidArgumentException: Enums are not compatible with ArrayObject
  it: InvalidArgumentException: Enums are not compatible with ArrayIterator
ao: 1
  it: 1
sos: 0
  it: 0
dt: InvalidArgumentException: Overloaded object of type DateTime is not compatible with ArrayObject
  it: InvalidArgumentException: Overloaded object of type DateTime is not compatible with ArrayIterator
weakmap: InvalidArgumentException: Overloaded object of type WeakMap is not compatible with ArrayObject
  it: InvalidArgumentException: Overloaded object of type WeakMap is not compatible with ArrayIterator
fiber: 0
  it: 0
