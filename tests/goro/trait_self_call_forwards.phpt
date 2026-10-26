--TEST--
self::m() in a trait method (and its closures) forwards the called class
--FILE--
<?php
trait T {
    private static function priv($x) { return static::class . ":$x"; }
    public static function m() { return self::priv(1); }
    public static function k() { return (function () { return self::priv(2); })(); }
    public static function j() { return (static fn() => self::priv(3))(); }
}
class B { use T; }
class C extends B {}
class D { public static function priv($x) { return static::class . ":$x"; } public static function m() { return (fn() => self::priv(4))(); } }
class E extends D {}
echo C::m(), ' ', C::k(), ' ', C::j(), ' ', E::m(), "\n";

?>
--EXPECT--
C:1 C:2 C:3 E:4
