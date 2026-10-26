--TEST--
The backtrace of a class link fatal error shows the frames' arguments (not NULL)
--INI--
fatal_error_backtraces=1
--FILE--
<?php
function load($f, $arr, $o) {
    eval('class P { final function f() {} } class C extends P { function f() {} }');
}
load('x', [1, 2], new stdClass);
?>
--EXPECTF--
Fatal error: Cannot override final method P::f() in %s(3) : eval()'d code on line 1
Stack trace:
#0 %s(5): load('x', Array, Object(stdClass))
#1 {main}
