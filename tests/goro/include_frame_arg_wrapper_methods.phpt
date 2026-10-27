--TEST--
Include frame argument of functions called by the include opline (wrapper methods, error handler): include_filename
--INI--
include_path=.
zend.exception_string_param_max_len=15
--FILE--
<?php
function inner() { debug_print_backtrace(); }
set_error_handler(function ($no, $str) { echo $str, "\n"; inner(); return true; });
include 'nonexistent_file.php';
restore_error_handler();
class W {
    public $context;
    function stream_open($p, $m, $o, &$op) { debug_print_backtrace(); inner(); return true; }
    function stream_read($n) { static $done = false; if ($done) return ''; $done = true; return '<?php echo "hi\n"; inner();'; }
    function stream_eof() { inner(); return true; }
    function stream_stat() { return []; }
    function stream_set_option($o, $a, $b) { return false; }
}
stream_wrapper_register('wr', 'W');
include 'wr://x';
function f() { require_once 'wr://y'; }
f();
try { throw new Exception("x"); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
--EXPECTF--
include(nonexistent_file.php): Failed to open stream: No such file or directory
#0 %s(3): inner()
#1 %s(4): {closure:%s:3}(2, 'include(nonexis...', '%s', 4)
#2 %s(4): include('%s')
include(): Failed opening 'nonexistent_file.php' for inclusion (include_path='.')
#0 %s(3): inner()
#1 %s(4): {closure:%s:3}(2, 'include(): Fail...', '%s', 4)
#2 %s(4): include('%s')
#0 %s(15): W->stream_open('wr://x', 'rb', 65665, NULL)
#1 %s(15): include()
#0 %s(8): inner()
#1 %s(15): W->stream_open('wr://x', 'rb', 65665, NULL)
#2 %s(15): include('%s')
#0 %s(10): inner()
#1 %s(15): W->stream_eof()
#2 %s(15): include('%s')
#0 %s(10): inner()
#1 %s(15): W->stream_eof()
#2 %s(15): include('%s')
hi
#0 wr://x(1): inner()
#1 %s(15): include('wr://x')
#0 %s(16): W->stream_open('wr://y', 'rb', 65665, NULL)
#1 %s(16): require_once()
#2 %s(17): f()
#0 %s(8): inner()
#1 %s(16): W->stream_open('wr://y', 'rb', 65665, NULL)
#2 %s(16): require_once('%s')
#3 %s(17): f()
#0 %s(10): inner()
#1 %s(16): W->stream_eof()
#2 %s(16): require_once('%s')
#3 %s(17): f()
#0 {main}
