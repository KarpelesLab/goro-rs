--TEST--
ReflectionClass of a child: the parent's private constants, properties (static too) and methods
--FILE--
<?php
class A { private const P = 1; protected const Q = 2; const R = 3; }
class B extends A { const R = 30; }
$r = new ReflectionClass('B');
var_dump($r->getConstants(), $r->hasConstant('P'), $r->hasConstant('Q'));
var_dump(array_map(fn($c) => $c->class . '::' . $c->name, $r->getReflectionConstants()));
var_dump($r->getReflectionConstant('P'));
var_dump((new ReflectionClassConstant('B', 'R'))->class);
try { new ReflectionClassConstant('B', 'P'); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
class SP { private static $ps = 1; private $pi = 2; private function pm() {} }
class SC extends SP {}
$r = new ReflectionClass('SC');
var_dump($r->hasProperty('ps'), $r->hasProperty('pi'), $r->hasMethod('pm'));
var_dump(array_map(fn($p) => $p->class . '::' . $p->name, $r->getProperties()));
var_dump(array_map(fn($p) => $p->class . '::' . $p->name, $r->getMethods()));
try { new ReflectionProperty('SC', 'pi'); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
try { var_dump((new ReflectionMethod('SC', 'pm'))->class); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
?>
--EXPECT--
array(2) {
  ["R"]=>
  int(30)
  ["Q"]=>
  int(2)
}
bool(false)
bool(true)
array(2) {
  [0]=>
  string(4) "B::R"
  [1]=>
  string(4) "A::Q"
}
bool(false)
string(1) "B"
ReflectionException: Constant B::P does not exist
bool(false)
bool(false)
bool(true)
array(0) {
}
array(0) {
}
ReflectionException: Property SC::$pi does not exist
string(2) "SP"
