--TEST--
A class link fatal error at an included file's compile-time binding has no include frame
--INI--
fatal_error_backtraces=1
--FILE--
<?php
$file = sys_get_temp_dir() . '/link_fatal_backtrace_include_' . getmypid() . '.inc';
file_put_contents($file, '<?php class P2 { final function f() {} } class C2 extends P2 { function f() {} }');
register_shutdown_function(fn() => @unlink($file));
function load($f, $arr, $o) {
    include $f;
}
load($file, [1, 2], new stdClass);
?>
--EXPECTF--
Fatal error: Cannot override final method P2::f() in %s on line 1
Stack trace:
#0 %s(8): load('%s', Array, Object(stdClass))
#1 {main}
