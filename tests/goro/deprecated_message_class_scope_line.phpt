--TEST--
#[\Deprecated] message evaluated in a class scope reports its diagnostics on the expression's line
--FILE--
<?php
class K {
    #[\Deprecated]
    const OLD = "old msg";
}
#[\Deprecated(
    K::OLD
)]
function f() {}
f();
#[\Deprecated(UNDEF_C)]
function g() {}
try {
    g();
} catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), " @", $e->getLine(), "\n"; echo $e->getTraceAsString(), "\n"; }
class M {
    #[\Deprecated(K::OLD)]
    public function m() {}
}
(new M)->m();
--EXPECTF--

Deprecated: Constant K::OLD is deprecated in %s on line 10

Deprecated: Function f() is deprecated, old msg in %s on line 10
Error: Undefined constant "UNDEF_C" @14
#0 {main}

Deprecated: Constant K::OLD is deprecated in %s on line 17

Deprecated: Method M::m() is deprecated, old msg in %s on line 20
