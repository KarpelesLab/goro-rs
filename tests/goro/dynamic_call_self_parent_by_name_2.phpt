--TEST--
Dynamic calls of callables naming self/parent/static: Class not found
--FILE--
<?php
class A { function m() { return "A::m"; } static function s() { return "A::s " . static::class; } }
class B extends A {
  function m() { return "B::m"; }
  static function s() { return "B::s"; }
  function t() {
    foreach ([['parent', 'm'], ['parent', 's'], 'parent::s', 'parent::m', ['self', 'm'], 'self::s', ['static', 's'], 'static::m', ['PARENT', 'm'], [$this, 'parent::m'], ['B', 'parent::s'], ['A', 'm']] as $f) {
      try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    }
  }
  static function st() {
    foreach ([['parent', 's'], 'parent::s', ['self', 's'], 'static::s'] as $f) {
      try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    }
  }
}
(new B)->t();
B::st();
--EXPECT--
Error: Class "parent" not found
Error: Class "parent" not found
Error: Class "parent" not found
Error: Class "parent" not found
Error: Class "self" not found
Error: Class "self" not found
Error: Class "static" not found
Error: Class "static" not found
Error: Class "PARENT" not found
Error: Call to undefined method B::parent::m()
Error: Call to undefined method B::parent::s()
Error: Non-static method A::m() cannot be called statically
Error: Class "parent" not found
Error: Class "parent" not found
Error: Class "self" not found
Error: Class "static" not found
