--TEST--
__sleep() warnings print the name up to its first NUL byte
--FILE--
<?php
class K { function __sleep() { return ["\0*\0b"]; } }
echo serialize(new K), "\n";
class K3 { public $a = 1; function __sleep() { return ["a", "a", "x\0y"]; } }
echo serialize(new K3), "\n";
--EXPECTF--

Warning: serialize(): "" returned as member variable from __sleep() but does not exist in %s on line 3
O:1:"K":0:{}

Warning: serialize(): "a" is returned from __sleep() multiple times in %s on line 5

Warning: serialize(): "x" returned as member variable from __sleep() but does not exist in %s on line 5
O:2:"K3":1:{s:1:"a";i:1;}
