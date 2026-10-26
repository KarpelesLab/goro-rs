--TEST--
ReflectionFunction: getStaticVariables() copies an unshared by-ref use as its value; a method closure lists its statics as bound variables
--FILE--
<?php
function g() { $y = [2]; return function () use (&$y) { static $s = 1; }; }
$rf = new ReflectionFunction(g());
var_dump($rf->getStaticVariables());
class A { function m(int $z) { static $cnt = 0; } }
echo new ReflectionFunction((new A)->m(...));
echo new ReflectionMethod(Closure::fromCallable([new A, 'm']), '__invoke');
--EXPECTF--
array(2) {
  ["y"]=>
  array(1) {
    [0]=>
    int(2)
  }
  ["s"]=>
  int(1)
}
Closure [ <user> public method m ] {
  @@ %s 5 - 5

  - Bound Variables [1] {
      Variable #0 [ $cnt ]
  }

  - Parameters [1] {
    Parameter #0 [ <required> int $z ]
  }
}
Method [ <internal> public method __invoke ] {

  - Parameters [1] {
    Parameter #0 [ <required> int $z ]
  }
}
