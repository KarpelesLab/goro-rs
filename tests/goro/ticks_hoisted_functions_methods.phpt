--TEST--
declare(ticks): hoisted functions and class methods declared after it tick (a destructor at shutdown too)
--FILE--
<?php
register_tick_function(function() { echo "tick\n"; });
function before() { echo "before\n"; }
declare(ticks=1);
function f() { echo "f\n"; }
class D { function __destruct() { echo "dtor\n"; } function m() { echo "m\n"; } }
before();
f();
$d = new D;
$d->m();
register_shutdown_function(function() { echo "shutdown\n"; });
ob_start(function($b) { return strtoupper($b); });
echo "end\n";
--EXPECTF--
tick
before
tick
f
tick
tick
tick
m
tick
tick
tick
TICK
END
TICK
SHUTDOWN
TICK
DTOR
TICK
