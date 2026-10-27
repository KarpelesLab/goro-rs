--TEST--
array_push() of the array itself appends a copy
--FILE--
<?php
$c = [1]; array_push($c, $c); var_dump($c);
$c = [1]; $o = new stdClass; $o->p = $c; $c = $o->p; array_push($c, $c); var_dump($c);
$c = [1]; $d = $c; array_push($c, $c, $d); var_dump($c);
--EXPECT--
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(1)
  }
}
array(2) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(1)
  }
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  array(1) {
    [0]=>
    int(1)
  }
  [2]=>
  array(1) {
    [0]=>
    int(1)
  }
}
