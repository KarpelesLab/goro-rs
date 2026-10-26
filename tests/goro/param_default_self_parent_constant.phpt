--TEST--
Method parameter defaults: self::/parent:: constants resolve in the method's scope; an undefined one is reported as self::X
--FILE--
<?php
class Pp { const K = 5; function m($a = self::K, $b = self::NOPE4) { return $a; } }
try { var_dump((new Pp)->m()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Qq extends Pp { function n($c = parent::NOPE5) {} }
try { (new Qq)->n(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
function ff($x = Pp::NOPE6) {}
try { ff(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class A { const X = 1; function m($a = self::X, $b = [self::X => self::X + 1], $c = parent::class ?? 1) { return [$a, $b]; } }
var_dump((new A)->m());
class B extends A { const X = 2; function m($a = self::X, $b = [self::X => self::X + 1], $c = 3) { return [$a, $b]; } function p($q = parent::X) { return $q; } }
var_dump((new B)->m(), (new B)->p());
echo new ReflectionMethod("B", "m");
var_dump((new ReflectionMethod("B", "p"))->getParameters()[0]->getDefaultValue());
?>
--EXPECTF--

Fatal error: Cannot use "parent" when current class scope has no parent in %s on line 8
