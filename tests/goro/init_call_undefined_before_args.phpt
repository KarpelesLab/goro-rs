--TEST--
A call of an undefined function or method fails before its arguments are evaluated
--FILE--
<?php
namespace {
function f(){echo "evaluated\n"; return 1;}
class V {}
try { (new V)->nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { nofunc(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { V::nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $x = 'nofunc2'; $x(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $o = null; $o->m(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { new Nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { [new V, 'zz'](f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $v = new V; $v->nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $m = 'zz'; $v->$m(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { Nope2::m(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class W { function i() { return self::nope(f()); } }
try { (new W)->i(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
namespace Foo {
try { nofunc3(\f()); } catch (\Error $e) { echo $e->getMessage(), "\n"; }
try { \Foo\nofunc4(\f()); } catch (\Error $e) { echo $e->getMessage(), "\n"; }
}
--EXPECTF--
Call to undefined method V::nope()
Call to undefined function nofunc()
Call to undefined method V::nope()
Call to undefined function nofunc2()
Call to a member function m() on null
Class "Nope" not found
Call to undefined method V::zz()
Call to undefined method V::nope()
Call to undefined method V::zz()
Class "Nope2" not found
Call to undefined method W::nope()
Call to undefined function Foo\nofunc3()
Call to undefined function Foo\nofunc4()
