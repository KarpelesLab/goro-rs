--TEST--
#[\NoDiscard] names a method after its declaring class; is_a()/is_subclass_of() with aliases of interfaces
--FILE--
<?php
class Base { #[\NoDiscard] public function keep(): int { return 1; } #[\NoDiscard("use it")] public static function s(): int { return 2; } }
class Child extends Base {}
$c = new Child;
$c->keep();
Child::s();
(new Base)->keep();
trait T { #[\NoDiscard] function t() { return 1; } }
class U { use T; }
(new U)->t();
interface I {}
class_alias('I', 'IA');
class K implements I {}
var_dump(is_a(new K, 'IA'), is_subclass_of(new K, 'IA'), is_a('K', 'IA', true), is_subclass_of('K', 'IA'), new K instanceof IA, in_array('IA', class_implements('K')));
class_alias('Base', 'BA');
var_dump(is_a($c, 'BA'), is_subclass_of($c, 'BA'), is_subclass_of('Child', 'BA'));
interface J extends I {}
class_alias('J', 'JA');
class L implements JA {}
var_dump(is_a('L', 'IA', true), is_subclass_of('L', 'JA'), is_subclass_of('JA', 'IA'), is_a('JA', 'I', true));
class Base2 { #[\Deprecated] public function dep() {} #[\NoDiscard] public function keep(): int { return 1; } }
class Child2 extends Base2 {}
(new Child2)->dep();
$f = [new Child2, 'keep']; $f();
call_user_func([new Child2, 'keep']);
--EXPECTF--

Warning: The return value of method Base::keep() should either be used or intentionally ignored by casting it as (void) in %s on line 5

Warning: The return value of method Base::s() should either be used or intentionally ignored by casting it as (void), use it in %s on line 6

Warning: The return value of method Base::keep() should either be used or intentionally ignored by casting it as (void) in %s on line 7

Warning: The return value of method U::t() should either be used or intentionally ignored by casting it as (void) in %s on line 10
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(false)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)

Deprecated: Method Base2::dep() is deprecated in %s on line 23

Warning: The return value of method Base2::keep() should either be used or intentionally ignored by casting it as (void) in %s on line 24

Warning: The return value of method Base2::keep() should either be used or intentionally ignored by casting it as (void) in %s on line 25
