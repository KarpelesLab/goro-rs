--TEST--
Override cannot target an enum case
--FILE--
<?php
enum E { #[Override] case A; }
--EXPECTF--

Fatal error: Attribute "Override" cannot target class constant (allowed targets: method, property) in %s on line 2
