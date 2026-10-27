--TEST--
readonly properties initialized by their declaring class, run repeatedly: errors on a second write, from another scope, in __clone
--FILE--
<?php
class P {
    public function __construct(public readonly int $a, protected readonly ?string $s = null, private readonly float $f = 1.5) {}
    public function again(int $v) { $this->a = $v; }
    public function f() { return $this->f; }
}
class C extends P {
    public readonly array $list;
    public function __construct(int $a) {
        parent::__construct($a, "s$a", $a);
        $this->list = [$a];
    }
    public function setParentProp() { $this->a = 99; }
    public function __clone() { $this->list = ['cloned']; }
}
final class R {
    public readonly int $x;
    public function init($v) { $this->x = $v; }
}
for ($i = 0; $i < 3; $i++) {
    $c = new C($i);
    echo $c->a, ' ', json_encode($c->list), ' ', $c->f(), "\n";
    try { $c->again(5); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    try { $c->setParentProp(); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    try { $c->list = []; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    $d = clone $c;
    echo json_encode($d->list), "\n";
    $r = new R;
    try { $r->x = 1; } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    $r->init($i);
    try { $r->init(7); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    try { (new R)->init("12"); } catch (Error $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    try { (new R)->init("x"); } catch (TypeError $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
    echo $r->x, "\n";
}
?>
--EXPECT--
0 [0] 0
Error: Cannot modify readonly property P::$a
Error: Cannot modify readonly property P::$a
Error: Cannot modify readonly property C::$list
["cloned"]
Error: Cannot modify protected(set) readonly property R::$x from global scope
Error: Cannot modify readonly property R::$x
TypeError: Cannot assign string to property R::$x of type int
0
1 [1] 1
Error: Cannot modify readonly property P::$a
Error: Cannot modify readonly property P::$a
Error: Cannot modify readonly property C::$list
["cloned"]
Error: Cannot modify protected(set) readonly property R::$x from global scope
Error: Cannot modify readonly property R::$x
TypeError: Cannot assign string to property R::$x of type int
1
2 [2] 2
Error: Cannot modify readonly property P::$a
Error: Cannot modify readonly property P::$a
Error: Cannot modify readonly property C::$list
["cloned"]
Error: Cannot modify protected(set) readonly property R::$x from global scope
Error: Cannot modify readonly property R::$x
TypeError: Cannot assign string to property R::$x of type int
2
