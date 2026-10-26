--TEST--
Invalid callback errors print the function name up to its first NUL byte
--FILE--
<?php
// A callback name is printed up to its first NUL byte
try { array_map("a\0b", [1]); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
try { Closure::fromCallable("nope\0x"); } catch (TypeError $e) { echo $e->getMessage(), "\n"; }
--EXPECT--
array_map(): Argument #1 ($callback) must be a valid callback or null, function "a" not found or invalid function name
Failed to create closure from callable: function "nope" not found or invalid function name
