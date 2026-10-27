--TEST--
Cannot redeclare function of an included file: a compile-time error, without the include frame
--INI--
fatal_error_backtraces=1
--FILE--
<?php
$f = sys_get_temp_dir() . '/goro_redecl_' . getmypid() . '.inc';
file_put_contents($f, "<?php\nfunction redecl_test() {}\n");
register_shutdown_function(fn() => unlink($f));
function g($f) { include $f; }
g($f);
g($f);
--EXPECTF--

Fatal error: Cannot redeclare function redecl_test() (previously declared in /tmp/goro_redecl_%d.inc:2) in /tmp/goro_redecl_%d.inc on line 2
Stack trace:
#0 %s(7): g('/tmp/goro_redec...')
#1 {main}
