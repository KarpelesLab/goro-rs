--TEST--
debug_zval_dump(): objects with a destructor show their real refcount
--FILE--
<?php
class A { public $a = 1; public function __destruct() { echo "dtor\n"; } }
$x = new A;
debug_zval_dump($x);
$y = [new A];
debug_zval_dump($y);
--EXPECT--
object(A)#1 (1) refcount(2){
  ["a"]=>
  int(1)
}
array(1) packed refcount(2){
  [0]=>
  object(A)#2 (1) refcount(1){
    ["a"]=>
    int(1)
  }
}
dtor
dtor
