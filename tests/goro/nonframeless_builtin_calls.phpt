--TEST--
min()/max()/implode() called other than directly (dynamic, unpacked, callback) run their PHP_FUNCTION's semantics, not the frameless ones
--FILE--
<?php
function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
$a = [-5 => 1, -4 => 2]; $b = ["a" => 1, "b" => 2]; $args = [$a, $b];
$fn = "min"; $m = "max";
t(fn() => min($a, $b));
t(fn() => min(...$args));
t(fn() => $fn(...$args));
t(fn() => $fn($a, $b));
t(fn() => max($a, $b));
t(fn() => $m($a, $b));
t(fn() => call_user_func("min", $a, $b));
t(fn() => array_map("max", [$a], [$b]));
$f = "implode"; $j = "join";
t(fn() => implode([1], true));
t(fn() => $f([1], true));
t(fn() => call_user_func("implode", [1], true));
t(fn() => implode(...[[1], true]));
t(fn() => implode([1], [2]));
t(fn() => $f([1], [2]));
t(fn() => $f(null));
t(fn() => $f(null, [1, 2]));
t(fn() => join(null, [1, 2]));
t(fn() => implode(null, [1, 2]));
t(fn() => array_unique([[1]]));
t(fn() => array_unique([[1]], SORT_REGULAR));
$u = "array_unique";
t(fn() => $u([[1]]));
--EXPECTF--
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
array(2) {
  [-5]=>
  int(1)
  [-4]=>
  int(2)
}
array(2) {
  [-5]=>
  int(1)
  [-4]=>
  int(2)
}
array(2) {
  [-5]=>
  int(1)
  [-4]=>
  int(2)
}
array(2) {
  [-5]=>
  int(1)
  [-4]=>
  int(2)
}
array(2) {
  ["a"]=>
  int(1)
  ["b"]=>
  int(2)
}
array(2) {
  [-5]=>
  int(1)
  [-4]=>
  int(2)
}
array(1) {
  [0]=>
  array(2) {
    ["a"]=>
    int(1)
    ["b"]=>
    int(2)
  }
}
TypeError: implode(): Argument #1 ($separator) must be of type string, array given
TypeError: implode(): Argument #2 ($array) must be of type ?array, true given
TypeError: implode(): Argument #2 ($array) must be of type ?array, true given
TypeError: implode(): Argument #2 ($array) must be of type ?array, true given
TypeError: implode(): Argument #1 ($separator) must be of type string, array given
TypeError: implode(): Argument #1 ($separator) must be of type string, array given

Deprecated: implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 20
TypeError: implode(): If argument #1 ($separator) is of type string, argument #2 ($array) must be of type array, null given

Deprecated: implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 21
string(2) "12"

Deprecated: join(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 22
string(2) "12"

Deprecated: implode(): Passing null to parameter #1 ($separator) of type array|string is deprecated in %s on line 23
string(2) "12"
array(1) {
  [0]=>
  array(1) {
    [0]=>
    int(1)
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
  array(1) {
    [0]=>
    int(1)
  }
}
