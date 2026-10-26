--TEST--
A rebound closure's self (self::class, self::$m(), self::C, new self) names its new scope; __CLASS__ does not change
--FILE--
<?php
class M {
    const C = 'M::C';
    public static $sp = 'M::$sp';
    public static function __callStatic($m, $a) { return "M::$m"; }
    public static function real() { return 'M::real'; }
    public function __toString(): string { return 'M obj'; }
}
class T {
    const C = 'T::C';
    public static $sp = 'T::$sp';
    public function make() {
        $name = 'dyn';
        return static function () use ($name) {
            return [self::class, __CLASS__, self::$name(), self::real(), self::lit(), static::class,
                self::C, self::$sp, (string) new self, get_class(new self)];
        };
    }
    public function inst() {
        return function () { return [self::class, __CLASS__, get_class($this)]; };
    }
}
$f = (new T)->make();
echo json_encode(Closure::bind($f, null, M::class)()), "\n";
echo json_encode(Closure::bind($f, null, 'M')()), "\n";
$g = (new T)->inst();
echo json_encode($g->call(new M)), "\n";
echo json_encode(Closure::bind($g, new M, M::class)()), "\n";
echo json_encode($g->bindTo(new T, T::class)()), "\n";

?>
--EXPECT--
["M","T","M::dyn","M::real","M::lit","M","M::C","M::$sp","M obj","M"]
["M","T","M::dyn","M::real","M::lit","M","M::C","M::$sp","M obj","M"]
["M","T","M"]
["M","T","M"]
["T","T","T"]
