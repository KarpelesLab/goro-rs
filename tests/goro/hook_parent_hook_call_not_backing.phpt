--TEST--
parent::$x::get() is not a use of the backing store
--FILE--
<?php
class P { public $x = 1 { get => parent::$x::get(); } }
--EXPECTF--

Fatal error: Cannot specify default value for virtual hooked property P::$x in %s on line 2
