--TEST--
A function's backtrace frame is named as declared, not as called (case, namespace fallback)
--FILE--
<?php
namespace {
    function MyReport($x) {
        $bt = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS);
        echo $bt[0]['function'], isset($bt[0]['type']) ? ' (type)' : '', "\n";
        throw new Exception('x');
    }
}
namespace Foo\Bar {
    function Local() {
        echo debug_backtrace()[0]['function'], "\n";
    }
    function t() {
        try { myreport(1); } catch (\Exception $e) { echo $e->getTrace()[0]['function'], "\n"; }
        $f = function () { MYREPORT(2); };
        try { $f(); } catch (\Exception $e) { echo $e->getTrace()[0]['function'], "\n"; }
        local();
        \foo\bar\LOCAL();
    }
    t();
    try { myReport(3); } catch (\Exception $e) { echo $e->getTrace()[0]['function'], "\n"; }
    local();
}

?>
--EXPECT--
MyReport
MyReport
MyReport
MyReport
Foo\Bar\Local
Foo\Bar\Local
MyReport
MyReport
Foo\Bar\Local
