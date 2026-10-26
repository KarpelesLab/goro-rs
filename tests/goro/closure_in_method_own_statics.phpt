--TEST--
Each closure object created in a method has its own static variables
--FILE--
<?php
class M { function mk() { return function() { static $k = 0; return ++$k; }; } static function smk() { return static function() { static $k = 0; return ++$k; }; } }
$m = (new M)->mk(); $n = (new M)->mk();
var_dump($m(), $m(), $n());
$m = M::smk(); $n = M::smk();
var_dump($m(), $m(), $n());
class N extends M {}
$m = (new N)->mk(); $n = (new N)->mk();
var_dump($m(), $m(), $n());
$m = (new M)->mk(); $b = $m->bindTo(new N); $c = Closure::bind($m, null, N::class);
var_dump($m(), $b(), $b(), $c(), $m());
var_dump((new ReflectionFunction($m))->getStaticVariables());
?>
--EXPECT--
int(1)
int(2)
int(1)
int(1)
int(2)
int(1)
int(1)
int(2)
int(1)
int(1)
int(1)
int(2)
int(1)
int(2)
array(1) {
  ["k"]=>
  int(2)
}
