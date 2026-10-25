--TEST--
A compatible trait property replaces the inherited one
--FILE--
<?php
trait T { public $c = "t"; protected $p = 1; public static $s = 's'; public ?int $typed = null; }
class A { protected $c = 5; protected $p = 0; public static $s = 'a'; public ?int $typed = 3; }
class B extends A { use T; }
var_dump(new B, B::$s);
--EXPECT--
object(B)#1 (3) {
  ["c"]=>
  string(1) "t"
  ["p":protected]=>
  int(1)
  ["typed"]=>
  NULL
}
string(1) "s"
