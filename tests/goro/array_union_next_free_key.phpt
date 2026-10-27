--TEST--
Array union keeps the left array's next free key
--FILE--
<?php
$a = [1, 1, 1, 1]; $d = array_unique($a); $d[] = 'x'; var_dump($d);
$a = [1, 2, 3]; unset($a[2]); $d = $a + []; $d[] = 'x'; var_dump($d);
$a = [1, 2, 3]; unset($a[2]); $d = [] + $a; $d[] = 'x'; var_dump($d);
$a = [1, 2, 3]; unset($a[2]); $d = $a; $d += ['k' => 1]; $d[] = 'x'; var_dump($d);
$a = [1, 1, 2]; $d = array_unique($a, SORT_REGULAR); $d[] = 'x'; var_dump($d);
$a = [5 => 1, 2]; $d = array_filter($a, fn($x) => $x > 1); $d[] = 'x'; var_dump($d);
$a = [1, 2, 3]; $d = array_diff_key($a, [2 => 0]); $d[] = 'x'; var_dump($d);
$a = [1, 2, 3]; $d = array_intersect_key($a, [0 => 0]); $d[] = 'x'; var_dump($d);
$a = [1, 2, 3]; $d = array_diff($a, [3]); $d[] = 'x'; var_dump($d);
--EXPECT--
array(2) {
  [0]=>
  int(1)
  [1]=>
  string(1) "x"
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [3]=>
  string(1) "x"
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  string(1) "x"
}
array(4) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  ["k"]=>
  int(1)
  [3]=>
  string(1) "x"
}
array(3) {
  [0]=>
  int(1)
  [2]=>
  int(2)
  [3]=>
  string(1) "x"
}
array(2) {
  [6]=>
  int(2)
  [7]=>
  string(1) "x"
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  string(1) "x"
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  string(1) "x"
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  string(1) "x"
}
