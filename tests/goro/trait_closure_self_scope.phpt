--TEST--
self / __CLASS__ / new self in a closure declared in a trait method name the using class
--FILE--
<?php
trait T {
    public static $sp = 'sp';
    const C = 'c';
    private static function priv($x) { return static::class . ":$x"; }
    private function ipriv($x) { return get_class($this) . ":i$x"; }
    public static function make() {
        $f = function () {
            return [self::class, __CLASS__, get_class(new self), self::C, self::$sp, self::priv(1),
                static::class, (fn() => self::priv(2))(), self::class === static::class];
        };
        return $f();
    }
    public function inst() {
        $f = function () { return [self::class, $this->ipriv(3), self::priv(4), __CLASS__]; };
        return $f();
    }
    public static function viaStatic() {
        return array_map(static fn($v) => self::priv($v), [5]);
    }
}
class A { use T; }
class B { use T; }
class C extends B {}
echo json_encode(A::make()), "\n";
echo json_encode(B::make()), "\n";
echo json_encode(C::make()), "\n";
echo json_encode((new A)->inst()), "\n";
echo json_encode((new C)->inst()), "\n";
echo json_encode(A::viaStatic()), "\n";
echo json_encode(C::viaStatic()), "\n";

?>
--EXPECT--
["A","A","A","c","sp","A:1","A","A:2",true]
["B","B","B","c","sp","B:1","B","B:2",true]
["B","B","B","c","sp","C:1","C","C:2",false]
["A","A:i3","A:4","A"]
["B","C:i3","C:4","B"]
["A:5"]
["C:5"]
