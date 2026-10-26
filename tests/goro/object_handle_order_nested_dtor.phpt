--TEST--
A freed object's handle is recycled after the destructed objects it held
--FILE--
<?php
class A {
    public $a = 1;
    public $next;
    public function __destruct() { echo "dtor ", spl_object_id($this), "\n"; }
}
function ids() {
    $a = new stdClass; $b = new stdClass; $c = new stdClass; $d = new stdClass;
    echo "ids ", spl_object_id($a), spl_object_id($b), spl_object_id($c), spl_object_id($d), "\n";
}
echo "-- container of a destructible object\n";
$x = new stdClass;
$x->a = new A;
$x = null;
ids();
echo "-- chain: plain -> plain -> destructible\n";
$x = new stdClass;
$x->y = new stdClass;
$x->y->a = new A;
$x = null;
ids();
echo "-- destructible -> destructible\n";
$x = new A;
$x->next = new A;
$x = null;
ids();
echo "-- initialized lazy proxy\n";
$r = new ReflectionClass('A');
$p = $r->newLazyProxy(function () { return new A; });
$p->a;
$p = null;
ids();
--EXPECT--
-- container of a destructible object
dtor 2
ids 1234
-- chain: plain -> plain -> destructible
dtor 2
ids 4321
-- destructible -> destructible
dtor 1
dtor 2
ids 1234
-- initialized lazy proxy
dtor 1
ids 2135
