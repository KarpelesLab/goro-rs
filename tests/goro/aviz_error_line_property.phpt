--TEST--
Asymmetric visibility errors are reported at the property's line
--FILE--
<?php
class C {

    private(set) public $x;
}
--EXPECTF--

Fatal error: Property with asymmetric visibility C::$x must have type in %s on line 4
