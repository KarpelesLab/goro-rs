--TEST--
Property hooks: the set hook's parameter type coerces / checks the value (weak mode)
--FILE--
<?php
class A { public $log = []; public int $x { set { $this->log[] = $value; } } }
$a = new A; $a->x = "7"; var_dump($a->log[0]);
try { $a->x = "abc"; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
class B { public $log = []; public string $x { set(string|int $v) { $this->log[] = $v; } } }
$b = new B;
try { $b->x = []; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$b->x = 1.5; var_dump($b->log);
$b->x = 2.0; var_dump($b->log);
--EXPECTF--
int(7)
A::$x::set(): Argument #1 ($value) must be of type int, string given, called in %s on line 4
B::$x::set(): Argument #1 ($v) must be of type string|int, array given, called in %s on line 7

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 5
array(1) {
  [0]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
