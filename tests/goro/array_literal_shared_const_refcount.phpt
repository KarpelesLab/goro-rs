--TEST--
Constant array literals are shared IS_CONST operands (debug_zval_dump() refcounts)
--FILE--
<?php
// A constant array literal is one IS_CONST operand every evaluation shares.
debug_zval_dump([1, 2]);
debug_zval_dump([]);
$a = [1, 2]; debug_zval_dump($a);
$b = $a; debug_zval_dump($a);
foreach ([1, 2] as $v) { debug_zval_dump([3, 4]); }
debug_zval_dump(['ab', 'k' => 'cd', ['ef'], 'g']);
$n = [1, [2, 3]]; debug_zval_dump($n[1]); debug_zval_dump($n);
$x = [1, 2]; $x[] = 3; debug_zval_dump($x);
function r() { return [1, 2]; }
debug_zval_dump(r()); $q = r(); debug_zval_dump($q);
debug_zval_dump([...[1, 2], 'x' => 1]);
debug_zval_dump([PHP_INT_MAX, E_ALL, __LINE__]);
debug_zval_dump([1 + 2, 'a' . 'b', true ? 1 : 2]);
debug_zval_dump([1.5 => 'x']);
debug_zval_dump([true => 'x', 2.0 => 'y', '3' => 'z', '03' => 'w']);
$y = [1, $a]; debug_zval_dump($y);
$m = ['x' => [1]]; $mm = $m['x']; debug_zval_dump($mm);
function h($a = [5]) { debug_zval_dump($a); } h();
function st() { static $z = [11, [12]]; debug_zval_dump($z); $z[] = 1; }
st(); st();
$f = array(); $f[] = 1; array_pop($f); debug_zval_dump($f);
$g = ['k' => 1]; unset($g['k']); debug_zval_dump($g);
--EXPECTF--
array(2) packed refcount(2){
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(0) interned {
}
array(2) packed refcount(3){
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) packed refcount(4){
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) packed refcount(2){
  [0]=>
  int(3)
  [1]=>
  int(4)
}
array(2) packed refcount(2){
  [0]=>
  int(3)
  [1]=>
  int(4)
}
array(4) refcount(2){
  [0]=>
  string(2) "ab" refcount(1)
  ["k"]=>
  string(2) "cd" refcount(1)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    string(2) "ef" refcount(1)
  }
  [2]=>
  string(1) "g" interned
}
array(2) packed refcount(2){
  [0]=>
  int(2)
  [1]=>
  int(3)
}
array(2) packed refcount(3){
  [0]=>
  int(1)
  [1]=>
  array(2) packed refcount(1){
    [0]=>
    int(2)
    [1]=>
    int(3)
  }
}
array(3) packed refcount(2){
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) packed refcount(2){
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
array(3) refcount(2){
  [0]=>
  int(1)
  [1]=>
  int(2)
  ["x"]=>
  int(1)
}
array(3) packed refcount(2){
  [0]=>
  int(9223372036854775807)
  [1]=>
  int(30719)
  [2]=>
  int(14)
}
array(3) packed refcount(2){
  [0]=>
  int(3)
  [1]=>
  string(2) "ab" refcount(1)
  [2]=>
  int(1)
}

Deprecated: Implicit conversion from float 1.5 to int loses precision in %s on line 16
array(1) packed refcount(1){
  [1]=>
  string(1) "x" interned
}
array(4) refcount(2){
  [1]=>
  string(1) "x" interned
  [2]=>
  string(1) "y" interned
  [3]=>
  string(1) "z" interned
  ["03"]=>
  string(1) "w" interned
}
array(2) packed refcount(2){
  [0]=>
  int(1)
  [1]=>
  array(2) packed refcount(4){
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
}
array(1) packed refcount(3){
  [0]=>
  int(1)
}
array(1) packed refcount(3){
  [0]=>
  int(5)
}
array(2) packed refcount(3){
  [0]=>
  int(11)
  [1]=>
  array(1) packed refcount(1){
    [0]=>
    int(12)
  }
}
array(3) packed refcount(2){
  [0]=>
  int(11)
  [1]=>
  array(1) packed refcount(2){
    [0]=>
    int(12)
  }
  [2]=>
  int(1)
}
array(0) packed refcount(2){
}
array(0) refcount(2){
}
