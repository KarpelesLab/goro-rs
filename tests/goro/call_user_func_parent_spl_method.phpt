--TEST--
call_user_func() of parent::/Class:: SPL methods on a subclass object
--FILE--
<?php
class S extends SplObjectStorage {
  function contains($o): bool { echo "child\n"; return false; }
  function count($m = COUNT_NORMAL): int { return 42; }
  function t($o) { return parent::contains($o); }
}
$s = new S; $o = new stdClass; $s->attach($o);
var_dump(call_user_func([$s, 'parent::count']));
var_dump(call_user_func([$s, 'parent::offsetExists'], $o));
var_dump($s->t($o));
var_dump(call_user_func([$s, 'parent::contains'], $o));
var_dump(call_user_func([$s, 'SplObjectStorage::contains'], $o));
--EXPECTF--

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 7

Deprecated: Callables of the form ["S", "parent::count"] are deprecated in %s on line 8
int(1)

Deprecated: Callables of the form ["S", "parent::offsetExists"] are deprecated in %s on line 9
bool(true)

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 5
bool(true)

Deprecated: Callables of the form ["S", "parent::contains"] are deprecated in %s on line 11

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 11
bool(true)

Deprecated: Callables of the form ["S", "SplObjectStorage::contains"] are deprecated in %s on line 12

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 12
bool(true)
