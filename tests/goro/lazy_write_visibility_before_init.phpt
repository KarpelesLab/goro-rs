--TEST--
Writing a lazy object's property checks its (set) visibility before initializing it
--FILE--
<?php
class A {
    public readonly int $ro;
    private $priv = 1;
    protected $prot = 2;
    public private(set) int $ps = 3;
    public $pub = 4;
    public function __construct() { $this->ro = 42; }
    public function setRo($v) { $this->ro = $v; }
    public function setPs($v) { $this->ps = $v; }
}
class M { private $priv = 1; public function __set($n, $v) { echo "__set $n\n"; } }
function t($f) { try { $f(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$r = new ReflectionClass('A');
$mk = fn() => $r->newLazyGhost(function ($o) { echo "init\n"; });
$mkp = fn() => $r->newLazyProxy(function ($o) { echo "init\n"; return new A; });
foreach (['ghost' => $mk, 'proxy' => $mkp] as $kind => $f) {
    echo "-- $kind\n";
    $o = $f(); t(fn() => $o->ro = 1); var_dump($r->isUninitializedLazyObject($o));
    $o = $f(); t(fn() => $o->priv = 1); var_dump($r->isUninitializedLazyObject($o));
    $o = $f(); t(fn() => $o->prot = 1); var_dump($r->isUninitializedLazyObject($o));
    $o = $f(); t(fn() => $o->ps = 1); var_dump($r->isUninitializedLazyObject($o));
    $o = $f(); t(fn() => $o->pub = 1); var_dump($r->isUninitializedLazyObject($o));
    $o = $f(); t(fn() => $o->setRo(5)); var_dump($r->isUninitializedLazyObject($o), $o->ro);
    $o = $f(); t(fn() => $o->setPs(5)); var_dump($r->isUninitializedLazyObject($o), $o->ps);
    $o = $f(); t(fn() => $o->dyn = 1); var_dump($r->isUninitializedLazyObject($o));
}
$rm = new ReflectionClass('M');
$o = $rm->newLazyGhost(function ($o) { echo "init M\n"; });
t(fn() => $o->priv = 5);
var_dump($rm->isUninitializedLazyObject($o));
--EXPECTF--
-- ghost
Error: Cannot modify protected(set) readonly property A::$ro from global scope
bool(true)
Error: Cannot access private property A::$priv
bool(true)
Error: Cannot access protected property A::$prot
bool(true)
Error: Cannot modify private(set) property A::$ps from global scope
bool(true)
init
bool(false)
init
bool(false)
int(5)
init
bool(false)
int(5)
init

Deprecated: Creation of dynamic property A::$dyn is deprecated in %s on line 26
bool(false)
-- proxy
Error: Cannot modify protected(set) readonly property A::$ro from global scope
bool(true)
Error: Cannot access private property A::$priv
bool(true)
Error: Cannot access protected property A::$prot
bool(true)
Error: Cannot modify private(set) property A::$ps from global scope
bool(true)
init
bool(false)
init
Error: Cannot modify readonly property A::$ro
bool(false)
int(42)
init
bool(false)
int(5)
init

Deprecated: Creation of dynamic property A::$dyn is deprecated in %s on line 26
bool(false)
__set priv
bool(true)
