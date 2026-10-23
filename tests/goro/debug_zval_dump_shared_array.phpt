--TEST--
debug_zval_dump(): copies of an array sharing its storage count as holders
--FILE--
<?php
function f() { $y = [1, mt_rand(1,1)]; debug_zval_dump($y); $b = $y; debug_zval_dump($y); $c = $y; debug_zval_dump($y); }
f();
$x = [new stdClass]; $r = [1 => &$x]; unset($x);
$a = $r[1];
debug_zval_dump($r[1]);
$p = unserialize('a:3:{i:1;N;i:2;a:1:{i:0;O:8:"stdClass":0:{}}i:1;R:3;}');
debug_zval_dump($p);
$a = $p[1];
debug_zval_dump($p[1]);
$p = unserialize('a:3:{i:1;N;i:2;O:8:"stdClass":1:{s:8:"filename";a:1:{i:0;O:8:"stdClass":0:{}}}i:1;R:4;}');
debug_zval_dump($p);
$a = $p[1];
debug_zval_dump($p[1]);
$x = [new stdClass]; $a = $x; debug_zval_dump($x);
$y = [new stdClass]; $r = &$y; $b = $r; debug_zval_dump($y);
$z = [new stdClass]; $arr = [&$z]; $c = $arr[0]; debug_zval_dump($z);
$w = [new stdClass]; $arr2 = [&$w]; $d = $arr2[0]; debug_zval_dump($arr2[0]);
?>
--EXPECT--
array(2) packed refcount(2){
  [0]=>
  int(1)
  [1]=>
  int(1)
}
array(2) packed refcount(3){
  [0]=>
  int(1)
  [1]=>
  int(1)
}
array(2) packed refcount(4){
  [0]=>
  int(1)
  [1]=>
  int(1)
}
array(1) packed refcount(3){
  [0]=>
  object(stdClass)#1 (0) refcount(1){
  }
}
array(2) refcount(2){
  [1]=>
  reference refcount(2) {
    array(1) refcount(1){
      [0]=>
      object(stdClass)#2 (0) refcount(1){
      }
    }
  }
  [2]=>
  reference refcount(2) {
    array(1) refcount(1){
      [0]=>
      object(stdClass)#2 (0) refcount(1){
      }
    }
  }
}
array(1) refcount(3){
  [0]=>
  object(stdClass)#2 (0) refcount(1){
  }
}
array(2) refcount(2){
  [1]=>
  reference refcount(2) {
    array(1) refcount(1){
      [0]=>
      object(stdClass)#4 (0) refcount(1){
      }
    }
  }
  [2]=>
  object(stdClass)#3 (1) refcount(1){
    ["filename"]=>
    reference refcount(2) {
      array(1) refcount(1){
        [0]=>
        object(stdClass)#4 (0) refcount(1){
        }
      }
    }
  }
}
array(1) refcount(3){
  [0]=>
  object(stdClass)#4 (0) refcount(1){
  }
}
array(1) packed refcount(3){
  [0]=>
  object(stdClass)#2 (0) refcount(1){
  }
}
array(1) packed refcount(3){
  [0]=>
  object(stdClass)#5 (0) refcount(1){
  }
}
array(1) packed refcount(3){
  [0]=>
  object(stdClass)#1 (0) refcount(1){
  }
}
array(1) packed refcount(3){
  [0]=>
  object(stdClass)#6 (0) refcount(1){
  }
}
