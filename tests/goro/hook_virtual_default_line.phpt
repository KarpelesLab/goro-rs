--TEST--
A virtual hooked property default is reported after its last hook
--FILE--
<?php
class A {
    public $a;
    public $x = 5 {
        get {
            return 1;
        }
    }
}
--EXPECTF--

Fatal error: Cannot specify default value for virtual hooked property A::$x in %s on line 7
