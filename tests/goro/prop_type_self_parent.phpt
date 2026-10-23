--TEST--
self and parent in property types name the classes
--FILE--
<?php
// self and parent in a class's property types name the classes
class A { public ?self $s; public self|int $t; }
class B extends A { public ?parent $q; public static $z; }
var_dump(new A, new B);
print_r(new B); echo "\n";
try { $a = new A; $a->s = 5; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
class A2 { public ?self $s = null; function f(self $x): ?self { return $x; } }
class B2 extends A2 { public parent|int $q = 1; function g(parent $p) {} }
trait T2 { public ?self $ts = null; }
class C2 { use T2; }
$a = new A2;
try { $a->s = 5; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $a->f(5); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $b = new B2; $b->q = []; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { (new B)->g(5); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { $c = new C2; $c->ts = 5; } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
echo (new ReflectionProperty("A2", "s"))->getType(), " ", (new ReflectionMethod("A2", "f"))->getReturnType(), "\n";
--EXPECTF--
object(A)#1 (0) {
  ["s"]=>
  uninitialized(?A)
  ["t"]=>
  uninitialized(A|int)
}
object(B)#2 (0) {
  ["s"]=>
  uninitialized(?A)
  ["t"]=>
  uninitialized(A|int)
  ["q"]=>
  uninitialized(?A)
}
B Object
(
)

Cannot assign int to property A::$s of type ?A
Cannot assign int to property A2::$s of type ?A2
A2::f(): Argument #1 ($x) must be of type A2, int given, called in %s on line 14
Cannot assign array to property B2::$q of type A2|int

Fatal error: Uncaught Error: Call to undefined method B::g() in %s:16
Stack trace:
#0 {main}
  thrown in %s on line 16
