--TEST--
Multi-line $this method calls / property dims / foreach over a property: opline lines (g20_3)
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; } }
class P { public $pp = 1; protected function pm($a = 0) { return $a; } public static function ps() { return static::class; } }
class C extends P {
    public array $arr = [];
    public ?C $next = null;
    private int $priv = 0;
    public function run() {
        foreach
        (
        $this
        ->arr
        as
        $k
        =>
        $v
        )
        {
        }
        foreach
        (
        $this
        ->nope
        as
        $v
        )
        {}
    }
    public function m($a = 0, $b = 0) { return $this; }
    private function pv($a = 0) { return $a; }
}
t(fn() => (new C)->run());
echo "\n-done-\n";
--EXPECTF--

Warning: Undefined property: C::$nope in %s on line 23

Warning: foreach() argument must be of type array|object, null given in %s on line 23

-done-
