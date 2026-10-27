--TEST--
SPL methods called as callbacks / through Reflection: deprecation, zpp and frame as a direct call; SplHeap compares with zend_compare(); year PHP_INT_MIN
--FILE--
<?php
function t($c) { try { var_dump($c()); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; } }
$s = new SplObjectStorage; $o = new stdClass;
t(fn() => array_map([$s, 'contains'], [5]));
t(fn() => call_user_func([$s, 'contains'], 5));
t(fn() => call_user_func_array([$s, 'offsetExists'], [5]));
$f = new SplFixedArray(2);
t(fn() => call_user_func([$f, 'offsetGet'], 10));
t(fn() => array_map([$f, 'offsetGet'], [10]));
t(fn() => (new ReflectionMethod($f, 'offsetGet'))->invoke($f, 10));
t(fn() => call_user_func([$f, 'setSize'], -1));
t(fn() => call_user_func([$f, 'setSize'], size: 3));
t(fn() => call_user_func([$f, 'getSize']));
$ao = new ArrayObject([3, 1, 2]);
t(fn() => call_user_func([$ao, 'asort'], "x"));
t(fn() => call_user_func([$ao, 'count']));
t(fn() => call_user_func([$ao, 'offsetGet'], 1));
t(fn() => array_map([$ao, 'offsetGet'], [0, 1]));
t(fn() => call_user_func([$ao, 'uasort'], fn($a, $b) => $a <=> $b));
t(fn() => $ao->getArrayCopy());
$h = new SplMinHeap;
t(fn() => usort($x, [$h, 'compare']));
$arr = [3, 1];
t(function () use ($arr, $h) { usort($arr, [$h, 'compare']); return $arr; });
$l = new SplDoublyLinkedList; $l->push(1);
t(fn() => call_user_func([$l, 'offsetGet'], 5));
t(fn() => array_map([$l, 'offsetExists'], [0, 1]));
$m = new MyStorage;
t(fn() => call_user_func([$m, 'contains'], $o));
t(fn() => call_user_func([$m, 'contains2'], $o));
t(fn() => call_user_func('SplObjectStorage::contains', $o));
$s = new SplObjectStorage; $o = new stdClass;
t(fn() => $s->contains($o));
t(fn() => (new ReflectionMethod('SplObjectStorage', 'contains'))->invoke($s, $o));
t(fn() => (new ReflectionMethod('SplObjectStorage', 'attach'))->invokeArgs($s, [$o]));
t(fn() => call_user_func([$s, 'detach'], $o));
t(fn() => array_map([$s, 'contains'], [$o]));
t(fn() => $s->contains(5));
$ao = new ArrayObject([3, 1, 2]);
t(fn() => $ao->asort("x"));
t(fn() => $ao->asort(1.5));
t(fn() => $ao->ksort([]));
t(fn() => $ao->asort(null));
t(fn() => $ao->uasort("x"));
t(fn() => $ao->natsort(1));
$ai = new ArrayIterator([3, 1, 2]);
t(fn() => $ai->asort("x"));
t(fn() => $ai->ksort(1.5));
t(fn() => $ai->asort(SORT_STRING, 5));
$h = new SplMinHeap; $h->insert(3); $h->insert(1);
t(fn() => $h->insert(new stdClass));
$h = new SplMaxHeap; $h->insert(3);
t(fn() => $h->insert(new stdClass));
$h = new SplMinHeap; $h->insert(new stdClass);
t(fn() => $h->insert(2));
$q = new SplPriorityQueue; $q->insert('a', 3);
t(fn() => $q->insert('b', new stdClass));
t(fn() => $q->insert('c', [1]));
t(fn() => $q->insert('d', "x"));
var_dump(count($q));
$d = new DateTimeImmutable("2020-01-01"); var_dump($d->setDate(PHP_INT_MIN, 2, 29)->format("Y x X"));
--EXPECTF--

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 4
TypeError: SplObjectStorage::contains(): Argument #1 ($object) must be of type object, int given
#0 [internal function]: SplObjectStorage->contains(5)
#1 %s(4): array_map(Array, Array)
#2 %s(2): {closure:%s:4}()
#3 %s(4): t(Object(Closure))
#4 {main}

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 5
TypeError: SplObjectStorage::contains(): Argument #1 ($object) must be of type object, int given
#0 %s(5): SplObjectStorage->contains(5)
#1 %s(2): {closure:%s:5}()
#2 %s(5): t(Object(Closure))
#3 {main}
TypeError: SplObjectStorage::offsetExists(): Argument #1 ($object) must be of type object, int given
#0 %s(6): SplObjectStorage->offsetExists(5)
#1 %s(2): {closure:%s:6}()
#2 %s(6): t(Object(Closure))
#3 {main}
OutOfBoundsException: Index invalid or out of range
#0 %s(8): SplFixedArray->offsetGet(10)
#1 %s(2): {closure:%s:8}()
#2 %s(8): t(Object(Closure))
#3 {main}
OutOfBoundsException: Index invalid or out of range
#0 [internal function]: SplFixedArray->offsetGet(10)
#1 %s(9): array_map(Array, Array)
#2 %s(2): {closure:%s:9}()
#3 %s(9): t(Object(Closure))
#4 {main}
OutOfBoundsException: Index invalid or out of range
#0 [internal function]: SplFixedArray->offsetGet(10)
#1 %s(10): ReflectionMethod->invoke(Object(SplFixedArray), 10)
#2 %s(2): {closure:%s:10}()
#3 %s(10): t(Object(Closure))
#4 {main}
ValueError: SplFixedArray::setSize(): Argument #1 ($size) must be greater than or equal to 0
#0 %s(11): SplFixedArray->setSize(-1)
#1 %s(2): {closure:%s:11}()
#2 %s(11): t(Object(Closure))
#3 {main}
bool(true)
int(3)
TypeError: ArrayObject::asort(): Argument #1 ($flags) must be of type int, string given
#0 %s(15): ArrayObject->asort('x')
#1 %s(2): {closure:%s:15}()
#2 %s(15): t(Object(Closure))
#3 {main}
int(3)
int(1)
array(2) {
  [0]=>
  int(3)
  [1]=>
  int(1)
}
bool(true)
array(3) {
  [1]=>
  int(1)
  [2]=>
  int(2)
  [0]=>
  int(3)
}
TypeError: usort(): Argument #1 ($array) must be of type array, null given
#0 %s(22): usort(NULL, Array)
#1 %s(2): {closure:%s:22}()
#2 %s(22): t(Object(Closure))
#3 {main}
TypeError: usort(): Argument #2 ($callback) must be a valid callback, cannot access protected method SplMinHeap::compare()
#0 %s(24): usort(Array, Array)
#1 %s(2): {closure:%s:24}()
#2 %s(24): t(Object(Closure))
#3 {main}
OutOfRangeException: SplDoublyLinkedList::offsetGet(): Argument #1 ($index) is out of range
#0 %s(26): SplDoublyLinkedList->offsetGet(5)
#1 %s(2): {closure:%s:26}()
#2 %s(26): t(Object(Closure))
#3 {main}
array(2) {
  [0]=>
  bool(true)
  [1]=>
  bool(false)
}

Fatal error: Uncaught Error: Class "MyStorage" not found in %s:28
Stack trace:
#0 {main}
  thrown in %s on line 28
