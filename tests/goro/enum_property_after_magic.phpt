--TEST--
An enum property after a forbidden magic method
--FILE--
<?php
enum E {
    case A;
    public function __clone() {}
    public $x;
}
--EXPECTF--

Fatal error: Enum E cannot include properties in %s on line 5
