--TEST--
array_replace_recursive(): recursion checks of php_array_replace_recursive(), destination references separated
--FILE--
<?php
// php_array_replace_recursive(): the same reference in the destination copy
// and the source, with an odd refcount, is "Recursion detected"; a
// destination reference is separated, not written through.
function t($f) { try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$s0 = [[3]]; $s2 = true;
$d = ['y' => &$s2, 1 => true, 'k' => &$s0];
t(fn() => array_replace_recursive($d, $d));
t(fn() => array_replace_recursive($d, $d, $d));
$e = ['k' => &$s0];
t(fn() => array_replace_recursive($d, $e));
$t = [1]; $x = ['x' => ['y' => &$t]];
t(fn() => array_replace_recursive($x, $x));
t(fn() => array_replace_recursive($x, ['x' => ['y' => [2, 3]]]));
var_dump($t);
$r = ['a' => [1]]; $r['a']['self'] = &$r['a'];
t(fn() => array_replace_recursive(['a' => [0]], $r));
$s = ['p' => 1]; $u = ['q' => &$s]; $v = ['q' => ['p' => 2, 'z' => 3]];
t(fn() => array_replace_recursive($u, $v));
var_dump($s, $u);
t(fn() => array_merge_recursive($u, $v));
var_dump($s);
--EXPECT--
Error: Recursion detected
Error: Recursion detected
array(3) {
  ["y"]=>
  &bool(true)
  [1]=>
  bool(true)
  ["k"]=>
  array(1) {
    [0]=>
    array(1) {
      [0]=>
      int(3)
    }
  }
}
Error: Recursion detected
array(1) {
  ["x"]=>
  array(1) {
    ["y"]=>
    array(2) {
      [0]=>
      int(2)
      [1]=>
      int(3)
    }
  }
}
array(1) {
  [0]=>
  int(1)
}
array(1) {
  ["a"]=>
  array(2) {
    [0]=>
    int(1)
    ["self"]=>
    &array(2) {
      [0]=>
      int(1)
      ["self"]=>
      *RECURSION*
    }
  }
}
array(1) {
  ["q"]=>
  array(2) {
    ["p"]=>
    int(2)
    ["z"]=>
    int(3)
  }
}
array(1) {
  ["p"]=>
  int(1)
}
array(1) {
  ["q"]=>
  &array(1) {
    ["p"]=>
    int(1)
  }
}
array(1) {
  ["q"]=>
  array(2) {
    ["p"]=>
    array(2) {
      [0]=>
      int(1)
      [1]=>
      int(2)
    }
    ["z"]=>
    int(3)
  }
}
array(1) {
  ["p"]=>
  int(1)
}
