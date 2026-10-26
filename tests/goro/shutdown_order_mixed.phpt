--TEST--
Shutdown: exception handler, shutdown functions, output handler, global/cycle/static destructors in order
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
$a = new D('global-a');
$b = new D('global-b');
$cyc1 = new D('cyc'); $cyc1->self = $cyc1;
register_shutdown_function(function () { echo "shutdown 1\n"; register_shutdown_function(function () { echo "nested shutdown\n"; }); });
register_shutdown_function(function () use ($b) { echo "shutdown 2 {$b->n}\n"; });
ob_start(function ($buf, $phase) { return "[ob phase=$phase len=" . strlen($buf) . "]\n" . $buf; });
echo "body\n";
function f() { static $s; $s = new D('static'); }
f();
class K { static $k; } K::$k = new D('static-prop');
set_exception_handler(function ($e) { echo "handler: ", $e->getMessage(), "\n"; });
throw new Exception("boom");
--EXPECTF--

Deprecated: Creation of dynamic property D::$self is deprecated in %s on line 5
[ob phase=9 len=132]
body
handler: boom
shutdown 1
shutdown 2 global-b
nested shutdown
dtor global-a
dtor global-b
dtor cyc
dtor static
dtor static-prop
