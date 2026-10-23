--TEST--
__call() reached through a static-syntax call runs with $this's class as the called scope
--FILE--
<?php
class A {
    function __call($n, $a) { return "call:" . static::class . ":" . get_class($this) . ":" . get_called_class(); }
    static function __callStatic($n, $a) { return "cs:" . static::class . ":" . get_called_class(); }
    function viaSelf() { return [self::y(), static::w()]; }
}
class B extends A {
    function t() { return [A::x(), parent::z(), self::y(), static::w(), B::v()]; }
    static function st() { return [A::x(), parent::z(), self::y(), static::w()]; }
}
var_dump((new B)->t(), B::st(), (new B)->viaSelf());
--EXPECT--
array(5) {
  [0]=>
  string(10) "call:B:B:B"
  [1]=>
  string(10) "call:B:B:B"
  [2]=>
  string(10) "call:B:B:B"
  [3]=>
  string(10) "call:B:B:B"
  [4]=>
  string(10) "call:B:B:B"
}
array(4) {
  [0]=>
  string(6) "cs:A:A"
  [1]=>
  string(6) "cs:B:B"
  [2]=>
  string(6) "cs:B:B"
  [3]=>
  string(6) "cs:B:B"
}
array(2) {
  [0]=>
  string(10) "call:B:B:B"
  [1]=>
  string(10) "call:B:B:B"
}
