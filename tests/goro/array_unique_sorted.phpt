--TEST--
array_unique(): SORT_REGULAR/SORT_NUMERIC/SORT_FLAG_CASE use zend_sort() and neighbour comparison
--FILE--
<?php
// array_unique() with flags other than SORT_STRING sorts the buckets with
// zend_sort() and drops equal neighbours (non-transitive comparisons included)
var_dump(array_unique(["10", 9, "9a", "1e1", 10.0, "abc", "ABC", "_"], SORT_REGULAR));
var_dump(array_unique(["x" => [2, 1], "y" => [1, 2], "z" => 3], SORT_REGULAR));
var_dump(array_unique([1.5, "1.5", 1, "01", "1"], SORT_REGULAR));
var_dump(array_unique(["a", "A", "b", "B", "a"], SORT_STRING | SORT_FLAG_CASE));
var_dump(array_unique(["1e1", "10", 10, "010", 9.99], SORT_NUMERIC));
enum E { case A; case B; }
var_dump(array_unique([E::A, E::B, E::A, 1, E::B], SORT_REGULAR));
--EXPECT--
array(6) {
  [0]=>
  string(2) "10"
  [1]=>
  int(9)
  [2]=>
  string(2) "9a"
  [5]=>
  string(3) "abc"
  [6]=>
  string(3) "ABC"
  [7]=>
  string(1) "_"
}
array(3) {
  ["x"]=>
  array(2) {
    [0]=>
    int(2)
    [1]=>
    int(1)
  }
  ["y"]=>
  array(2) {
    [0]=>
    int(1)
    [1]=>
    int(2)
  }
  ["z"]=>
  int(3)
}
array(2) {
  [0]=>
  float(1.5)
  [2]=>
  int(1)
}
array(2) {
  [0]=>
  string(1) "a"
  [2]=>
  string(1) "b"
}
array(2) {
  [0]=>
  string(3) "1e1"
  [4]=>
  float(9.99)
}
array(3) {
  [0]=>
  enum(E::A)
  [1]=>
  enum(E::B)
  [3]=>
  int(1)
}
