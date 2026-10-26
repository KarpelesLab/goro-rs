--TEST--
Closures created by included or eval()'d code in a method get its $this and scope
--FILE--
<?php
$inc = sys_get_temp_dir() . '/goro_inc_closure_' . getmypid() . '.php';
file_put_contents($inc, '<?php
$cl = function ($a) { return isset($this) ? get_class($this) . $this->p : "none"; };
$st = static fn () => static::class . "/" . self::class;
return [$cl, $st];
');
class K {
    private $p = ':priv';
    function m($inc) {
        [$cl, $st] = include $inc;
        $r = new ReflectionFunction($cl);
        var_dump($cl(1), $r->getClosureThis() === $this, $r->getClosureScopeClass()?->name, $st());
        $c = eval('return function () { return get_class($this) . $this->p; };');
        $s = eval('return static fn () => static::class . self::class;');
        var_dump($c(), $s());
    }
    static function s($inc) {
        [$cl, $st] = include $inc;
        var_dump($cl(1), (new ReflectionFunction($cl))->getClosureScopeClass()?->name, $st());
    }
}
class L extends K {}
(new L)->m($inc);
L::s($inc);
[$cl] = include $inc;
var_dump($cl(1));
unlink($inc);

?>
--EXPECT--
string(6) "L:priv"
bool(true)
string(1) "K"
string(3) "L/K"
string(6) "L:priv"
string(2) "LK"
string(4) "none"
string(1) "K"
string(3) "L/K"
string(4) "none"
