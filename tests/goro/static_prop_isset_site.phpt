--TEST--
isset(A::$p) / A::$p ?? x / A::$p ??= x by constant names, run repeatedly as the properties change
--FILE--
<?php
class A {
    public static $n = null;
    public static ?int $t;
    public static $arr = [];
    private static $priv = 1;
    protected static $prot = 2;
    public static $obj;
    public static function once(): string {
        return self::$obj ??= 'made';
    }
    public static function seen(): string {
        return (isset(self::$priv) ? 'p' : '-') . (isset(static::$prot) ? 'q' : '-');
    }
}
class B extends A {
    public static $n = 'b';
}
function probe() {
    return (isset(A::$n) ? 'n' : '-')
        . (isset(B::$n) ? 'N' : '-')
        . (isset(A::$t) ? 't' : '-')
        . (A::$t ?? 'T')
        . (isset(A::$arr) ? 'a' : '-')
        . (isset(A::$priv) ? 'p' : '-')
        . (isset(A::$prot) ? 'q' : '-')
        . (isset(A::$nope) ? '?' : '-');
}
for ($i = 0; $i < 3; $i++) {
    echo probe(), ' ', A::seen(), "\n";
    if ($i == 1) {
        A::$n = 0;
        A::$t = 5;
    }
}
A::$t = null;
echo probe(), "\n";
$r = &A::$n;
$r = null;
echo probe(), "\n";
$r = 'x';
echo probe(), "\n";
for ($i = 0; $i < 3; $i++) {
    echo A::once(), "\n";
    A::$obj = null;
}
?>
--EXPECT--
-N-Ta--- pq
-N-Ta--- pq
nNt5a--- pq
nN-Ta---
-N-Ta---
nN-Ta---
made
made
made
