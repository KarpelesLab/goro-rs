--TEST--
func_get_args() in a generator function called through call_user_func[_array]() sees all its arguments
--FILE--
<?php
$calls = [];
$f = static function () use (&$calls) { $calls[] = func_get_args(); yield 1; };
foreach ($f(null) as $v) {}
foreach ($f(null, 2) as $v) {}
foreach ($f(...[null]) as $v) {}
foreach (call_user_func_array($f, [null]) as $v) {}
foreach (call_user_func($f, null, 3) as $v) {}
$g = function () { var_dump(func_get_args(), func_num_args()); yield 1; };
foreach ($g(null) as $v) {}
function gen() { var_dump(func_get_args()); yield 1; }
foreach (gen(null, 'x') as $v) {}
$h = static function () use (&$calls) { $calls[] = func_get_args(); return 1; };
$h(null);
var_dump($calls);

?>
--EXPECTF--
array(1) {
  [0]=>
  NULL
}
int(1)
array(2) {
  [0]=>
  NULL
  [1]=>
  string(1) "x"
}
array(6) {
  [0]=>
  array(1) {
    [0]=>
    NULL
  }
  [1]=>
  array(2) {
    [0]=>
    NULL
    [1]=>
    int(2)
  }
  [2]=>
  array(1) {
    [0]=>
    NULL
  }
  [3]=>
  array(1) {
    [0]=>
    NULL
  }
  [4]=>
  array(2) {
    [0]=>
    NULL
    [1]=>
    int(3)
  }
  [5]=>
  array(1) {
    [0]=>
    NULL
  }
}
