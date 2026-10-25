--TEST--
Closure getStaticVariables() includes use vars; Bound Variables lists statics
--FILE--
<?php
$a = 1; $b = 2; $o = new stdClass;
$c = function() use($a, &$b, $o) { static $s = 3; static $t; return $s; };
$r = new ReflectionFunction($c);
var_dump($r->getStaticVariables());
var_dump(array_keys($r->getClosureUsedVariables()));
echo $r;
$c();
var_dump($r->getStaticVariables());
$b = [5];
$f = fn() => $a + $b[0];
$r = new ReflectionFunction($f);
var_dump($r->getClosureUsedVariables(), $r->getStaticVariables());
echo $r;
$g = function() { static $z = 9; };
var_dump((new ReflectionFunction($g))->getStaticVariables());
echo new ReflectionFunction($g);
--EXPECTF--
array(5) {
  ["a"]=>
  int(1)
  ["b"]=>
  &int(2)
  ["o"]=>
  object(stdClass)#1 (0) {
  }
  ["s"]=>
  int(3)
  ["t"]=>
  NULL
}
array(3) {
  [0]=>
  string(1) "a"
  [1]=>
  string(1) "b"
  [2]=>
  string(1) "o"
}
Closure [ <user> function {closure:%s:3} ] {
  @@ %s 3 - 3

  - Bound Variables [5] {
      Variable #0 [ $a ]
      Variable #1 [ $b ]
      Variable #2 [ $o ]
      Variable #3 [ $s ]
      Variable #4 [ $t ]
  }
}
array(5) {
  ["a"]=>
  int(1)
  ["b"]=>
  &int(2)
  ["o"]=>
  object(stdClass)#1 (0) {
  }
  ["s"]=>
  int(3)
  ["t"]=>
  NULL
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  array(1) {
    [0]=>
    int(5)
  }
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  array(1) {
    [0]=>
    int(5)
  }
}
Closure [ <user> function {closure:%s:11} ] {
  @@ %s 11 - 11

  - Bound Variables [2] {
      Variable #0 [ $a ]
      Variable #1 [ $b ]
  }
}
array(1) {
  ["z"]=>
  int(9)
}
Closure [ <user> function {closure:%s:15} ] {
  @@ %s 15 - 15

  - Bound Variables [1] {
      Variable #0 [ $z ]
  }
}
