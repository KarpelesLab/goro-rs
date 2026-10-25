--TEST--
Asymmetric visibility errors name the called scope
--FILE--
<?php
class P { public private(set) int $i = 0; public protected(set) array $a = [];
  function f() { $this->i = 1; } static function g($o) { $o->i = 2; } }
class C extends P { function h() { try { $this->i = 3; } catch (Error $e) { echo $e->getMessage(), "\n"; } try { $this->a[] = 3; echo "ok\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; } } static function s($o) { try { $o->i = 4; } catch (Error $e) { echo $e->getMessage(), "\n"; } } }
class D { function t($o) { try { $o->i = 5; } catch (Error $e) { echo $e->getMessage(), "\n"; } try { unset($o->i); } catch (Error $e) { echo $e->getMessage(), "\n"; } try { $o->a[] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; } } static function u($o) { try { $o->i = 5; } catch (Error $e) { echo $e->getMessage(), "\n"; } } }
class E extends D {}
$c = new C;
$c->h(); C::s($c); (new D)->t($c); (new E)->t($c); E::u($c);
try { $c->i = 9; } catch (Error $e) { echo $e->getMessage(), "\n"; }
$f = function() use ($c) { try { $c->i = 9; } catch (Error $e) { echo $e->getMessage(), "\n"; } };
$f();
Closure::bind($f, new E, E::class)();
Closure::bind($f, null, E::class)();
--EXPECT--
Cannot modify private(set) property P::$i from scope C
ok
Cannot modify private(set) property P::$i from scope C
Cannot modify private(set) property P::$i from scope D
Cannot unset private(set) property P::$i from scope D
Cannot indirectly modify protected(set) property P::$a from scope D
Cannot modify private(set) property P::$i from scope E
Cannot unset private(set) property P::$i from scope E
Cannot indirectly modify protected(set) property P::$a from scope E
Cannot modify private(set) property P::$i from scope E
Cannot modify private(set) property P::$i from global scope
Cannot modify private(set) property P::$i from global scope
Cannot modify private(set) property P::$i from scope E
Cannot modify private(set) property P::$i from scope E
