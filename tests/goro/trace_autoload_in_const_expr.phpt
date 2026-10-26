--TEST--
An autoloader run while a class constant is evaluated: no [constant expression] frame, at the access line
--FILE--
<?php
spl_autoload_register(function($c) { throw new Exception("x"); });
class D { const HW = Missing::y; }
function f() {
    return D::HW;
}
try { f(); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
#0 %s(5): {closure:%s:2}('Missing')
#1 %s(7): f()
#2 {main}
