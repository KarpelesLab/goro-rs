--TEST--
Cloning an initialized lazy proxy clones its instance, which is destructed
--FILE--
<?php
class A { public $a = 1; public function __destruct() { echo "dtor ", spl_object_id($this), "\n"; } }
$r = new ReflectionClass('A');
$o = $r->newLazyProxy(function ($o) { return new A; });
$o->a;
$c = clone $o;
var_dump($c);
$c->a = 5;
var_dump($o->a, $c->a);
$c = null;
echo "end\n";
--EXPECT--
lazy proxy object(A)#2 (1) {
  ["instance"]=>
  object(A)#5 (1) {
    ["a"]=>
    int(1)
  }
}
int(1)
int(5)
dtor 5
end
dtor 4
