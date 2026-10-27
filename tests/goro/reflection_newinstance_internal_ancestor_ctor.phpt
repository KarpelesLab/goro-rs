--TEST--
ReflectionClass::newInstance[Args]() with arguments on a class whose constructor is an internal ancestor's (Exception)
--FILE--
<?php
abstract class PE extends Exception {}
class Mock_PE extends PE { public function foo() {} }
foreach (['PE', 'Mock_PE', 'Exception', 'ErrorException', 'ArrayObject'] as $c) {
    $r = new ReflectionClass($c);
    $ct = $r->getConstructor();
    echo $c, ": ", $ct ? $ct->class . '::' . $ct->name : 'NULL', " ", var_export($r->hasMethod('__construct'), true), "\n";
}
class A2 extends ArrayObject {}
$r = new ReflectionClass('A2'); var_dump($r->getConstructor()?->class);
eval('class Mock_PE2 extends PE { }');
$r = new ReflectionClass('Mock_PE2'); var_dump($r->getConstructor()?->class);
$o = $r->newInstanceArgs(['msg']); var_dump($o->getMessage());
$o = $r->newInstance("m2", 7); var_dump($o->getMessage(), $o->getCode());
class NoCtor {}
try { (new ReflectionClass("NoCtor"))->newInstanceArgs([1]); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
try { (new ReflectionClass("NoCtor"))->newInstance(1); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
class E2 extends ErrorException {} var_dump((new ReflectionClass("E2"))->newInstanceArgs(["x", 1, E_WARNING])->getSeverity());

?>
--EXPECTF--
PE: Exception::__construct true
Mock_PE: Exception::__construct true
Exception: Exception::__construct true
ErrorException: ErrorException::__construct true
ArrayObject: ArrayObject::__construct true
string(11) "ArrayObject"
string(9) "Exception"
string(3) "msg"
string(2) "m2"
int(7)
Class NoCtor does not have a constructor, so you cannot pass any constructor arguments
Class NoCtor does not have a constructor, so you cannot pass any constructor arguments
int(2)
