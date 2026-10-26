--TEST--
compact(), print_r(), var_export(), vprintf(): argument count and type errors
--FILE--
<?php
function f() { try { return compact(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; } }
f();
try { compact(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$g = fn() => compact();
try { $g(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
$h = function() { $a = 1; return compact('a'); };
var_dump($h());
$k = fn() => compact('nope');
var_dump($k());
try { vprintf("%s"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vprintf(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vsprintf("%s"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vsprintf(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vfprintf(STDOUT, "%s"); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vfprintf(); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { vprintf("%s", [1], 2); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { var_export(1, []); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
try { print_r(1, new stdClass); } catch (Error $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
var_dump(print_r(1, null), var_export(1, null), print_r(2, "1"), var_export(2, 1.0));
--EXPECTF--
ArgumentCountError: compact() expects at least 1 argument, 0 given
ArgumentCountError: compact() expects at least 1 argument, 0 given
ArgumentCountError: compact() expects at least 1 argument, 0 given
array(1) {
  ["a"]=>
  int(1)
}

Warning: compact(): Undefined variable $nope in %s on line 9
array(0) {
}
ArgumentCountError: vprintf() expects exactly 2 arguments, 1 given
ArgumentCountError: vprintf() expects exactly 2 arguments, 0 given
ArgumentCountError: vsprintf() expects exactly 2 arguments, 1 given
ArgumentCountError: vsprintf() expects exactly 2 arguments, 0 given
ArgumentCountError: vfprintf() expects exactly 3 arguments, 2 given
ArgumentCountError: vfprintf() expects exactly 3 arguments, 0 given
ArgumentCountError: vprintf() expects exactly 2 arguments, 3 given
TypeError: var_export(): Argument #2 ($return) must be of type bool, array given
TypeError: print_r(): Argument #2 ($return) must be of type bool, stdClass given

Deprecated: print_r(): Passing null to parameter #2 ($return) of type bool is deprecated in %s on line 20
1
Deprecated: var_export(): Passing null to parameter #2 ($return) of type bool is deprecated in %s on line 20
1bool(true)
NULL
string(1) "2"
string(1) "2"
