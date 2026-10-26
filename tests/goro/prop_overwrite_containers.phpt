--TEST--
Property writes over arrays and objects: cycles stay collectable, destructors run when the last holder goes
--FILE--
<?php
class N { public $p; public $q = []; }
class D { function __construct(public $n) {} function __destruct() { echo "destroy {$this->n}\n"; } }
function mk() { $a = new N; $b = new N; $a->p = $b; $b->p = $a; return $a; }
function run() {
    $h = new N;
    for ($i = 0; $i < 5; $i++) {
        $h->p = mk();
        $h->q = [mk(), [mk()], range(1, 20)];
    }
    $h->p = null;
    $h->q = null;
    var_dump(gc_collect_cycles());
    $x = new N;
    $y = new N;
    $x->p = $y;
    $y->p = $x;
    $h->p = $x;
    unset($x, $y);
    $h->p = 1;
    var_dump(gc_collect_cycles());
    $keep = new N;
    for ($i = 0; $i < 3; $i++) {
        $h->p = [$keep, new N, str_repeat('x', $i)];
        $h->q = $keep;
    }
    $h->q = [new D(1), [new D(2)]];
    echo "set\n";
    $h->q = [];
    echo "reset\n";
    $h->p = new D(3);
    $h->p = new D(4);
    echo "end\n";
}
run();
echo "done\n";
?>
--EXPECT--
int(30)
int(2)
set
destroy 1
destroy 2
reset
destroy 3
end
destroy 4
done
