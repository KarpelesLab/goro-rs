--TEST--
Argument unpacking into by-reference parameters separates the unpacked array and its elements
--FILE--
<?php
$a = [[3, 1, 2]]; $copy = $a; sort(...$copy); var_dump($a, $copy);
$b = [[3, 1, 2]]; $fn = 'sort'; $c2 = $b; $fn(...$c2); var_dump($b, $c2);
function byref(&$x) { $x[] = 9; }
$d = [[1]]; $c3 = $d; byref(...$c3); var_dump($d, $c3);
$e = [[1]]; byref(...$e); var_dump($e);
$f = ['k' => [1]]; $c4 = $f; byref(...$c4); var_dump($f, $c4);
$g = [[1]]; call_user_func_array('byref', $g); var_dump($g);
$h = [[5, 4]]; $c5 = $h; array_push(...$c5); var_dump($h);
$args = ['/(a)/', 'xa', null]; var_dump(preg_match(...$args)); var_dump($args);
$x = ["5", "int"]; settype(...$x); var_dump($x);
$sr = ['a', 'b', 'aaa', 0]; var_dump(str_replace(...$sr)); var_dump($sr);
$cp = $sr; str_replace(...$cp); var_dump($sr[3], $cp[3]);
$arrs = [[3, 1], [2, 0]]; array_multisort(...$arrs); var_dump($arrs);
$st = [[1, 2, 3]]; array_shift(...$st); var_dump($st);
$named = ['array' => [3, 1, 2]]; sort(...$named); var_dump($named);
$n2 = $named; rsort(...$n2); var_dump($named, $n2);
$a = [[3, 1, 2], 0]; $copy = $a; array_multisort(...$copy); var_dump($a[0], $copy[0]);
$a = [[3, 1, 2]]; $copy = $a; array_multisort(...$copy); var_dump($a[0], $copy[0]);
$b = [3, 1, 2]; $c = $b; array_multisort($c); var_dump($b, $c);
$b = [3, 1, 2]; $c = $b; array_multisort($c, SORT_DESC); var_dump($b, $c);
$a = [[3, 1, 2]]; $copy = $a; var_dump(sort(...$copy)); var_dump($copy);
$b = [[3, 1, 2]]; var_dump(sort(...$b)); var_dump($b);
--EXPECTF--
array(1) {
  [0]=>
  array(3) {
    [0]=>
    int(3)
    [1]=>
    int(1)
    [2]=>
    int(2)
  }
}
array(1) {
  [0]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    int(2)
    [2]=>
    int(3)
  }
}
array(1) {
  [0]=>
  array(3) {
    [0]=>
    int(3)
    [1]=>
    int(1)
    [2]=>
    int(2)
  }
}
array(1) {
  [0]=>
  array(3) {
    [0]=>
    int(1)
    [1]=>
    int(2)
    [2]=>
    int(3)
  }
}
array(1) {
  [0]=>
  array(1) {
    [0]=>
    int(1)
  }
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(9)
  }
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(9)
  }
}

Fatal error: Uncaught Error: Unknown named parameter $k in %s:7
Stack trace:
#0 {main}
  thrown in %s on line 7
