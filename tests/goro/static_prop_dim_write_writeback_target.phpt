--TEST--
C::$p[k] = v after a read of another static property does not store into that one
--FILE--
<?php
class P {
    public static ?array $n = null;
    public static array $m = [];
    static function f() {
        var_dump(self::$n);
        self::$m['k'] = 1;
        var_dump(self::$n, self::$m);
    }
}
P::f();

?>
--EXPECT--
NULL
NULL
array(1) {
  ["k"]=>
  int(1)
}
