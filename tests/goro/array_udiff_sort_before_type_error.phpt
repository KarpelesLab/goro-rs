--TEST--
array_udiff() family: each array is sorted (calling the callback) before the next argument is validated
--FILE--
<?php
$cb = function($a, $b) { echo "($a,$b)"; return $a <=> $b; };
var_dump(array_udiff([3, 1, 2, 5], [2, 4], $cb)); echo "\n";
var_dump(array_uintersect([3, 1, 2, 5], [2, 4], [5, 2], $cb)); echo "\n";
var_dump(array_diff_ukey(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 0], $cb)); echo "\n";
var_dump(array_intersect_ukey(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 0, 'b' => 0], $cb)); echo "\n";
var_dump(array_udiff_assoc(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 2, 'b' => 0], $cb)); echo "\n";
var_dump(array_udiff_uassoc(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 2, 'b' => 0], $cb, $cb)); echo "\n";
var_dump(array_uintersect_assoc(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 2, 'b' => 0], $cb)); echo "\n";
var_dump(array_uintersect_uassoc(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 2, 'b' => 3], $cb, $cb)); echo "\n";
var_dump(array_diff_uassoc(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 2, 'b' => 0], $cb)); echo "\n";
var_dump(array_intersect_uassoc(['c' => 1, 'a' => 2, 'b' => 3], ['a' => 2, 'b' => 3], $cb)); echo "\n";
try { array_udiff([3, 1], 5, $cb); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_uintersect([3, 1], [2], 5, $cb); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_diff_ukey(['b' => 1, 'a' => 1], 5, $cb); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_intersect_ukey(['b' => 1, 'a' => 1], 5, $cb); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_udiff_assoc(['b' => 1, 'a' => 1], 5, $cb); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_uintersect_uassoc(['b' => 1, 'a' => 1], 5, $cb, $cb); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { array_udiff([3, 1], [1]); } catch (Error $e) { echo get_class($e), $e->getMessage(), "\n"; }
try { array_udiff([3, 1], [1], 'nope'); } catch (Error $e) { echo get_class($e), $e->getMessage(), "\n"; }
try { array_udiff([3, 1]); } catch (Error $e) { echo get_class($e), $e->getMessage(), "\n"; }
try { array_intersect([[1], [2]], 5); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
--EXPECTF--
(3,1)(2,1)(3,2)(3,5)(2,4)(1,2)(1,2)(2,2)(2,3)(3,4)(3,5)(5,4)array(3) {
  [0]=>
  int(3)
  [1]=>
  int(1)
  [3]=>
  int(5)
}

(3,1)(2,1)(3,2)(3,5)(2,4)(5,2)(1,2)(2,2)(2,2)(2,2)(2,3)(3,4)(5,4)(5,4)array(1) {
  [2]=>
  int(2)
}

(c,a)(b,a)(c,b)(a,a)(b,a)(c,a)array(2) {
  ["c"]=>
  int(1)
  ["b"]=>
  int(3)
}

(c,a)(b,a)(c,b)(a,b)(a,a)(b,b)array(2) {
  ["a"]=>
  int(2)
  ["b"]=>
  int(3)
}

(2,2)(3,0)array(2) {
  ["c"]=>
  int(1)
  ["b"]=>
  int(3)
}

(c,a)(b,a)(c,b)(a,b)(a,a)(2,2)(b,a)(b,b)(3,0)(c,a)(c,b)array(2) {
  ["c"]=>
  int(1)
  ["b"]=>
  int(3)
}

(2,2)(3,0)array(1) {
  ["a"]=>
  int(2)
}

(c,a)(b,a)(c,b)(a,b)(a,a)(2,2)(b,b)(3,3)array(2) {
  ["a"]=>
  int(2)
  ["b"]=>
  int(3)
}

(c,a)(b,a)(c,b)(a,b)(a,a)(b,a)(b,b)(c,a)(c,b)array(2) {
  ["c"]=>
  int(1)
  ["b"]=>
  int(3)
}

(c,a)(b,a)(c,b)(a,b)(a,a)(b,b)array(2) {
  ["a"]=>
  int(2)
  ["b"]=>
  int(3)
}

(3,1)array_udiff(): Argument #2 must be of type array, int given
(3,1)array_uintersect(): Argument #3 must be of type array, int given
(b,a)array_diff_ukey(): Argument #2 must be of type array, int given
(b,a)array_intersect_ukey(): Argument #2 must be of type array, int given
array_udiff_assoc(): Argument #2 must be of type array, int given
(b,a)array_uintersect_uassoc(): Argument #2 must be of type array, int given
TypeErrorarray_udiff(): Argument #2 must be a valid callback, array callback must have exactly two members
TypeErrorarray_udiff(): Argument #3 must be a valid callback, function "nope" not found or invalid function name
ArgumentCountErrorarray_udiff() expects at least 2 arguments, 1 given

Warning: Array to string conversion in %s on line 22

Warning: Array to string conversion in %s on line 22
array_intersect(): Argument #2 must be of type array, int given
