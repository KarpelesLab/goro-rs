--TEST--
Destroying a suspended fiber keeps the argument values of the frames below (backtraces)
--FILE--
<?php
class H {
    public $a; public $self;
    function __construct() {
        $this->self = $this;
        $this->a = new Fiber(function () { Fiber::suspend('a'); });
        $this->a->start();
    }
}
class W { public $v = 'w'; }
function top(W $w) {
    $h = new H; $h = null;
    gc_collect_cycles();
    echo (new Exception)->getTraceAsString(), "\n";
}
echo "-- main\n";
top(new W);
echo "-- fiber\n";
$f = new Fiber(function () { top(new W); });
$f->start();
echo "-- fiber, no cycle\n";
function top3(W $w) {
    $x = new Fiber(function () { Fiber::suspend('a'); });
    $x->start();
    $x = null;
    echo (new Exception)->getTraceAsString(), "\n";
}
$f = new Fiber(function () { top3(new W); });
$f->start();

?>
--EXPECTF--
-- main
#0 %s(17): top(Object(W))
#1 {main}
-- fiber
#0 %s(19): top(Object(W))
#1 [internal function]: {closure:%s:19}()
#2 %s(20): Fiber->start()
#3 {main}
-- fiber, no cycle
#0 %s(28): top3(Object(W))
#1 [internal function]: {closure:%s:28}()
#2 %s(29): Fiber->start()
#3 {main}
