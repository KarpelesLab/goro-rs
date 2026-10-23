--TEST--
ArrayObject::append() calls an overridden offsetSet()
--FILE--
<?php
class A extends ArrayObject{function offsetSet($k,$v):void{echo "set(",var_export($k,true),")\n";parent::offsetSet($k,$v);}}
$a = new A; $a->append(1); $a[] = 2; $a['x'] = 3; var_dump($a->getArrayCopy());
class B extends ArrayIterator{function offsetSet($k,$v):void{echo "B set(",var_export($k,true),")\n";parent::offsetSet($k,$v);}}
$b = new B([]); $b->append(1); var_dump($b->getArrayCopy());
class C extends ArrayObject{function offsetSet($k,$v):void{throw new Exception("no");}}
try { (new C)->append(1); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
set(NULL)
set(NULL)
set('x')
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  ["x"]=>
  int(3)
}
B set(NULL)
array(1) {
  [0]=>
  int(1)
}
#0 [internal function]: C->offsetSet(NULL, 1)
#1 %s(7): ArrayObject->append(1)
#2 {main}
