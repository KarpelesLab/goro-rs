--TEST--
__sleep() naming a virtual (hooked) property: it does not exist
--FILE--
<?php
class A {
    public $b = 2;
    public $v { get => 1; }
    public $w = 3 { get => $this->w * 2; }
    function __sleep() { return ['v', 'b', 'w']; }
}
var_dump(serialize(new A));
--EXPECTF--

Warning: serialize(): "v" returned as member variable from __sleep() but does not exist in %s on line 8
string(36) "O:1:"A":2:{s:1:"b";i:2;s:1:"w";i:3;}"
