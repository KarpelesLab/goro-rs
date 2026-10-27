--TEST--
A fatal error marks every object destructed: the abandoned frames' locals are not destructed either
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
$a = new D('global');
register_shutdown_function(function() { echo "shutdown\n"; });
function f() { $l = new D('local'); $arr = [new D('in-array')]; trigger_error("fatal", E_USER_ERROR); }
f();
--EXPECTF--

Deprecated: Passing E_USER_ERROR to trigger_error() is deprecated since 8.4, throw an exception or call exit with a string message instead in %s on line 5

Fatal error: fatal in %s on line 5
shutdown
