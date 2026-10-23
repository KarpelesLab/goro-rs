--TEST--
zpp: objects given to class-typed parameters; number_format() takes int|float
--FILE--
<?php
// zpp: objects given to class-typed parameters; number_format()'s int|float
function gen() { yield 1; }
foreach ([fn() => iterator_to_array(new stdClass), fn() => iterator_to_array(new ArrayIterator([1])), fn() => iterator_to_array(gen()),
          fn() => iterator_count(new stdClass), 
          fn() => number_format("abc"), fn() => number_format("5 apples"), fn() => number_format("1234.5", 1),
          fn() => number_format([1]), fn() => number_format(true, 2), fn() => number_format(" 12 ")] as $f) {
    try { var_dump($f()); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
}
--EXPECT--
iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, stdClass given
array(1) {
  [0]=>
  int(1)
}
array(1) {
  [0]=>
  int(1)
}
iterator_count(): Argument #1 ($iterator) must be of type Traversable|array, stdClass given
number_format(): Argument #1 ($num) must be of type int|float, string given
number_format(): Argument #1 ($num) must be of type int|float, string given
string(7) "1,234.5"
number_format(): Argument #1 ($num) must be of type int|float, array given
string(4) "1.00"
string(2) "12"
