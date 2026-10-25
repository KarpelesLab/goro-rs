--TEST--
Reflection: private parent constants, declaring class of constants and properties
--FILE--
<?php
interface I { const IC = 1; }
interface J extends I { const JC = 2; }
class A { private const P=1; /** docR */ const R=3; protected const Q = 4; }
class B extends A { const R=30; }
class C implements J {}
$r=new ReflectionClass('B');
var_dump($r->hasConstant('P'), $r->getReflectionConstant('P'), $r->getConstant('P'));
var_dump($r->hasConstant('Q'), $r->getConstant('Q'));
try { new ReflectionClassConstant('B','P'); } catch (Exception $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$c = new ReflectionClassConstant('B','R');
var_dump($c->class, $c->getDeclaringClass()->name, $c->getDocComment());
foreach ($r->getReflectionConstants() as $k) echo $k->name, " ", $k->class, "\n";
var_dump($r->getConstants());
$rc = new ReflectionClass('C');
foreach ($rc->getReflectionConstants() as $k) echo $k->name, " ", $k->class, "\n";
var_dump((new ReflectionClassConstant('C','IC'))->class);
var_dump((new ReflectionClassConstant('C','JC'))->getDeclaringClass()->name);
var_dump($rc->getReflectionConstant('IC')->class);
$ra = new ReflectionClass('A');
var_dump($ra->hasConstant('P'), $ra->getConstants());
class A3{public $x; public static $s;} class B3 extends A3{}
var_dump((new ReflectionProperty('B3','x'))->class, (new ReflectionProperty('B3','s'))->class);
var_dump((new ReflectionClass('B3'))->getProperty('x')->class);
foreach ((new ReflectionClass('B3'))->getProperties() as $p) echo $p->name, " ", $p->class, "\n";
--EXPECTF--

Deprecated: ReflectionClass::getConstant() for a non-existent constant is deprecated, use ReflectionClass::hasConstant() to check if the constant exists in %s on line 8
bool(false)
bool(false)
bool(false)
bool(true)
int(4)
ReflectionException: Constant B::P does not exist
string(1) "B"
string(1) "B"
bool(false)
R B
Q A
array(2) {
  ["R"]=>
  int(30)
  ["Q"]=>
  int(4)
}
JC J
IC I
string(1) "I"
string(1) "J"
string(1) "I"
bool(true)
array(3) {
  ["P"]=>
  int(1)
  ["R"]=>
  int(3)
  ["Q"]=>
  int(4)
}
string(2) "A3"
string(2) "A3"
string(2) "A3"
x A3
s A3
