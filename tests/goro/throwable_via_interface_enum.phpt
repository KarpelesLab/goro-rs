--TEST--
An enum implementing Throwable through a user interface
--FILE--
<?php
interface I extends Throwable {} enum E implements I { case A; }
--EXPECTF--

Fatal error: Enum E cannot implement interface Throwable in %s on line 2
