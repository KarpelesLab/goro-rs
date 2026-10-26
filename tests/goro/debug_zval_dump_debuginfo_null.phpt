--TEST--
debug_zval_dump(): refcount in the header after a deprecation from __debugInfo()
--FILE--
<?php
class N { function __debugInfo() { return null; } }
debug_zval_dump(new N);
$n = new N;
debug_zval_dump($n);
--EXPECTF--

Deprecated: Returning null from N::__debugInfo() is deprecated, return an empty array instead in %s on line 3
object(N)#1 (0) refcount(1){
}

Deprecated: Returning null from N::__debugInfo() is deprecated, return an empty array instead in %s on line 5
object(N)#1 (0) refcount(2){
}
