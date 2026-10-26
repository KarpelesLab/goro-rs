--TEST--
zpp: iterator_to_array() $preserve_keys after the iterator, ob_*() functions
--FILE--
<?php
function t($f) { try { $f(); } catch (\Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
t(fn() => var_dump(iterator_to_array([1], [])));
t(fn() => var_dump(iterator_to_array([1], null)));
t(fn() => var_dump(iterator_to_array(new ArrayIterator(['a' => 1]), 0)));
t(fn() => var_dump(iterator_to_array("x", [])));
t(fn() => var_dump(ob_get_clean(1)));
t(fn() => var_dump(ob_get_level(1)));
t(fn() => var_dump(ob_implicit_flush([])));
t(fn() => var_dump(ob_get_status(new stdClass)));
t(fn() => var_dump(ob_list_handlers(1)));
--EXPECTF--
TypeError: iterator_to_array(): Argument #2 ($preserve_keys) must be of type bool, array given

Deprecated: iterator_to_array(): Passing null to parameter #2 ($preserve_keys) of type bool is deprecated in %s on line 4
array(1) {
  [0]=>
  int(1)
}
array(1) {
  [0]=>
  int(1)
}
TypeError: iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, string given
ArgumentCountError: ob_get_clean() expects exactly 0 arguments, 1 given
ArgumentCountError: ob_get_level() expects exactly 0 arguments, 1 given
TypeError: ob_implicit_flush(): Argument #1 ($enable) must be of type bool, array given
TypeError: ob_get_status(): Argument #1 ($full_status) must be of type bool, stdClass given
ArgumentCountError: ob_list_handlers() expects exactly 0 arguments, 1 given
