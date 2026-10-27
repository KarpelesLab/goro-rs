--TEST--
A stream wrapper constructor called without its arguments: ArgumentCountError at its declaration
--FILE--
<?php
class VS { public $context; function __construct(array $v) {} function stream_open() { return true; } }
stream_wrapper_register("vsx", "VS");
try { fopen("vsx://x", "r"); } catch (ArgumentCountError $e) { echo $e->getLine(), "\n"; }
--EXPECT--
2
