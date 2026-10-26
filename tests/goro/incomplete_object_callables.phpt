--TEST--
Callables naming a method of an incomplete object: get_method() throws
--FILE--
<?php
$o = unserialize('O:3:"Foo":1:{s:1:"a";i:1;}');
$tests = [
  'array_map' => fn() => array_map([$o, 'bar'], [1]),
  'usort' => function() use ($o) { $a = [2, 1]; usort($a, [$o, 'cmp']); },
  'call_user_func' => fn() => call_user_func([$o, 'bar']),
  'call_user_func_array' => fn() => call_user_func_array([$o, 'bar'], []),
  'array_filter' => fn() => array_filter([1], [$o, 'f']),
  'is_callable' => fn() => is_callable([$o, 'bar']),
  'is_callable_syntax' => fn() => is_callable([$o, 'bar'], true),
  'fromCallable' => fn() => Closure::fromCallable([$o, 'bar']),
  'direct' => fn() => [$o, 'bar'](),
  'method' => fn() => $o->bar(),
  'fcc' => fn() => $o->bar(...),
  'register_shutdown' => fn() => register_shutdown_function([$o, 'bar']),
  'array_walk' => function() use ($o) { $a = [1]; array_walk($a, [$o, 'w']); },
];
foreach ($tests as $k => $f) {
  echo "$k: ";
  try { var_dump($f()); } catch (Throwable $e) { echo get_class($e), ": ", substr($e->getMessage(), 0, 150), "\n"; }
}
--EXPECT--
array_map: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
usort: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
call_user_func: TypeError: call_user_func(): Argument #1 ($callback) must be a valid callback, class __PHP_Incomplete_Class does not have a method "bar"
call_user_func_array: TypeError: call_user_func_array(): Argument #1 ($callback) must be a valid callback, class __PHP_Incomplete_Class does not have a method "bar"
array_filter: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
is_callable: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
is_callable_syntax: bool(true)
fromCallable: TypeError: Failed to create closure from callable: class __PHP_Incomplete_Class does not have a method "bar"
direct: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
method: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
fcc: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
register_shutdown: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
array_walk: Error: The script tried to call a method on an incomplete object. Please ensure that the class definition "Foo" of the object you are trying to operate on wa
