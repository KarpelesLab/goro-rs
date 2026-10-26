--TEST--
Explicitly abstract class must implement all private abstract trait methods; a concrete class gets the 'contains' message
--FILE--
<?php
trait T { abstract private function m(); abstract private function n(); }
abstract class A { use T; }
--EXPECTF--

Fatal error: Class A must implement 2 abstract methods (A::m, A::n) in %s on line 3
