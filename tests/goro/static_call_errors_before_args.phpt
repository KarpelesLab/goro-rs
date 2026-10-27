--TEST--
Static calls of inaccessible, abstract or non-static methods fail before their arguments are evaluated
--FILE--
<?php
function a() { echo "arg evaluated\n"; return 1; }
class P { private function priv() {} private static function spriv() {} public function inst() { return "inst"; } protected static function prot() {} }
try { P::spriv(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { P::inst(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { P::prot(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
abstract class AB { abstract static function s(); }
try { AB::s(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
interface I { static function is(); }
try { I::is(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$cls = 'P'; $sm = 'spriv';
try { $cls::$sm(a()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Q extends P { function t() { return P::inst(a()); } static function st() { P::prot(a()); return "prot ok"; } }
var_dump((new Q)->t(), Q::st());
class M { private static function hidden() {} static function __callStatic($n, $a) { return "callStatic $n"; } }
var_dump(M::hidden(a()));
--EXPECT--
Call to private method P::spriv() from global scope
Non-static method P::inst() cannot be called statically
Call to protected method P::prot() from global scope
Cannot call abstract method AB::s()
Cannot call abstract method I::is()
Call to private method P::spriv() from global scope
arg evaluated
arg evaluated
string(4) "inst"
string(7) "prot ok"
arg evaluated
string(17) "callStatic hidden"
