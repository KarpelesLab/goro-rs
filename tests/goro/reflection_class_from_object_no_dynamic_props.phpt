--TEST--
Only a ReflectionObject sees the reflected object's dynamic properties
--FILE--
<?php
#[AllowDynamicProperties]
class A { public $decl = 1; }
$a = new A; $a->dyn = 3;
class MyRO extends ReflectionObject {}
foreach ([new ReflectionClass($a), new ReflectionObject($a), new MyRO($a)] as $r) {
  echo get_class($r), "\n";
  var_dump($r->hasProperty('dyn'), count($r->getProperties()));
  try { var_dump($r->getProperty('dyn')->name); } catch (ReflectionException $e) { echo $e->getMessage(), "\n"; }
}
--EXPECT--
ReflectionClass
bool(false)
int(1)
Property A::$dyn does not exist
ReflectionObject
bool(true)
int(2)
string(3) "dyn"
MyRO
bool(true)
int(2)
string(3) "dyn"
