--TEST--
Multi-line $this method calls / property dims / foreach over a property: opline lines (g16_2)
--FILE--
<?php
function t($f) { try { $f(); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; } }
class P { public $pp = 1; protected function pm($a = 0) { return $a; } public static function ps() { return static::class; } }
class C extends P {
    public array $arr = [];
    public ?C $next = null;
    private int $priv = 0;
    public function run() {
        return $this
        ->m( 1,
        $u1
        );
    }
    public function m($a = 0, $b = 0) { return $this; }
    private function pv($a = 0) { return $a; }
}
t(fn() => (new C)->run());
echo "\n-done-\n";
--EXPECTF--

Warning: Undefined variable $u1 in %s on line 10

-done-
