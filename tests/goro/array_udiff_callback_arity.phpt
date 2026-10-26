--TEST--
array_udiff() & co. called as callbacks: their own argument count message
--FILE--
<?php function t($f) { try { var_dump($f()); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } } foreach (["array_udiff", "array_udiff_assoc", "array_udiff_uassoc", "array_uintersect", "array_uintersect_assoc", "array_uintersect_uassoc", "array_diff_ukey", "array_diff_uassoc", "array_intersect_ukey", "array_intersect_uassoc"] as $f) { t(fn() => call_user_func($f)); t(fn() => call_user_func($f, [1])); t(fn() => call_user_func_array($f, [[1], [2]])); t(fn() => $f()); }
--EXPECTF--
ArgumentCountError: array_udiff() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff() expects at least 2 arguments, 1 given
TypeError: array_udiff(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_udiff() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff_assoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff_assoc() expects at least 2 arguments, 1 given
TypeError: array_udiff_assoc(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_udiff_assoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_udiff_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_udiff_uassoc() expects at least 3 arguments, 1 given
ArgumentCountError: array_udiff_uassoc() expects at least 3 arguments, 2 given
ArgumentCountError: array_udiff_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_uintersect() expects at least 2 arguments, 0 given
ArgumentCountError: array_uintersect() expects at least 2 arguments, 1 given
TypeError: array_uintersect(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_uintersect() expects at least 2 arguments, 0 given
ArgumentCountError: array_uintersect_assoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_uintersect_assoc() expects at least 2 arguments, 1 given
TypeError: array_uintersect_assoc(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_uintersect_assoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 1 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 2 given
ArgumentCountError: array_uintersect_uassoc() expects at least 3 arguments, 0 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 1 given
TypeError: array_diff_ukey(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_diff_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 1 given
TypeError: array_diff_uassoc(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_diff_uassoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_intersect_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_intersect_ukey() expects at least 2 arguments, 1 given
TypeError: array_intersect_ukey(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_intersect_ukey() expects at least 2 arguments, 0 given
ArgumentCountError: array_intersect_uassoc() expects at least 2 arguments, 0 given
ArgumentCountError: array_intersect_uassoc() expects at least 2 arguments, 1 given
TypeError: array_intersect_uassoc(): Argument #2 must be a valid callback, array callback must have exactly two members
ArgumentCountError: array_intersect_uassoc() expects at least 2 arguments, 0 given
