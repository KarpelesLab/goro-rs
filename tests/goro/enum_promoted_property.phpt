--TEST--
An enum promoted constructor property: properties are rejected before magic methods
--FILE--
<?php
enum E { public function __construct(public $x) {} }
--EXPECTF--

Fatal error: Enum E cannot include properties in %s on line 2
