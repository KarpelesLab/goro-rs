--TEST--
gc_collect_cycles(): a suspended fiber in a cycle with an object that has a destructor is counted once destroyed
--FILE--
<?php
class D { public $s; function __destruct() { echo "dtor\n"; } }
function t0() { $o = new D; $o->s = $o; }
t0();
var_dump(gc_collect_cycles());
function t3() {
    $o = new D;
    $f = new Fiber(function () use ($o) { Fiber::suspend(1); });
    $o->s = $f;
    $f->start();
}
t3();
var_dump(gc_collect_cycles());
function t4() {
    $o = new stdClass;
    $f = new Fiber(function () use ($o) { try { Fiber::suspend(1); } finally { echo "finally\n"; } });
    $o->s = $f;
    $f->start();
}
t4();
var_dump(gc_collect_cycles());
var_dump(gc_collect_cycles());
echo "end\n";
--EXPECT--
dtor
int(1)
dtor
int(1)
finally
int(1)
int(0)
end
