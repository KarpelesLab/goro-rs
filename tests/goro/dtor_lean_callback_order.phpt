--TEST--
Callbacks builtins call while objects with destructors exist: release order of arguments, results and locals
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
class K { public $v; function __construct($v) { $this->v = $v; } function get() { return $this->v; } }
function run() {
    call_user_func(function ($p) { $l = new D("cuf-local"); return 1; }, 5); echo "--2\n";
    $r = array_map(function ($x) { return $x * 2; }, [1, 2, 3]); echo implode(",", $r), "\n";
    $c = new D("captured");
    $r = array_map(function ($x) use ($c) { return $x . $c->n; }, [1, 2]); echo implode(",", $r), "\n";
    $c = null; echo "--3\n";
    $objs = [new K(3), new K(1), new K(2)];
    usort($objs, fn($a, $b) => $a->get() <=> $b->get());
    echo $objs[0]->get(), $objs[2]->get(), "\n";
    $r = array_filter([new D("f1"), new D("f2")], fn($d) => $d->n == "f2"); echo "--4\n";
    $r = null; echo "--5\n";
    $s = 0;
    for ($i = 0; $i < 2000; $i++) { $s += call_user_func(fn($x) => $x + 1, $i); }
    echo $s, "\n";
    echo call_user_func([new K(new D("method-held")), 'get'])->n, "\n";
    echo "--6\n";
}
$keep = new D("keep");
run();
echo "end\n";
?>
--EXPECT--
dtor cuf-local
--2
2,4,6
1captured,2captured
dtor captured
--3
13
dtor f1
--4
dtor f2
--5
2001000
dtor method-held
method-held
--6
end
dtor keep
