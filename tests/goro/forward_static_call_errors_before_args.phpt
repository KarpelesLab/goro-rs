--TEST--
self::/parent::/static:: calls of inaccessible or abstract methods fail before their arguments; an abstract trait method gets no trait deprecation
--FILE--
<?php
function a($n) { echo "arg $n\n"; return $n; }
class P { protected function prot() {} private function priv() {} }
class C extends P { function t() { $this->priv(a(1)); } }
try { (new C)->t(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class D { function t($o) { $o->prot(a(2)); } }
try { (new D)->t(new P); } catch (Error $e) { echo $e->getMessage(), "\n"; }
abstract class AB { abstract function f(); function call() { } }
class Impl extends AB { function f() { return parent::f(a(3)); } }
try { (new Impl)->f(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class E { static function s() { return self::nope(a(4)); } static function t() { return static::nope2(a(5)); } }
try { E::s(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { E::t(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class F { function i() {} static function s() { return self::i(a(6)); } }
try { F::s(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class G { private static function ps() {} }
class H extends G { static function t() { return parent::ps(a(7)); } }
try { H::t(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { new P(a(8)); echo "ok\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
enum En { case A; public function m() {} }
try { En::m(a(9)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { En::A->nope(a(10)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { En::nope(a(11)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
trait T { static function ts() { return "ts"; } abstract static function at(); }
try { var_dump(T::ts()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { T::at(a(12)); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
Call to private method P::priv() from scope C
Call to protected method P::prot() from scope D
Cannot call abstract method AB::f()
Call to undefined method E::nope()
Call to undefined method E::nope2()
Non-static method F::i() cannot be called statically
Call to private method G::ps() from scope H
arg 8
ok
Non-static method En::m() cannot be called statically
Call to undefined method En::nope()
Call to undefined method En::nope()

Deprecated: Calling static trait method T::ts is deprecated, it should only be called on a class using the trait in %s on line 25
string(2) "ts"
Cannot call abstract method T::at()
