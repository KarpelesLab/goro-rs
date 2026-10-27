--TEST--
Sorting and other array-modifying builtins separate a shared array first: a lone reference becomes a value of each copy
--FILE--
<?php
echo "== 1\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; ksort($a); var_dump($a, $b);
echo "== 2\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; sort($a); var_dump($a, $b);
echo "== 3\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; usort($a, fn($p, $q) => 0); var_dump($a, $b);
echo "== 4\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; shuffle($a); var_dump(count($a), $b);
echo "== 5\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_unshift($a, 0); var_dump($a, $b);
echo "== 6\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_shift($a); var_dump($a, $b);
echo "== 7\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_splice($a, 1, 0, ['i']); var_dump($a, $b);
echo "== 8\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; $a = array_reverse($a); var_dump($a, $b);
echo "== 9\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_multisort($a); var_dump($a, $b);
echo "== 10\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; natsort($a); var_dump($a, $b);
echo "== 11\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; arsort($a); var_dump($a, $b);
echo "== 12\n";
$x = 1; $a = ['k' => &$x, 'j' => 2]; unset($x); $b = $a; uksort($a, fn($p, $q) => 0); var_dump($a, $b);
echo "== 13\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; $a[] = 3; var_dump($a, $b);
echo "== 14\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; foreach ($a as &$v) {} unset($v); var_dump($a, $b);
echo "== 15\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_walk($a, fn($v) => 0); var_dump($b);
echo "== 16\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; $r = &$a[1]; unset($r); var_dump($a, $b);
echo "== 17\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_pop($a); var_dump($a, $b);
echo "== 18\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; array_push($a, 5); var_dump($a, $b);
echo "== 19\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; $a[0]++; var_dump($a, $b);
echo "== 20\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; $a[0] .= 'z'; var_dump($a, $b);
echo "== 21\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; unset($a[1]); var_dump($a, $b);
echo "== 22\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; extract($a, EXTR_PREFIX_ALL | EXTR_REFS, 'p'); $p_0 = 'E'; var_dump($a, $b);
echo "== 23\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; settype($a, 'array'); var_dump($a, $b);
echo "== 24\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; reset($a); next($a); var_dump(current($a), $b);
echo "== 25\n";
$x = 1; $a = [&$x, 2]; unset($x); $b = $a; $c = &$a; $c[0] = 'C'; var_dump($a, $b);
echo "== 26\n";
$x = 1; $a = [[&$x], 2]; unset($x); $b = $a; $a[0][0] = 'N'; var_dump($a, $b);
--EXPECT--
== 1
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 2
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 3
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 4
int(2)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 5
array(3) {
  [0]=>
  int(0)
  [1]=>
  int(1)
  [2]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 6
array(1) {
  [0]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 7
array(3) {
  [0]=>
  int(1)
  [1]=>
  string(1) "i"
  [2]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 8
array(2) {
  [0]=>
  int(2)
  [1]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 9
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 10
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 11
array(2) {
  [1]=>
  int(2)
  [0]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 12
array(2) {
  ["k"]=>
  int(1)
  ["j"]=>
  int(2)
}
array(2) {
  ["k"]=>
  int(1)
  ["j"]=>
  int(2)
}
== 13
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 14
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 15
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 16
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 17
array(1) {
  [0]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 18
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(5)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 19
array(2) {
  [0]=>
  int(2)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 20
array(2) {
  [0]=>
  string(2) "1z"
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 21
array(1) {
  [0]=>
  int(1)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 22
array(2) {
  [0]=>
  &string(1) "E"
  [1]=>
  &int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 23
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 24
int(2)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 25
array(2) {
  [0]=>
  string(1) "C"
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
== 26
array(2) {
  [0]=>
  array(1) {
    [0]=>
    string(1) "N"
  }
  [1]=>
  int(2)
}
array(2) {
  [0]=>
  array(1) {
    [0]=>
    int(1)
  }
  [1]=>
  int(2)
}
