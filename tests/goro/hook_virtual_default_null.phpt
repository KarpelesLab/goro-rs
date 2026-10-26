--TEST--
An explicit null default of a virtual hooked property is an error
--FILE--
<?php
class A {
    public $a;
    public $x = null {
        get {
            return 1;
        }
    }
}
--EXPECTF--

Fatal error: Cannot specify default value for virtual hooked property A::$x in %s on line 7
