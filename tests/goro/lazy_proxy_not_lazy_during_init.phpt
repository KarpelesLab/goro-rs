--TEST--
A lazy proxy is not lazy while its factory runs (a clone of it is a plain object)
--FILE--
<?php
class W {
    public $foo;
    public function with($f): static { $n = clone $this; $n->foo = $f; return $n; }
}
$r = new ReflectionClass(W::class);
$g = $r->newLazyGhost(function ($ghost) use ($r) {
    echo "init: ", var_export($r->isUninitializedLazyObject($ghost), true), "\n";
    $c = clone $ghost;
    echo "cloned\n";
    $ghost->foo = 1;
    $x = $ghost->with(2);
    var_dump($x->foo, $r->isUninitializedLazyObject($x));
});
var_dump($r->isUninitializedLazyObject($g));
$y = $g->with(3);
var_dump($y->foo, $g->foo);
$p = $r->newLazyProxy(function ($proxy) use ($r) {
    echo "pinit: ", var_export($r->isUninitializedLazyObject($proxy), true), "\n";
    $c = clone $proxy;
    echo "pcloned ", var_export($r->isUninitializedLazyObject($c), true), "\n";
    return new W;
});
$z = $p->with(4);
var_dump($z->foo);

?>
--EXPECT--
bool(true)
init: false
cloned
int(2)
bool(false)
int(3)
int(1)
pinit: false
pcloned false
int(4)
