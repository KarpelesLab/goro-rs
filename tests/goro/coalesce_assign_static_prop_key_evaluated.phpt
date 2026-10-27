--TEST--
self::$r[self::K] ??= v with self::$r unset: the key is still evaluated
--FILE--
<?php
class V {
    const A = 1;
    public static array $r;
    public static $u;
    static function f() { return self::$r[self::A] ??= 'x'; }
    static function g() { return self::$u[self::A] ??= 'y'; }
    static function h() { $k = 3; return self::$r[$k] ??= 'z'; }
}
var_dump(V::f(), V::$r);
var_dump(V::g(), V::$u);
var_dump(V::h(), V::$r);
class X { public static array $r; static function f() { return self::$r[1] ??= 'x'; } }
var_dump(X::f(), X::$r);
class Y { public static array $r; static function f() { self::$r[1] = 'x'; return self::$r; } }
var_dump(Y::f());

?>
--EXPECT--
string(1) "x"
array(1) {
  [1]=>
  string(1) "x"
}
string(1) "y"
array(1) {
  [1]=>
  string(1) "y"
}
string(1) "z"
array(2) {
  [1]=>
  string(1) "x"
  [3]=>
  string(1) "z"
}
string(1) "x"
array(1) {
  [1]=>
  string(1) "x"
}
array(1) {
  [1]=>
  string(1) "x"
}
