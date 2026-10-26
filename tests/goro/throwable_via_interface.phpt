--TEST--
Implementing Throwable through a user interface
--FILE--
<?php
interface I extends Throwable {} class A implements I {}
--EXPECTF--

Fatal error: Class A cannot implement interface Throwable, extend Exception or Error instead in %s on line 2
