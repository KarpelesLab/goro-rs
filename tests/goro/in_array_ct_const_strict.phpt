--TEST--
in_array(): a constant $strict that php-src substitutes at compile time is not type checked
--FILE--
<?php
declare(strict_types=1);
namespace Foo;
var_dump(\in_array(1, [], \PHP_INT_MAX));
var_dump(\in_array(1, [1, 2], \PHP_INT_MAX));
try { var_dump(\in_array(1, [], PHP_INT_MAX)); } catch (\TypeError $e) { echo $e->getMessage(), "\n"; }
try { var_dump(in_array(1, [], \PHP_INT_MAX)); } catch (\TypeError $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
bool(false)
bool(true)
in_array(): Argument #3 ($strict) must be of type bool, int given
in_array(): Argument #3 ($strict) must be of type bool, int given
