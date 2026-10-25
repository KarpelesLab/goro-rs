--TEST--
Calls the INIT opcodes resolve: magic methods, traits, internal ancestors, enums
--FILE--
<?php
function f(){echo "evaluated\n"; return 1;}
trait T { function tm($x) { return "tm$x"; } static function ts($x) { return "ts$x"; } }
class A { use T; function __call($n, $a) { return "call $n"; } static function __callStatic($n, $a) { return "cs $n"; } }
class B extends A { function bm() { return self::zzz(f()); } }
class E extends Exception {}
enum En: int { case X = 1; }
echo (new B)->nothing(f()), "\n";
echo B::nothing(f()), "\n";
echo (new B)->tm(f()), "\n";
echo B::ts(f()), "\n";
echo (new B)->bm(), "\n";
echo (new E("m"))->getMessage(), "\n";
try { (new E("m"))->nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { E::nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
var_dump(En::from(f()));
try { En::nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$o = new class { function m() { return 1; } };
try { $o->nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $x = '\nofunc'; $x(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $x = 'Nope::m'; $x(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { $x = 'B::m'; $x(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class P { private function priv() {} static function sp() {} }
try { DateTime::nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { strlen2(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo strlen(f()), "\n";
function later() { return "later"; }
echo later(), "\n";
try { $c = function() {}; $c->nope(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
class Q { function __get($n) { return 1; } }
try { (new Q)->x(f()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
abstract class Ab { abstract function am(); static function make() { return static::am2(f()); } }
class Cc extends Ab { function am() {} }
try { Cc::make(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { (new Cc)->am(f()); echo "ok\n"; } catch (Error $e) { echo $e->getMessage(), "\n"; }
interface I { function im(); }
--EXPECTF--
evaluated
call nothing
evaluated
cs nothing
evaluated
tm1
evaluated
ts1
evaluated
call zzz
m
Call to undefined method E::nope()
Call to undefined method E::nope()
evaluated
enum(En::X)
Call to undefined method En::nope()
Call to undefined method class@anonymous::nope()
Call to undefined function \nofunc()
Class "Nope" not found
evaluated
Call to undefined method DateTime::nope()
Call to undefined function strlen2()
evaluated
1
later
Call to undefined method Closure::nope()
Call to undefined method Q::x()
Call to undefined method Cc::am2()
evaluated
ok
