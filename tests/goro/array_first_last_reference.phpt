--TEST--
array_first()/array_last() return a referenced element's value, not the reference
--FILE--
<?php
var_dump(array_first([1, 2, 3]), array_last([1, 2, 3]));
var_dump(array_first([]), array_last([]));
var_dump(array_first(['a' => 'x', 'b' => 'y']), array_last(['a' => 'x', 'b' => 'y']));
$a = [5 => 'five', 1 => 'one']; var_dump(array_first($a), array_last($a));
$a = [1, 2, 3]; unset($a[2]); var_dump(array_last($a));
$a = [1, 2, 3]; unset($a[0]); var_dump(array_first($a));
try { array_first('x'); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
$r = 1; $a = [&$r]; $f = array_first($a); $f = 2; var_dump($r);
var_dump(array_first([null]), array_last([[1]]));
?>
--EXPECT--
int(1)
int(3)
NULL
NULL
string(1) "x"
string(1) "y"
string(4) "five"
string(3) "one"
int(2)
int(2)
array_first(): Argument #1 ($array) must be of type array, string given
int(1)
NULL
array(1) {
  [0]=>
  int(1)
}
