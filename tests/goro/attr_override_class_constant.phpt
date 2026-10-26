--TEST--
Override cannot target a class constant
--FILE--
<?php
class A { #[Override] const X = 1; }
--EXPECTF--

Fatal error: Attribute "Override" cannot target class constant (allowed targets: method, property) in %s on line 2
