--TEST--
Cannot use empty array elements in a class constant's expression
--FILE--
<?php
class C {
    const K = [1, , 2];
}
--EXPECTF--

Fatal error: Cannot use empty array elements in arrays in %s on line 3
