--TEST--
method_exists(): stdClass and SplDoublyLinkedList have no constructor
--FILE--
<?php
var_dump(method_exists('stdClass', '__construct'), method_exists(new stdClass, '__construct'));
var_dump(method_exists('ArrayObject', '__construct'), method_exists('Exception', '__construct'));
var_dump(method_exists('Closure', '__construct'), method_exists('Generator', '__construct'));
var_dump(method_exists('SplObjectStorage', '__construct'), method_exists('DateTime', '__construct'));
var_dump(method_exists('WeakMap', '__construct'), method_exists('ArrayIterator', '__construct'));
var_dump(method_exists('SplStack', '__construct'), method_exists('SplDoublyLinkedList', '__construct'));
var_dump(method_exists('stdClass', '__destruct'));

?>
--EXPECT--
bool(false)
bool(false)
bool(true)
bool(true)
bool(true)
bool(false)
bool(false)
bool(true)
bool(false)
bool(true)
bool(false)
bool(false)
bool(false)
