--TEST--
"C::m" / ["C", "m"] dynamic calls check visibility and abstract methods before static-ness
--FILE--
<?php
class A { function m() {} private function p() {} protected function q() {} static private function sp() {} }
abstract class D { abstract function a(); abstract static function sa(); }
interface I { function i(); }
foreach (['A::p', 'A::q', 'A::sp', 'D::a', 'D::sa', 'I::i', 'A::m', ['A', 'p'], ['D', 'a'], ['I', 'i']] as $v) {
  try { $v(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
try { A::p(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { D::a(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { D::sa(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { I::i(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$c = 'A'; try { $c::p(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
Call to private method A::p() from global scope
Call to protected method A::q() from global scope
Call to private method A::sp() from global scope
Cannot call abstract method D::a()
Cannot call abstract method D::sa()
Cannot call abstract method I::i()
Non-static method A::m() cannot be called statically
Call to private method A::p() from global scope
Cannot call abstract method D::a()
Cannot call abstract method I::i()
Call to private method A::p() from global scope
Cannot call abstract method D::a()
Cannot call abstract method D::sa()
Cannot call abstract method I::i()
Call to private method A::p() from global scope
