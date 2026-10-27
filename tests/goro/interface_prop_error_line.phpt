--TEST--
Interface property errors are reported on the property's line
--FILE--
<?php
interface I {
    public function f();

    public $prop { get; }
    protected $p2 {
        get;
    }
}
--EXPECTF--

Fatal error: Property in interface cannot be protected or private in %s on line 6
