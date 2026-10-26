--TEST--
Calls with class-typed parameters and returns given subclasses and implementers
--FILE--
<?php
interface I {} interface J extends I {}
class A {} class B extends A implements J {} class C extends B {}
function fa(A $a): A { return $a; }
function fi(I $a): ?I { return $a; }
function fj(J $a): J { return $a; }
function fr(A $a): B { return $a; }
foreach ([new A, new B, new C] as $o) {
  foreach (['fa', 'fi', 'fj', 'fr'] as $f) {
    try { echo $f, ':', get_class($f($o)), ' '; } catch (TypeError $e) { echo $f, ':', $e->getMessage(), ' '; }
  }
  echo "\n";
}
class P { function m(self $x): static { return $this; } }
class Q extends P {}
$q = new Q; echo get_class($q->m(new Q)), get_class($q->m(new P)), "\n";
--EXPECTF--
fa:A fi:fi:fi(): Argument #1 ($a) must be of type I, A given, called in %s on line 10 fj:fj:fj(): Argument #1 ($a) must be of type J, A given, called in %s on line 10 fr:fr:fr(): Return value must be of type B, A returned 
fa:B fi:B fj:B fr:B 
fa:C fi:C fj:C fr:C 
QQ
