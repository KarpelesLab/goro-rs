--TEST--
Array union, (object) casts and unpacked by-value arguments copy the references only their array holds as values
--FILE--
<?php
function g(array $x) { foreach ($x as &$v) { $v = 'g'; } return $x; }
$c = g([1, 2]); $d = [9]; $d += $c; $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = [9] + $c; $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_replace([9], $c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_merge([9], $c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_slice($c, 1); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_values($c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_reverse($c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_pad($c, 3, 0); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_filter($c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_unique($c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_diff_key($c, []); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_intersect_key($c, $c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_chunk($c, 5)[0]; $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_combine([5, 6], $c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_fill_keys($c, 0); $d = array_flip($c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); [, $d] = $c; $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = [...$c]; $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_splice($c, 0, 1); $s = &$c[0]; var_dump($d);
$c = g([1, 2]); $d = array_column([$c], 1); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = (object) $c; $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_map(null, $c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_replace_recursive([9], $c); $s = &$c[1]; var_dump($d);
$c = g([1, 2]); $d = array_merge_recursive([9], $c); $s = &$c[1]; var_dump($d);
echo "--\n";
$c = g([1, 2]); $d = [...$c]; $s = &$c[1]; var_dump($d);
echo "--\n";
$c = g([1, 2]); $d = (object) $c; $s = &$c[1]; var_dump($d);
echo "--\n";
$c = g(['a' => 1, 'b' => 2]); $d = (object) $c; $s = &$c['b']; var_dump($d);
echo "--\n";
$c = g([1, 2]); $d = new ArrayObject($c); $s = &$c[1]; var_dump($d->getArrayCopy());
echo "--\n";
$c = g([1, 2]); $d = get_object_vars((object) $c); $s = &$c[1]; var_dump($d);
echo "--\n";
$c = g([1, 2]); $d = func(...$c); $s = &$c[1]; var_dump($d);
function func(...$a) { return $a; }
echo "--\n";
$c = g([1, 2]); $gen = (function () use ($c) { yield from $c; })(); $d = iterator_to_array($gen); $s = &$c[1]; var_dump($d);
--EXPECT--
array(2) {
  [0]=>
  int(9)
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  int(9)
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(3) {
  [0]=>
  int(9)
  [1]=>
  string(1) "g"
  [2]=>
  string(1) "g"
}
array(1) {
  [0]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(3) {
  [0]=>
  &string(1) "g"
  [1]=>
  &string(1) "g"
  [2]=>
  int(0)
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(1) {
  [0]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [5]=>
  string(1) "g"
  [6]=>
  string(1) "g"
}
array(1) {
  ["g"]=>
  int(1)
}
string(1) "g"
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(1) {
  [0]=>
  string(1) "g"
}
array(1) {
  [0]=>
  string(1) "g"
}
object(stdClass)#1 (2) {
  ["0"]=>
  string(1) "g"
  ["1"]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
array(3) {
  [0]=>
  int(9)
  [1]=>
  string(1) "g"
  [2]=>
  string(1) "g"
}
--
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
--
object(stdClass)#1 (2) {
  ["0"]=>
  string(1) "g"
  ["1"]=>
  string(1) "g"
}
--
object(stdClass)#2 (2) {
  ["a"]=>
  string(1) "g"
  ["b"]=>
  string(1) "g"
}
--
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
--
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
--
array(2) {
  [0]=>
  string(1) "g"
  [1]=>
  string(1) "g"
}
--
array(2) {
  [0]=>
  &string(1) "g"
  [1]=>
  &string(1) "g"
}
