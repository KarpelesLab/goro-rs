--TEST--
A user stream wrapper's methods called by include are called from the include line
--FILE--
<?php
class W {
    public $context;
    function stream_open() { return true; }
    function stream_read() { return '.'; }
    function stream_set_option() {}
    function stream_stat() {}
    function stream_eof() { throw new Exception("from eof"); }
    function stream_close() {}
}
stream_wrapper_register('ww', 'W');
function f() { include 'ww://x'; }
try { f(); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
#0 %s(12): W->stream_eof()
#1 %s(12): include()
#2 %s(13): f()
#3 {main}
