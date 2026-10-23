--TEST--
__sleep(): an uninitialized typed property is left out without a warning
--FILE--
<?php
class K { function __sleep() { return ["\0*\0b"]; } }
echo serialize(new K), "\n";
class K2 { public ?int $c = 1; public $d = 2; function __sleep() { return ["c", "d"]; } }
$o = new K2; unset($o->c); unset($o->d); echo serialize($o), "\n";
class K3 { public $a = 1; function __sleep() { return ["a", "a", "x\0y"]; } }
echo serialize(new K3), "\n";
class K4 { private int $p; protected string $q; public $r = 3; function __sleep() { return ["p", "q", "r"]; } }
echo serialize(new K4), "\n";
--EXPECTF--

Warning: serialize(): "" returned as member variable from __sleep() but does not exist in %s on line 3
O:1:"K":0:{}

Warning: serialize(): "d" returned as member variable from __sleep() but does not exist in %s on line 5
O:2:"K2":0:{}

Warning: serialize(): "a" is returned from __sleep() multiple times in %s on line 7

Warning: serialize(): "x" returned as member variable from __sleep() but does not exist in %s on line 7
O:2:"K3":1:{s:1:"a";i:1;}
O:2:"K4":1:{s:1:"r";i:3;}
