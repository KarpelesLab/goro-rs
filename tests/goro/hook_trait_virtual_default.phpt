--TEST--
A trait virtual hooked property cannot have a default value
--FILE--
<?php
trait T { public $x = 1 { get => 1; } }
--EXPECTF--

Fatal error: Cannot specify default value for virtual hooked property T::$x in %s on line 2
