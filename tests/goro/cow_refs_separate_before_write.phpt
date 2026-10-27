--TEST--
Arrays sharing storage that holds references separate before builtins and compound writes read them
--FILE--
<?php
function w(&$a) { array_walk($a, function (&$x) { $x = 'W'; }); }
$a = [1, 2]; w($a); $cl = function () use ($a) { return $a; }; $a = $cl(); ksort($a); var_dump($a);
echo "--1\n";
$a = [1, 2]; w($a); $cl = function () use ($a) { return $a; }; $b = $cl(); ksort($b); var_dump($b);
echo "--2\n";
$a = [1, 2]; w($a); $b = [...$a]; var_dump($b);
echo "--3\n";
$a = [1, 2]; w($a); $b = array_chunk($a, 1, true)[0]; var_dump($b);
echo "--4\n";
$a = [1, 2]; w($a); [$r] = [$a]; ksort($a); var_dump($r);
echo "--5\n";
$a = [1, 2]; w($a); $b = $a; ksort($a); var_dump($a, $b);
echo "--6\n";
$a = [1, 2]; w($a); $fn = fn() => $a; $b = $fn(); ksort($b); var_dump($b);
echo "--7\n";
$a = [1, 2]; w($a); $b = unserialize(serialize($a)); var_dump($b);
echo "--8\n";
$a = [1, 2]; w($a); $o = new stdClass; $o->p = $a; $b = $o->p; ksort($b); var_dump($b, $o);
echo "--9\n";
$a = [1, 2]; w($a); $b = array_diff_key($a, []); var_dump($b);
--EXPECT--
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--1
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--2
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--3
array(1) {
  [0]=>
  string(1) "W"
}
--4
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--5
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--6
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--7
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
--8
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
object(stdClass)#3 (1) {
  ["p"]=>
  array(2) {
    [0]=>
    string(1) "W"
    [1]=>
    string(1) "W"
  }
}
--9
array(2) {
  [0]=>
  string(1) "W"
  [1]=>
  string(1) "W"
}
