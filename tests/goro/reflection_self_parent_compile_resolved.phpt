--TEST--
Types: self/parent are resolved to class names at compile time in a known scope (not traits, closures, anonymous classes)
--FILE--
<?php
class P {}
class A extends P {
    function m(self $a, parent $b, ?self $c = null, self|int $d = 1): self { return $this; }
    public self $prop;
    function cl() { return function (self $x): static {}; }
}
trait T { function t(self $x): self {} }
class B { use T; }
$m = new ReflectionMethod('A', 'm');
foreach ($m->getParameters() as $p) { var_dump((string)$p->getType()); }
var_dump((string)$m->getReturnType());
var_dump((string)(new ReflectionProperty('A','prop'))->getType());
echo $m;
$c = (new A)->cl();
echo new ReflectionFunction($c);
echo new ReflectionMethod('B', 't');
$o = new class { function m(self $x) {} };
echo new ReflectionMethod($o, 'm');
try { (new A)->m(new P, new P); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $a = new A; $a->prop = 1; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
string(1) "A"
string(1) "P"
string(2) "?A"
string(5) "A|int"
string(1) "A"
string(1) "A"
Method [ <user> public method m ] {
  @@ %s 4 - 4

  - Parameters [4] {
    Parameter #0 [ <required> A $a ]
    Parameter #1 [ <required> P $b ]
    Parameter #2 [ <optional> ?A $c = NULL ]
    Parameter #3 [ <optional> A|int $d = 1 ]
  }
  - Return [ A ]
}
Closure [ <user> public method {closure:A::cl():6} ] {
  @@ %s 6 - 6

  - Parameters [1] {
    Parameter #0 [ <required> self $x ]
  }
  - Return [ static ]
}
Method [ <user> public method t ] {
  @@ %s 8 - 8

  - Parameters [1] {
    Parameter #0 [ <required> self $x ]
  }
  - Return [ self ]
}
Method [ <user> public method m ] {
  @@ %s 18 - 18

  - Parameters [1] {
    Parameter #0 [ <required> self $x ]
  }
}
A::m(): Argument #1 ($a) must be of type A, P given, called in %s on line 20
Cannot assign int to property A::$prop of type A
