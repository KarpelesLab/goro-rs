--TEST--
ReflectionFunction::__toString() of closures: bound variables, parameters, method and internal closures
--FILE--
<?php
$x = 1;
$c = function (int $a, &$b = null) use ($x): ?string { return null; };
echo new ReflectionFunction($c);
$s = static fn() => 1;
echo new ReflectionFunction($s);
class K {
    function m(self $a, ?self $b): static { return $this; }
    function n(): self { return $this; }
    function c() { return function () {}; }
}
class L extends K { function p(parent $p): parent|int { return $p; } }
$m = new ReflectionMethod('K', 'm');
foreach ($m->getParameters() as $p) { $t = $p->getType(); var_dump(get_class($t), $t->getName(), $t->isBuiltin(), (string)$t); }
$t = (new ReflectionMethod('K', 'n'))->getReturnType(); var_dump($t->getName(), $t->isBuiltin());
$t = (new ReflectionMethod('L', 'p'))->getParameters()[0]->getType(); var_dump($t->getName(), $t->isBuiltin());
$t = (new ReflectionMethod('L', 'p'))->getReturnType(); var_dump((string)$t);
echo new ReflectionFunction((new K)->c());
echo new ReflectionFunction(strlen(...));
$y = 2;
echo new ReflectionFunction(fn(int $a) => $a + $x + $y);
echo new ReflectionFunction(function (...$r) use (&$x, $y) { return $r; });
class K2 {
    public $p = 1;
    function m() { $z = 3; return function (string $s = "d") use ($z) { return $this->p; }; }
    static function s2() { return static function () {}; }
}
echo new ReflectionFunction((new K2)->m());
echo new ReflectionFunction(K::s2());
$b = Closure::bind(function ($q) { return $q; }, new K2, K2::class);
echo new ReflectionFunction($b);
echo new ReflectionFunction(Closure::fromCallable('strtoupper'));
echo new ReflectionFunction((new K2)->m(...));
function named(int $i, $j = [1]) {}
echo new ReflectionFunction(named(...));
echo new ReflectionFunction('named');
--EXPECTF--
Closure [ <user> function {closure:%s:3} ] {
  @@ %s 3 - 3

  - Bound Variables [1] {
      Variable #0 [ $x ]
  }

  - Parameters [2] {
    Parameter #0 [ <required> int $a ]
    Parameter #1 [ <optional> &$b = NULL ]
  }
  - Return [ ?string ]
}
Closure [ <user> static function {closure:%s:5} ] {
  @@ %s 5 - 5
}
string(19) "ReflectionNamedType"
string(1) "K"
bool(false)
string(1) "K"
string(19) "ReflectionNamedType"
string(1) "K"
bool(false)
string(2) "?K"
string(1) "K"
bool(false)
string(1) "K"
bool(false)
string(5) "K|int"
Closure [ <user> public method {closure:K::c():10} ] {
  @@ %s 10 - 10
}
Closure [ <internal:Core> function strlen ] {

  - Parameters [1] {
    Parameter #0 [ <required> string $string ]
  }
  - Return [ int ]
}
Closure [ <user> function {closure:%s:21} ] {
  @@ %s 21 - 21

  - Bound Variables [2] {
      Variable #0 [ $x ]
      Variable #1 [ $y ]
  }

  - Parameters [1] {
    Parameter #0 [ <required> int $a ]
  }
}
Closure [ <user> function {closure:%s:22} ] {
  @@ %s 22 - 22

  - Bound Variables [2] {
      Variable #0 [ $x ]
      Variable #1 [ $y ]
  }

  - Parameters [1] {
    Parameter #0 [ <optional> ...$r ]
  }
}
Closure [ <user> public method {closure:K2::m():25} ] {
  @@ %s 25 - 25

  - Bound Variables [1] {
      Variable #0 [ $z ]
  }

  - Parameters [1] {
    Parameter #0 [ <optional> string $s = 'd' ]
  }
}

Fatal error: Uncaught Error: Call to undefined method K::s2() in %s:29
Stack trace:
#0 {main}
  thrown in %s on line 29
