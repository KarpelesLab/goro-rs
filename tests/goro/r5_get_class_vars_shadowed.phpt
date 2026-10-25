--TEST--
get_class_vars(): shadowed privates, autoload, TypeError
--FILE--
<?php
class A {
    public $pub = 1; protected $pro = 2; private $pri = 3;
    private static $spri = 6;
    function fromA() { var_dump(get_class_vars('B'), get_class_vars('C')); }
}
class B extends A { private $pri = 'B'; public $pub = 'Bpub'; }
class C extends A {}
(new A)->fromA();
var_dump(get_class_vars('\B'));
spl_autoload_register(function ($c) { echo "autoload $c\n"; if ($c === 'Auto') { eval('class Auto { public $x = 1; }'); } });
var_dump(get_class_vars('Auto'));
try { get_class_vars('Nope'); } catch (TypeError $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
--EXPECT--
array(3) {
  ["pub"]=>
  string(4) "Bpub"
  ["pro"]=>
  int(2)
  ["spri"]=>
  int(6)
}
array(4) {
  ["pub"]=>
  int(1)
  ["pro"]=>
  int(2)
  ["pri"]=>
  int(3)
  ["spri"]=>
  int(6)
}
array(1) {
  ["pub"]=>
  string(4) "Bpub"
}
autoload Auto
array(1) {
  ["x"]=>
  int(1)
}
autoload Nope
TypeError: get_class_vars(): Argument #1 ($class) must be a valid class name, Nope given
