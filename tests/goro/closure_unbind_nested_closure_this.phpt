--TEST--
Closure::bindTo(null): a nested closure capturing $this does not make the outer closure one using $this
--FILE--
<?php
class A { static function name() { return 'A'; } }
class T {
    function run() {
        $c = function ($n) { return A::class . (fn () => "x$n")(); };
        $b = $c->bindTo(null, A::class);
        var_dump($b === null ? null : $b(1));
        $c2 = function () { return function () { return isset($this) ? 'this' : 'no this'; }; };
        $b2 = $c2->bindTo(null, A::class);
        var_dump($b2 === null ? null : $b2()());
        $c3 = function () { return $this; };
        var_dump($c3->bindTo(null, A::class) === null);
        $c4 = function () { return static fn () => static::name(); };
        $b4 = $c4->bindTo(null, A::class);
        var_dump($b4()());
        $c5 = function () { return fn () => isset($this); };
        var_dump($c5->bindTo(null, A::class)()());
        $c6 = function () { $a = []; $a[] = $this; return $a; };
        var_dump($c6->bindTo(null, A::class) === null);
    }
}
(new T)->run();
?>
--EXPECTF--
string(3) "Ax1"
string(7) "no this"

Warning: Cannot unbind $this of closure using $this, this will be an error in PHP 9 in %s on line 12
bool(true)
string(1) "A"
bool(false)

Warning: Cannot unbind $this of closure using $this, this will be an error in PHP 9 in %s on line 19
bool(true)
