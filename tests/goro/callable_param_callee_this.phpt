--TEST--
A callable parameter is checked in the callee's frame (its $this makes [Class, 'instanceMethod'] callable)
--FILE--
<?php
class P {
    protected function inst($x) { return true; }
    public function has(callable|string $f): bool { return is_callable($f); }
    public function has2(callable $f): bool { return is_callable($f); }
    public function chk($f) { return is_callable($f); }
    public static function run() {
        $p = new static;
        foreach (['has', 'has2'] as $m) {
            try { var_dump($p->$m([self::class, 'inst'])); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
        }
        var_dump($p->chk([self::class, 'inst']), is_callable([self::class, 'inst']));
    }
}
P::run();
try { var_dump((new P)->has2([P::class, 'inst'])); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }

?>
--EXPECT--
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
