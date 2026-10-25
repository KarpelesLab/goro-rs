--TEST--
Static methods called without a frame of their own: late static binding, scope, backtraces
--FILE--
<?php
class A {
    public static $n = 0;
    private static function priv($x) { return $x * 3; }
    static function s($k) { return $k + 1; }
    static function who() { return static::class . "/" . self::class . "/" . get_called_class(); }
    static function cnt($x) { static::$n++; return self::priv($x); }
    static function bt() { return debug_backtrace()[0]['class'] . debug_backtrace()[0]['type'] . debug_backtrace()[0]['function']; }
    static function obj($o) { return $o; }
    static function thr() { return str_repeat("x", -1); }
}
class B extends A {}
for ($i = 0; $i < 3; $i++) {
    echo A::s($i), " ", B::s($i), " ", A::who(), " ", B::who(), " ", B::cnt(2), " ", A::bt(), "\n";
    $o = new stdClass; var_dump(A::obj($o) === $o);
    try { A::thr(); } catch (ValueError $e) { echo $e->getMessage(), " ", $e->getTrace()[0]['class'], $e->getTrace()[0]['type'], "\n"; }
}
echo A::$n, "\n";
--EXPECTF--
1 1 A/A/A B/A/B 6 A::bt
bool(true)
str_repeat(): Argument #2 ($times) must be greater than or equal to 0 
Warning: Undefined array key "class" in %s on line 16

Warning: Undefined array key "type" in %s on line 16

2 2 A/A/A B/A/B 6 A::bt
bool(true)
str_repeat(): Argument #2 ($times) must be greater than or equal to 0 
Warning: Undefined array key "class" in %s on line 16

Warning: Undefined array key "type" in %s on line 16

3 3 A/A/A B/A/B 6 A::bt
bool(true)
str_repeat(): Argument #2 ($times) must be greater than or equal to 0 
Warning: Undefined array key "class" in %s on line 16

Warning: Undefined array key "type" in %s on line 16

3
