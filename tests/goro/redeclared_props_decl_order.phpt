--TEST--
Redeclared properties are listed in the class's declaration order (properties_info)
--FILE--
<?php
class A { public $a=1; public $b=2; static public $sa=1; static protected $sb=2; }
class B extends A { private $c=0; public $b=3; public $a=4; static private $sc = 0; static public $sb=5; static public $sa=6;}
echo implode(" ", array_map(fn($p) => $p->name, (new ReflectionClass("B"))->getProperties())), "\n";
print_r((new ReflectionClass("B"))->getStaticProperties()); print_r(get_object_vars(new B)); print_r(get_class_vars("B"));
?>
--EXPECT--
c b a sc sb sa
Array
(
    [sc] => 0
    [sb] => 5
    [sa] => 6
)
Array
(
    [a] => 4
    [b] => 3
)
Array
(
    [b] => 3
    [a] => 4
    [sb] => 5
    [sa] => 6
)
