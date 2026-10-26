--TEST--
Enum implementing Serializable: missing abstract methods are reported first
--FILE--
<?php
enum E implements Serializable { case A; }
?>
--EXPECTF--
Deprecated: E implements the Serializable interface, which is deprecated. Implement __serialize() and __unserialize() instead (or in addition, if support for old PHP versions is necessary) in %s on line 2

Fatal error: Enum E must implement 2 abstract methods (Serializable::serialize, Serializable::unserialize) in %s on line 2
