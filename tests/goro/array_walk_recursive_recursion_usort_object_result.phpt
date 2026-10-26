--TEST--
array_walk_recursive() recursion Error, usort() object comparison result
--FILE--
<?php
// array_walk_recursive(): walking into a nested array already being walked is an Error
$a = [1, [2, [3]]]; $a[1][1][] = &$a;
try { array_walk_recursive($a, function($v) { echo $v, ","; }); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$b = [1]; $b[] = &$b;
try { array_walk_recursive($b, function($v) { echo $v, ","; }); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$c = ['x' => [1, 2], 'y' => [3]]; $c['z'] = &$c['x'];
array_walk_recursive($c, function(&$v, $k) { $v *= 10; });
var_dump($c);
// usort(): a comparison result that is an object is converted with zval_get_long()
$d = [3, 1, 2]; usort($d, function($x, $y) { return new stdClass; }); var_dump($d);
--EXPECTF--
1,2,3,1,Error: Recursion detected
1,1,Error: Recursion detected
array(3) {
  ["x"]=>
  &array(2) {
    [0]=>
    int(100)
    [1]=>
    int(200)
  }
  ["y"]=>
  array(1) {
    [0]=>
    int(30)
  }
  ["z"]=>
  &array(2) {
    [0]=>
    int(100)
    [1]=>
    int(200)
  }
}

Warning: Object of class stdClass could not be converted to int in %s on line 11

Warning: Object of class stdClass could not be converted to int in %s on line 11

Warning: Object of class stdClass could not be converted to int in %s on line 11
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
