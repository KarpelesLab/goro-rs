--TEST--
A promoted property hook of an anonymous class needs a body
--FILE--
<?php
new class { public function __construct(public $x { get; }) {} };
--EXPECTF--

Fatal error: Non-abstract property hook must have a body in %s on line 2
