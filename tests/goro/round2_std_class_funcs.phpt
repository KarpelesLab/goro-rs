--TEST--
Internal classes in get_parent_class()/method_exists()/class_implements(), interface order, substr/substr_replace/htmlspecialchars/levenshtein/array_fill_keys/sprintf/idate edges
--FILE--
<?php
function t($l, $f) { try { $r = $f(); echo "$l: "; var_dump($r); } catch (\Throwable $e) { echo "$l: EXC ", get_class($e), ": ", $e->getMessage(), "\n"; } }
class MyIt extends ArrayIterator {}
interface I1 {} interface I2 extends I1 {} class Y implements I2 {}
class Agg implements IteratorAggregate { function getIterator(): Iterator { return new ArrayIterator([]); } }
t(1, fn() => [get_parent_class('RuntimeException'), get_parent_class(new DateMalformedStringException), get_parent_class('DateRangeError'), get_parent_class('MyIt'), get_parent_class('ArrayIterator'), get_parent_class('Exception')]);
t(2, fn() => [method_exists('IteratorIterator', 'rewind'), method_exists('ArrayIterator', 'current'), method_exists('SplMinHeap', 'insert'), method_exists('MyIt', 'seek'), method_exists(new MyIt, 'getArrayCopy'), method_exists('Iterator', 'current'), method_exists('Countable', 'count'), method_exists('ArrayIterator', 'nope'), method_exists('ReflectionClass', 'getName')]);
t(3, fn() => [class_implements('Y'), class_implements('Agg'), class_implements(new ArrayObject)]);
t(4, fn() => [substr("abc", 1, PHP_INT_MAX), substr("abc", -1, PHP_INT_MAX), substr("abc", PHP_INT_MAX), substr("abc", PHP_INT_MIN, 1)]);
t(5, fn() => htmlspecialchars([]));
t(6, fn() => htmlentities(new stdClass));
t(7, fn() => [substr_replace("abc", "X", 1, null), substr_replace(["abc", "def"], "X", 1, null), substr_replace("abc", "X", 1, 0)]);
t(8, fn() => [http_build_query(["k\xe9" => "v\xff", 'a' => ['b' => "\xfe"]]), http_build_query(['x' => 1, 'y' => 2], '', "\xff")]);
t(9, fn() => [levenshtein("\xe9", "e"), levenshtein("\xff\xfe", "\xfe\xff"), levenshtein("Hello World", "\0a\0", 10, -10, -2), levenshtein("", "abc", 3), levenshtein("abc", "", 1, 1, 4), levenshtein("kitten", "sitting")]);
t(10, fn() => [array_fill_keys([1.0, true, 1.5, "1", null, false], 0)]);
t(11, fn() => [sprintf("%d|%x", 2**63, 2**63), sprintf("%d", -(2**63)), sprintf("%d", 2**64), sprintf("%u", 2**63)]);
t(12, fn() => [idate('U', -1), idate('Y', -62198755200), idate('y', 0)]);
--EXPECTF--
1: array(6) {
  [0]=>
  string(9) "Exception"
  [1]=>
  string(13) "DateException"
  [2]=>
  string(9) "DateError"
  [3]=>
  string(13) "ArrayIterator"
  [4]=>
  bool(false)
  [5]=>
  bool(false)
}
2: array(9) {
  [0]=>
  bool(true)
  [1]=>
  bool(true)
  [2]=>
  bool(true)
  [3]=>
  bool(true)
  [4]=>
  bool(true)
  [5]=>
  bool(true)
  [6]=>
  bool(true)
  [7]=>
  bool(false)
  [8]=>
  bool(true)
}
3: array(3) {
  [0]=>
  array(2) {
    ["I2"]=>
    string(2) "I2"
    ["I1"]=>
    string(2) "I1"
  }
  [1]=>
  array(2) {
    ["IteratorAggregate"]=>
    string(17) "IteratorAggregate"
    ["Traversable"]=>
    string(11) "Traversable"
  }
  [2]=>
  array(5) {
    ["IteratorAggregate"]=>
    string(17) "IteratorAggregate"
    ["Traversable"]=>
    string(11) "Traversable"
    ["ArrayAccess"]=>
    string(11) "ArrayAccess"
    ["Serializable"]=>
    string(12) "Serializable"
    ["Countable"]=>
    string(9) "Countable"
  }
}
4: array(4) {
  [0]=>
  string(2) "bc"
  [1]=>
  string(1) "c"
  [2]=>
  string(0) ""
  [3]=>
  string(1) "a"
}
5: EXC TypeError: htmlspecialchars(): Argument #1 ($string) must be of type string, array given
6: EXC TypeError: htmlentities(): Argument #1 ($string) must be of type string, stdClass given
7: array(3) {
  [0]=>
  string(2) "aX"
  [1]=>
  array(2) {
    [0]=>
    string(2) "aX"
    [1]=>
    string(2) "dX"
  }
  [2]=>
  string(4) "aXbc"
}
8: array(2) {
  [0]=>
  string(22) "k%E9=v%FF&a%5Bb%5D=%FE"
  [1]=>
  string(7) "x=1ÿy=2"
}
9: array(6) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(-46)
  [3]=>
  int(9)
  [4]=>
  int(12)
  [5]=>
  int(3)
}
10: array(1) {
  [0]=>
  array(3) {
    [1]=>
    int(0)
    ["1.5"]=>
    int(0)
    [""]=>
    int(0)
  }
}

Warning: The float 9.223372036854776E+18 is not representable as an int, cast occurred in %s on line 16

Warning: The float 9.223372036854776E+18 is not representable as an int, cast occurred in %s on line 16

Warning: The float 1.8446744073709552E+19 is not representable as an int, cast occurred in %s on line 16

Warning: The float 9.223372036854776E+18 is not representable as an int, cast occurred in %s on line 16
11: array(4) {
  [0]=>
  string(37) "-9223372036854775808|8000000000000000"
  [1]=>
  string(20) "-9223372036854775808"
  [2]=>
  string(1) "0"
  [3]=>
  string(19) "9223372036854775808"
}

Warning: idate(): Unrecognized date format token in %s on line 17

Warning: idate(): Unrecognized date format token in %s on line 17
12: array(3) {
  [0]=>
  bool(false)
  [1]=>
  bool(false)
  [2]=>
  int(70)
}
