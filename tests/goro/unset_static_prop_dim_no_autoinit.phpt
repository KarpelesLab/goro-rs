--TEST--
unset(C::$p[k]) does not auto-initialize a null or uninitialized typed static property
--FILE--
<?php
class P {
    private static \WeakMap $m;
    private static array $a;
    private static ?array $n = null;
    public $p;
    function f() {
        unset(self::$m[$this]);
        echo "ok1\n";
        try { unset(self::$a['x']); echo "ok2\n"; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
        unset(self::$n['x']);
        var_dump(self::$n);
        self::$m ??= new WeakMap();
        self::$m[$this] = 1;
        unset(self::$m[$this]);
        var_dump(count(self::$m));
    }
}
(new P)->f();
class Q { public WeakMap $m; public array $a; }
$q = new Q;
unset($q->a['k']);
echo "done\n";

?>
--EXPECT--
ok1
ok2
NULL
int(0)
done
