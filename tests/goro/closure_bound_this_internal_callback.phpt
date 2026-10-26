--TEST--
A closure bound with bindTo() keeps $this through internal callers; Closure::getCurrent() from array_map()/call_user_func()
--FILE--
<?php
class A { public $v = "A"; }
$d = (function() { return $this->v; })->bindTo(new A);
var_dump($d());
var_dump(call_user_func($d));
var_dump(call_user_func_array($d, []));
var_dump(array_map($d, [1]));
$f = new Fiber($d); var_dump($f->start());
var_dump((new ReflectionFunction($d))->invoke());
$e = Closure::bind(function() { return $this->v; }, new A, A::class);
var_dump(call_user_func($e));
$g = fn($x) => Closure::getCurrent();
var_dump(call_user_func($g, 1) === $g);
var_dump(array_map($g, [1])[0] === $g);
$g = function($x) { return Closure::getCurrent(); };
var_dump(array_map($g, [1])[0] === $g);
$h = fn($x) => Closure::getCurrent();
var_dump(array_map($h, [1])[0] === $h);
var_dump(call_user_func_array($h, [1]) === $h);
$a=[2,1]; usort($a, function($x,$y){ global $r; $r = Closure::getCurrent(); return $x<=>$y;}); var_dump($r instanceof Closure);
?>
--EXPECT--
string(1) "A"
string(1) "A"
string(1) "A"
array(1) {
  [0]=>
  string(1) "A"
}
NULL
string(1) "A"
string(1) "A"
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
bool(true)
