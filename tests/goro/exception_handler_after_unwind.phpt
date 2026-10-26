--TEST--
The user exception handler runs after the stack is unwound
--FILE--
<?php
function f() { throw new Exception("x"); }
set_exception_handler(function($e) {
    var_dump(count(debug_backtrace()));
    debug_print_backtrace();
    set_exception_handler(function ($e) { echo "second: ", $e->getMessage(), "\n", $e->getTraceAsString(), "\n"; });
    throw new Exception("from handler");
});
class A { function m() { f(); } static function s() { (new A)->m(); } }
A::s();
--EXPECTF--
int(1)
#0 [internal function]: {closure:%s:3}(Object(Exception))
second: from handler
#0 [internal function]: {closure:%s:3}(Object(Exception))
#1 {main}
