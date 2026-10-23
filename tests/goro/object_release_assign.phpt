--TEST--
Overwriting and freeing objects: destructor order, object ids, weak references
--FILE--
<?php
class P { public $x = 1; public $s = 'str'; }
class D { public $n; function __construct($n) { $this->n = $n; } function __destruct() { echo "dtor {$this->n}\n"; } }
class Holder { public $d; function __construct($d) { $this->d = $d; } }
function ids() {
    $o = new P;
    $a = spl_object_id($o);
    $o = new P;
    $b = spl_object_id($o);
    $o = new P;
    $c = spl_object_id($o);
    return [$a, $b, $c];
}
echo json_encode(ids()), "\n";
function loop() {
    for ($i = 0; $i < 3; $i++) {
        $o = new D($i);
        echo "made $i\n";
    }
    echo "end loop\n";
}
loop();
echo "after loop\n";
function nested() {
    for ($i = 0; $i < 2; $i++) {
        $h = new Holder(new D("h$i"));
        echo "holder $i\n";
    }
    $h = 1;
    echo "end nested\n";
}
nested();
function weak() {
    $o = new P;
    $w = WeakReference::create($o);
    $o = new P;
    var_dump($w->get());
    $m = new WeakMap;
    $k = new P;
    $m[$k] = 'v';
    $k = new P;
    echo count($m), "\n";
}
weak();
function tmp_free() {
    foo(new P);
    foo(new D('t'));
    echo "after tmp\n";
}
function foo($x) { return 1; }
tmp_free();
$g = new D('global');
$g = new P;
echo "global replaced\n";
$p = new P;
$p->dyn = 5;
$p = new P;
$q = new P;
$q->x = new D('inner');
$q = null;
echo "q nulled\n";
function gc_track() {
    for ($i = 0; $i < 3; $i++) { $o = new stdClass; $o->a = $i; }
    echo gc_collect_cycles(), "\n";
}
gc_track();
echo "done\n";

function z() { new D("z1"); echo "after z1\n"; $o = new D("z2"); $o = new P; echo "after z2\n"; for ($i = 0; $i < 2; $i++) { new D("loop$i"); } echo "after loop\n"; }
z();
class DD { function __destruct() { echo "dd\n"; } }
function zz() { $a = [new DD]; $a = 1; echo "after zz\n"; }
zz();
class E { public $v = 1; function __destruct() { echo "e dtor\n"; } }
function ze() { new E; echo "after e1\n"; for ($i = 0; $i < 2; $i++) { new E; echo "iter $i\n"; } $x = new E; $x = 5; echo "after e2\n"; }
ze();
?>
--EXPECTF--
[1,2,1]
made 0
dtor 0
made 1
dtor 1
made 2
end loop
dtor 2
after loop
holder 0
dtor h0
holder 1
dtor h1
end nested
NULL
0
dtor t
after tmp
dtor global
global replaced

Deprecated: Creation of dynamic property P::$dyn is deprecated in %s on line 56
dtor inner
q nulled
0
done
dtor z1
after z1
dtor z2
after z2
dtor loop0
dtor loop1
after loop
dd
after zz
e dtor
after e1
e dtor
iter 0
e dtor
iter 1
e dtor
after e2
