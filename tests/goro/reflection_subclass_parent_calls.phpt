--TEST--
parent::m() in Reflection subclasses calls the method on $this
--FILE--
<?php
class MR extends ReflectionMethod {
  public function getName(): string { return "n:" . parent::getName(); }
  public function isPublic(): bool { return !parent::isPublic(); }
  public function getDocComment(): string|false { return parent::getDocComment(); }
}
class Z { function f() {} }
$m = new MR("Z", "f");
var_dump($m->getName(), $m->isPublic(), $m->getDocComment());
class RC extends ReflectionClass { public function getParentClass(): ReflectionClass|false { return parent::getParentClass(); } }
var_dump((new RC("MR"))->getParentClass()->name);
class RP extends ReflectionProperty { public function getDeclaringClass(): ReflectionClass { return parent::getDeclaringClass(); } }
class Y { public $p; } var_dump((new RP("Y", "p"))->getDeclaringClass()->name);
class MR0 extends ReflectionMethod {
  #[ReturnTypeWillChange] public function getDeclaringClass() { $p = parent::getDeclaringClass(); return "my:" . $p->getName(); }
  public function getName(): string { return "n:" . parent::getName(); }
}
$m = new MR0("Z", "f");
var_dump($m->getDeclaringClass(), $m->getName(), $m->name);
$m2 = new MR0(new Z, "f"); var_dump($m2->getDeclaringClass());

?>
--EXPECT--
string(3) "n:f"
bool(false)
bool(false)
string(16) "ReflectionMethod"
string(1) "Y"
string(4) "my:Z"
string(3) "n:f"
string(1) "f"
string(4) "my:Z"
