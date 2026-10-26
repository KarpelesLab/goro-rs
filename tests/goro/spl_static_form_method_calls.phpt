--TEST--
SPL methods called as parent::/self::/static::/Class::m() from a subclass: $this, arguments, deprecations
--FILE--
<?php
class OS extends SplObjectStorage {
    function f() {
        $o = new stdClass;
        SplObjectStorage::offsetSet($o, 'i');
        var_dump(SplObjectStorage::offsetGet($o));
        var_dump(self::count());
        var_dump(static::contains($o));
        SplObjectStorage::detach($o);
        var_dump(count($this));
    }
    function a($o) { parent::attach($o); self::attach($o); static::attach($o); SplObjectStorage::attach($o); }
}
(new OS)->f();
(new OS)->a(new stdClass);
class AO extends ArrayObject {
    function f() { ArrayObject::offsetSet('a', 1); var_dump(parent::offsetGet('a'), self::count()); }
}
(new AO)->f();
class OS2 extends SplObjectStorage {
    function offsetGet($o): mixed { return "user"; }
    function f() {
        $o = new stdClass;
        try { SplObjectStorage::offsetGet($o); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; }
        parent::offsetSet(object: $o, info: 5);
        var_dump(parent::offsetGet($o), $this->offsetGet($o), self::offsetGet($o));
        try { parent::offsetSet(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
        try { parent::offsetSet(bad: 1); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    }
}
(new OS2)->f();
class AO2 extends ArrayObject {
    function f() { parent::offsetSet(key: 'a', value: 1); var_dump(parent::getArrayCopy());
      try { ArrayObject::offsetGet(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
}
(new AO2)->f();
class FA extends SplFixedArray { function f() { parent::setSize(2); parent::offsetSet(0, 'x'); var_dump(self::toArray()); } }
(new FA)->f();
--EXPECTF--
string(1) "i"
int(1)

Deprecated: Method SplObjectStorage::contains() is deprecated since 8.5, use method SplObjectStorage::offsetExists() instead in %s on line 8
bool(true)

Deprecated: Method SplObjectStorage::detach() is deprecated since 8.5, use method SplObjectStorage::offsetUnset() instead in %s on line 9
int(0)

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 12

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 12

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 12

Deprecated: Method SplObjectStorage::attach() is deprecated since 8.5, use method SplObjectStorage::offsetSet() instead in %s on line 12
int(1)
int(1)
UnexpectedValueException: Object not found
#0 %s(24): SplObjectStorage->offsetGet(Object(stdClass))
#1 %s(31): OS2->f()
#2 {main}
int(5)
string(4) "user"
string(4) "user"
ArgumentCountError: SplObjectStorage::offsetSet() expects at least 1 argument, 0 given
Error: Unknown named parameter $bad
array(1) {
  ["a"]=>
  int(1)
}
ArgumentCountError: ArrayObject::offsetGet() expects exactly 1 argument, 0 given
array(2) {
  [0]=>
  string(1) "x"
  [1]=>
  NULL
}
