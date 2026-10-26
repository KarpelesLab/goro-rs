--TEST--
Fiber::start() named args, a terminated fiber releases its callable; ReflectionFunction::invokeArgs() frame, ReflectionMethod::invoke() static::
--FILE--
<?php
class D { function __destruct(){ echo "dtor\n"; } }
$d = new D; $g = new Fiber(function() use($d){ echo "run\n"; }); unset($d); $g->start(); echo "after\n";
$f = new Fiber(function(...$a){ var_dump($a); }); $f->start(1, x: 2);
$f = new Fiber(function($a, $b = 5){ var_dump($a, $b); }); $f->start(b: 7, a: 1);
function h($a){ return func_get_args(); }
var_dump((new ReflectionFunction('h'))->invokeArgs([3]));
var_dump((new ReflectionFunction('h'))->invoke(4, 5));
class A { static function who(){ return static::class; } }
class B extends A {}
var_dump((new ReflectionMethod('B','who'))->invoke(null));
var_dump((new ReflectionMethod('B','who'))->invokeArgs(null, []));
var_dump((new ReflectionMethod('A','who'))->invoke(null));
echo "end\n";
?>
--EXPECT--
run
dtor
after
array(2) {
  [0]=>
  int(1)
  ["x"]=>
  int(2)
}
int(1)
int(7)
array(1) {
  [0]=>
  int(3)
}
array(2) {
  [0]=>
  int(4)
  [1]=>
  int(5)
}
string(1) "B"
string(1) "B"
string(1) "A"
end
