--TEST--
Property hooks: a parent's private hooked property and a child's same-named property
--FILE--
<?php
class P { private $x { get => "P"; } public function f() { return $this->x; } } class C extends P { public $x = "C"; } var_dump((new C)->f(), (new C)->x);
class P2 { private $x = "Pv"; } class C2 extends P2 { public $x { get => "C"; } } var_dump(new C2); var_dump((new C2)->x);
--EXPECT--
string(1) "P"
string(1) "C"
object(C2)#1 (1) {
  ["x":"P2":private]=>
  string(2) "Pv"
}
string(1) "C"
