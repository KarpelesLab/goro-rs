--TEST--
A fatal error binding a class at runtime in an included file has the include() frame in its backtrace
--INI--
fatal_error_backtraces=1
--FILE--
<?php
$f = __DIR__ . '/include_fatal_trace_frame.inc';
file_put_contents($f, '<?php interface I { function f(); } class C implements I {}');
register_shutdown_function(fn() => @unlink($f));
function g($f) { include $f; }
g($f);
--EXPECTF--
Fatal error: Class C contains 1 abstract method and must therefore be declared abstract or implement the remaining method (I::f) in %s on line 1
Stack trace:
#0 %s(5): include()
#1 %s(6): g('%s')
#2 {main}
