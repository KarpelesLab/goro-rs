--TEST--
ReflectionClass::getProperties(): own properties in source order, then inherited ones
--FILE--
<?php
class A { public $a = 1; public static $s0; }
class B extends A { protected $c = 1; public $a = 2; public static $s1; }
foreach ((new ReflectionClass("B"))->getProperties() as $p) echo $p->getName(), " ";
echo "\n"; var_dump(new B); print_r(get_class_vars("B"));
--EXPECT--
c a s1 s0 
object(B)#4 (2) {
  ["a"]=>
  int(2)
  ["c":protected]=>
  int(1)
}
Array
(
    [a] => 2
    [s1] => 
    [s0] => 
)
