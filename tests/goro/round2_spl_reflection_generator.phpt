--TEST--
Rewinding a started generator in foreach, ReflectionProperty::IS_READONLY and getModifierNames(), uncloneable dual iterators, ArrayObject sort callbacks
--FILE--
<?php
function t($l, $f) { try { $r = $f(); echo "$l: "; var_dump($r); } catch (\Throwable $e) { echo "$l: EXC ", get_class($e), ": ", $e->getMessage(), "\n"; } }
function gen() { yield 1; yield 2; yield 3; }
t(3, function () { $g = gen(); $g->next(); foreach ($g as $v) echo $v; });
t(4, function () { $g = gen(); $g->current(); foreach ($g as $v) echo $v; echo "\n"; });
t(5, function () { $g = gen(); foreach ($g as $v) { echo $v; break; } foreach ($g as $v) echo $v; });
class RO { public function __construct(public readonly int $a = 1, protected private(set) int $b = 2, public protected(set) readonly array $c = []) {} }
t(6, fn() => [ReflectionProperty::IS_READONLY, ReflectionClass::IS_READONLY, (new ReflectionProperty('RO', 'a'))->getModifiers(), (new ReflectionProperty('RO', 'c'))->getModifiers()]);
t(7, fn() => [Reflection::getModifierNames((new ReflectionProperty('RO', 'a'))->getModifiers()), Reflection::getModifierNames((new ReflectionProperty('RO', 'b'))->getModifiers()), Reflection::getModifierNames((new ReflectionProperty('RO', 'c'))->getModifiers()), Reflection::getModifierNames(0x40 | 0x200 | 1), Reflection::getModifierNames(0x10000 | 0x20)]);
t(8, fn() => clone new LimitIterator(new ArrayIterator([1])));
t(9, fn() => clone new CachingIterator(new ArrayIterator([1])));
t(10, fn() => clone new RecursiveIteratorIterator(new RecursiveArrayIterator([1])));
t(11, fn() => get_class(clone new ArrayIterator([1])));
t(12, fn() => clone new AppendIterator());
t(13, fn() => (new ArrayObject([1]))->uasort('nope'));
t(14, fn() => (new ArrayObject([1]))->uksort(123));
t(15, fn() => (new ArrayIterator([2, 1]))->uasort(fn($a, $b) => $a <=> $b));
t(16, fn() => get_class(clone new SplObjectStorage));
class MyLimit extends LimitIterator {}
t(17, fn() => clone new MyLimit(new ArrayIterator([1])));
--EXPECT--
3: EXC Exception: Cannot rewind a generator that was already run
123
4: NULL
11235: NULL
6: array(4) {
  [0]=>
  int(128)
  [1]=>
  int(65536)
  [2]=>
  int(2177)
  [3]=>
  int(2177)
}
7: array(5) {
  [0]=>
  array(3) {
    [0]=>
    string(6) "public"
    [1]=>
    string(14) "protected(set)"
    [2]=>
    string(8) "readonly"
  }
  [1]=>
  array(3) {
    [0]=>
    string(5) "final"
    [1]=>
    string(9) "protected"
    [2]=>
    string(12) "private(set)"
  }
  [2]=>
  array(3) {
    [0]=>
    string(6) "public"
    [1]=>
    string(14) "protected(set)"
    [2]=>
    string(8) "readonly"
  }
  [3]=>
  array(3) {
    [0]=>
    string(8) "abstract"
    [1]=>
    string(7) "virtual"
    [2]=>
    string(6) "public"
  }
  [4]=>
  array(2) {
    [0]=>
    string(5) "final"
    [1]=>
    string(8) "readonly"
  }
}
8: EXC Error: Trying to clone an uncloneable object of class LimitIterator
9: EXC Error: Trying to clone an uncloneable object of class CachingIterator
10: EXC Error: Trying to clone an uncloneable object of class RecursiveIteratorIterator
11: string(13) "ArrayIterator"
12: EXC Error: Trying to clone an uncloneable object of class AppendIterator
13: EXC TypeError: uasort(): Argument #2 ($callback) must be a valid callback, function "nope" not found or invalid function name
14: EXC TypeError: uksort(): Argument #2 ($callback) must be a valid callback, no array or string given
15: bool(true)
16: string(16) "SplObjectStorage"
17: EXC Error: Trying to clone an uncloneable object of class MyLimit
