--TEST--
#[\Deprecated] magic property methods called implicitly, ReflectionFunction/Method::invoke(), call_user_func_array() #[\NoDiscard], constant()
--FILE--
<?php
class C {
    #[\Deprecated] function __get($n) { return 1; }
    #[\Deprecated("m")] function __set($n, $v) { }
    #[\Deprecated] function __isset($n) { return true; }
    #[\Deprecated] function __unset($n) { }
}
$c = new C;
var_dump($c->a);
$c->b = 2;
var_dump(isset($c->x));
unset($c->y);

#[\Deprecated] function df() { return 5; }
$rf = new ReflectionFunction('df');
var_dump($rf->invoke());
var_dump($rf->invokeArgs([]));
class M { #[\Deprecated] function m() { return 6; } #[\Deprecated] static function s() { return 7; } }
$rm = new ReflectionMethod('M', 'm');
var_dump($rm->invoke(new M));
var_dump($rm->invokeArgs(new M, []));

#[\NoDiscard] function nd() { return 1; }
call_user_func('nd');
call_user_func_array('nd', []);

#[\Deprecated] const D1 = 1;
var_dump(constant('D1'));
var_dump(D1);
class CI {
    #[\Deprecated] function __isset($n) { debug_print_backtrace(); return true; }
}
$ci = new CI;
var_dump(isset($ci->x));
var_dump(empty($ci->x));
var_dump($ci->x ?? 5);
--EXPECTF--

Deprecated: Method C::__get() is deprecated in %s on line 9
int(1)

Deprecated: Method C::__set() is deprecated, m in %s on line 10

Deprecated: Method C::__isset() is deprecated in %s on line 11
bool(true)

Deprecated: Method C::__unset() is deprecated in %s on line 12

Deprecated: Function df() is deprecated in %s on line 16
int(5)

Deprecated: Function df() is deprecated in %s on line 17
int(5)

Deprecated: Method M::m() is deprecated in %s on line 20
int(6)

Deprecated: Method M::m() is deprecated in %s on line 21
int(6)

Warning: The return value of function nd() should either be used or intentionally ignored by casting it as (void) in %s on line 24

Warning: The return value of function nd() should either be used or intentionally ignored by casting it as (void) in %s on line 25

Deprecated: Constant D1 is deprecated in %s on line 28
int(1)

Deprecated: Constant D1 is deprecated in %s on line 29
int(1)

Deprecated: Method CI::__isset() is deprecated in %s on line 34
#0 %s(34): CI->__isset('x')
bool(true)

Deprecated: Method CI::__isset() is deprecated in %s on line 35
#0 %s(35): CI->__isset('x')
bool(true)

Deprecated: Method CI::__isset() is deprecated in %s on line 36
#0 %s(36): CI->__isset('x')
int(5)
