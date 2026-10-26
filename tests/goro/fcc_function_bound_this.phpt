--TEST--
A closure of a function given a $this runs with it in the dummy Closure scope
--FILE--
<?php
class A { public $p = 1; }
class B {}
function ff($x = 5, &$y = null) { $y = 9; return [$x, isset($this) ? get_class($this) : null]; }
function tr() { throw new Exception("x"); }
$b = ff(...)->bindTo(new A);
echo new ReflectionFunction($b);
$z = 0;
var_dump($b(1, $z), $z);
var_dump($b(y: $z, x: 3));
$c = $b->bindTo(new B);
var_dump($c);
var_dump($c ? $c() : null);
$d = $b->bindTo(null);
var_dump($d);
$e = Closure::bind(ff(...), new A, 'static');
var_dump($e);
var_dump($b == ff(...)->bindTo($b->call(new A, 1) ? new A : null));
$t = tr(...)->bindTo(new A);
try { $t(); } catch (Exception $ex) { echo $ex->getTraceAsString(), "\n"; }
var_dump((new ReflectionFunction($b))->getClosureScopeClass()->name, (new ReflectionFunction($b))->getClosureCalledClass()->name);
$m = new ReflectionMethod($b, '__invoke');
echo $m;
$s = strlen(...)->bindTo(new A);
var_dump($s('ab'));
var_dump(call_user_func($b, 4), array_map($b, [1]));

$c = function($x = 5) { return $this; };
$b=$c->bindTo(new A); echo new ReflectionMethod($b,"__invoke"); echo new ReflectionFunction($b);
var_dump(count((new ReflectionMethod($b,"__invoke"))->getParameters()));
function fb($x = 5, &$y = null) { return $this; }
$b=fb(...)->bindTo(new A); var_dump(count((new ReflectionMethod($b,"__invoke"))->getParameters()), (new ReflectionMethod($b,"__invoke"))->getNumberOfParameters(), (new ReflectionFunction($b))->getNumberOfParameters());
--EXPECTF--
Closure [ <user> public method ff ] {
  @@ %s 4 - 4

  - Parameters [2] {
    Parameter #0 [ <optional> $x = 5 ]
    Parameter #1 [ <optional> &$y = NULL ]
  }
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  string(1) "A"
}
int(9)
array(2) {
  [0]=>
  int(3)
  [1]=>
  string(1) "A"
}

Warning: Cannot bind method Closure::ff() to object of class B, this will be an error in PHP 9 in %s on line 11
NULL
NULL

Warning: Cannot unbind $this of method, this will be an error in PHP 9 in %s on line 14
NULL
object(Closure)#5 (3) {
  ["function"]=>
  string(11) "Closure::ff"
  ["this"]=>
  object(A)#4 (1) {
    ["p"]=>
    int(1)
  }
  ["parameter"]=>
  array(2) {
    ["$x"]=>
    string(10) "<optional>"
    ["&$y"]=>
    string(10) "<optional>"
  }
}

Warning: Cannot bind method Closure::ff() to object of class A, this will be an error in PHP 9 in %s on line 18
bool(false)
#0 %s(20): Closure->tr()
#1 {main}
string(7) "Closure"
string(1) "A"
Method [ <internal> public method __invoke ] {

  - Parameters [2] {
    Parameter #0 [ <optional> $x = <default> ]
    Parameter #1 [ <optional> &$y = <default> ]
  }
}
int(2)
array(2) {
  [0]=>
  int(4)
  [1]=>
  string(1) "A"
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    string(1) "A"
  }
}
Method [ <internal> public method __invoke ] {

  - Parameters [1] {
    Parameter #0 [ <optional> $x = <default> ]
  }
}
Closure [ <user> public method {closure:%s:28} ] {
  @@ %s 28 - 28

  - Parameters [1] {
    Parameter #0 [ <optional> $x = 5 ]
  }
}
int(1)
int(2)
int(2)
int(2)
