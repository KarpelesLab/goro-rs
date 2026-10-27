--TEST--
Multi-line static call / property assignment / unary operators / sprintf(): opline lines (k01)
--FILE--
<?php
class A { static function s() {} }
A
    ::zz();
--EXPECTF--

Fatal error: Uncaught Error: Call to undefined method A::zz() in %s:4
Stack trace:
#0 {main}
  thrown in %s on line 4
