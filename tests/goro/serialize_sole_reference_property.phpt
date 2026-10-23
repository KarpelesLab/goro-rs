--TEST--
serialize(): a property reference only the property holds is its value
--FILE--
<?php
echo serialize(unserialize('O:8:"stdClass":1:{s:1:"x";R:1;}')), "\n";
echo serialize(unserialize('a:1:{i:0;R:1;}')), "\n";
echo serialize(unserialize('O:8:"stdClass":2:{s:1:"a";O:8:"stdClass":0:{}s:1:"b";R:2;}')), "\n";
class A { public $a; public $b; }
$o = new A; $x = new stdClass; $o->a = &$x; unset($x); $o->b = $o->a;
echo serialize($o), "\n";
class S { public $a; public $b; function __sleep() { return ['a', 'b']; } }
$s = new S; $x = new stdClass; $s->a = &$x; unset($x); $s->b = $s->a;
echo serialize($s), "\n";
$s = new S; $x = new stdClass; $s->a = &$x; $s->b = $s->a;
echo serialize($s), "\n";
--EXPECT--
O:8:"stdClass":1:{s:1:"x";r:1;}
a:1:{i:0;N;}
O:8:"stdClass":2:{s:1:"a";O:8:"stdClass":0:{}s:1:"b";R:2;}
O:1:"A":2:{s:1:"a";O:8:"stdClass":0:{}s:1:"b";r:2;}
O:1:"S":2:{s:1:"a";O:8:"stdClass":0:{}s:1:"b";r:2;}
O:1:"S":2:{s:1:"a";O:8:"stdClass":0:{}s:1:"b";r:2;}
