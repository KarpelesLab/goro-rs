--TEST--
A first-class callable of an invalid array callable: zend_init_dynamic_call_array()'s errors
--FILE--
<?php
foreach ([[1, 2], [new stdClass, 5], ['stdClass'], [1 => 'a', 2 => 'b'], [1, 2, 3], [null, 'm']] as $x) {
    try { $f = $x(...); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
    try { $x(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECT--
Error: First array member is not a valid class name or object
Error: First array member is not a valid class name or object
Error: Second array member is not a valid method
Error: Second array member is not a valid method
Error: Array callback must have exactly two elements
Error: Array callback must have exactly two elements
Error: Array callback has to contain indices 0 and 1
Error: Array callback has to contain indices 0 and 1
Error: Array callback must have exactly two elements
Error: Array callback must have exactly two elements
Error: First array member is not a valid class name or object
Error: First array member is not a valid class name or object
