--TEST--
Arrays of constants, class constants, static and default properties: php-src's holders only
--FILE--
<?php
// Constant and class arrays: the holders php-src has (the literal, the
// constants table, the default and static members tables), not goro's
// compiled declaration or run-time caches.
class K { const X = [1, [2]]; public static $s = [3, [4]]; public $p = [5, [6]]; }
debug_zval_dump(K::X);
debug_zval_dump(K::X);
debug_zval_dump(K::$s);
$k = new K;
debug_zval_dump($k->p);
debug_zval_dump($k);
class N { public static $s = [3]; }
debug_zval_dump(N::$s);
const C = [7, [8]];
debug_zval_dump(C);
debug_zval_dump(C[1]);
const C2 = [1, 2];
debug_zval_dump(C2);
define('D', [9, [10]]);
debug_zval_dump(D);
for ($i = 0; $i < 2; $i++) { debug_zval_dump(C2); }
--EXPECT--
array(2) packed refcount(2){
  [0]=>
  int(1)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(2)
  }
}
array(2) packed refcount(2){
  [0]=>
  int(1)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(2)
  }
}
array(2) packed refcount(3){
  [0]=>
  int(3)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(4)
  }
}
array(2) packed refcount(3){
  [0]=>
  int(5)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(6)
  }
}
object(K)#1 (1) refcount(2){
  ["p"]=>
  array(2) packed refcount(2){
    [0]=>
    int(5)
    [1]=>
    array(1) packed refcount(1){
      [0]=>
      int(6)
    }
  }
}
array(1) packed refcount(3){
  [0]=>
  int(3)
}
array(2) packed refcount(3){
  [0]=>
  int(7)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(8)
  }
}
array(1) packed refcount(2){
  [0]=>
  int(8)
}
array(2) packed refcount(3){
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) packed refcount(2){
  [0]=>
  int(9)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(10)
  }
}
array(2) packed refcount(3){
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) packed refcount(3){
  [0]=>
  int(1)
  [1]=>
  int(2)
}
