--TEST--
A first-class callable of a protected internal method (or parent::m(...)) is a closure over the method, callable from anywhere
--FILE--
<?php
class H extends SplMinHeap {
    function get() { return $this->compare(...); }
    function getS() { return parent::compare(...); }
    function getP() { return $this->insert(...); }
    protected function mine($a) {}
    function getM() { return $this->mine(...); }
}
$h = new H;
echo new ReflectionFunction($h->get());
echo new ReflectionFunction($h->getP());
echo new ReflectionFunction($h->getM());
echo new ReflectionFunction(strlen(...));
$s = $h->getS();
var_dump($s(3, 1));
class A extends ArrayIterator { function g() { return $this->valid(...); } }
echo new ReflectionFunction((new A([]))->g());
--EXPECTF--
Closure [ <internal:SPL, prototype SplHeap> <visibility error> method compare ] {

  - Parameters [2] {
    Parameter #0 [ <required> mixed $value1 ]
    Parameter #1 [ <required> mixed $value2 ]
  }
  - Tentative return [ int ]
}
Closure [ <internal:SPL> public method insert ] {

  - Parameters [1] {
    Parameter #0 [ <required> mixed $value ]
  }
  - Tentative return [ true ]
}
Closure [ <user> <visibility error> method mine ] {
  @@ %s 6 - 6

  - Parameters [1] {
    Parameter #0 [ <required> $a ]
  }
}
Closure [ <internal:Core> function strlen ] {

  - Parameters [1] {
    Parameter #0 [ <required> string $string ]
  }
  - Return [ int ]
}
int(-1)
Closure [ <internal:SPL, prototype Iterator> public method valid ] {

  - Parameters [0] {
  }
  - Tentative return [ bool ]
}
