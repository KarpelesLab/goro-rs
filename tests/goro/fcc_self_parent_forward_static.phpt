--TEST--
self::m(...) / parent::m(...) of a static method forward the called scope
--FILE--
<?php
class C {
    private static function era() { return static::class; }
    public static function era2() { return static::class; }
    protected static function era3() { return static::class; }
    public function inst() { return [(self::era(...))(), (self::era2(...))(), (static::era2(...))()]; }
    public static function f() { return [(self::era(...))(), (self::era2(...))(), (static::era2(...))(), (C::era2(...))(), (self::era3(...))()]; }
    public static function g() { $c = self::era(...); var_dump($c); $r = new ReflectionFunction($c); var_dump($r->getName(), $r->getClosureScopeClass()->name, $r->getClosureCalledClass()->name); return $c; }
}
class D extends C {
    public static function h() { return [(parent::era2(...))(), (parent::era3(...))()]; }
}
class E extends D {}
echo json_encode(D::f()), "\n";
echo json_encode(E::f()), "\n";
echo json_encode(E::h()), "\n";
echo json_encode((new D)->inst()), "\n";
$c = D::g();
echo $c(), "\n";
$c2 = Closure::bind(fn() => self::era(...), null, C::class);
echo $c2()(), "\n";
var_dump(D::g() == C::g());
class C2 {
    private static function era() { return static::class; }
    public static function era2() { return static::class; }
    public static function f() { return [(self::era(...))(), (self::era2(...))(), (static::era2(...))(), (C::era2(...))()]; }
}
class D2 extends C2 {}
echo json_encode(D2::f()), "\n";

?>
--EXPECT--
["D","D","D","C","D"]
["E","E","E","C","E"]
["E","E"]
["D","D","D"]
object(Closure)#1 (1) {
  ["function"]=>
  string(6) "C::era"
}
string(3) "era"
string(1) "C"
string(1) "D"
D
C
object(Closure)#2 (1) {
  ["function"]=>
  string(6) "C::era"
}
string(3) "era"
string(1) "C"
string(1) "D"
object(Closure)#4 (1) {
  ["function"]=>
  string(6) "C::era"
}
string(3) "era"
string(1) "C"
string(1) "C"
bool(false)
["D2","D2","D2","C"]
