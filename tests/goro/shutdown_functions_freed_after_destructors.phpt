--TEST--
Shutdown callbacks (and what they capture) are freed after the global destructors
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
$a = new D('a');
$b = new D('b');
$c = new D('c');
register_shutdown_function(function () use ($b) { echo "shutdown\n"; });
--EXPECT--
shutdown
dtor c
dtor a
dtor b
