--TEST--
A compile error in an included file bails out without destructing the includer's locals
--FILE--
<?php
class D { function __construct(public $n) {} function __destruct() { echo "dtor {$this->n}\n"; } }
$inc = __DIR__ . '/fatal_include_compile_error_no_destructors.inc';
file_put_contents($inc, '<?php class A { abstract function f(); }');
register_shutdown_function(function() use ($inc) { unlink($inc); echo "shutdown\n"; });
function f($inc) { $l = new D('local'); include $inc; }
f($inc);
--EXPECTF--

Fatal error: Class A declares abstract method f() and must therefore be declared abstract in %sfatal_include_compile_error_no_destructors.inc on line 1
shutdown
