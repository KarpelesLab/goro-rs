--TEST--
sort()/array_push()/shuffle() of a missing element or property: null is given (TypeError), not a new array
--FILE--
<?php
$a = [];
try { sort($a['x']); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_push($a['y'], 1); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($a);
$o = new stdClass;
try { sort($o->p); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($o);
try { sort($u); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($u);
preg_match('/a/', 'a', $a['m']);
var_dump($a['m']);
$b = [];
try { shuffle($b[0][1]); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($b);
$o = new stdClass;
try { sort($o->p); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($o);
class C { public $q; public array $t = []; }
$c = new C; try { sort($c->q); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
var_dump($c);
?>
--EXPECT--
sort(): Argument #1 ($array) must be of type array, null given
array_push(): Argument #1 ($array) must be of type array, null given
array(2) {
  ["x"]=>
  NULL
  ["y"]=>
  NULL
}
sort(): Argument #1 ($array) must be of type array, null given
object(stdClass)#1 (1) {
  ["p"]=>
  NULL
}
sort(): Argument #1 ($array) must be of type array, null given
NULL
array(1) {
  [0]=>
  string(1) "a"
}
shuffle(): Argument #1 ($array) must be of type array, null given
array(1) {
  [0]=>
  array(1) {
    [1]=>
    NULL
  }
}
sort(): Argument #1 ($array) must be of type array, null given
object(stdClass)#2 (1) {
  ["p"]=>
  NULL
}
sort(): Argument #1 ($array) must be of type array, null given
object(C)#3 (2) {
  ["q"]=>
  NULL
  ["t"]=>
  array(0) {
  }
}
