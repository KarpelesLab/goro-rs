--TEST--
Enum forbidden magic method: the method's signature is checked first (zend_verify_enum() runs at linking)
--FILE--
<?php
enum E { case A; public function __set_state($x) {} }
?>
--EXPECTF--
Fatal error: Method E::__set_state() must be static in %s on line 2
