--TEST--
Cannot redeclare a property: reported at the redeclaration
--FILE--
<?php
class A {
    public $c = 0;

    protected $c = 1;
}
--EXPECTF--

Fatal error: Cannot redeclare A::$c in %s on line 5
