--TEST--
zpp: non-object values given to object, class and array|object parameters
--FILE--
<?php
// zpp: a non-object given to an object/class (or array|object) parameter
foreach ([
    fn() => key("abc"),
    fn() => current(null),
    fn() => get_object_vars(null),
    fn() => get_object_vars(1.5),
    fn() => http_build_query(true),
    fn() => iterator_to_array(true),
    fn() => iterator_count(5),
    fn() => key([1]),
    fn() => iterator_to_array([1, 2]),
    fn() => get_object_vars(new ArrayObject([1])),
] as $f) {
    try {
        var_dump($f());
    } catch (TypeError $e) {
        echo $e->getMessage(), "\n";
    }
}
--EXPECT--
key(): Argument #1 ($array) must be of type array, string given
current(): Argument #1 ($array) must be of type array, null given
get_object_vars(): Argument #1 ($object) must be of type object, null given
get_object_vars(): Argument #1 ($object) must be of type object, float given
http_build_query(): Argument #1 ($data) must be of type array, true given
iterator_to_array(): Argument #1 ($iterator) must be of type Traversable|array, true given
iterator_count(): Argument #1 ($iterator) must be of type Traversable|array, int given
int(0)
array(2) {
  [0]=>
  int(1)
  [1]=>
  int(2)
}
array(0) {
}
