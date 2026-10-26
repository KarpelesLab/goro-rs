--TEST--
A closure unbound from its $this (Closure::bind($f, null, ...)) called as a callback gets its arguments in place
--FILE--
<?php
class M { public static function who() { return 'M'; } }
class TT {
    public function t() {
        $f = function ($a, $b = 'B') { return "a=$a b=$b " . self::who(); };
        $g = Closure::bind($f, null, M::class);
        var_dump($g('x'));
        var_dump(call_user_func_array($g, ['y', 'z']));
        var_dump(call_user_func($g, 'w'));
        $h = $f->bindTo(null);
        var_dump((new ReflectionFunction($h))->getClosureThis());
    }
    public static function who() { return 'TT'; }
}
(new TT)->t();

?>
--EXPECT--
string(9) "a=x b=B M"
string(9) "a=y b=z M"
string(9) "a=w b=B M"
NULL
