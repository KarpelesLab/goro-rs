--TEST--
The abstract methods error lists at most 3 methods, in table order
--FILE--
<?php
interface I { function d(); function a(); function c(); function b(); }
class X implements I { function c() {} }
--EXPECTF--

Fatal error: Class X contains 3 abstract methods and must therefore be declared abstract or implement the remaining methods (I::d, I::a, I::b) in %s on line 3
