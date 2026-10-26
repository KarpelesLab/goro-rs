--TEST--
A set hook parameter cannot be named $this
--FILE--
<?php
class A { public $p { set($this) {} } }
--EXPECTF--

Fatal error: Cannot use $this as parameter in %s on line 2
