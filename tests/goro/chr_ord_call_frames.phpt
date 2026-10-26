--TEST--
chr()/ord() calls that are not folded at compile time have a frame
--FILE--
<?php
function thr($x) { throw new Exception("e"); }
$v = 65;
try { echo chr(thr($v)); } catch (Exception $e) { echo $e->getTraceAsString(), "\n"; }
set_error_handler(function ($n, $s) { echo $s, "\n"; debug_print_backtrace(); return true; });
echo chr($v), "\n";
echo chr("a"), "\n";
echo ord([]), "\n";
--EXPECTF--
#0 %s(4): thr(65)
#1 {main}
A

Fatal error: Uncaught TypeError: chr(): Argument #1 ($codepoint) must be of type int, string given in %s:7
Stack trace:
#0 %s(7): chr('a')
#1 {main}
  thrown in %s on line 7
